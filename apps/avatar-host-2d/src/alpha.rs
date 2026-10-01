//! Explicit compatibility policy for NVIDIA's X11 WSI alpha reporting.
use anyhow::{Result, bail};
use wgpu::{Backend, CompositeAlphaMode as Alpha};

pub fn select(
    supported: &[Alpha],
    backend: Backend,
    vendor: u32,
    verified_x11: bool,
) -> Result<Alpha> {
    for mode in [Alpha::PreMultiplied, Alpha::PostMultiplied] {
        if supported.contains(&mode) {
            return Ok(mode);
        }
    }
    // The caller only supplies true for an explicit opt-in, a live compositor,
    // and an XRender visual with alpha. Opaque alone is not transparency evidence.
    if verified_x11
        && backend == Backend::Vulkan
        && vendor == 0x10de
        && supported.contains(&Alpha::Opaque)
    {
        return Ok(Alpha::Opaque);
    }
    bail!(
        "surface does not support transparent compositing; NVIDIA X11 compatibility requires DESKTOPPET_X11_OPAQUE_ALPHA=1, an ARGB visual and an active compositor"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_alpha_is_preferred_and_opaque_requires_all_evidence() {
        assert_eq!(
            select(
                &[Alpha::Opaque, Alpha::PreMultiplied],
                Backend::Vulkan,
                0x10de,
                true
            )
            .unwrap(),
            Alpha::PreMultiplied
        );
        assert_eq!(
            select(&[Alpha::Opaque], Backend::Vulkan, 0x10de, true).unwrap(),
            Alpha::Opaque
        );
        for (backend, vendor, verified) in [
            (Backend::Vulkan, 0x10de, false),
            (Backend::Vulkan, 0x1002, true),
            (Backend::Gl, 0x10de, true),
            (Backend::Metal, 0x10de, true),
        ] {
            assert!(select(&[Alpha::Opaque], backend, vendor, verified).is_err());
        }
        assert!(select(&[], Backend::Vulkan, 0x10de, true).is_err());
    }
}
