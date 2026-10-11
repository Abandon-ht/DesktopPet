//! A single output stream for a complete spoken turn. Resampling and queue
//! backpressure run on the playback worker; the device callback only copies PCM.

use crate::{Pcm, PlaybackItem, TurnToken};
use anyhow::{Context, Result, bail, ensure};
use cpal::{
    SampleFormat, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use sherpa_onnx::LinearResampler;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const PREBUFFER_MS: usize = 120;
// The upstream channel holds another 4 × 250 ms, for ~3 s total queued PCM.
const BUFFER_SECONDS: usize = 2;
const POLL_INTERVAL: Duration = Duration::from_millis(2);
const NO_CALLBACK_TIMEOUT: Duration = Duration::from_secs(3);

/// Exactly one producer (playback worker) and one consumer (CPAL callback).
/// Atomic sample storage avoids unsafe shared memory. Neither operation retries,
/// waits, takes a lock or allocates. Indices wrap at twice the capacity, preserving
/// the distinction between full and empty without unbounded integer counters.
struct SampleRing {
    samples: Box<[AtomicU32]>,
    read: AtomicUsize,
    write: AtomicUsize,
}

impl SampleRing {
    fn new(capacity: usize) -> Self {
        assert!(capacity > 0 && capacity <= usize::MAX / 2);
        Self {
            samples: (0..capacity).map(|_| AtomicU32::new(0)).collect(),
            read: AtomicUsize::new(0),
            write: AtomicUsize::new(0),
        }
    }

    fn advance(&self, position: usize) -> usize {
        if position + 1 == self.samples.len() * 2 {
            0
        } else {
            position + 1
        }
    }

    fn distance(&self, read: usize, write: usize) -> usize {
        if write >= read {
            write - read
        } else {
            self.samples.len() * 2 - read + write
        }
    }

    /// Returns queue size at submission or the unchanged sample if full.
    fn push(&self, sample: f32) -> std::result::Result<usize, f32> {
        let write = self.write.load(Ordering::Relaxed);
        let read = self.read.load(Ordering::Acquire);
        let queued = self.distance(read, write);
        if queued == self.samples.len() {
            return Err(sample);
        }
        self.samples[write % self.samples.len()].store(sample.to_bits(), Ordering::Relaxed);
        self.write.store(self.advance(write), Ordering::Release);
        Ok(queued + 1)
    }

    fn pop(&self) -> Option<f32> {
        let read = self.read.load(Ordering::Relaxed);
        if read == self.write.load(Ordering::Acquire) {
            return None;
        }
        let sample =
            f32::from_bits(self.samples[read % self.samples.len()].load(Ordering::Relaxed));
        self.read.store(self.advance(read), Ordering::Release);
        Some(sample)
    }
}

struct PlaybackBuffer {
    ring: SampleRing,
    submitted: AtomicU64,
    consumed: AtomicU64,
    max_queued: AtomicUsize,
    underrun_frames: AtomicU64,
    underrun_callbacks: AtomicU64,
    first_audio_us: AtomicU64,
    first_non_silent_us: AtomicU64,
    last_audio_end_us: AtomicU64,
    last_callback_us: AtomicU64,
    ready: AtomicBool,
    finished: AtomicBool,
    failed: AtomicBool,
    epoch: Instant,
}

impl PlaybackBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            ring: SampleRing::new(capacity),
            submitted: AtomicU64::new(0),
            consumed: AtomicU64::new(0),
            max_queued: AtomicUsize::new(0),
            underrun_frames: AtomicU64::new(0),
            underrun_callbacks: AtomicU64::new(0),
            first_audio_us: AtomicU64::new(u64::MAX),
            first_non_silent_us: AtomicU64::new(u64::MAX),
            last_audio_end_us: AtomicU64::new(0),
            last_callback_us: AtomicU64::new(0),
            ready: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            epoch: Instant::now(),
        }
    }

    fn elapsed_us(&self) -> u64 {
        self.epoch.elapsed().as_micros().min(u64::MAX as u128) as u64
    }

    fn check(&self, token: &TurnToken, started_us: Option<u64>) -> Result<()> {
        token.check()?;
        ensure!(
            !self.failed.load(Ordering::Acquire),
            "扬声器播放失败或设备断开"
        );
        if let Some(started_us) = started_us {
            let last = self
                .last_callback_us
                .load(Ordering::Acquire)
                .max(started_us);
            ensure!(
                self.elapsed_us().saturating_sub(last) < NO_CALLBACK_TIMEOUT.as_micros() as u64,
                "扬声器超过 3 秒没有播放回调，输出设备可能已失效"
            );
        }
        Ok(())
    }

    fn drained(&self, now_us: u64) -> bool {
        self.finished.load(Ordering::Acquire)
            && self.consumed.load(Ordering::Acquire) >= self.submitted.load(Ordering::Acquire)
            && now_us >= self.last_audio_end_us.load(Ordering::Acquire)
    }
}

