/// Read trust status only: never requests permission or changes system settings.
#[cfg(target_os = "macos")]
pub fn ax_trusted() -> Option<bool> {
    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    // The system function takes no pointers and has no ownership requirements.
    Some(unsafe { AXIsProcessTrusted() != 0 })
}

/// Native screen floor snap. Call only after an owned drag release on the main
/// thread; all positions remain AppKit logical points, including mixed DPI.
#[cfg(target_os = "macos")]
pub fn snap_floor(window: &winit::window::Window, anchor_ratio: f64) -> anyhow::Result<()> {
    use anyhow::Context;
    use objc2_app_kit::{NSScreen, NSView};
    use objc2_foundation::{MainThreadMarker, NSPoint};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let mtm = MainThreadMarker::new().context("screen snap requires main thread")?;
    let RawWindowHandle::AppKit(handle) = window.window_handle()?.as_raw() else {
        anyhow::bail!("AppKit window required");
    };
    // winit owns the live view. This function runs on its event-loop thread.
    unsafe {
        let native = (&*handle.ns_view.as_ptr().cast::<NSView>())
            .window()
            .context("missing NSWindow")?;
        let screen = native.screen().context("window has no screen")?;
        let screens = NSScreen::screens(mtm);
        let primary = screens.iter().next().context("no screens")?.frame();
        let top = primary.origin.y + primary.size.height;
        let convert = |r: objc2_foundation::NSRect| {
            crate::snap::Rect {
                x: r.origin.x,
                y: r.origin.y,
                width: r.size.width,
                height: r.size.height,
            }
            .flip_y(top)
        };
        let before = convert(native.frame());
        // Requery for each release: visibleFrame may change with Dock settings.
        let work = convert(screen.visibleFrame());
        let target = crate::snap::floor(before, work, before.height * anchor_ratio);
        if let Some(target) = target {
            let origin = target.flip_y(top);
            native.setFrameOrigin(NSPoint::new(origin.x, origin.y));
        }
        let after = convert(native.frame());
        p0_ipc_probe::event_log!(
            "{}",
            serde_json::json!({"event":"screen_snap","screen":screen.localizedName().to_string(),"scale":screen.backingScaleFactor(),"work_area":[work.x,work.y,work.width,work.height],"before":[before.x,before.y],"after":[after.x,after.y],"anchor_ratio":anchor_ratio,"snapped":target.is_some(),"ax_trusted":ax_trusted()})
        );
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
pub fn snap_floor(_: &winit::window::Window, _: f64) -> anyhow::Result<()> {
    anyhow::bail!("screen snap is currently implemented only for macOS")
}

/// AppKit points are independent of whether this window receives mouse events.
#[cfg(target_os = "macos")]
pub fn pointer(window: &winit::window::Window) -> Option<(f64, f64, bool)> {
    use objc2_app_kit::{NSEvent, NSView};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // Called on the event-loop thread while winit owns the live view/window.
    unsafe {
        let view = &*handle.ns_view.as_ptr().cast::<NSView>();
        let native = view.window()?;
        let point = native.mouseLocationOutsideOfEventStream();
        let height = window.inner_size().height as f64 / window.scale_factor();
        Some((
            point.x,
            height - point.y,
            NSEvent::pressedMouseButtons() & 1 != 0,
        ))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn pointer(_: &winit::window::Window) -> Option<(f64, f64, bool)> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn ax_trusted() -> Option<bool> {
    None
}
