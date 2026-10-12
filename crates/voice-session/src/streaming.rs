//! Sentence-level streaming: network, synthesis and device output overlap.
//! Only the caller reports UI state; none of the workers needs its closure.

use super::{LlmPort, Pcm, PlaybackItem, PlaybackPort, Source, TtsPort, TurnToken, VoiceStatus};
use anyhow::{Result, anyhow, bail};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender, TryRecvError, TrySendError},
    },
    thread,
    time::{Duration, Instant},
};

const SENTENCE_QUEUE: usize = 2;
// Quarter-second frames make backpressure independent of sentence duration.
const PCM_FRAME_MS: usize = 250;
// The device adapter keeps up to two further seconds, for ~3 s total queued.
const PCM_QUEUE: usize = 4;
const QUEUE_POLL: Duration = Duration::from_millis(10);

enum Event {
    Delta(String),
    Final(String, Vec<Source>),
    Synthesizing,
    Speaking,
}

type Failure = Arc<Mutex<Option<anyhow::Error>>>;

struct PipelineTiming {
    epoch: Instant,
    first_sentence_us: AtomicU64,
    llm_done_us: AtomicU64,
}

impl PipelineTiming {
    fn new() -> Self {
        Self {
            epoch: Instant::now(),
            first_sentence_us: AtomicU64::new(u64::MAX),
            llm_done_us: AtomicU64::new(u64::MAX),
        }
    }

    fn elapsed_us(&self) -> u64 {
        self.epoch.elapsed().as_micros().min(u64::MAX as u128) as u64
    }

