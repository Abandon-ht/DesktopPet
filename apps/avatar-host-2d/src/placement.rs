//! Desktop coordinates are top-left logical points; DPI is only used at boundaries.
use crate::snap::Rect;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub id: String,
    pub work: Rect,
    pub scale: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Saved {
    pub version: u32,
    pub monitor: String,
    pub x: f64,
    pub y: f64,
    pub floor: bool,
}
impl Saved {
    pub fn valid(&self) -> bool {
        self.version == 1
            && !self.monitor.is_empty()
            && self.monitor.len() <= 128
            && [self.x, self.y]
                .iter()
                .all(|n| n.is_finite() && (0.0..=1.0).contains(n))
    }
    pub fn capture(window: Rect, screen: &Screen, anchor: f64) -> Self {
        let work = screen.work;
        let x = (window.x - work.x) / (work.width - window.width).max(1.0);
        let y = (window.y - work.y) / (work.height - window.height).max(1.0);
        Self {
            version: 1,
            monitor: screen.id.clone(),
            x: x.clamp(0.0, 1.0),
            y: y.clamp(0.0, 1.0),
            floor: (window.y + window.height * anchor - work.y - work.height).abs() <= 2.0,
        }
    }
    pub fn restore(&self, screens: &[Screen], size: [f64; 2], anchor: f64) -> Option<Rect> {
        if !self.valid()
            || !size.iter().all(|v| v.is_finite() && *v > 0.0)
            || !anchor.is_finite()
            || !(0.0..=1.0).contains(&anchor)
        {
            return None;
        }
        let screen = screens
            .iter()
            .find(|s| s.id == self.monitor)
            .or(screens.first())?;
        let work = screen.work;
        Some(Rect {
            x: work.x + self.x * (work.width - size[0]).max(0.0),
            y: if self.floor {
                work.y + work.height - size[1] * anchor
            } else {
                work.y + self.y * (work.height - size[1]).max(0.0)
            },
            width: size[0],
            height: size[1],
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn screens() -> Vec<Screen> {
        vec![
            Screen {
                id: "primary".into(),
                work: Rect {
                    x: 0.0,
                    y: 30.0,
                    width: 1440.0,
                    height: 850.0,
                },
                scale: 2.0,
            },
            Screen {
                id: "external".into(),
                work: Rect {
                    x: -1920.0,
                    y: -1080.0,
                    width: 1920.0,
                    height: 1040.0,
                },
                scale: 1.0,
            },
        ]
    }
    #[test]
    fn negative_monitor_roundtrip_and_mixed_dpi_do_not_change_logical_position() {
        let screens = screens();
        let original = Rect {
            x: -1500.0,
            y: -900.0,
            width: 500.0,
            height: 600.0,
        };
        let saved = Saved::capture(original, &screens[1], 0.96);
        assert_eq!(
            saved.restore(&screens, [500.0, 600.0], 0.96),
            Some(original)
        );
        let mut changed = screens.clone();
        changed[1].scale = 2.0;
        assert_eq!(
            saved.restore(&changed, [500.0, 600.0], 0.96),
            Some(original)
        );
    }
    #[test]
    fn unplug_falls_back_to_primary_and_scale_preserves_floor_anchor() {
        let screens = screens();
        let original = Rect {
            x: -1400.0,
            y: -616.0,
            width: 500.0,
            height: 600.0,
        };
        let saved = Saved::capture(original, &screens[1], 0.96);
        assert!(saved.floor);
        let restored = saved.restore(&screens[..1], [250.0, 300.0], 0.96).unwrap();
        assert!(restored.x >= 0.0);
        assert_eq!(restored.y + 300.0 * 0.96, 880.0);
    }
    #[test]
    fn corrupt_and_missing_displays_are_safe() {
        let mut saved = Saved {
            version: 1,
            monitor: "gone".into(),
            x: 0.5,
            y: 0.5,
            floor: false,
        };
        assert!(saved.restore(&[], [500.0, 600.0], 0.96).is_none());
        saved.x = f64::NAN;
        assert!(saved.restore(&screens(), [500.0, 600.0], 0.96).is_none());
    }
    #[test]
    fn oversized_window_keeps_top_left_accessible() {
        let saved = Saved {
            version: 1,
            monitor: "primary".into(),
            x: 1.0,
            y: 1.0,
            floor: false,
        };
        let restored = saved.restore(&screens(), [2000.0, 1200.0], 0.96).unwrap();
        assert_eq!([restored.x, restored.y], [0.0, 30.0]);
    }
}
