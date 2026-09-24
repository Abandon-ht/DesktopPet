//! Bounded CPU-submit and presentation-call interval samples; never GPU timing.
use std::{collections::VecDeque, time::Instant};
#[derive(Default)]
pub struct Metrics {
    samples: VecDeque<(f64, Option<f64>)>,
    last: Option<Instant>,
    count: u64,
}
impl Metrics {
    pub fn presented(&mut self, started: Instant) {
        let now = Instant::now();
        let interval = self
            .last
            .replace(now)
            .map(|last| now.duration_since(last).as_secs_f64() * 1000.0);
        if self.samples.len() == 1800 {
            self.samples.pop_front();
        }
        self.samples
            .push_back((now.duration_since(started).as_secs_f64() * 1000.0, interval));
        self.count += 1;
    }
    pub fn pause(&mut self) {
        self.last = None;
    }
    pub fn report(&self) -> serde_json::Value {
        fn p95(mut samples: Vec<f64>) -> Option<f64> {
            if samples.is_empty() {
                return None;
            }
            samples.sort_by(f64::total_cmp);
            Some(samples[(samples.len() * 95).div_ceil(100) - 1])
        }
        serde_json::json!({"presented_frames":self.count,"sample_window_max_frames":1800,
            "cpu_submit_ms_p95":p95(self.samples.iter().map(|(cpu,_)|*cpu).collect()),
            "present_interval_ms_p95":p95(self.samples.iter().filter_map(|(_,interval)|*interval).collect()),
            "scope":"CPU animation/render/submit/present-call duration and call intervals; not GPU completion or display scanout"})
    }
}