fn frames_us(frames: usize, sample_rate: u32) -> u64 {
    // Round upwards so the final fraction of a frame is never truncated.
    (frames as u64 * 1_000_000).div_ceil(sample_rate as u64)
}

/// When a backend cannot supply a sensible DAC timestamp, retain one callback
/// buffer of latency as a conservative, bounded fallback at the end of the turn.
fn playback_latency_us(predicted: Option<Duration>, buffer_frames: usize, rate: u32) -> u64 {
    match predicted {
        Some(delay) if !delay.is_zero() && delay <= Duration::from_secs(2) => {
            delay.as_micros() as u64
        }
        _ => frames_us(buffer_frames, rate) + 80_000,
    }
}

trait OutputSample: Copy {
    fn from_pcm(sample: f32) -> Self;
}

impl OutputSample for f32 {
    fn from_pcm(sample: f32) -> Self {
        sample
    }
}

impl OutputSample for i16 {
    fn from_pcm(sample: f32) -> Self {
        (sample * i16::MAX as f32) as i16
    }
}

impl OutputSample for u16 {
    fn from_pcm(sample: f32) -> Self {
        ((sample + 1.0) * 32767.5) as u16
    }
}

/// Pure callback body, also driven directly by deterministic fake-device tests.
// Keep device format, turn state and callback timing explicit for those tests.
#[allow(clippy::too_many_arguments)]
fn render<T: OutputSample>(
    data: &mut [T],
    channels: usize,
    rate: u32,
    gain: f32,
    buffer: &PlaybackBuffer,
    token: &TurnToken,
    now_us: u64,
    latency_us: u64,
) {
    let mut consumed = 0_u64;
    let mut underrun = 0_u64;
    let mut last_audio_frame = None;
    for (index, frame) in data.chunks_mut(channels).enumerate() {
        // Check each frame so cancellation silences queued audio immediately.
        let value = if buffer.ready.load(Ordering::Acquire) && token.is_current() {
            match buffer.ring.pop() {
                Some(value) => {
                    consumed += 1;
                    last_audio_frame = Some(index);
                    let delivered_us = now_us + frames_us(index, rate);
                    let _ = buffer.first_audio_us.compare_exchange(
                        u64::MAX,
                        delivered_us,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    );
                    if value != 0.0 && gain != 0.0 {
                        let _ = buffer.first_non_silent_us.compare_exchange(
                            u64::MAX,
                            delivered_us,
                            Ordering::Relaxed,
                            Ordering::Relaxed,
                        );
                    }
                    value
                }
                None => {
                    if !buffer.finished.load(Ordering::Acquire) {
                        underrun += 1;
                    }
                    0.0
                }
            }
        } else {
            0.0
        };
        frame.fill(T::from_pcm((value * gain).clamp(-1.0, 1.0)));
    }
    if let Some(last) = last_audio_frame {
        buffer.last_audio_end_us.store(
            now_us + latency_us + frames_us(last + 1, rate),
            Ordering::Release,
        );
        // Publish after the final sample's deadline. A producer observing this
        // count cannot drain against a stale deadline from a previous callback.
        buffer.consumed.fetch_add(consumed, Ordering::Release);
    }
    if underrun > 0 {
        buffer
            .underrun_frames
            .fetch_add(underrun, Ordering::Relaxed);
        buffer.underrun_callbacks.fetch_add(1, Ordering::Relaxed);
    }
    buffer.last_callback_us.store(now_us, Ordering::Release);
}

