//! Logical-coordinate coarse regions retained from the P0 probe for P1-02, not an asset silhouette.
#[derive(Default)]
pub struct Input {
    pub receiving: bool,
    pub dragging: bool,
}

impl Input {
    pub fn sample(&mut self, x: f64, y: f64, width: f64, height: f64, down: bool) -> bool {
        if !down {
            self.dragging = false;
        }
        let margin = if self.receiving { 6. } else { 0. };
        let head = ((x - width * 0.5) / (width * 0.22 + margin)).powi(2)
            + ((y - height * 0.28) / (height * 0.22 + margin)).powi(2)
            <= 1.;
        let body = (x - width * 0.5).abs() <= width * 0.16 + margin
            && y >= height * 0.42 - margin
            && y <= height * 0.9 + margin;
        // Do not intercept a drag that started in another application.
        self.receiving = self.dragging || ((!down || self.receiving) && (head || body));
        self.receiving
    }

    pub fn sample_region(&mut self, hit: bool, down: bool) -> bool {
        if !down {
            self.dragging = false;
        }
        self.receiving = self.dragging || ((!down || self.receiving) && hit);
        self.receiving
    }
    pub fn press(&mut self) {
        self.dragging = self.receiving;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reentry_drag_lock_and_release() {
        let mut input = Input::default();
        assert!(!input.sample(0., 0., 500., 600., false));
        assert!(input.sample(250., 168., 500., 600., false));
        input.press();
        assert!(input.sample(-100., -100., 500., 600., true));
        assert!(!input.sample(-100., -100., 500., 600., false));
    }
    #[test]
    fn external_drag_is_not_captured() {
        let mut input = Input::default();
        assert!(!input.sample(250., 168., 500., 600., true));
        assert!(input.sample(250., 168., 500., 600., false));
    }
    #[test]
    fn hysteresis_retains_near_edge() {
        let mut input = Input::default();
        assert!(input.sample(250., 168., 500., 600., false));
        assert!(input.sample(364., 168., 500., 600., false));
        assert!(!input.sample(370., 168., 500., 600., false));
    }
}

#[cfg(test)]
mod p1_tests {
    use super::*;
    #[test]
    fn hundred_sequences_preserve_passthrough_and_drag_ownership() {
        let mut input = Input::default();
        for _ in 0..100 {
            assert!(!input.sample_region(false, false));
            assert!(!input.sample_region(true, true)); // A press originating elsewhere is not ours.
            assert!(input.sample_region(true, false));
            input.press();
            assert!(input.sample_region(false, true));
            assert!(input.dragging);
            assert!(!input.sample_region(false, false));
            assert!(!input.dragging);
        }
    }
}
