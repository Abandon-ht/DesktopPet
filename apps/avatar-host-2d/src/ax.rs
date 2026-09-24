//! Permission-gated, bounded AX snapshots on a worker thread.
//! Only window identity, position, size and minimized state are read.
use crate::external_snap::Target;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
struct Observation {
    at: Instant,
    target: Option<Target>,
}

pub struct Probe {
    enabled: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    latest: Arc<Mutex<Option<Observation>>>,
    worker: Option<JoinHandle<()>>,
}

impl Probe {
    pub fn new() -> Self {
        let enabled = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let latest = Arc::new(Mutex::new(None));
        let (enabled_worker, stop_worker, latest_worker) =
            (enabled.clone(), stop.clone(), latest.clone());
        let worker = std::thread::spawn(move || {
            let mut focused = native::FocusedProbe::default();
            while !stop_worker.load(Ordering::Acquire) {
                if enabled_worker.load(Ordering::Acquire) {
                    let target = if crate::platform::ax_trusted() == Some(true) {
                        focused.sample(std::process::id() as i32)
                    } else {
                        None
                    };
                    *latest_worker.lock().unwrap() = Some(Observation {
                        at: Instant::now(),
                        target,
                    });
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        });
        Self {
            enabled,
            stop,
            latest,
            worker: Some(worker),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
        if !enabled {
            *self.latest.lock().unwrap() = None;
        }
    }

    pub fn latest(&self) -> Option<Target> {
        self.latest
            .lock()
            .unwrap()
            .as_ref()
            .filter(|observation| observation.at.elapsed() <= Duration::from_millis(750))
            .and_then(|observation| observation.target)
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::Target;
    use crate::snap::Rect;
    use std::ffi::{c_char, c_int, c_void};

    type Ref = *const c_void;
    #[repr(C)]
    #[derive(Default)]
    struct Point {
        x: f64,
        y: f64,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> Ref;
        fn AXUIElementCopyAttributeValue(element: Ref, attribute: Ref, value: *mut Ref) -> c_int;
        fn AXUIElementGetPid(element: Ref, pid: *mut c_int) -> c_int;
        fn AXUIElementSetMessagingTimeout(element: Ref, seconds: f32) -> c_int;
        fn AXValueGetType(value: Ref) -> u32;
        fn AXValueGetValue(value: Ref, kind: u32, output: *mut c_void) -> u8;
        fn AXValueGetTypeID() -> usize;
        fn AXUIElementGetTypeID() -> usize;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(allocator: Ref, text: *const c_char, encoding: u32) -> Ref;
        fn CFRelease(value: Ref);
        fn CFGetTypeID(value: Ref) -> usize;
        fn CFHash(value: Ref) -> usize;
        fn CFEqual(a: Ref, b: Ref) -> u8;
        fn CFBooleanGetTypeID() -> usize;
        fn CFBooleanGetValue(value: Ref) -> u8;
    }

    struct Owned(Ref);
    impl Owned {
        fn new(value: Ref) -> Option<Self> {
            (!value.is_null()).then_some(Self(value))
        }
        fn attribute(&self, key: &'static [u8]) -> Option<Self> {
            // Every key is a static nul-terminated ASCII AX attribute name.
            let name = Self::new(unsafe {
                CFStringCreateWithCString(std::ptr::null(), key.as_ptr().cast(), 0x0800_0100)
            })?;
            let mut value = std::ptr::null();
            let result = unsafe { AXUIElementCopyAttributeValue(self.0, name.0, &mut value) };
            if result != 0 {
                if !value.is_null() {
                    unsafe { CFRelease(value) };
                }
                return None;
            }
            Self::new(value)
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) };
        }
    }

    fn geometry(window: &Owned) -> Option<Rect> {
        let position = window.attribute(b"AXPosition\0")?;
        let size = window.attribute(b"AXSize\0")?;
        if unsafe { CFGetTypeID(position.0) } != unsafe { AXValueGetTypeID() }
            || unsafe { CFGetTypeID(size.0) } != unsafe { AXValueGetTypeID() }
            || unsafe { AXValueGetType(position.0) } != 1
            || unsafe { AXValueGetType(size.0) } != 2
        {
            return None;
        }
        let mut point = Point::default();
        let mut extent = Point::default();
        if unsafe { AXValueGetValue(position.0, 1, (&raw mut point).cast()) } == 0
            || unsafe { AXValueGetValue(size.0, 2, (&raw mut extent).cast()) } == 0
        {
            return None;
        }
        let rect = Rect {
            x: point.x,
            y: point.y,
            width: extent.x,
            height: extent.y,
        };
        ([rect.x, rect.y, rect.width, rect.height]
            .iter()
            .all(|v| v.is_finite())
            && rect.width > 0.0
            && rect.height > 0.0)
            .then_some(rect)
    }

    #[derive(Default)]
    pub struct FocusedProbe {
        tracked: Option<Owned>,
        pid: i32,
        id: u64,
    }

    impl FocusedProbe {
        pub fn sample(&mut self, own_pid: i32) -> Option<Target> {
            let system = Owned::new(unsafe { AXUIElementCreateSystemWide() })?;
            unsafe { AXUIElementSetMessagingTimeout(system.0, 0.2) };
            let app = system.attribute(b"AXFocusedApplication\0")?;
            if unsafe { CFGetTypeID(app.0) } != unsafe { AXUIElementGetTypeID() } {
                return None;
            }
            unsafe { AXUIElementSetMessagingTimeout(app.0, 0.2) };
            let mut pid = 0;
            if unsafe { AXUIElementGetPid(app.0, &mut pid) } != 0 || pid <= 0 {
                return None;
            }
            // The pet can become focused while it is being dragged. Keep observing
            // the previously focused external window until another window takes focus.
            if pid == own_pid {
                let tracked = self.tracked.as_ref()?;
                return target(tracked, self.pid, self.id);
            }
            let window = app.attribute(b"AXFocusedWindow\0")?;
            if unsafe { CFGetTypeID(window.0) } != unsafe { AXUIElementGetTypeID() } {
                return None;
            }
            unsafe { AXUIElementSetMessagingTimeout(window.0, 0.2) };
            let same = self.pid == pid
                && self
                    .tracked
                    .as_ref()
                    .is_some_and(|old| unsafe { CFEqual(old.0, window.0) != 0 });
            let id = if same {
                self.id
            } else {
                (unsafe { CFHash(window.0) }) as u64
            };
            self.pid = pid;
            self.id = id;
            self.tracked = Some(window);
            target(self.tracked.as_ref()?, pid, id)
        }
    }

    fn target(window: &Owned, pid: i32, id: u64) -> Option<Target> {
        let minimized = window
            .attribute(b"AXMinimized\0")
            .filter(|value| unsafe { CFGetTypeID(value.0) } == unsafe { CFBooleanGetTypeID() })
            .is_some_and(|value| unsafe { CFBooleanGetValue(value.0) != 0 });
        Some(Target {
            pid,
            window: id,
            bounds: geometry(window)?,
            minimized,
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod native {
    use super::Target;
    #[derive(Default)]
    pub struct FocusedProbe;
    impl FocusedProbe {
        pub fn sample(&mut self, _: i32) -> Option<Target> {
            None
        }
    }
}