#[allow(clippy::too_many_arguments)]
fn output_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    buffer: Arc<PlaybackBuffer>,
    token: TurnToken,
    gain: f32,
) -> Result<cpal::Stream> {
    let channels = config.channels as usize;
    let rate = config.sample_rate.0;
    let failed = buffer.clone();
    macro_rules! stream {
        ($sample:ty) => {
            device.build_output_stream(
                config,
                move |data: &mut [$sample], info: &cpal::OutputCallbackInfo| {
                    let timestamp = info.timestamp();
                    let latency = playback_latency_us(
                        timestamp.playback.duration_since(&timestamp.callback),
                        data.len().div_ceil(channels),
                        rate,
                    );
                    render(
                        data,
                        channels,
                        rate,
                        gain,
                        &buffer,
                        &token,
                        buffer.elapsed_us(),
                        latency,
                    );
                },
                move |_| {
                    failed.failed.store(true, Ordering::Release);
                },
                None,
            )?
        };
    }
    Ok(match format {
        SampleFormat::F32 => stream!(f32),
        SampleFormat::I16 => stream!(i16),
        SampleFormat::U16 => stream!(u16),
        _ => bail!("扬声器采样格式暂不支持：{format:?}"),
    })
}

/// Retain sherpa's bandlimited resampler state across PCM frame and sentence
/// boundaries. Flushing every 250 ms would reset the filter and lose continuity.
struct OutputResampler {
    output_rate: u32,
    input_rate: Option<u32>,
    resampler: Option<LinearResampler>,
}

impl OutputResampler {
    fn new(output_rate: u32) -> Self {
        Self {
            output_rate,
            input_rate: None,
            resampler: None,
        }
    }

    fn convert(&mut self, pcm: Pcm) -> Result<Vec<f32>> {
        ensure!(
            pcm.sample_rate > 0 && pcm.sample_rate <= i32::MAX as u32,
            "无效音频采样率"
        );
        ensure!(pcm.samples.len() <= i32::MAX as usize, "音频块过大");
        let mut samples = Vec::new();
        if self.input_rate != Some(pcm.sample_rate) {
            samples = self.finish();
            self.input_rate = Some(pcm.sample_rate);
            if self.output_rate != pcm.sample_rate {
                self.resampler = Some(
                    LinearResampler::create(pcm.sample_rate as i32, self.output_rate as i32)
                        .context("扬声器重采样器创建失败")?,
                );
            }
        }
        if let Some(resampler) = &self.resampler {
            samples.extend(resampler.resample(&pcm.samples, false));
        } else if samples.is_empty() {
            return Ok(pcm.samples);
        } else {
            samples.extend(pcm.samples);
        }
        Ok(samples)
    }

    fn finish(&mut self) -> Vec<f32> {
        self.input_rate = None;
        self.resampler
            .take()
            .map(|r| r.resample(&[], true))
            .unwrap_or_default()
    }
}

struct PlaybackDiagnostic {
    buffer: Arc<PlaybackBuffer>,
    rate: u32,
    turn_id: u64,
    complete: bool,
    started_us: Option<u64>,
    backpressure_us: u64,
    drain_us: u64,
}

impl Drop for PlaybackDiagnostic {
    fn drop(&mut self) {
        let buffer = &self.buffer;
        let first = |value: u64| {
            if value == u64::MAX {
                -1.0
            } else {
                value as f64 / 1000.0
            }
        };
        eprintln!(
            "voice_playback turn={} result={} rate={} submitted_ms={:.1} consumed_ms={:.1} first_audio_callback_ms={:.1} first_non_silent_callback_ms={:.1} max_queue_ms={:.1} underrun_callbacks={} underrun_ms={:.1} backpressure_ms={:.1} drain_ms={:.1} elapsed_ms={:.1}",
            self.turn_id,
            if self.complete { "complete" } else { "aborted" },
            self.rate,
            buffer.submitted.load(Ordering::Acquire) as f64 * 1000.0 / self.rate as f64,
            buffer.consumed.load(Ordering::Acquire) as f64 * 1000.0 / self.rate as f64,
            first(buffer.first_audio_us.load(Ordering::Acquire)),
            first(buffer.first_non_silent_us.load(Ordering::Acquire)),
            buffer.max_queued.load(Ordering::Acquire) as f64 * 1000.0 / self.rate as f64,
            buffer.underrun_callbacks.load(Ordering::Acquire),
            buffer.underrun_frames.load(Ordering::Acquire) as f64 * 1000.0 / self.rate as f64,
            self.backpressure_us as f64 / 1000.0,
            self.drain_us as f64 / 1000.0,
            buffer.elapsed_us() as f64 / 1000.0,
        );
    }
}

