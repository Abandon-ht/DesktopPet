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
