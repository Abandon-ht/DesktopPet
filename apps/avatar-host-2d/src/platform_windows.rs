//! Win32 boundary for the per-monitor DPI-aware winit event-loop thread.
use crate::{placement::Screen, snap::Rect};
use anyhow::{Context, Result, ensure};
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MONITORINFO, ScreenToClient},
    UI::{
        Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON},
        WindowsAndMessaging::{
            GetCursorPos, GetWindowRect, MONITORINFOF_PRIMARY, SWP_NOACTIVATE, SWP_NOSIZE,
            SWP_NOZORDER, SetWindowPos,
        },
    },
};
use winit::{
    platform::windows::MonitorHandleExtWindows,
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

fn hwnd(window: &Window) -> Result<HWND> {
    let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
        anyhow::bail!("Win32 window required");
    };
    Ok(handle.hwnd.get() as HWND)
}

fn logical(rect: RECT, scale: f64) -> Rect {
    Rect {
        x: f64::from(rect.left) / scale,
        y: f64::from(rect.top) / scale,
        width: (f64::from(rect.right) - f64::from(rect.left)) / scale,
        height: (f64::from(rect.bottom) - f64::from(rect.top)) / scale,
    }
}

pub fn pointer(window: &Window) -> Option<(f64, f64, bool)> {
    let native = hwnd(window).ok()?;
    let mut point = POINT { x: 0, y: 0 };
    // winit owns this HWND; both calls run on its event-loop thread. The pointer
    // can be sampled even while the entire window ignores cursor events.
    unsafe {
        if GetCursorPos(&mut point) == 0 || ScreenToClient(native, &mut point) == 0 {
            // The input desktop may be unavailable during lock/UAC. The caller
            // pauses sampling; this must not kill the renderer or its heartbeat.
            return None;
        }
        Some((
            f64::from(point.x) / window.scale_factor(),
            f64::from(point.y) / window.scale_factor(),
            GetAsyncKeyState(i32::from(VK_LBUTTON)) < 0,
        ))
    }
}

pub fn desktop(window: &Window) -> Result<(Rect, Vec<Screen>, String)> {
    let mut rect = RECT::default();
    // Convert *all* rectangles using one scale, that of the current window.
    // Dividing each monitor's absolute origin by its own DPI would introduce
    // gaps/overlaps in a mixed-DPI desktop. Normalized placement is independent
    // of this snapshot's scale, and move_to applies its inverse.
    let scale = window.scale_factor();
    ensure!(
        unsafe { GetWindowRect(hwnd(window)?, &mut rect) } != 0,
        "GetWindowRect: {}",
        std::io::Error::last_os_error()
    );
    let mut screens = Vec::new();
    let mut primary = None;
    for monitor in window.available_monitors() {
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        ensure!(
            unsafe { GetMonitorInfoW(monitor.hmonitor() as _, &mut info) } != 0,
            "GetMonitorInfoW: {}",
            std::io::Error::last_os_error()
        );
        if info.dwFlags & MONITORINFOF_PRIMARY != 0 {
            primary = Some(screens.len());
        }
        screens.push(Screen {
            id: monitor.native_id(),
            work: logical(info.rcWork, scale),
            scale: monitor.scale_factor(),
        });
    }
    ensure!(!screens.is_empty(), "no Windows monitors available");
    if let Some(primary) = primary {
        screens.swap(0, primary);
    }
    let current = window
        .current_monitor()
        .map(|m| m.native_id())
        .unwrap_or_else(|| screens[0].id.clone());
    Ok((logical(rect, scale), screens, current))
}

pub fn move_to(window: &Window, target: Rect) -> Result<()> {
    let scale = window.scale_factor();
    let [x, y] = [target.x * scale, target.y * scale];
    ensure!(
        [x, y]
            .iter()
            .all(|v| v.is_finite() && *v >= f64::from(i32::MIN) && *v <= f64::from(i32::MAX)),
        "invalid Windows position"
    );
    // Preserve size, z-order and keyboard focus. HWND is live on this thread.
    ensure!(
        unsafe {
            SetWindowPos(
                hwnd(window)?,
                std::ptr::null_mut(),
                x.round() as i32,
                y.round() as i32,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        } != 0,
        "SetWindowPos: {}",
        std::io::Error::last_os_error()
    );
    Ok(())
}

pub fn snap_floor(window: &Window, anchor_ratio: f64) -> Result<()> {
    let (before, screens, id) = desktop(window)?;
    let screen = screens
        .iter()
        .find(|s| s.id == id)
        .context("window has no monitor")?;
    let target = crate::snap::floor(before, screen.work, before.height * anchor_ratio);
    if let Some(target) = target {
        move_to(window, target)?;
    }
    let (after, _, _) = desktop(window)?;
    pet_ipc::event_log!(
        "{}",
        serde_json::json!({"event":"screen_snap", "screen":id,"scale":window.scale_factor(),"work_area":[screen.work.x,screen.work.y,screen.work.width,screen.work.height],"before":[before.x,before.y],"after":[after.x,after.y],"anchor_ratio":anchor_ratio,"snapped":target.is_some()})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_dpi_snapshot_keeps_shared_edges_and_negative_origins() {
        let left = RECT {
            left: -1920,
            top: -200,
            right: 0,
            bottom: 880,
        };
        let right = RECT {
            left: 0,
            top: 0,
            right: 2560,
            bottom: 1400,
        };
        for scale in [1.0, 1.25, 1.5, 2.0] {
            let a = logical(left, scale);
            let b = logical(right, scale);
            assert_eq!(a.x + a.width, b.x);
            assert_eq!((a.x * scale).round(), -1920.0);
            let original = Rect {
                x: a.x + 100.0 / scale,
                y: a.y + 50.0 / scale,
                width: 500.0 / scale,
                height: 600.0 / scale,
            };
            let screen = Screen {
                id: "left".into(),
                work: a,
                scale: 1.0,
            };
            let saved = crate::placement::Saved::capture(original, &screen, 0.96);
            let restored = saved
                .restore(&[screen], [original.width, original.height], 0.96)
                .unwrap();
            assert!((restored.x - original.x).abs() < 1e-9);
            assert!((restored.y - original.y).abs() < 1e-9);
        }
    }
}