fn feed(
    samples: &[f32],
    token: &TurnToken,
    diagnostics: &mut PlaybackDiagnostic,
    start: &mut impl FnMut() -> Result<()>,
    wait: &mut impl FnMut(),
) -> Result<()> {
    if samples.is_empty() {
        return Ok(());
    }
    let buffer = &diagnostics.buffer;
    buffer
        .submitted
        .fetch_add(samples.len() as u64, Ordering::Release);
    // Start even a tiny first frame immediately after queuing its available PCM;
    // waiting for another frame could otherwise wait for the whole next sentence.
    let prebuffer = (diagnostics.rate as usize * PREBUFFER_MS / 1000)
        .max(1)
        .min(samples.len());
    let mut max_queued = 0;
    for (index, sample) in samples.iter().enumerate() {
        if index % 256 == 0 {
            buffer.check(token, diagnostics.started_us)?;
        }
        let mut wait_start = None;
        loop {
            match buffer.ring.push(*sample) {
                Ok(queued) => {
                    max_queued = max_queued.max(queued);
                    break;
                }
                Err(_) => {
                    buffer.check(token, diagnostics.started_us)?;
                    wait_start.get_or_insert_with(|| buffer.elapsed_us());
                    wait();
                }
            }
        }
        if let Some(wait_start) = wait_start {
            diagnostics.backpressure_us += buffer.elapsed_us().saturating_sub(wait_start);
        }
        if diagnostics.started_us.is_none() && index + 1 >= prebuffer {
            // ALSA can invoke callbacks immediately during stream construction,
            // even before play(). Gate consumption explicitly on all backends.
            buffer.ready.store(true, Ordering::Release);
            start()?;
            diagnostics.started_us = Some(buffer.elapsed_us());
        }
    }
    buffer.max_queued.fetch_max(max_queued, Ordering::Relaxed);
    Ok(())
}

pub(super) fn play_stream(
    gain: f32,
    next: &mut dyn FnMut() -> Result<PlaybackItem>,
    token: &TurnToken,
) -> Result<()> {
    token.check()?;
    let device = cpal::default_host()
        .default_output_device()
        .context("没有扬声器输出设备")?;
    let supported = device
        .default_output_config()
        .context("无法读取扬声器格式")?;
    let rate = supported.sample_rate().0;
    ensure!(
        supported.channels() > 0 && rate > 0 && rate <= i32::MAX as u32,
        "无效输出格式"
    );
    let buffer = Arc::new(PlaybackBuffer::new(rate as usize * BUFFER_SECONDS));
    let mut diagnostics = PlaybackDiagnostic {
        buffer: buffer.clone(),
        rate,
        turn_id: token.id(),
        complete: false,
        started_us: None,
        backpressure_us: 0,
        drain_us: 0,
    };
    // CPAL streams are created, used and dropped in this worker. Some backends
    // intentionally make Stream !Send; no device handle crosses worker threads.
    let stream = output_stream(
        &device,
        &supported.config(),
        supported.sample_format(),
        buffer.clone(),
        token.clone(),
        gain,
    )?;
    let mut resampler = OutputResampler::new(rate);
    let mut start = || stream.play().context("扬声器播放启动失败");
    let mut wait = || std::thread::sleep(POLL_INTERVAL);
    while let Some(pcm) = receive_audio(next, token, &diagnostics, &mut wait)? {
        feed(
            &resampler.convert(pcm)?,
            token,
            &mut diagnostics,
            &mut start,
            &mut wait,
        )?;
    }
    buffer.check(token, diagnostics.started_us)?;
    feed(
        &resampler.finish(),
        token,
        &mut diagnostics,
        &mut start,
        &mut wait,
    )?;
    buffer.finished.store(true, Ordering::Release);
    let drain_start = buffer.elapsed_us();
    while !buffer.drained(buffer.elapsed_us()) {
        buffer.check(token, diagnostics.started_us)?;
        std::thread::sleep(POLL_INTERVAL);
    }
    buffer.check(token, diagnostics.started_us)?;
    diagnostics.drain_us = buffer.elapsed_us().saturating_sub(drain_start);
    diagnostics.complete = true;
    // Stop once, after the last consumed sample's predicted audible deadline.
    drop(stream);
    Ok(())
}