    fn sentence_ready(&self) {
        let _ = self.first_sentence_us.compare_exchange(
            u64::MAX,
            self.elapsed_us(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }

    fn report(&self, token: &TurnToken, success: bool) {
        let millis = |value| {
            if value == u64::MAX {
                -1.0
            } else {
                value as f64 / 1000.0
            }
        };
        eprintln!(
            "voice_pipeline turn={} result={} first_sentence_ms={:.1} llm_done_ms={:.1} elapsed_ms={:.1}",
            token.id(),
            if success { "complete" } else { "aborted" },
            millis(self.first_sentence_us.load(Ordering::Relaxed)),
            millis(self.llm_done_us.load(Ordering::Relaxed)),
            self.epoch.elapsed().as_secs_f64() * 1000.0,
        );
    }
}

pub(super) fn run(
    llm: &mut dyn LlmPort,
    tts: &mut dyn TtsPort,
    playback: &mut dyn PlaybackPort,
    transcript: &str,
    token: &TurnToken,
    mut status: VoiceStatus,
    report: &mut dyn FnMut(VoiceStatus),
) -> Result<()> {
    token.check()?;
    let work_token = token.linked();
    let failure: Failure = Arc::new(Mutex::new(None));
    let timing = Arc::new(PipelineTiming::new());
    let (sentences_tx, sentences_rx) = mpsc::sync_channel(SENTENCE_QUEUE);
    let (audio_tx, audio_rx) = mpsc::sync_channel(PCM_QUEUE);
    let (events_tx, events_rx) = mpsc::channel();
    let mut received_final = false;
    let mut speaking = false;

    thread::scope(|scope| {
        let llm_token = work_token.clone();
        let llm_failure = failure.clone();
        let llm_events = events_tx.clone();
        let llm_timing = timing.clone();
        scope.spawn(move || {
            stage("LLM", &llm_token, &llm_failure, || {
                produce_sentences(
                    llm,
                    transcript,
                    &llm_token,
                    &sentences_tx,
                    &llm_events,
                    &llm_timing,
                )
            });
        });

        let tts_token = work_token.clone();
        let tts_failure = failure.clone();
        let tts_events = events_tx.clone();
        scope.spawn(move || {
            stage("TTS", &tts_token, &tts_failure, || {
                synthesize_sentences(tts, &tts_token, &sentences_rx, &audio_tx, &tts_events)
            });
        });

        let playback_token = work_token.clone();
        let playback_failure = failure.clone();
        let playback_events = events_tx.clone();
        scope.spawn(move || {
            stage("播放", &playback_token, &playback_failure, || {
                let mut announced = false;
                playback.play_stream(
                    &mut || {
                        let item = poll_audio(&audio_rx, &playback_token)?;
                        if matches!(&item, PlaybackItem::Audio(_)) && !announced {
                            announced = true;
                            let _ = playback_events.send(Event::Speaking);
                        }
                        Ok(item)
                    },
                    &playback_token,
                )
            });
        });
        drop(events_tx);

        // All events are consumed on the caller's thread. Keeping `speaking`
        // latched avoids later text/synthesis events resetting the phase.
        loop {
            if !token.is_current() {
                work_token.abort();
            }
            match events_rx.recv_timeout(QUEUE_POLL) {
                Ok(event) if work_token.is_current() => {
                    let changed = match event {
                        Event::Delta(delta) => {
                            status.response.push_str(&delta);
                            !delta.is_empty()
                        }
                        Event::Final(response, sources) => {
                            status.response = response;
                            status.sources = sources;
                            received_final = true;
                            true
                        }
                        Event::Synthesizing if !speaking => {
                            let changed = status.phase != "synthesizing";
                            status.phase = "synthesizing".into();
                            changed
                        }
                        Event::Speaking => {
                            speaking = true;
                            status.phase = "speaking".into();
                            true
                        }
                        Event::Synthesizing => false,
                    };
                    if changed {
                        report(status.clone());
                    }
                }
                Ok(_) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    let error = failure.lock().unwrap().take();
    timing.report(
        token,
        error.is_none() && token.is_current() && received_final,
    );
    if let Some(error) = error {
        return Err(error);
    }
    token.check()?;
    if !received_final {
        bail!("LLM 未返回可朗读正文");
    }
    status.phase = "idle".into();
    report(status);
    Ok(())
}

fn stage(name: &str, token: &TurnToken, failure: &Failure, work: impl FnOnce() -> Result<()>) {
    let result = catch_unwind(AssertUnwindSafe(work))
        .unwrap_or_else(|_| Err(anyhow!("语音 {name} 工作线程异常")));
    if let Err(error) = result {
        // Store the originating failure before siblings observe cancellation.
        let mut first = failure.lock().unwrap();
        if first.is_none() {
            *first = Some(error);
        }
        token.abort();
    }
}

fn produce_sentences(
    llm: &mut dyn LlmPort,
    transcript: &str,
    token: &TurnToken,
    sentences: &SyncSender<String>,
    events: &Sender<Event>,
    timing: &PipelineTiming,
) -> Result<()> {
    let mut segmenter = SpeechSegmenter::new(40, 80);
    let mut emitted = String::new();
    let mut sentence_count = 0;
    let mut callback_error = None;
    let result = llm.reply(transcript, token, &mut |delta| {
        if callback_error.is_some() || !token.is_current() {
            return;
        }
        emitted.push_str(delta);
        let _ = events.send(Event::Delta(delta.to_owned()));
        for sentence in segmenter.push(delta) {
            timing.sentence_ready();
            if let Err(error) = send(sentences, sentence, token) {
                callback_error = Some(error);
                token.abort();
                break;
            }
            sentence_count += 1;
        }
    });
    if let Some(error) = callback_error {
        return Err(error);
    }
    let response = result?;
    timing
        .llm_done_us
        .store(timing.elapsed_us(), Ordering::Relaxed);
    token.check()?;
    if response.trim().is_empty() {
        bail!("LLM 未返回可朗读正文");
    }
    let _ = events.send(Event::Final(response.clone(), llm.take_sources()));

    // A nonstreaming provider remains supported, as does a provider that sends
    // only a prefix and includes its remaining body in the final result.
    if let Some(tail) = response.strip_prefix(&emitted) {
        for sentence in segmenter.push(tail) {
            timing.sentence_ready();
            send(sentences, sentence, token)?;
            sentence_count += 1;
        }
    }
    for sentence in segmenter.finish() {
        timing.sentence_ready();
        send(sentences, sentence, token)?;
        sentence_count += 1;
    }
    if sentence_count == 0 {
        bail!("LLM 未返回可朗读正文");
    }
    token.check()
}

fn synthesize_sentences(
    tts: &mut dyn TtsPort,
    token: &TurnToken,
    sentences: &Receiver<String>,
    audio: &SyncSender<Pcm>,
    events: &Sender<Event>,
) -> Result<()> {
    let mut sentence_index = 0;
    while let Some(sentence) = receive(sentences, token)? {
        sentence_index += 1;
        let _ = events.send(Event::Synthesizing);
        let mut chunks = 0;
        let mut audio_ms = 0.0;
        let mut enqueue_time = Duration::ZERO;
        let synthesis_started = Instant::now();
        let result = tts.synthesize(&sentence, token, &mut |pcm| {
            token.check()?;
            if pcm.samples.is_empty() || pcm.sample_rate == 0 {
                bail!("TTS 输出了无效音频块");
            }
            chunks += 1;
            audio_ms += pcm.samples.len() as f64 * 1000.0 / pcm.sample_rate as f64;
            let frame_samples = ((pcm.sample_rate as usize * PCM_FRAME_MS) / 1000).max(1);
            let enqueue_started = Instant::now();
            let result = (|| {
                for samples in pcm.samples.chunks(frame_samples) {
                    send(
                        audio,
                        Pcm {
                            samples: samples.to_vec(),
                            sample_rate: pcm.sample_rate,
                        },
                        token,
                    )?;
                }
                Ok(())
            })();
            enqueue_time += enqueue_started.elapsed();
            result
        });
        let synthesis_ms = synthesis_started
            .elapsed()
            .saturating_sub(enqueue_time)
            .as_secs_f64()
            * 1000.0;
        eprintln!(
            "voice_tts turn={} sentence={} result={} char_count={} synthesize_ms={:.1} audio_ms={:.1} rtf={:.3} enqueue_ms={:.1}",
            token.id(),
            sentence_index,
            if result.is_ok() && chunks > 0 {
                "complete"
            } else {
                "aborted"
            },
            sentence.chars().count(),
            synthesis_ms,
            audio_ms,
            if audio_ms > 0.0 {
                synthesis_ms / audio_ms
            } else {
                -1.0
            },
            enqueue_time.as_secs_f64() * 1000.0,
        );
        result?;
        token.check()?;
        if chunks == 0 {
            bail!("TTS 未输出音频");
        }
    }
    token.check()
}

fn send<T>(sender: &SyncSender<T>, mut value: T, token: &TurnToken) -> Result<()> {
    loop {
        token.check()?;
        match sender.try_send(value) {
            Ok(()) => return Ok(()),
            Err(TrySendError::Full(returned)) => value = returned,
            Err(TrySendError::Disconnected(_)) => bail!("语音流水线队列已关闭"),
        }
        thread::sleep(QUEUE_POLL);
    }
}

fn receive<T>(receiver: &Receiver<T>, token: &TurnToken) -> Result<Option<T>> {
    loop {
        token.check()?;
        match receiver.recv_timeout(QUEUE_POLL) {
            Ok(value) => {
                token.check()?;
                return Ok(Some(value));
            }
            Err(RecvTimeoutError::Disconnected) => return Ok(None),
            Err(RecvTimeoutError::Timeout) => {}
        }
    }
}

fn poll_audio(receiver: &Receiver<Pcm>, token: &TurnToken) -> Result<PlaybackItem> {
    token.check()?;
    match receiver.try_recv() {
        Ok(pcm) => {
            token.check()?;
            Ok(PlaybackItem::Audio(pcm))
        }
        Err(TryRecvError::Empty) => Ok(PlaybackItem::Pending),
        Err(TryRecvError::Disconnected) => Ok(PlaybackItem::End),
    }
}

/// Holds incomplete sentences and source markers across arbitrary SSE tokens.
/// A period is deferred for one character so `3.` + `14` stays one number.
pub(super) struct SpeechSegmenter {
    current: String,
    source_marker: String,
    first_limit: usize,
    later_limit: usize,
    emitted: bool,
}

impl SpeechSegmenter {
    pub(super) fn new(first_limit: usize, later_limit: usize) -> Self {
        Self {
            current: String::new(),
            source_marker: String::new(),
            first_limit: first_limit.max(1),
            later_limit: later_limit.max(1),
            emitted: false,
        }
    }

    pub(super) fn push(&mut self, delta: &str) -> Vec<String> {
        let mut output = Vec::new();
        for ch in delta.chars() {
            if self.source_marker.is_empty() {
                if ch == '[' {
                    self.source_marker.push(ch);
                } else {
                    self.body_char(ch, &mut output);
                }
                continue;
            }
            let is_prefix = self.source_marker == "[" && ch == 'S';
            let is_digit = self.source_marker.starts_with("[S") && ch.is_ascii_digit();
            if is_prefix || (is_digit && self.source_marker.len() < 32) {
                self.source_marker.push(ch);
            } else if ch == ']'
                && self.source_marker.len() > 2
                && self.source_marker.starts_with("[S")
            {
                self.source_marker.clear();
            } else {
                let literal = std::mem::take(&mut self.source_marker);
                for held in literal.chars() {
                    self.body_char(held, &mut output);
                }
                if ch == '[' {
                    self.source_marker.push(ch);
                } else {
                    self.body_char(ch, &mut output);
                }
            }
        }
        output
    }

    pub(super) fn finish(&mut self) -> Vec<String> {
        let mut output = Vec::new();
        let literal = std::mem::take(&mut self.source_marker);
        for ch in literal.chars() {
            self.body_char(ch, &mut output);
        }
        self.flush(&mut output);
        output
    }

    fn body_char(&mut self, ch: char, output: &mut Vec<String>) {
        if self.current.ends_with('.') {
            let before_period = self.current.chars().rev().nth(1);
            let decimal = before_period.is_some_and(|previous| previous.is_ascii_digit())
                && ch.is_ascii_digit();
            if !decimal && ch != '.' {
                self.flush(output);
            }
        }
        // Do not spend the sentence cap on leading whitespace.
        if self.current.is_empty() && ch.is_whitespace() {
            return;
        }
        self.current.push(ch);
        let limit = if self.emitted {
            self.later_limit
        } else {
            self.first_limit
        };
        if "。！？；!?\n".contains(ch) || self.current.chars().count() >= limit {
            self.flush(output);
        }
    }

    fn flush(&mut self, output: &mut Vec<String>) {
        let segment = self.current.trim();
        if segment.chars().any(char::is_alphanumeric) {
            output.push(segment.to_owned());
            self.emitted = true;
        }
        self.current.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

    fn token() -> TurnToken {
        TurnToken::new(Arc::new(AtomicU64::new(1)), 1)
    }

    fn run_ports(
        llm: &mut dyn LlmPort,
        tts: &mut dyn TtsPort,
        playback: &mut dyn PlaybackPort,
        token: &TurnToken,
        statuses: &mut Vec<VoiceStatus>,
    ) -> Result<()> {
        run(
            llm,
            tts,
            playback,
            "问题",
            token,
            VoiceStatus {
                phase: "thinking".into(),
                transcript: "问题".into(),
                session_id: token.session_id(),
                turn_id: token.id(),
                ..Default::default()
            },
            &mut |status| statuses.push(status),
        )
    }

    struct StreamingLlm {
        playback_started: Receiver<()>,
        finished: Arc<AtomicBool>,
    }
    impl LlmPort for StreamingLlm {
        fn reply(
            &mut self,
            _: &str,
            token: &TurnToken,
            on_delta: &mut dyn FnMut(&str),
        ) -> Result<String> {
            on_delta("第一句。");
            // Failure to play before the final LLM reply is a test failure,
            // with a bounded timeout instead of a hanging regression.
            self.playback_started.recv_timeout(Duration::from_secs(2))?;
            token.check()?;
            on_delta("第二句。");
            self.finished.store(true, Ordering::Release);
            Ok("第一句。第二句。".into())
        }
    }

    struct OrderedTts {
        started_second: Sender<()>,
        count: usize,
        spoken: Arc<Mutex<Vec<String>>>,
    }
    impl TtsPort for OrderedTts {
        fn synthesize(
            &mut self,
            text: &str,
            token: &TurnToken,
            on_audio: &mut dyn FnMut(Pcm) -> Result<()>,
        ) -> Result<()> {
            token.check()?;
            self.count += 1;
            self.spoken.lock().unwrap().push(text.to_owned());
            if self.count == 2 {
                self.started_second.send(())?;
            }
            on_audio(Pcm {
                samples: vec![self.count as f32],
                sample_rate: 1000,
            })
        }
    }

    struct OverlapPlayback {
        playback_started: Sender<()>,
        second_synthesis: Receiver<()>,
        llm_finished: Arc<AtomicBool>,
        played: Vec<f32>,
        stream_calls: usize,
    }
    impl PlaybackPort for OverlapPlayback {
        fn play(&mut self, pcm: &Pcm, token: &TurnToken) -> Result<()> {
            token.check()?;
            if self.played.is_empty() {
                assert!(!self.llm_finished.load(Ordering::Acquire));
                self.playback_started.send(())?;
                // Sentence two must synthesize while sentence one is playing.
                self.second_synthesis.recv_timeout(Duration::from_secs(2))?;
            }
            self.played.extend(&pcm.samples);
            Ok(())
        }
        fn play_stream(
            &mut self,
            next: &mut dyn FnMut() -> Result<PlaybackItem>,
            token: &TurnToken,
        ) -> Result<()> {
            self.stream_calls += 1;
            loop {
                token.check()?;
                match next()? {
                    PlaybackItem::Audio(pcm) => self.play(&pcm, token)?,
                    PlaybackItem::Pending => thread::sleep(Duration::from_millis(2)),
                    PlaybackItem::End => return token.check(),
                }
            }
        }
    }

    #[test]
    fn first_audio_precedes_llm_end_and_next_synthesis_overlaps_playback() {
        let (played_tx, played_rx) = mpsc::channel();
        let (second_tx, second_rx) = mpsc::channel();
        let finished = Arc::new(AtomicBool::new(false));
        let spoken = Arc::new(Mutex::new(Vec::new()));
        let mut llm = StreamingLlm {
            playback_started: played_rx,
            finished: finished.clone(),
        };
        let mut tts = OrderedTts {
            started_second: second_tx,
            count: 0,
            spoken: spoken.clone(),
        };
        let mut playback = OverlapPlayback {
            playback_started: played_tx,
            second_synthesis: second_rx,
            llm_finished: finished,
            played: Vec::new(),
            stream_calls: 0,
        };
        let mut statuses = Vec::new();
        run_ports(&mut llm, &mut tts, &mut playback, &token(), &mut statuses).unwrap();
        assert_eq!(playback.played, [1.0, 2.0]);
        assert_eq!(playback.stream_calls, 1);
        assert_eq!(*spoken.lock().unwrap(), ["第一句。", "第二句。"]);
        assert_eq!(statuses.last().unwrap().response, "第一句。第二句。");
        assert_eq!(statuses.last().unwrap().phase, "idle");
        let speaking = statuses
            .iter()
            .position(|status| status.phase == "speaking")
            .unwrap();
        assert!(
            statuses[speaking..statuses.len() - 1]
                .iter()
                .all(|status| status.phase == "speaking")
        );
    }

    struct BodyLlm {
        deltas: Vec<String>,
        final_body: String,
    }
    impl LlmPort for BodyLlm {
        fn take_sources(&mut self) -> Vec<Source> {
            vec![Source {
                id: "S12".into(),
                title: "来源".into(),
                url: "https://example.com".into(),
                snippet: String::new(),
                retrieved_at: 0,
            }]
        }
        fn reply(
            &mut self,
            _: &str,
            token: &TurnToken,
            on_delta: &mut dyn FnMut(&str),
        ) -> Result<String> {
            for delta in &self.deltas {
                token.check()?;
                on_delta(delta);
            }
            Ok(self.final_body.clone())
        }
    }

    #[derive(Default)]
    struct TextTts(Vec<String>);
    impl TtsPort for TextTts {
        fn synthesize(
            &mut self,
            text: &str,
            token: &TurnToken,
            on_audio: &mut dyn FnMut(Pcm) -> Result<()>,
        ) -> Result<()> {
            token.check()?;
            self.0.push(text.into());
            on_audio(Pcm {
                samples: vec![0.1],
                sample_rate: 1000,
            })
        }
    }

    struct NullPlayback;
    impl PlaybackPort for NullPlayback {
        fn play(&mut self, _: &Pcm, token: &TurnToken) -> Result<()> {
            token.check()
        }
    }

    #[test]
    fn no_delta_and_final_suffix_do_not_lose_or_duplicate_speech() {
        for deltas in [
            vec![],
            vec!["正文。[".into(), "S12]".into()],
            vec!["正文。[S12]尾巴".into()],
        ] {
            let mut llm = BodyLlm {
                deltas,
                final_body: "正文。[S12]尾巴".into(),
            };
            let mut tts = TextTts::default();
            let mut statuses = Vec::new();
            run_ports(
                &mut llm,
                &mut tts,
                &mut NullPlayback,
                &token(),
                &mut statuses,
            )
            .unwrap();
            assert_eq!(tts.0, ["正文。", "尾巴"]);
            let last = statuses.last().unwrap();
            assert_eq!(last.response, "正文。[S12]尾巴");
            assert_eq!(last.sources[0].id, "S12");
        }
    }

    #[test]
    fn source_only_body_is_rejected_without_calling_tts() {
        let mut llm = BodyLlm {
            deltas: vec!["[S12]".into()],
            final_body: "[S12]".into(),
        };
        let mut tts = TextTts::default();
        let mut statuses = Vec::new();
        let error = run_ports(
            &mut llm,
            &mut tts,
            &mut NullPlayback,
            &token(),
            &mut statuses,
        )
        .unwrap_err();
        assert!(error.to_string().contains("未返回可朗读正文"));
        assert!(tts.0.is_empty());
        assert!(!statuses.iter().any(|status| status.phase == "idle"));
    }

    struct FloodLlm {
        fourth_delta: Sender<()>,
        produced: Arc<AtomicUsize>,
    }
    impl LlmPort for FloodLlm {
        fn reply(
            &mut self,
            _: &str,
            token: &TurnToken,
            on_delta: &mut dyn FnMut(&str),
        ) -> Result<String> {
            for index in 0..100 {
                token.check()?;
                self.produced.fetch_add(1, Ordering::AcqRel);
                if index == 3 {
                    self.fourth_delta.send(())?;
                }
                on_delta("句子。");
            }
            Ok("句子。".repeat(100))
        }
    }

    struct FailingTts(Receiver<()>);
    impl TtsPort for FailingTts {
        fn synthesize(
            &mut self,
            _: &str,
            _: &TurnToken,
            _: &mut dyn FnMut(Pcm) -> Result<()>,
        ) -> Result<()> {
            // First sentence is active, two are queued and the fourth is
            // entering its send: fail with the LLM under backpressure.
            self.0.recv_timeout(Duration::from_secs(2))?;
            bail!("injected TTS failure")
        }
    }

    #[test]
    fn tts_failure_unblocks_a_full_sentence_queue_and_preserves_root_error() {
        let (fourth_tx, fourth_rx) = mpsc::channel();
        let produced = Arc::new(AtomicUsize::new(0));
        let mut llm = FloodLlm {
            fourth_delta: fourth_tx,
            produced: produced.clone(),
        };
        let mut tts = FailingTts(fourth_rx);
        let external = token();
        let mut statuses = Vec::new();
        let error = run_ports(
            &mut llm,
            &mut tts,
            &mut NullPlayback,
            &external,
            &mut statuses,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "injected TTS failure");
        assert!(
            external.is_current(),
            "internal failure must preserve the caller's generation"
        );
        assert!(produced.load(Ordering::Acquire) <= 4);
        assert!(!statuses.iter().any(|status| status.phase == "idle"));
    }

    struct LongTts;
    impl TtsPort for LongTts {
        fn synthesize(
            &mut self,
            _: &str,
            token: &TurnToken,
            on_audio: &mut dyn FnMut(Pcm) -> Result<()>,
        ) -> Result<()> {
            token.check()?;
            // A real local TTS emits a full sentence, then bounded framing
            // must stop it feeding an arbitrarily large playback backlog.
            on_audio(Pcm {
                samples: vec![0.1; 20_000],
                sample_rate: 1000,
            })
        }
    }

    struct FailingPlayback {
        fourth_delta: Receiver<()>,
        cancel_generation: Option<Arc<AtomicU64>>,
    }
    impl PlaybackPort for FailingPlayback {
        fn play(&mut self, _: &Pcm, token: &TurnToken) -> Result<()> {
            self.fourth_delta.recv_timeout(Duration::from_secs(2))?;
            if let Some(generation) = &self.cancel_generation {
                generation.store(2, Ordering::Release);
                token.check()
            } else {
                bail!("injected playback failure")
            }
        }
    }

    #[test]
    fn playback_failure_and_external_cancel_stop_backpressured_workers() {
        for cancel in [false, true] {
            let (fourth_tx, fourth_rx) = mpsc::channel();
            let produced = Arc::new(AtomicUsize::new(0));
            let mut llm = FloodLlm {
                fourth_delta: fourth_tx,
                produced: produced.clone(),
            };
            let mut tts = LongTts;
            let generation = Arc::new(AtomicU64::new(1));
            let external = TurnToken::new(generation.clone(), 1);
            let mut playback = FailingPlayback {
                fourth_delta: fourth_rx,
                cancel_generation: cancel.then_some(generation),
            };
            let mut statuses = Vec::new();
            let error =
                run_ports(&mut llm, &mut tts, &mut playback, &external, &mut statuses).unwrap_err();
            assert_eq!(
                error.to_string(),
                if cancel {
                    "voice turn cancelled"
                } else {
                    "injected playback failure"
                }
            );
            assert_eq!(external.is_current(), !cancel);
            assert!(produced.load(Ordering::Acquire) <= 4);
            assert!(!statuses.iter().any(|status| status.phase == "idle"));
        }
    }

    struct FailingLlm;
    impl LlmPort for FailingLlm {
        fn reply(&mut self, _: &str, _: &TurnToken, _: &mut dyn FnMut(&str)) -> Result<String> {
            bail!("injected LLM failure")
        }
    }

    struct NoAudioTts;
    impl TtsPort for NoAudioTts {
        fn synthesize(
            &mut self,
            _: &str,
            _: &TurnToken,
            _: &mut dyn FnMut(Pcm) -> Result<()>,
        ) -> Result<()> {
            Ok(())
        }
    }

    struct WaitingLlm;
    impl LlmPort for WaitingLlm {
        fn reply(
            &mut self,
            _: &str,
            token: &TurnToken,
            on_delta: &mut dyn FnMut(&str),
        ) -> Result<String> {
            on_delta("第一句。");
            let deadline = Instant::now() + Duration::from_secs(2);
            while token.is_current() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(2));
            }
            token.check()?;
            bail!("LLM 等待期间没有收到设备故障取消")
        }
    }

    struct FaultWhilePending;
    impl PlaybackPort for FaultWhilePending {
        fn play(&mut self, _: &Pcm, _: &TurnToken) -> Result<()> {
            unreachable!("streaming device must use its continuous output path")
        }

        fn play_stream(
            &mut self,
            next: &mut dyn FnMut() -> Result<PlaybackItem>,
            token: &TurnToken,
        ) -> Result<()> {
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut received_audio = false;
            loop {
                token.check()?;
                if Instant::now() >= deadline {
                    bail!("播放设备未能在等待 PCM 时检查故障");
                }
                match next()? {
                    PlaybackItem::Audio(_) => received_audio = true,
                    PlaybackItem::Pending if received_audio => {
                        bail!("injected pending device failure")
                    }
                    PlaybackItem::Pending => thread::sleep(Duration::from_millis(2)),
                    PlaybackItem::End => bail!("播放设备检查故障前流水线已结束"),
                }
            }
        }
    }

    #[test]
    fn device_failure_while_waiting_for_pcm_cancels_upstream_immediately() {
        let external = token();
        let mut statuses = Vec::new();
        let started = Instant::now();
        let error = run_ports(
            &mut WaitingLlm,
            &mut TextTts::default(),
            &mut FaultWhilePending,
            &external,
            &mut statuses,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "injected pending device failure");
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(external.is_current());
        assert!(!statuses.iter().any(|status| status.phase == "idle"));
    }

    #[test]
    fn audio_poll_distinguishes_pending_audio_end_and_cancellation() {
        let (sender, receiver) = mpsc::sync_channel(1);
        let generation = Arc::new(AtomicU64::new(1));
        let token = TurnToken::new(generation.clone(), 1);
        assert!(matches!(
            poll_audio(&receiver, &token).unwrap(),
            PlaybackItem::Pending
        ));
        sender
            .send(Pcm {
                samples: vec![0.1],
                sample_rate: 1000,
            })
            .unwrap();
        drop(sender);
        assert!(matches!(
            poll_audio(&receiver, &token).unwrap(),
            PlaybackItem::Audio(_)
        ));
        assert!(matches!(
            poll_audio(&receiver, &token).unwrap(),
            PlaybackItem::End
        ));
        generation.store(2, Ordering::Release);
        assert!(poll_audio(&receiver, &token).is_err());
    }

    #[test]
    fn llm_failure_and_empty_tts_do_not_report_success() {
        let mut statuses = Vec::new();
        let error = run_ports(
            &mut FailingLlm,
            &mut TextTts::default(),
            &mut NullPlayback,
            &token(),
            &mut statuses,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "injected LLM failure");
        assert!(!statuses.iter().any(|status| status.phase == "idle"));

        let mut llm = BodyLlm {
            deltas: vec!["回答。".into()],
            final_body: "回答。".into(),
        };
        let mut statuses = Vec::new();
        let error = run_ports(
            &mut llm,
            &mut NoAudioTts,
            &mut NullPlayback,
            &token(),
            &mut statuses,
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "TTS 未输出音频");
        assert!(!statuses.iter().any(|status| status.phase == "idle"));
    }

    #[test]
    fn full_audio_queue_send_is_cancellable_and_retains_only_its_capacity() {
        let (sender, receiver) = mpsc::sync_channel(PCM_QUEUE);
        for _ in 0..PCM_QUEUE {
            sender
                .try_send(Pcm {
                    samples: vec![0.1; 250],
                    sample_rate: 1000,
                })
                .unwrap();
        }
        let generation = Arc::new(AtomicU64::new(1));
        let token = TurnToken::new(generation.clone(), 1);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (done_tx, done_rx) = mpsc::channel();
        thread::scope(|scope| {
            scope.spawn(move || {
                entered_tx.send(()).unwrap();
                let result = send(
                    &sender,
                    Pcm {
                        samples: vec![0.1; 250],
                        sample_rate: 1000,
                    },
                    &token,
                );
                done_tx.send(result).unwrap();
            });
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            generation.store(2, Ordering::Release);
            assert!(
                done_rx
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap()
                    .is_err()
            );
        });
        assert_eq!(receiver.try_iter().count(), PCM_QUEUE);
    }

    #[test]
    fn source_markers_and_decimals_survive_every_delta_boundary() {
        let body = "圆周率是 3.14。[S12] Next sentence!结尾[S2]";
        let expected = ["圆周率是 3.14。", "Next sentence!", "结尾"];
        for boundary in body
            .char_indices()
            .map(|(index, _)| index)
            .chain([body.len()])
        {
            let mut segmenter = SpeechSegmenter::new(40, 80);
            let mut segments = segmenter.push(&body[..boundary]);
            segments.extend(segmenter.push(&body[boundary..]));
            segments.extend(segmenter.finish());
            assert_eq!(segments, expected, "split at byte {boundary}");
        }
        let mut segmenter = SpeechSegmenter::new(40, 80);
        let mut segments = Vec::new();
        for ch in body.chars() {
            segments.extend(segmenter.push(&ch.to_string()));
        }
        segments.extend(segmenter.finish());
        assert_eq!(segments, expected);
    }

    #[test]
    fn unfinished_source_lookalikes_remain_literal_and_tail_is_flushed_once() {
        let mut segmenter = SpeechSegmenter::new(40, 80);
        assert!(segmenter.push("读取 [Sabc] 与 [S").is_empty());
        assert_eq!(segmenter.finish(), ["读取 [Sabc] 与 [S"]);
        assert!(segmenter.finish().is_empty());
    }

    #[test]
    fn long_unpunctuated_text_caps_first_segment_and_preserves_unicode() {
        let body = "甲".repeat(170);
        let mut segmenter = SpeechSegmenter::new(40, 80);
        let mut segments = segmenter.push(&body);
        segments.extend(segmenter.finish());
        assert_eq!(
            segments
                .iter()
                .map(|text| text.chars().count())
                .collect::<Vec<_>>(),
            [40, 80, 50]
        );
        assert_eq!(segments.concat(), body);
    }
}
