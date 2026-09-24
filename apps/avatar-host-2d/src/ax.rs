//! Permission-gated AX window notifications with bounded polling fallback.
//! Only window identity, position, size and minimized state are read.
use crate::external_snap::Target;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[derive(Clone, Copy)]
struct Observation {
    at: Instant,
    target: Option<Target>,
    notification_at: Option<Instant>,
}

pub struct Probe {
    enabled: Arc<AtomicBool>,
    following: Arc<AtomicBool>,
    following_changed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    latest: Arc<Mutex<Option<Observation>>>,
    revision: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
}

impl Probe {
    pub fn new(on_change: impl Fn() + Send + 'static) -> Self {
        let enabled = Arc::new(AtomicBool::new(false));
        let following = Arc::new(AtomicBool::new(false));
        let following_changed = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let latest: Arc<Mutex<Option<Observation>>> = Arc::new(Mutex::new(None));
        let revision = Arc::new(AtomicU64::new(0));
        let (enabled_worker, stop_worker, latest_worker) =
            (enabled.clone(), stop.clone(), latest.clone());
        let following_worker = following.clone();
        let following_changed_worker = following_changed.clone();
        let revision_worker = revision.clone();
        let worker = std::thread::spawn(move || {
            let mut focused = native::FocusedProbe::default();
            let mut observer = native::WindowObserver::default();
            let mut next_poll = Instant::now();
            let mut last_sample = Instant::now() - Duration::from_millis(33);
            while !stop_worker.load(Ordering::Acquire) {
                if !enabled_worker.load(Ordering::Acquire) {
                    observer.clear();
                    std::thread::sleep(Duration::from_millis(100));
                    next_poll = Instant::now();
                    continue;
                }
                let now = Instant::now();
                let due = now >= next_poll;
                let attachment_changed = following_changed_worker.swap(false, Ordering::AcqRel);
                let notified = observer.notified();
                if due
                    || attachment_changed
                    || (notified && now.duration_since(last_sample) >= Duration::from_millis(33))
                {
                    let notification_at = observer.take_notification_time();
                    let trusted = crate::platform::ax_trusted() == Some(true);
                    let target = if trusted {
                        focused.sample(std::process::id() as i32)
                    } else {
                        None
                    };
                    if trusted && following_worker.load(Ordering::Acquire) {
                        observer.follow(&focused, target);
                    } else {
                        observer.clear();
                    }
                    let sampled_at = Instant::now();
                    let mut latest = latest_worker.lock().unwrap();
                    let changed = latest.as_ref().and_then(|value| value.target) != target;
                    if changed {
                        revision_worker.fetch_add(1, Ordering::Release);
                    }
                    *latest = Some(Observation {
                        at: sampled_at,
                        target,
                        notification_at,
                    });
                    drop(latest);
                    if changed && following_worker.load(Ordering::Acquire) {
                        on_change();
                    }
                    last_sample = sampled_at;
                    next_poll = sampled_at + Duration::from_millis(250);
                    continue;
                }
                let until_poll = next_poll.saturating_duration_since(now);
                let until_sample = if notified {
                    Duration::from_millis(33).saturating_sub(now.duration_since(last_sample))
                } else {
                    until_poll
                };
                observer.wait(until_poll.min(until_sample).min(Duration::from_millis(100)));
            }
            observer.clear();
        });
        Self {
            enabled,
            following,
            following_changed,
            stop,
            latest,
            revision,
            worker: Some(worker),
        }
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Release);
        if !enabled {
            *self.latest.lock().unwrap() = None;
            self.revision.fetch_add(1, Ordering::Release);
        }
    }

    pub fn set_following(&self, following: bool) {
        if self.following.swap(following, Ordering::AcqRel) != following {
            self.following_changed.store(true, Ordering::Release);
        }
    }

    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn latest(&self) -> Option<Target> {
        self.latest_with_timing().map(|(target, _, _)| target)
    }

    pub fn latest_with_timing(&self) -> Option<(Target, f64, Option<f64>)> {
        self.latest
            .lock()
            .unwrap()
            .as_ref()
            .filter(|observation| observation.at.elapsed() <= Duration::from_millis(750))
            .and_then(|observation| {
                observation.target.map(|target| {
                    (
                        target,
                        observation.at.elapsed().as_secs_f64() * 1000.0,
                        observation
                            .notification_at
                            .map(|at| at.elapsed().as_secs_f64() * 1000.0),
                    )
                })
            })
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
    use std::{
        ffi::{c_char, c_int, c_void},
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

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
        fn AXObserverCreate(pid: c_int, callback: ObserverCallback, observer: *mut Ref) -> c_int;
        fn AXObserverAddNotification(
            observer: Ref,
            element: Ref,
            notification: Ref,
            refcon: *mut c_void,
        ) -> c_int;
        fn AXObserverRemoveNotification(observer: Ref, element: Ref, notification: Ref) -> c_int;
        fn AXObserverGetRunLoopSource(observer: Ref) -> Ref;
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
        fn CFRetain(value: Ref) -> Ref;
        fn CFRunLoopGetCurrent() -> Ref;
        fn CFRunLoopAddSource(run_loop: Ref, source: Ref, mode: Ref);
        fn CFRunLoopRemoveSource(run_loop: Ref, source: Ref, mode: Ref);
        fn CFRunLoopRunInMode(mode: Ref, seconds: f64, return_after_source: u8) -> c_int;
        static kCFRunLoopDefaultMode: Ref;
    }

    type ObserverCallback = unsafe extern "C" fn(Ref, Ref, Ref, *mut c_void);
    const NOTIFICATIONS: [&[u8]; 4] = [
        b"AXMoved\0",
        b"AXResized\0",
        b"AXWindowMiniaturized\0",
        b"AXUIElementDestroyed\0",
    ];

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
        fn retained(&self) -> Self {
            Self(unsafe { CFRetain(self.0) })
        }
        fn string(value: &[u8]) -> Option<Self> {
            Self::new(unsafe {
                CFStringCreateWithCString(std::ptr::null(), value.as_ptr().cast(), 0x0800_0100)
            })
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) };
        }
    }

    #[derive(Default)]
    pub struct WindowObserver {
        active: Option<ActiveObserver>,
        unsupported: Option<(i32, u64)>,
    }

    impl WindowObserver {
        pub fn follow(&mut self, focused: &FocusedProbe, target: Option<Target>) {
            let Some(target) = target else {
                self.clear();
                return;
            };
            let key = (target.pid, target.window);
            if self.active.as_ref().is_some_and(|active| active.key == key)
                || self.unsupported == Some(key)
            {
                return;
            }
            self.clear();
            if let Some(window) = focused.tracked.as_ref()
                && let Some(active) = ActiveObserver::new(key, window)
            {
                pet_ipc::event_log!(
                    "{}",
                    serde_json::json!({"event":"ax_observer","mode":"notification","pid":key.0,"window":key.1})
                );
                self.active = Some(active);
            } else {
                pet_ipc::event_log!(
                    "{}",
                    serde_json::json!({"event":"ax_observer","mode":"poll_fallback","pid":key.0,"window":key.1})
                );
                self.unsupported = Some(key);
            }
        }
        pub fn clear(&mut self) {
            self.active = None;
            self.unsupported = None;
        }
        pub fn notified(&self) -> bool {
            self.active
                .as_ref()
                .is_some_and(|active| active.signal.dirty.load(Ordering::Acquire))
        }
        pub fn take_notification_time(&self) -> Option<Instant> {
            self.active.as_ref().and_then(|active| active.signal.take())
        }
        pub fn wait(&self, duration: Duration) {
            if self.active.is_some() {
                unsafe {
                    CFRunLoopRunInMode(kCFRunLoopDefaultMode, duration.as_secs_f64(), 1);
                }
            } else {
                std::thread::sleep(duration);
            }
        }
    }

    struct ActiveObserver {
        key: (i32, u64),
        observer: Owned,
        window: Owned,
        notifications: Vec<Owned>,
        signal: Box<CallbackSignal>,
    }

    #[derive(Default)]
    struct CallbackSignal {
        dirty: AtomicBool,
        first_at: Mutex<Option<Instant>>,
    }

    impl CallbackSignal {
        fn mark(&self) {
            let mut first_at = self.first_at.lock().unwrap();
            if first_at.is_none() {
                *first_at = Some(Instant::now());
            }
            self.dirty.store(true, Ordering::Release);
        }
        fn take(&self) -> Option<Instant> {
            if self.dirty.swap(false, Ordering::AcqRel) {
                self.first_at.lock().unwrap().take()
            } else {
                None
            }
        }
    }

    impl ActiveObserver {
        fn new(key: (i32, u64), window: &Owned) -> Option<Self> {
            let mut raw = std::ptr::null();
            if unsafe { AXObserverCreate(key.0, observer_callback, &mut raw) } != 0 {
                return None;
            }
            let mut active = Self {
                key,
                observer: Owned::new(raw)?,
                window: window.retained(),
                notifications: Vec::new(),
                signal: Box::new(CallbackSignal::default()),
            };
            let mut movement_notifications = 0;
            for (index, name) in NOTIFICATIONS.into_iter().enumerate() {
                let notification = Owned::string(name)?;
                let refcon = (&*active.signal as *const CallbackSignal).cast_mut().cast();
                if unsafe {
                    AXObserverAddNotification(
                        active.observer.0,
                        active.window.0,
                        notification.0,
                        refcon,
                    )
                } == 0
                {
                    if index < 2 {
                        movement_notifications += 1;
                    }
                    active.notifications.push(notification);
                }
            }
            if movement_notifications == 0 {
                return None;
            }
            let source = unsafe { AXObserverGetRunLoopSource(active.observer.0) };
            if source.is_null() {
                return None;
            }
            unsafe { CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode) };
            Some(active)
        }
    }

    impl Drop for ActiveObserver {
        fn drop(&mut self) {
            let source = unsafe { AXObserverGetRunLoopSource(self.observer.0) };
            if !source.is_null() {
                unsafe {
                    CFRunLoopRemoveSource(CFRunLoopGetCurrent(), source, kCFRunLoopDefaultMode)
                };
            }
            for notification in &self.notifications {
                unsafe {
                    AXObserverRemoveNotification(self.observer.0, self.window.0, notification.0)
                };
            }
        }
    }

    unsafe extern "C" fn observer_callback(_: Ref, _: Ref, _: Ref, refcon: *mut c_void) {
        if !refcon.is_null() {
            unsafe { &*refcon.cast::<CallbackSignal>() }.mark();
        }
    }

    #[cfg(test)]
    #[test]
    fn notification_signal_coalesces_and_rearms() {
        let signal = CallbackSignal::default();
        assert!(signal.take().is_none());
        signal.mark();
        let first = *signal.first_at.lock().unwrap();
        signal.mark();
        assert_eq!(signal.take(), first);
        assert!(signal.take().is_none());
        signal.mark();
        assert!(signal.take().is_some());
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
    use std::time::Duration;
    #[derive(Default)]
    pub struct FocusedProbe;
    impl FocusedProbe {
        pub fn sample(&mut self, _: i32) -> Option<Target> {
            None
        }
    }
    #[derive(Default)]
    pub struct WindowObserver;
    impl WindowObserver {
        pub fn follow(&mut self, _: &FocusedProbe, _: Option<Target>) {}
        pub fn clear(&mut self) {}
        pub fn notified(&self) -> bool {
            false
        }
        pub fn take_notification_time(&self) -> Option<std::time::Instant> {
            None
        }
        pub fn wait(&self, duration: Duration) {
            std::thread::sleep(duration);
        }
    }
}