/// The source poll never waits for synthesis. Keep checking output device faults
/// and cancellation even when the LLM or synthesizer has not produced more PCM.
fn receive_audio(
    next: &mut dyn FnMut() -> Result<PlaybackItem>,
    token: &TurnToken,
    diagnostics: &PlaybackDiagnostic,
    wait: &mut impl FnMut(),
) -> Result<Option<Pcm>> {
    loop {
        diagnostics.buffer.check(token, diagnostics.started_us)?;
        match next()? {
            PlaybackItem::Audio(pcm) => return Ok(Some(pcm)),
            PlaybackItem::Pending => wait(),
            PlaybackItem::End => return Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> TurnToken {
        TurnToken::new(Arc::new(AtomicU64::new(1)), 1)
    }

    fn enqueue(buffer: &PlaybackBuffer, samples: &[f32]) {
        for sample in samples {
            buffer.ring.push(*sample).unwrap();
        }
        buffer
            .submitted
            .fetch_add(samples.len() as u64, Ordering::Release);
        buffer.ready.store(true, Ordering::Release);
    }

    #[test]
    fn ring_is_bounded_and_preserves_order_across_many_wraps() {
        let ring = SampleRing::new(3);
        for lap in 0..20 {
            assert_eq!(ring.pop(), None);
            for index in 0..3 {
                assert_eq!(ring.push((lap * 3 + index) as f32), Ok(index + 1));
            }
            assert_eq!(ring.push(-1.0), Err(-1.0));
            for index in 0..3 {
                assert_eq!(ring.pop(), Some((lap * 3 + index) as f32));
            }
        }
    }

    #[test]
    fn concurrent_producer_consumer_preserve_every_sample() {
        let ring = SampleRing::new(31);
        let deadline = Instant::now() + Duration::from_secs(10);
        let stop = AtomicBool::new(false);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for index in 0..100_000 {
                    while ring.push(index as f32).is_err() {
                        if stop.load(Ordering::Acquire) {
                            return;
                        }
                        assert!(
                            Instant::now() < deadline,
                            "concurrent ring producer stalled"
                        );
                        std::thread::yield_now();
                    }
                }
            });
            scope.spawn(|| {
                for index in 0..100_000 {
                    loop {
                        if let Some(value) = ring.pop() {
                            if value != index as f32 {
                                stop.store(true, Ordering::Release);
                            }
                            assert_eq!(value, index as f32);
                            break;
                        }
                        if Instant::now() >= deadline {
                            stop.store(true, Ordering::Release);
                            panic!("concurrent ring consumer stalled");
                        }
                        std::thread::yield_now();
                    }
                }
            });
        });
        assert_eq!(ring.pop(), None);
    }

    fn diagnostics(capacity: usize, rate: u32) -> PlaybackDiagnostic {
        PlaybackDiagnostic {
            buffer: Arc::new(PlaybackBuffer::new(capacity)),
            rate,
            turn_id: 1,
            complete: false,
            started_us: None,
            backpressure_us: 0,
            drain_us: 0,
        }
    }

    #[test]
    fn short_first_frame_starts_without_waiting_for_the_next_sentence() {
        let mut diagnostics = diagnostics(2000, 1000);
        let mut starts = 0;
        feed(
            &[0.5; 8],
            &token(),
            &mut diagnostics,
            &mut || {
                starts += 1;
                Ok(())
            },
            &mut || panic!("short frame must not wait"),
        )
        .unwrap();
        assert_eq!(starts, 1);
        assert!(diagnostics.started_us.is_some());
        assert_eq!(diagnostics.buffer.submitted.load(Ordering::Acquire), 8);
    }

    #[test]
    fn an_early_alsa_callback_waits_for_prebuffer_without_counting_underruns() {
        let buffer = PlaybackBuffer::new(4);
        buffer.ring.push(0.5).unwrap();
        buffer.submitted.store(1, Ordering::Release);
        let mut output = [1.0; 4];
        render(&mut output, 1, 1000, 1.0, &buffer, &token(), 0, 0);
        assert_eq!(output, [0.0; 4]);
        assert_eq!(buffer.consumed.load(Ordering::Acquire), 0);
        assert_eq!(buffer.underrun_frames.load(Ordering::Acquire), 0);
        buffer.ready.store(true, Ordering::Release);
        buffer.finished.store(true, Ordering::Release);
        render(&mut output, 1, 1000, 1.0, &buffer, &token(), 4000, 0);
        assert_eq!(output, [0.5, 0.0, 0.0, 0.0]);
        assert_eq!(buffer.consumed.load(Ordering::Acquire), 1);
    }

    #[test]
    fn backpressure_preserves_audio_and_opens_the_stream_only_once() {
        let mut diagnostics = diagnostics(128, 1000);
        let buffer = diagnostics.buffer.clone();
        let token = token();
        let samples = (0..350).map(|i| i as f32 / 350.0).collect::<Vec<_>>();
        let mut starts = 0;
        let mut heard = Vec::new();
        {
            let mut start = || {
                starts += 1;
                Ok(())
            };
            let mut callback = || {
                let mut output = [0.0; 64];
                render(&mut output, 1, 1000, 1.0, &buffer, &token, 0, 0);
                heard.extend(output);
            };
            feed(
                &samples[..250],
                &token,
                &mut diagnostics,
                &mut start,
                &mut callback,
            )
            .unwrap();
            feed(
                &samples[250..],
                &token,
                &mut diagnostics,
                &mut start,
                &mut callback,
            )
            .unwrap();
        }
        buffer.finished.store(true, Ordering::Release);
        let remaining = samples.len() - heard.len();
        let mut output = vec![0.0; remaining];
        render(&mut output, 1, 1000, 1.0, &buffer, &token, 0, 0);
        heard.extend(output);
        assert_eq!(starts, 1);
        assert_eq!(heard, samples);
        assert_eq!(buffer.max_queued.load(Ordering::Acquire), 128);
        assert_eq!(buffer.underrun_frames.load(Ordering::Acquire), 0);
    }

    #[test]
    fn a_full_queue_wait_stops_on_cancellation() {
        let generation = Arc::new(AtomicU64::new(1));
        let token = TurnToken::new(generation.clone(), 1);
        let mut diagnostics = diagnostics(128, 1000);
        let result = feed(
            &[0.5; 256],
            &token,
            &mut diagnostics,
            &mut || Ok(()),
            &mut || generation.store(2, Ordering::Release),
        );
        assert!(result.is_err());
    }

    #[test]
    fn pending_source_detects_device_failure_without_waiting_for_new_pcm() {
        let diagnostics = diagnostics(128, 1000);
        let mut polls = 0;
        let mut waits = 0;
        let result = receive_audio(
            &mut || {
                polls += 1;
                diagnostics.buffer.failed.store(true, Ordering::Release);
                Ok(PlaybackItem::Pending)
            },
            &token(),
            &diagnostics,
            &mut || waits += 1,
        );
        assert!(result.unwrap_err().to_string().contains("设备断开"));
        assert_eq!(polls, 1);
        assert_eq!(waits, 1);
    }

    #[test]
    fn pending_source_detects_stalled_callbacks_after_output_started() {
        let mut buffer = PlaybackBuffer::new(128);
        buffer.epoch = Instant::now() - Duration::from_secs(4);
        let diagnostics = PlaybackDiagnostic {
            buffer: Arc::new(buffer),
            started_us: Some(1),
            ..diagnostics(128, 1000)
        };
        let result = receive_audio(
            &mut || panic!("failed device must be checked before polling PCM"),
            &token(),
            &diagnostics,
            &mut || panic!("failed device must not wait"),
        );
        assert!(result.unwrap_err().to_string().contains("没有播放回调"));
    }

    #[test]
    fn pending_source_detects_cancellation_and_preserves_pcm_and_eof() {
        let diagnostics = diagnostics(128, 1000);
        let generation = Arc::new(AtomicU64::new(1));
        let active = TurnToken::new(generation.clone(), 1);
        let result = receive_audio(
            &mut || Ok(PlaybackItem::Pending),
            &active,
            &diagnostics,
            &mut || generation.store(2, Ordering::Release),
        );
        assert!(result.is_err());
        let pcm = receive_audio(
            &mut || {
                Ok(PlaybackItem::Audio(Pcm {
                    samples: vec![0.7],
                    sample_rate: 24000,
                }))
            },
            &token(),
            &diagnostics,
            &mut || panic!("ready PCM must not wait"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(pcm.samples, [0.7]);
        assert!(
            receive_audio(
                &mut || Ok(PlaybackItem::End),
                &token(),
                &diagnostics,
                &mut || panic!("EOF must not wait"),
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn two_chunks_share_one_callback_without_inserted_silence() {
        let buffer = PlaybackBuffer::new(12);
        enqueue(&buffer, &[0.1, 0.2]);
        enqueue(&buffer, &[0.3, 0.4]);
        buffer.finished.store(true, Ordering::Release);
        let mut stereo = [0.0; 8];
        render(&mut stereo, 2, 1000, 1.0, &buffer, &token(), 10_000, 7_000);
        assert_eq!(stereo, [0.1, 0.1, 0.2, 0.2, 0.3, 0.3, 0.4, 0.4]);
        assert_eq!(buffer.consumed.load(Ordering::Acquire), 4);
        assert_eq!(buffer.underrun_frames.load(Ordering::Acquire), 0);
        assert!(!buffer.drained(20_999));
        assert!(buffer.drained(21_000));
    }

    #[test]
    fn final_partial_callback_drains_at_last_audio_frame_and_excludes_final_silence() {
        let buffer = PlaybackBuffer::new(8);
        enqueue(&buffer, &[0.0, 0.4]);
        buffer.finished.store(true, Ordering::Release);
        let mut output = [1.0; 5];
        render(&mut output, 1, 1000, 1.0, &buffer, &token(), 100_000, 4_000);
        assert_eq!(output, [0.0, 0.4, 0.0, 0.0, 0.0]);
        assert_eq!(buffer.first_audio_us.load(Ordering::Acquire), 100_000);
        assert_eq!(buffer.first_non_silent_us.load(Ordering::Acquire), 101_000);
        assert_eq!(buffer.underrun_frames.load(Ordering::Acquire), 0);
        assert!(!buffer.drained(105_999));
        assert!(buffer.drained(106_000));
    }

    #[test]
    fn underrun_silence_does_not_advance_consumed_audio() {
        let buffer = PlaybackBuffer::new(8);
        enqueue(&buffer, &[0.5]);
        let mut output = [1.0; 4];
        render(&mut output, 1, 1000, 1.0, &buffer, &token(), 0, 0);
        assert_eq!(output, [0.5, 0.0, 0.0, 0.0]);
        assert_eq!(buffer.consumed.load(Ordering::Acquire), 1);
        assert_eq!(buffer.underrun_frames.load(Ordering::Acquire), 3);
        assert_eq!(buffer.underrun_callbacks.load(Ordering::Acquire), 1);
        enqueue(&buffer, &[0.6, 0.7]);
        buffer.finished.store(true, Ordering::Release);
        render(&mut output, 1, 1000, 1.0, &buffer, &token(), 4000, 0);
        assert_eq!(output, [0.6, 0.7, 0.0, 0.0]);
        assert_eq!(buffer.consumed.load(Ordering::Acquire), 3);
        assert_eq!(buffer.underrun_frames.load(Ordering::Acquire), 3);
        assert!(buffer.drained(6000));
    }

    #[test]
    fn stale_generation_silences_buffer_without_consuming_old_audio() {
        let generation = Arc::new(AtomicU64::new(1));
        let token = TurnToken::new(generation.clone(), 1);
        let buffer = PlaybackBuffer::new(4);
        enqueue(&buffer, &[0.5, -0.5]);
        generation.store(2, Ordering::Release);
        let mut output = [1.0; 4];
        render(&mut output, 2, 1000, 1.0, &buffer, &token, 0, 0);
        assert_eq!(output, [0.0; 4]);
        assert_eq!(buffer.consumed.load(Ordering::Acquire), 0);
        assert_eq!(buffer.ring.pop(), Some(0.5));
        assert!(buffer.check(&token, None).is_err());
    }

    #[test]
    fn gain_and_clamping_work_for_all_supported_formats() {
        fn output<T: OutputSample + Default>() -> [T; 4] {
            let buffer = PlaybackBuffer::new(4);
            enqueue(&buffer, &[-3.0, -0.5, 0.5, 3.0]);
            let mut data = [T::default(); 4];
            render(&mut data, 1, 1000, 0.5, &buffer, &token(), 0, 0);
            data
        }
        assert_eq!(output::<f32>(), [-1.0, -0.25, 0.25, 1.0]);
        assert_eq!(output::<i16>(), [-32767, -8191, 8191, 32767]);
        assert_eq!(output::<u16>(), [0, 24575, 40959, 65535]);
    }

    #[test]
    fn invalid_timestamps_use_bounded_callback_duration_fallback() {
        assert_eq!(playback_latency_us(None, 480, 48000), 90_000);
        assert_eq!(
            playback_latency_us(Some(Duration::ZERO), 480, 48000),
            90_000
        );
        assert_eq!(
            playback_latency_us(Some(Duration::from_secs(20)), 480, 48000),
            90_000
        );
        assert_eq!(
            playback_latency_us(Some(Duration::from_millis(35)), 480, 48000),
            35_000
        );
    }

    #[test]
    fn resampler_keeps_filter_state_and_flushes_tail_once() {
        for (input, output) in [(24000, 48000), (48000, 24000)] {
            let samples = (0..4000)
                .map(|i| (i as f32 * 0.071).sin())
                .collect::<Vec<_>>();
            let mut whole = OutputResampler::new(output);
            let mut expected = whole
                .convert(Pcm {
                    samples: samples.clone(),
                    sample_rate: input,
                })
                .unwrap();
            expected.extend(whole.finish());
            let mut split = OutputResampler::new(output);
            let mut actual = Vec::new();
            for chunk in samples.chunks(113) {
                actual.extend(
                    split
                        .convert(Pcm {
                            samples: chunk.to_vec(),
                            sample_rate: input,
                        })
                        .unwrap(),
                );
            }
            let tail = split.finish();
            assert!(!tail.is_empty());
            actual.extend(tail);
            assert!(split.finish().is_empty());
            assert_eq!(actual.len(), expected.len());
            assert!(
                actual
                    .iter()
                    .zip(&expected)
                    .all(|(a, b)| (a - b).abs() < 0.00001)
            );
        }
    }

    #[test]
    fn source_rate_change_flushes_previous_tail_before_new_audio() {
        let pcm = Pcm {
            samples: vec![0.2; 321],
            sample_rate: 24000,
        };
        let mut reference = OutputResampler::new(48000);
        let mut expected = reference.convert(pcm.clone()).unwrap();
        expected.extend(reference.finish());
        expected.extend([0.7, 0.8]);
        let mut resampler = OutputResampler::new(48000);
        let mut actual = resampler.convert(pcm).unwrap();
        actual.extend(
            resampler
                .convert(Pcm {
                    samples: vec![0.7, 0.8],
                    sample_rate: 48000,
                })
                .unwrap(),
        );
        actual.extend(resampler.finish());
        assert_eq!(actual, expected);
    }

    #[test]
    fn tiny_resampled_reply_is_not_lost_at_eof() {
        let mut resampler = OutputResampler::new(48000);
        let mut samples = resampler
            .convert(Pcm {
                samples: vec![0.5; 8],
                sample_rate: 24000,
            })
            .unwrap();
        samples.extend(resampler.finish());
        assert!(!samples.is_empty());
        let buffer = PlaybackBuffer::new(samples.len());
        enqueue(&buffer, &samples);
        buffer.finished.store(true, Ordering::Release);
        let mut output = vec![0.0; samples.len() + 100];
        render(&mut output, 1, 48000, 1.0, &buffer, &token(), 0, 0);
        assert_eq!(&output[..samples.len()], &samples);
        assert_eq!(
            buffer.consumed.load(Ordering::Acquire),
            samples.len() as u64
        );
        assert!(buffer.drained(frames_us(samples.len(), 48000)));
    }
}
