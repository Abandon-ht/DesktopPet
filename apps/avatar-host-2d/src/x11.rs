//! X11 boundaries. Called only on winit's event-loop thread with a live window.
//! Coordinates use root pixels divided by the window's X11 scale factor. This
//! first baseline does not claim per-monitor mixed-DPI support.
use crate::{placement::Screen, snap::Rect};
use anyhow::{Context, Result, ensure};
use std::{ffi::CString, sync::OnceLock};
use winit::{
    raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle},
    window::Window,
};
use x11_dl::{xfixes, xlib, xrender};

/// XFWM reparents even undecorated windows into an input-receiving frame. An
/// empty client input shape alone therefore does not implement click-through.
#[derive(Default)]
pub struct InputShape {
    applied: Option<(Vec<xlib::Window>, bool)>,
}
impl InputShape {
    pub fn invalidate(&mut self) {
        self.applied = None;
    }
    pub fn sync(&mut self, window: &Window, receiving: bool) -> Result<()> {
        static FIXES: OnceLock<Result<xfixes::Xlib, String>> = OnceLock::new();
        let fixes = FIXES
            .get_or_init(|| xfixes::Xlib::open().map_err(|e| e.to_string()))
            .as_ref()
            .map_err(|e| anyhow::anyhow!("XFixes unavailable: {e}"))?;
        let n = native(window)?;
        let mut chain = vec![n.xid];
        for _ in 0..8 {
            let (mut root, mut parent, mut count) = (0, 0, 0);
            let mut children = std::ptr::null_mut();
            let ok = unsafe {
                (n.api.XQueryTree)(
                    n.display,
                    *chain.last().unwrap(),
                    &mut root,
                    &mut parent,
                    &mut children,
                    &mut count,
                )
            };
            if !children.is_null() {
                unsafe {
                    (n.api.XFree)(children.cast());
                }
            }
            ensure!(ok != 0, "X11 input frame unavailable");
            if parent == root || parent == 0 {
                break;
            }
            chain.push(parent);
        }
        if self
            .applied
            .as_ref()
            .is_some_and(|(old, value)| old == &chain && *value == receiving)
        {
            return Ok(());
        }
        let (mut event, mut error) = (0, 0);
        ensure!(
            unsafe { (fixes.XFixesQueryExtension)(n.display, &mut event, &mut error) } != 0,
            "XFixes extension required for X11 input"
        );
        unsafe {
            // None restores the default input shape; an empty region passes
            // through. Modify only this pet's client and its WM ancestors.
            let region = if receiving {
                0
            } else {
                (fixes.XFixesCreateRegion)(n.display, std::ptr::null_mut(), 0)
            };
            for xid in &chain {
                (fixes.XFixesSetWindowShapeRegion)(
                    n.display, *xid, 2, /* ShapeInput */
                    0, 0, region,
                );
            }
            if region != 0 {
                (fixes.XFixesDestroyRegion)(n.display, region);
            }
            (n.api.XFlush)(n.display);
        }
        pet_ipc::event_log!(
            "{}",
            serde_json::json!({"event":"x11_input_shape","windows":chain,"receiving":receiving})
        );
        self.applied = Some((chain, receiving));
        Ok(())
    }
}

struct Native<'a> {
    api: &'static xlib::Xlib,
    display: *mut xlib::Display,
    xid: xlib::Window,
    // Keep the borrowed handle owner alive for every native call.
    _window: &'a Window,
}
impl Drop for Native<'_> {
    fn drop(&mut self) {
        // Matched with XLockDisplay in native(); no calls outlive the window.
        unsafe {
            (self.api.XUnlockDisplay)(self.display);
        }
    }
}
fn native(window: &Window) -> Result<Native<'_>> {
    static API: OnceLock<Result<xlib::Xlib, String>> = OnceLock::new();
    let api = API
        .get_or_init(|| xlib::Xlib::open().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| anyhow::anyhow!("Xlib unavailable: {e}"))?;
    let RawDisplayHandle::Xlib(display) = window.display_handle()?.as_raw() else {
        anyhow::bail!("Linux desktop integration requires native X11");
    };
    let RawWindowHandle::Xlib(handle) = window.window_handle()?.as_raw() else {
        anyhow::bail!("Xlib window required");
    };
    let display = display
        .display
        .context("missing Xlib display")?
        .as_ptr()
        .cast();
    // winit initializes Xlib threading; lock its shared display for this query.
    unsafe {
        (api.XLockDisplay)(display);
    }
    Ok(Native {
        api,
        display,
        xid: handle.window,
        _window: window,
    })
}
fn attributes(n: &Native<'_>) -> Result<xlib::XWindowAttributes> {
    let mut attributes = std::mem::MaybeUninit::uninit();
    // XGetWindowAttributes initializes the output exactly on a nonzero return.
    ensure!(
        unsafe { (n.api.XGetWindowAttributes)(n.display, n.xid, attributes.as_mut_ptr()) } != 0,
        "X11 window attributes unavailable"
    );
    Ok(unsafe { attributes.assume_init() })
}
fn atom(n: &Native<'_>, name: &std::ffi::CStr) -> xlib::Atom {
    unsafe { (n.api.XInternAtom)(n.display, name.as_ptr(), xlib::True) }
}
fn cardinal(n: &Native<'_>, root: xlib::Window, name: &std::ffi::CStr) -> Vec<u32> {
    let property = atom(n, name);
    if property == 0 {
        return Vec::new();
    }
    let (mut actual, mut format, mut count, mut after) = (0, 0, 0, 0);
    let mut data = std::ptr::null_mut();
    // Xlib returns each format-32 value in an unsigned long, including on LP64.
    let status = unsafe {
        (n.api.XGetWindowProperty)(
            n.display,
            root,
            property,
            0,
            256,
            xlib::False,
            xlib::XA_CARDINAL,
            &mut actual,
            &mut format,
            &mut count,
            &mut after,
            &mut data,
        )
    };
    let values = if status == 0
        && actual == xlib::XA_CARDINAL
        && format == 32
        && count <= 256
        && after == 0
        && !data.is_null()
    {
        unsafe { std::slice::from_raw_parts(data.cast::<std::ffi::c_ulong>(), count as usize) }
            .iter()
            .map(|v| *v as u32)
            .collect()
    } else {
        Vec::new()
    };
    if !data.is_null() {
        unsafe {
            (n.api.XFree)(data.cast());
        }
    }
    values
}

