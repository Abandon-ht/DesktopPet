//! Explicit, native-verified compatibility for X11 alpha capability reporting.
use anyhow::{Result, bail};
use wgpu::{Backend, CompositeAlphaMode as Alpha};

pub fn select(
    supported: &[Alpha],
    backend: Backend,
    vendor: u32,
    verified_x11: bool,
    verified_gl_x11: bool,
) -> Result<Alpha> {
    for mode in [Alpha::PreMultiplied, Alpha::PostMultiplied] {
        if supported.contains(&mode) {
            return Ok(mode);
        }
    }
    // The caller only supplies true for an explicit opt-in, a live compositor,
    // and an XRender visual with alpha. Opaque alone is not transparency evidence.
    let nvidia_vulkan = verified_x11 && backend == Backend::Vulkan && vendor == 0x10de;
    // wgpu-hal GLES reports only Opaque. AX650's software GL path was verified
    // with an ARGB X11 visual and xfwm4; never enable it from Opaque alone.
    let native_gl = verified_gl_x11 && backend == Backend::Gl;
    if (nvidia_vulkan || native_gl) && supported.contains(&Alpha::Opaque) {
        return Ok(Alpha::Opaque);
    }
    bail!(
        "surface does not support transparent compositing; verified X11 compatibility requires an ARGB visual and an active compositor plus DESKTOPPET_X11_GL_ALPHA=1 for GL, or DESKTOPPET_X11_OPAQUE_ALPHA=1 for NVIDIA Vulkan"
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
                true,
                true
            )
            .unwrap(),
            Alpha::PreMultiplied
        );
        assert_eq!(
            select(&[Alpha::Opaque], Backend::Vulkan, 0x10de, true, false).unwrap(),
            Alpha::Opaque
        );
        for (backend, vendor, verified) in [
            (Backend::Vulkan, 0x10de, false),
            (Backend::Vulkan, 0x1002, true),
            (Backend::Gl, 0x10de, true),
            (Backend::Metal, 0x10de, true),
        ] {
            assert!(select(&[Alpha::Opaque], backend, vendor, verified, false).is_err());
        }
        assert!(select(&[], Backend::Vulkan, 0x10de, true, false).is_err());
    }

    #[test]
    fn gl_requires_its_own_native_evidence_and_preserves_normal_alpha() {
        assert_eq!(
            select(&[Alpha::Opaque], Backend::Gl, 0x10005, false, true).unwrap(),
            Alpha::Opaque
        );
        assert!(select(&[Alpha::Opaque], Backend::Gl, 0x10005, false, false).is_err());
        assert!(select(&[Alpha::Opaque], Backend::Vulkan, 0x10005, false, true).is_err());
        assert!(select(&[Alpha::Inherit], Backend::Gl, 0x10005, false, true).is_err());
        assert_eq!(
            select(
                &[Alpha::Opaque, Alpha::PostMultiplied],
                Backend::Gl,
                0x10005,
                false,
                true
            )
            .unwrap(),
            Alpha::PostMultiplied
        );
    }
}
