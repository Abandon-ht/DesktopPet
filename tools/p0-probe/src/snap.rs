//! Pure logical-point geometry; AppKit uses bottom-left origins at the boundary.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn flip_y(self, primary_top: f64) -> Self {
        Self {
            y: primary_top - self.y - self.height,
            ..self
        }
    }
}

/// Snap the neutral model's lower anchor to the work-area floor on release.
/// A distant release does not move the pet. Dock/menu margins come from AppKit.
pub fn floor(window: Rect, work: Rect, anchor_y: f64) -> Option<Rect> {
    if ![
        window.x,
        window.y,
        window.width,
        window.height,
        work.x,
        work.y,
        work.width,
        work.height,
        anchor_y,
    ]
    .iter()
    .all(|v| v.is_finite())
        || window.width <= 0.
        || window.height <= 0.
        || work.width <= 0.
        || work.height <= 0.
        || !(0. ..=window.height).contains(&anchor_y)
    {
        return None;
    }
    let floor = work.y + work.height;
    if (window.y + anchor_y - floor).abs() > 12. {
        return None;
    }
    let x = window
        .x
        .clamp(work.x, work.x + (work.width - window.width).max(0.));
    Some(Rect {
        x,
        y: floor - anchor_y,
        ..window
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const WORK: Rect = Rect {
        x: -1920.,
        y: -1080.,
        width: 1920.,
        height: 1040.,
    };
    #[test]
    fn coordinates_round_trip_with_negative_monitor_origin() {
        let native = Rect {
            x: -1920.,
            y: 1080.,
            width: 1920.,
            height: 1080.,
        };
        assert_eq!(native.flip_y(1080.).y, -1080.);
        assert_eq!(native.flip_y(1080.).flip_y(1080.), native);
    }
    #[test]
    fn anchor_not_transparent_window_bottom_touches_floor() {
        let window = Rect {
            x: -1500.,
            y: -612.,
            width: 500.,
            height: 600.,
        };
        let snapped = floor(window, WORK, 580.).unwrap();
        assert_eq!(snapped.y + 580., -40.);
        assert_ne!(snapped.y + snapped.height, -40.);
    }
    #[test]
    fn distant_release_and_invalid_geometry_do_not_move() {
        let window = Rect {
            x: -1500.,
            y: -700.,
            width: 500.,
            height: 600.,
        };
        assert!(floor(window, WORK, 580.).is_none());
        assert!(floor(window, WORK, f64::NAN).is_none());
    }
    #[test]
    fn right_edge_clamps_in_logical_points() {
        let window = Rect {
            x: -10.,
            y: -620.,
            width: 500.,
            height: 600.,
        };
        assert_eq!(floor(window, WORK, 580.).unwrap().x, -500.);
    }
}