pub fn pointer(window: &Window) -> Option<(f64, f64, bool)> {
    let n = native(window).ok()?;
    let (mut root, mut child, mut rx, mut ry, mut x, mut y, mut mask) = (0, 0, 0, 0, 0, 0, 0);
    let same_screen = unsafe {
        (n.api.XQueryPointer)(
            n.display, n.xid, &mut root, &mut child, &mut rx, &mut ry, &mut x, &mut y, &mut mask,
        )
    };
    if same_screen == 0 {
        return None;
    }
    let scale = window.scale_factor();
    Some((
        f64::from(x) / scale,
        f64::from(y) / scale,
        mask & xlib::Button1Mask != 0,
    ))
}

/// winit's with_active(false) is unsupported on X11. Declare an ICCCM window
/// that receives pointer events without requesting keyboard focus from the WM.
pub fn pointer_only(window: &Window) -> Result<()> {
    let n = native(window)?;
    unsafe {
        let existing = (n.api.XGetWMHints)(n.display, n.xid);
        let mut hints = if existing.is_null() {
            std::mem::zeroed::<xlib::XWMHints>()
        } else {
            let value = *existing;
            (n.api.XFree)(existing.cast());
            value
        };
        hints.flags |= xlib::InputHint;
        hints.input = xlib::False;
        (n.api.XSetWMHints)(n.display, n.xid, &mut hints);
        (n.api.XFlush)(n.display);
    }
    Ok(())
}

pub fn alpha_visual_with_compositor(window: &Window) -> Result<bool> {
    let n = native(window)?;
    let attributes = attributes(&n)?;
    let screen = unsafe { (n.api.XScreenNumberOfScreen)(attributes.screen) };
    let name = CString::new(format!("_NET_WM_CM_S{screen}"))?;
    let selection = atom(&n, &name);
    let owner = if selection == 0 {
        0
    } else {
        unsafe { (n.api.XGetSelectionOwner)(n.display, selection) }
    };
    let render = xrender::Xrender::open()?;
    let format = unsafe { (render.XRenderFindVisualFormat)(n.display, attributes.visual) };
    let alpha = !format.is_null() && unsafe { (*format).direct.alphaMask != 0 };
    pet_ipc::event_log!(
        "{}",
        serde_json::json!({"event":"x11_alpha_evidence","depth":attributes.depth,"visual_alpha":alpha,"compositor_owner":owner})
    );
    Ok(attributes.depth == 32 && alpha && owner != 0)
}

fn work_area(values: &[u32], desktop: usize, monitor: Rect, scale: f64) -> Rect {
    let Some(v) = desktop
        .checked_mul(4)
        .and_then(|offset| values.get(offset..offset.checked_add(4)?))
    else {
        return monitor;
    };
    let x = f64::from(v[0] as i32) / scale;
    let y = f64::from(v[1] as i32) / scale;
    let right = (x + f64::from(v[2]) / scale).min(monitor.x + monitor.width);
    let bottom = (y + f64::from(v[3]) / scale).min(monitor.y + monitor.height);
    let x = x.max(monitor.x);
    let y = y.max(monitor.y);
    if right <= x || bottom <= y {
        return monitor;
    }
    Rect {
        x,
        y,
        width: right - x,
        height: bottom - y,
    }
}

