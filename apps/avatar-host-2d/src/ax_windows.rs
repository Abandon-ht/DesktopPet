//! Foreground Win32 window geometry and out-of-process WinEvent notifications.
//! Reads bounds only, with no window titles, contents or input injection.
use super::Target;
use crate::snap::Rect;
use std::{
    cell::Cell,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute},
    UI::{
        Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent},
        WindowsAndMessaging::*,
    },
};

thread_local! {
    static WATCHED: Cell<usize> = const { Cell::new(0) };
    static NOTIFICATION: Cell<Option<Instant>> = const { Cell::new(None) };
}

#[derive(Default)]
pub struct FocusedProbe {
    tracked: HWND,
    pid: i32,
}

impl FocusedProbe {
    pub fn sample(&mut self, own_pid: i32) -> Option<Target> {
        // This worker belongs to a winit per-monitor DPI-aware process. DWM
        // supplies physical desktop pixels; Probe converts at the read boundary.
        let foreground = unsafe { GetForegroundWindow() };
        if foreground.is_null() {
            return None;
        }
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(foreground, &mut pid);
        }
        let parent = std::env::var("DESKTOPPET_PARENT_PID")
            .ok()
            .and_then(|s| s.parse::<u32>().ok());
        if pid == own_pid as u32 {
            // Clicking / dragging the pet focuses its HWND. Retain the last
            // external candidate until another application takes foreground.
            return target(self.tracked, self.pid);
        }
        if Some(pid) == parent {
            self.tracked = std::ptr::null_mut();
            self.pid = 0;
            return None;
        }
        let Some(candidate) = target(foreground, pid as i32) else {
            self.tracked = std::ptr::null_mut();
            self.pid = 0;
            return None;
        };
        self.tracked = foreground;
        self.pid = pid as i32;
        Some(candidate)
    }
}

fn target(window: HWND, expected_pid: i32) -> Option<Target> {
    if window.is_null() || expected_pid <= 0 {
        return None;
    }
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        // Handle reuse is checked against process identity on every read.
        if pid != expected_pid as u32
            || IsWindow(window) == 0
            || IsWindowVisible(window) == 0
            || IsIconic(window) != 0
            || GetAncestor(window, GA_ROOT) != window
            || window == GetDesktopWindow()
            || window == GetShellWindow()
            || GetWindowLongPtrW(window, GWL_EXSTYLE) as u32 & (WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE)
                != 0
        {
            return None;
        }
        let mut cloaked = 0_u32;
        if DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED as u32,
            (&mut cloaked as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
        ) != 0
            || cloaked != 0
        {
            return None;
        }
        let mut bounds = RECT::default();
        if DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS as u32,
            (&mut bounds as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        ) != 0
        {
            return None;
        }
        let bounds = Rect {
            x: f64::from(bounds.left),
            y: f64::from(bounds.top),
            width: f64::from(bounds.right) - f64::from(bounds.left),
            height: f64::from(bounds.bottom) - f64::from(bounds.top),
        };
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }
        Some(Target {
            pid: expected_pid,
            window: window as usize as u64,
            bounds,
            minimized: false,
        })
    }
}

unsafe extern "system" fn changed(
    _: HWINEVENTHOOK,
    event: u32,
    window: HWND,
    object: i32,
    child: i32,
    _: u32,
    _: u32,
) {
    if window.is_null() || object != OBJID_WINDOW || child != 0 {
        return;
    }
    if !matches!(
        event,
        EVENT_OBJECT_LOCATIONCHANGE
            | EVENT_OBJECT_DESTROY
            | EVENT_OBJECT_HIDE
            | EVENT_OBJECT_SHOW
            | EVENT_SYSTEM_MINIMIZESTART
            | EVENT_SYSTEM_MINIMIZEEND
            | EVENT_SYSTEM_MOVESIZEEND
    ) {
        return;
    }
    if WATCHED.with(|tracked| tracked.get() == window as usize) {
        NOTIFICATION.with(|notification| {
            if notification.get().is_none() {
                notification.set(Some(Instant::now()));
            }
        });
    }
}

