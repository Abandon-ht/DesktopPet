//! Host-local procedural motion; elapsed time is injected for deterministic tests.
#[derive(Default)]
pub struct Animation {
    gaze: [f32; 2],
    clock: f32,
    interval: usize,
}
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub gaze: [f32; 2],
    pub eye_open: f32,
}
impl Animation {
    pub fn tick(&mut self, dt: f32, target: [f32; 2], dragging: bool) -> Pose {
        let dt = if dt.is_finite() {
            dt.clamp(0.0, 0.1)
        } else {
            0.0
        };
        let alpha = 1.0 - (-dt / 0.12).exp();
        for (axis, value) in self.gaze.iter_mut().zip(target) {
            let value = if value.is_finite() {
                value.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            *axis += (value * if dragging { 0.25 } else { 1.0 } - *axis) * alpha;
        }
        let intervals = [3.6, 4.8, 5.4, 4.2];
        self.clock += dt;
        let blink_start = intervals[self.interval];
        let t = self.clock - blink_start;
        let eye_open = if t < 0.0 {
            1.0
        } else if t < 0.09 {
            1.0 - t / 0.09
        } else if t < 0.15 {
            0.0
        } else {
            ((t - 0.15) / 0.14).min(1.0)
        };
        if t >= 0.29 {
            self.clock = 0.0;
            self.interval = (self.interval + 1) % intervals.len();
        }
        Pose {
            gaze: self.gaze,
            eye_open,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blink_closes_and_returns_to_open() {
        let mut motion = Animation::default();
        let mut closed = false;
        for _ in 0..500 {
            let pose = motion.tick(0.01, [0.0, 0.0], false);
            assert!((0.0..=1.0).contains(&pose.eye_open));
            closed |= pose.eye_open < 0.01;
        }
        assert!(closed);
        assert_eq!(motion.tick(0.01, [0.0, 0.0], false).eye_open, 1.0);
    }
    #[test]
    fn gaze_smooths_and_drag_limits_amplitude() {
        let mut motion = Animation::default();
        let first = motion.tick(1.0 / 30.0, [9.0, -9.0], false);
        assert!(first.gaze[0] > 0.0 && first.gaze[0] < 1.0);
        for _ in 0..100 {
            motion.tick(1.0 / 30.0, [9.0, -9.0], true);
        }
        let pose = motion.tick(1.0 / 30.0, [9.0, -9.0], true);
        assert!((pose.gaze[0] - 0.25).abs() < 0.001);
        assert!((pose.gaze[1] + 0.25).abs() < 0.001);
    }
    #[test]
    fn pause_and_invalid_values_never_jump_motion() {
        let mut motion = Animation::default();
        let a = motion.tick(0.0, [1.0, 1.0], false);
        let b = motion.tick(f32::NAN, [f32::NAN, f32::INFINITY], false);
        assert_eq!(a.gaze, b.gaze);
        let pose = motion.tick(3600.0, [1.0, 1.0], false);
        assert!(pose.gaze[0] < 0.6);
        assert_eq!(pose.eye_open, 1.0);
    }
}