pub fn desktop(window: &Window) -> Result<(Rect, Vec<Screen>, String)> {
    let (position, size, work, current_desktop) = {
        let n = native(window)?;
        let attributes = attributes(&n)?;
        let (mut x, mut y, mut child) = (0, 0, 0);
        ensure!(
            unsafe {
                (n.api.XTranslateCoordinates)(
                    n.display,
                    n.xid,
                    attributes.root,
                    0,
                    0,
                    &mut x,
                    &mut y,
                    &mut child,
                )
            } != 0,
            "X11 root coordinates unavailable"
        );
        (
            (x, y),
            (attributes.width, attributes.height),
            cardinal(&n, attributes.root, c"_NET_WORKAREA"),
            cardinal(&n, attributes.root, c"_NET_CURRENT_DESKTOP")
                .first()
                .copied()
                .unwrap_or(0) as usize,
        )
    }; // Unlock before asking winit about monitors on the shared connection.
    let scale = window.scale_factor();
    let rect = Rect {
        x: f64::from(position.0) / scale,
        y: f64::from(position.1) / scale,
        width: f64::from(size.0) / scale,
        height: f64::from(size.1) / scale,
    };
    let screens: Vec<_> = window
        .available_monitors()
        .map(|m| {
            let p = m.position();
            let s = m.size();
            let monitor = Rect {
                x: f64::from(p.x) / scale,
                y: f64::from(p.y) / scale,
                width: f64::from(s.width) / scale,
                height: f64::from(s.height) / scale,
            };
            Screen {
                id: m.name().unwrap_or_else(|| format!("X11-{},{}", p.x, p.y)),
                work: work_area(&work, current_desktop, monitor, scale),
                scale: m.scale_factor(),
            }
        })
        .collect();
    ensure!(!screens.is_empty(), "X11 has no monitors");
    let current = screens
        .iter()
        .find(|s| {
            rect.x + rect.width / 2. >= s.work.x
                && rect.x + rect.width / 2. < s.work.x + s.work.width
                && rect.y + rect.height / 2. >= s.work.y
                && rect.y + rect.height / 2. < s.work.y + s.work.height
        })
        .unwrap_or(&screens[0])
        .id
        .clone();
    Ok((rect, screens, current))
}

pub fn move_to(window: &Window, target: Rect) -> Result<()> {
    // Validate the backend and release the Xlib lock before calling winit.
    ensure!(
        [target.x, target.y]
            .iter()
            .all(|v| v.is_finite() && v.abs() < f64::from(i32::MAX) / window.scale_factor()),
        "invalid X11 window origin"
    );
    {
        let n = native(window)?;
        let mut hints = unsafe { std::mem::zeroed::<xlib::XSizeHints>() };
        let mut supplied = 0;
        unsafe {
            (n.api.XGetWMNormalHints)(n.display, n.xid, &mut hints, &mut supplied);
            // An explicit saved/user position must survive XFWM's initial smart
            // placement when the initially hidden window is first mapped.
            hints.flags |= xlib::USPosition;
            hints.x = (target.x * window.scale_factor()).round() as i32;
            hints.y = (target.y * window.scale_factor()).round() as i32;
            (n.api.XSetWMNormalHints)(n.display, n.xid, &mut hints);
            (n.api.XFlush)(n.display);
        }
    }
    window.set_outer_position(winit::dpi::LogicalPosition::new(target.x, target.y));
    Ok(())
}
pub fn snap_floor(window: &Window, anchor_ratio: f64) -> Result<()> {
    let (before, screens, current) = desktop(window)?;
    let screen = screens
        .iter()
        .find(|s| s.id == current)
        .context("missing X11 screen")?;
    let target = crate::snap::floor(before, screen.work, before.height * anchor_ratio);
    if let Some(target) = target {
        move_to(window, target)?;
    }
    pet_ipc::event_log!(
        "{}",
        serde_json::json!({"event":"screen_snap","screen":screen.id,"work_area":[screen.work.x,screen.work.y,screen.work.width,screen.work.height],"before":[before.x,before.y],"requested":target.map(|r|[r.x,r.y]),"snapped":target.is_some(),"anchor_ratio":anchor_ratio})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ewmh_workarea_selects_desktop_intersects_and_handles_bad_data() {
        let monitor = Rect {
            x: -960.,
            y: 0.,
            width: 960.,
            height: 540.,
        };
        let data = [
            (-1920i32) as u32,
            24,
            3840,
            1056,
            (-1920i32) as u32,
            40,
            3840,
            1000,
        ];
        assert_eq!(
            work_area(&data, 1, monitor, 2.),
            Rect {
                x: -960.,
                y: 20.,
                width: 960.,
                height: 500.
            }
        );
        for (data, desktop) in [
            (&data[..3], 0),
            (&data[..], usize::MAX),
            (&[0, 0, 0, 0][..], 0),
            (&[0, 0, 1920, 1080][..], 0),
        ] {
            assert_eq!(work_area(data, desktop, monitor, 2.), monitor);
        }
    }
}