fn pump() {
    // Out-of-context callbacks are delivered on the installing worker thread.
    unsafe {
        let mut message = MSG::default();
        while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

#[derive(Default)]
pub struct WindowObserver {
    hook: HWINEVENTHOOK,
    window: usize,
}

impl WindowObserver {
    pub fn follow(&mut self, _: &FocusedProbe, target: Option<Target>) {
        let Some(target) = target else {
            self.clear();
            return;
        };
        if self.window == target.window as usize {
            return;
        }
        self.clear();
        self.window = target.window as usize;
        WATCHED.with(|tracked| tracked.set(self.window));
        self.hook = unsafe {
            SetWinEventHook(
                EVENT_MIN,
                EVENT_MAX,
                std::ptr::null_mut(),
                Some(changed),
                target.pid as u32,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if self.hook.is_null() {
            eprintln!(
                "WinEvent hook unavailable: {}; retaining 250 ms polling",
                std::io::Error::last_os_error()
            );
        }
    }
    pub fn clear(&mut self) {
        if !self.hook.is_null() {
            unsafe {
                UnhookWinEvent(self.hook);
            }
            self.hook = std::ptr::null_mut();
        }
        self.window = 0;
        WATCHED.with(|tracked| tracked.set(0));
        NOTIFICATION.with(|notification| notification.set(None));
    }
    pub fn notified(&self) -> bool {
        pump();
        NOTIFICATION.with(|notification| notification.get().is_some())
    }
    pub fn take_notification_time(&self) -> Option<Instant> {
        NOTIFICATION.with(|notification| notification.take())
    }
    pub fn wait(&self, duration: Duration) {
        unsafe {
            MsgWaitForMultipleObjectsEx(
                0,
                std::ptr::null(),
                duration.as_millis().min(u128::from(u32::MAX)) as u32,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            );
        }
        pump();
    }
}
impl Drop for WindowObserver {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_handles_and_own_identity_never_produce_targets() {
        assert!(target(std::ptr::null_mut(), 42).is_none());
        assert!(target(unsafe { GetDesktopWindow() }, 42).is_none());
        assert!(target(unsafe { GetShellWindow() }, 0).is_none());
    }

    #[test]
    fn native_window_bounds_feed_snap_and_hidden_windows_are_rejected() {
        // An owned, non-activating fixture exercises DWM and the real filter.
        // No application belonging to the user is moved or inspected here.
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        unsafe {
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_OVERLAPPEDWINDOW,
                100,
                100,
                600,
                400,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            assert!(!window.is_null());
            struct Fixture(HWND);
            impl Drop for Fixture {
                fn drop(&mut self) {
                    unsafe {
                        DestroyWindow(self.0);
                    }
                }
            }
            let _fixture = Fixture(window);
            let pid = std::process::id() as i32;
            assert!(target(window, pid).is_none());
            ShowWindow(window, SW_SHOWNOACTIVATE);
            let observed = target(window, pid).expect("visible DWM fixture");
            assert!(target(window, pid + 1).is_none());
            let mut snap = crate::external_snap::ExternalSnap::default();
            let pet = Rect {
                x: observed.bounds.x + 20.0,
                y: observed.bounds.y - 96.0,
                width: 100.0,
                height: 100.0,
            };
            assert!(
                snap.release(pet, 0.96, 0.5, true, -1, &[observed])
                    .is_some()
            );
            SetWindowPos(
                window,
                std::ptr::null_mut(),
                150,
                140,
                0,
                0,
                SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER,
            );
            let moved = target(window, pid).unwrap();
            let followed = snap.follow(pet, 0.5, true, Some(moved)).unwrap();
            assert_eq!(followed.y + pet.height * 0.5, moved.bounds.y);
            ShowWindow(window, SW_HIDE);
            assert!(snap.follow(pet, 0.5, true, target(window, pid)).is_none());
            assert!(!snap.attached());
        }
    }
}
