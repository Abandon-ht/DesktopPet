//! Geometry and lifecycle policy for a permission-gated AX window attachment.
//! Native discovery and notifications are supplied by the platform adapter.
use crate::snap::Rect;
const SNAP_DISTANCE: f64 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub pid: i32,
    pub window: u64,
    pub bounds: Rect,
    pub minimized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Attachment {
    pid: i32,
    window: u64,
    horizontal: f64,
}

#[derive(Default)]
pub struct ExternalSnap {
    attachment: Option<Attachment>,
}

impl ExternalSnap {
    pub fn attached(&self) -> bool {
        self.attachment.is_some()
    }

    pub fn detach(&mut self) -> bool {
        self.attachment.take().is_some()
    }

    /// On drag release, accept a nearby foot or perch line, then seat the pet.
    /// Native discovery supplies accessible candidates; this policy also rejects
    /// stale authorization and our own process.
    pub fn release(
        &mut self,
        pet: Rect,
        anchor_ratio: f64,
        perch_ratio: f64,
        trusted: bool,
        own_pid: i32,
        candidates: &[Target],
    ) -> Option<Rect> {
        self.detach();
        if !trusted
            || !valid(pet)
            || !(0.0..=1.0).contains(&anchor_ratio)
            || !(0.2..=0.8).contains(&perch_ratio)
        {
            return None;
        }
        let foot = pet.y + pet.height * anchor_ratio;
        let perch = pet.y + pet.height * perch_ratio;
        let best = candidates
            .iter()
            .filter(|target| {
                valid(target.bounds) && !target.minimized && target.pid > 0 && target.pid != own_pid
            })
            .filter_map(|target| {
                let target_bounds = target.bounds;
                let distance = (foot - target_bounds.y)
                    .abs()
                    .min((perch - target_bounds.y).abs());
                let overlap = pet.x < target_bounds.x + target_bounds.width
                    && pet.x + pet.width > target_bounds.x;
                (distance <= SNAP_DISTANCE && overlap).then_some((target, distance))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))?
            .0;
        let bounds = best.bounds;
        let x = pet
            .x
            .clamp(bounds.x, bounds.x + (bounds.width - pet.width).max(0.0));
        self.attachment = Some(Attachment {
            pid: best.pid,
            window: best.window,
            horizontal: ((x - bounds.x) / (bounds.width - pet.width).max(1.0)).clamp(0.0, 1.0),
        });
        Some(Rect {
            x,
            y: bounds.y - pet.height * perch_ratio,
            ..pet
        })
    }

    /// Called with a fresh observation. Revocation, minimize, destruction or a
    /// missing target immediately drops the attachment; callers then keep the
    /// pet within its ordinary screen work area.
    pub fn follow(
        &mut self,
        pet: Rect,
        perch_ratio: f64,
        trusted: bool,
        target: Option<Target>,
    ) -> Option<Rect> {
        let attached = self.attachment?;
        let Some(target) = target else {
            self.detach();
            return None;
        };
        if !trusted
            || target.pid != attached.pid
            || target.window != attached.window
            || target.minimized
            || !valid(target.bounds)
            || !valid(pet)
            || !(0.2..=0.8).contains(&perch_ratio)
        {
            self.detach();
            return None;
        }
        let bounds = target.bounds;
        Some(Rect {
            x: bounds.x + attached.horizontal * (bounds.width - pet.width).max(0.0),
            y: bounds.y - pet.height * perch_ratio,
            ..pet
        })
    }
}

fn valid(rect: Rect) -> bool {
    [rect.x, rect.y, rect.width, rect.height]
        .iter()
        .all(|v| v.is_finite())
        && rect.width > 0.0
        && rect.height > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;
    const PET: Rect = Rect {
        x: 220.0,
        y: 24.0,
        width: 100.0,
        height: 100.0,
    };
    fn target() -> Target {
        Target {
            pid: 42,
            window: 7,
            bounds: Rect {
                x: 100.0,
                y: 120.0,
                width: 500.0,
                height: 400.0,
            },
            minimized: false,
        }
    }
    #[test]
    fn release_near_top_and_follow_move_and_resize() {
        let mut snap = ExternalSnap::default();
        let attached = snap.release(PET, 0.96, 0.5, true, 99, &[target()]).unwrap();
        assert_eq!(attached.y + 50.0, 120.0);
        assert!(snap.attached());
        let mut moved = target();
        moved.bounds.x = -300.0;
        moved.bounds.y = -100.0;
        moved.bounds.width = 700.0;
        let followed = snap.follow(PET, 0.5, true, Some(moved)).unwrap();
        assert_eq!(followed.y + 50.0, -100.0);
        assert!(followed.x >= -300.0);
    }
    #[test]
    fn release_at_perch_line_keeps_configured_overlap() {
        let mut snap = ExternalSnap::default();
        let pet = Rect { y: 70.0, ..PET };
        let seated = snap.release(pet, 0.96, 0.5, true, 99, &[target()]).unwrap();
        assert_eq!(seated.y, 70.0);
        let deeper = snap.follow(seated, 0.3, true, Some(target())).unwrap();
        assert_eq!(deeper.y, 90.0);
    }
    #[test]
    fn release_with_small_visual_gap_still_seats_on_window() {
        let mut snap = ExternalSnap::default();
        let pet = Rect { y: 5.0, ..PET };
        let seated = snap.release(pet, 0.96, 0.5, true, 99, &[target()]).unwrap();
        assert_eq!(seated.y + 50.0, target().bounds.y);
    }
    #[test]
    fn rejection_does_not_attach() {
        let mut snap = ExternalSnap::default();
        assert!(
            snap.release(PET, 0.96, 0.5, false, 99, &[target()])
                .is_none()
        );
        assert!(
            snap.release(PET, 0.96, 0.5, true, 42, &[target()])
                .is_none()
        );
        let mut t = target();
        t.bounds.y = 300.0;
        assert!(snap.release(PET, 0.96, 0.5, true, 99, &[t]).is_none());
        t.bounds.y = 120.0;
        t.minimized = true;
        assert!(snap.release(PET, 0.96, 0.5, true, 99, &[t]).is_none());
        t.minimized = false;
        t.bounds.x = 1000.0;
        assert!(snap.release(PET, 0.96, 0.5, true, 99, &[t]).is_none());
    }
    #[test]
    fn target_loss_minimize_and_permission_revoke_detach() {
        let mut snap = ExternalSnap::default();
        for next in [
            None,
            Some(Target {
                minimized: true,
                ..target()
            }),
            Some(target()),
        ] {
            snap.release(PET, 0.96, 0.5, true, 99, &[target()]).unwrap();
            assert!(snap.follow(PET, 0.5, false, next).is_none());
            assert!(!snap.attached());
        }
        snap.release(PET, 0.96, 0.5, true, 99, &[target()]).unwrap();
        assert!(snap.follow(PET, 0.5, true, None).is_none());
        assert!(!snap.detach());
    }
    #[test]
    fn another_window_cannot_reuse_attachment() {
        let mut snap = ExternalSnap::default();
        snap.release(PET, 0.96, 0.5, true, 99, &[target()]).unwrap();
        let another = Target {
            window: 8,
            ..target()
        };
        assert!(snap.follow(PET, 0.5, true, Some(another)).is_none());
        assert!(!snap.attached());
    }
}
