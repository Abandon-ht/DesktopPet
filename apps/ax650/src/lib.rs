//! AX650 application shell. Care state has one SQLite owner; rendering stays in
//! the existing host and communicates through the bounded P1 protocol.
use anyhow::{Context, Result, bail, ensure};
use pet_core::{
    Effect, Event, Intent, PetCore,
    care::{CareAction, CareState},
};
use pet_ipc::supervisor::Host;
use pet_persistence::{CareStore, StoreError};
use pet_protocol::{AvatarCommand, AvatarEvent, DesktopCommand};
use serde_json::{Value, json};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicI32, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const HELP: &str = "DesktopPet AX650 (X11, CPU software rendering)\n\
Usage: desktop-pet-ax650 [--model PATH/manifest.json] [--host PATH]\n\
  --data-dir PATH   Persistent care.sqlite3, scale and placement.json\n\
  --log-dir PATH    This run's log directory; defaults to logs/<unique-run>\n\
  --scale 50..150   Viewport percentage; saved value or 64 by default\n\
  --fps 1..30       Scheduling limit; default 20, no stable FPS guarantee\n\
  --seconds N      0 runs until quit or a signal; default 0\n\
  --backend gl|vulkan  gl is the verified AX650 default\n\
Commands: status | feed [request-id] | play [request-id] | rest [request-id]\n\
          stop | show | hide | scale 50..150 | expr NAME | reset | quit\n\
stdin EOF leaves the application running; use --seconds for a finite run.\n\
SIGINT, SIGTERM and SIGHUP request a normal shutdown and save logs.\n\
Exit codes: 0 normal shutdown, 1 runtime failure, 2 invalid configuration.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Gl,
    Vulkan,
}
impl Backend {
    fn name(self) -> &'static str {
        match self {
            Self::Gl => "gl",
            Self::Vulkan => "vulkan",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub model: PathBuf,
    pub host: PathBuf,
    pub data_dir: PathBuf,
    pub log_dir: PathBuf,
    pub scale: Option<u16>,
    pub fps: u16,
    pub seconds: u64,
    pub backend: Backend,
}
impl Config {
    pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Self> {
        let executable = std::env::current_exe()?;
        let root = executable.parent().context("executable has no parent")?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let mut config = Self {
            model: root.join("resources/avatar/manifest.json"),
            host: root.join("avatar-host-2d"),
            data_dir: root.join("data"),
            log_dir: root
                .join("logs")
                .join(format!("{nonce}-{}", std::process::id())),
            scale: None,
            fps: 20,
            seconds: 0,
            backend: Backend::Gl,
        };
        let mut arguments = arguments.into_iter();
        while let Some(argument) = arguments.next() {
            let key = argument.to_str().context("option is not UTF-8")?;
            ensure!(
                [
                    "--model",
                    "--host",
                    "--data-dir",
                    "--log-dir",
                    "--scale",
                    "--fps",
                    "--seconds",
                    "--backend"
                ]
                .contains(&key),
                "unknown option {key}"
            );
            let value = arguments
                .next()
                .with_context(|| format!("missing value for {key}"))?;
            match key {
                "--model" => config.model = value.into(),
                "--host" => config.host = value.into(),
                "--data-dir" => config.data_dir = value.into(),
                "--log-dir" => config.log_dir = value.into(),
                "--scale" => config.scale = Some(number(&value, key, 50, 150)? as u16),
                "--fps" => config.fps = number(&value, key, 1, 30)? as u16,
                "--seconds" => config.seconds = number(&value, key, 0, u64::MAX)?,
                "--backend" => {
                    config.backend = match value.to_str() {
                        Some("gl") => Backend::Gl,
                        Some("vulkan") => Backend::Vulkan,
                        _ => bail!("--backend must be gl or vulkan"),
                    }
                }
                _ => unreachable!(),
            }
        }
        let cwd = std::env::current_dir()?;
        for path in [
            &mut config.model,
            &mut config.host,
            &mut config.data_dir,
            &mut config.log_dir,
        ] {
            if path.is_relative() {
                *path = cwd.join(&*path);
            }
        }
        ensure!(
            config
                .model
                .file_name()
                .is_some_and(|name| name == "manifest.json"),
            "--model must point to a role pack manifest.json"
        );
        ensure!(
            config.model.is_file(),
            "role manifest does not exist: {}",
            config.model.display()
        );
        ensure!(
            config.host.is_file(),
            "rendering host does not exist: {}",
            config.host.display()
        );
        Ok(config)
    }
}
fn number(value: &OsString, key: &str, min: u64, max: u64) -> Result<u64> {
    let n: u64 = value
        .to_str()
        .context("value is not UTF-8")?
        .parse()
        .with_context(|| format!("{key} requires an unsigned integer"))?;
    ensure!((min..=max).contains(&n), "{key} must be in {min}..={max}");
    Ok(n)
}

struct Clock {
    epoch_utc_ms: i64,
    origin: Instant,
}
impl Clock {
    fn new() -> Self {
        Self {
            epoch_utc_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(i64::MAX as u128) as i64,
            origin: Instant::now(),
        }
    }
    fn elapsed_ms(&self) -> u64 {
        self.origin.elapsed().as_millis().min(u64::MAX as u128) as u64
    }
    fn now_ms(&self) -> i64 {
        self.epoch_utc_ms
            .saturating_add(self.elapsed_ms().min(i64::MAX as u64) as i64)
    }
}

static STOP_SIGNAL: AtomicI32 = AtomicI32::new(0);
extern "C" fn stop_signal(signal: libc::c_int) {
    STOP_SIGNAL.store(signal, Ordering::Relaxed);
}
#[cfg(unix)]
fn install_signals() -> Result<()> {
    // The handler only writes a lock-free atomic; all IO/IPC happens on the owner thread.
    for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = stop_signal as *const () as usize;
        action.sa_flags = libc::SA_RESTART;
        unsafe {
            libc::sigemptyset(&mut action.sa_mask);
        }
        ensure!(
            unsafe { libc::sigaction(signal, &action, std::ptr::null_mut()) } == 0,
            "install signal handler: {}",
            io::Error::last_os_error()
        );
    }
    Ok(())
}
#[cfg(not(unix))]
fn install_signals() -> Result<()> {
    bail!("AX650 application requires Unix")
}

struct InstanceLock(File);
impl InstanceLock {
    fn acquire(directory: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("instance.lock"))?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            ensure!(
                unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0,
                "data directory is already in use: {}",
                directory.display()
            );
        }
        Ok(Self(file))
    }
}
impl Drop for InstanceLock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            unsafe {
                libc::flock(self.0.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}

struct Log {
    directory: PathBuf,
    parent: File,
}
impl Log {
    fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        let directory = root.to_path_buf();
        Ok(Self {
            parent: OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(directory.join("parent.jsonl"))
                .context(
                    "log directory already contains a runtime log; choose a new run directory",
                )?,
            directory,
        })
    }
    fn emit(&mut self, value: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.parent.write_all(&bytes)?;
        self.parent.flush()?;
        // A closed SSH output stream does not own the application lifecycle.
        let mut output = io::stdout().lock();
        if let Err(error) = output.write_all(&bytes).and_then(|()| output.flush())
            && error.kind() != io::ErrorKind::BrokenPipe
        {
            return Err(error.into());
        }
        Ok(())
    }
}

fn host_command(config: &Config, log_dir: &Path) -> Command {
    let mut command = Command::new(&config.host);
    command
        .arg(&config.model)
        .env("WGPU_BACKEND", config.backend.name())
        .env("DESKTOPPET_MAX_FPS", config.fps.to_string())
        .env("DESKTOPPET_LAYOUT", config.data_dir.join("placement.json"))
        .env("DESKTOPPET_METRICS", log_dir.join("render-metrics.json"))
        .env_remove("DESKTOPPET_X11_OPAQUE_ALPHA")
        .env_remove("DESKTOPPET_FRAME_CAPTURE");
    if std::env::var_os("DISPLAY").is_none() {
        command.env("DISPLAY", ":0");
    }
    if config.backend == Backend::Gl {
        command
            .env("LIBGL_ALWAYS_SOFTWARE", "1")
            .env("DESKTOPPET_X11_GL_ALPHA", "1");
    } else {
        command.env_remove("DESKTOPPET_X11_GL_ALPHA");
        if std::env::var_os("VK_DRIVER_FILES").is_none()
            && std::env::var_os("VK_ICD_FILENAMES").is_none()
        {
            command.env(
                "VK_ICD_FILENAMES",
                "/usr/share/vulkan/icd.d/lvp_icd.aarch64.json",
            );
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Separate session prevents terminal Ctrl+C from aborting the child
        // before the parent can request shutdown and save render metrics.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    command
}

fn effects(host: &mut Host, effects: Vec<Effect>) -> Result<()> {
    for effect in effects {
        match effect {
            Effect::Avatar(command) => host.avatar(command)?,
            Effect::Desktop(command) => host.desktop(command)?,
        }
    }
    Ok(())
}

enum Input {
    Line(String),
    Error(String),
    Eof,
}
const MAX_INPUT_BYTES: usize = 4096;
fn input() -> mpsc::Receiver<Input> {
    let (tx, rx) = mpsc::sync_channel(16);
    thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = stdin.lock();
        loop {
            let message = match input_line(&mut reader) {
                Ok(Some(line)) => Input::Line(line),
                Ok(None) => Input::Eof,
                Err(error) => Input::Error(error.to_string()),
            };
            let terminal = matches!(message, Input::Eof | Input::Error(_));
            if tx.send(message).is_err() || terminal {
                break;
            }
        }
    });
    rx
}
fn input_line(reader: &mut impl BufRead) -> Result<Option<String>> {
    let mut bytes = Vec::new();
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            if bytes.is_empty() {
                return Ok(None);
            }
            break;
        }
        let end = chunk.iter().position(|&byte| byte == b'\n');
        let len = end.map_or(chunk.len(), |n| n + 1);
        ensure!(
            bytes.len() + len <= MAX_INPUT_BYTES,
            "stdin command exceeds {MAX_INPUT_BYTES} bytes"
        );
        bytes.extend_from_slice(&chunk[..len]);
        reader.consume(len);
        if end.is_some() {
            break;
        }
    }
    Ok(Some(
        String::from_utf8(bytes).context("stdin is not UTF-8")?,
    ))
}

struct Application {
    core: PetCore,
    store: CareStore,
    care: CareState,
    manifest: avatar_pack::Manifest,
    touch_session: String,
    variation_seed: u64,
    scale: u16,
    request_sequence: u64,
}
impl Application {
    fn status(&self, host: &Host, config: &Config, clock: &Clock) -> Value {
        json!({"event":"status","host_pid":host.id(),"ready":self.core.state().ready,
            "visible":self.core.state().visible,"activity":format!("{:?}",self.core.state().activity),
            "scale":self.scale,"fps_limit":config.fps,"backend":config.backend.name(),
            "avatar_capabilities":host.avatar_capabilities,
            "rendering":"software_backend; device details are in host.stderr.log",
            "care":self.care,"elapsed_ms":clock.elapsed_ms()})
    }
    fn care(
        &mut self,
        host: &mut Host,
        action: CareAction,
        request_id: Option<&str>,
        clock: &Clock,
        log: &mut Log,
    ) -> Result<()> {
        self.request_sequence += 1;
        let generated = format!("{}-care-{}", self.touch_session, self.request_sequence);
        let request_id = request_id.unwrap_or(&generated);
        match self.store.apply(request_id, action, clock.now_ms()) {
            Ok(outcome) => {
                self.care = outcome.state;
                log.emit(json!({"event":"care","request_id":request_id,"action":action,"replayed":outcome.replayed,"care":self.care}))?;
                // SQLite commit is complete before animation; replay must never
                // change inventory or repeat a cosmetic action after recovery.
                if !outcome.replayed {
                    effects(
                        host,
                        self.core.update(Event::CareCommitted {
                            action,
                            now_ms: clock.elapsed_ms(),
                            needs: self.care.needs,
                        }),
                    )?;
                }
            }
            Err(
                error @ (StoreError::Rules(_)
                | StoreError::InvalidRequestId
                | StoreError::RequestIdConflict),
            ) => {
                log.emit(json!({"event":"command_error","request_id":request_id,"error":error.to_string()}))?;
            }
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
    fn hit(
        &mut self,
        host: &mut Host,
        hit: pet_protocol::HitDetail,
        clock: &Clock,
        log: &mut Log,
    ) -> Result<()> {
        let outcome = self.store.apply_touch(
            &self.touch_session,
            hit.event_id,
            &self.manifest.id,
            hit.region,
            clock.now_ms(),
        )?;
        self.care = outcome.state.clone();
        if !outcome.replayed {
            effects(host, self.core.update(Event::Intent(Intent::StopActivity)))?;
            effects(
                host,
                self.core.update(Event::Tick {
                    now_ms: clock.elapsed_ms(),
                    needs: self.care.needs,
                }),
            )?;
            if host.avatar_capabilities.touch_reactions {
                if let Some(cue) = outcome.decision.expression_cue {
                    let cue = self.manifest.choose_touch_cue(
                        hit.region,
                        cue,
                        self.variation_seed ^ hit.event_id,
                    );
                    host.avatar(AvatarCommand::PlayTouchCue(cue))?;
                }
            } else {
                effects(host, self.core.update(Event::Avatar(AvatarEvent::Hit(hit))))?;
            }
        }
        log.emit(json!({"event":"touch","hit":hit,"outcome":outcome}))
    }
    fn command(
        &mut self,
        line: &str,
        host: &mut Host,
        config: &Config,
        clock: &Clock,
        log: &mut Log,
    ) -> Result<bool> {
        let mut words = line.split_whitespace();
        let Some(command) = words.next() else {
            return Ok(false);
        };
        let value = words.next();
        let extra = words.next();
        let invalid = match command {
            "feed" | "play" | "rest" => extra.is_some(),
            "scale" | "expr" => value.is_none() || extra.is_some(),
            "status" | "stop" | "show" | "hide" | "reset" | "quit" => value.is_some(),
            _ => true,
        };
        if invalid {
            log.emit(json!({"event":"command_error","error":"invalid command or arguments"}))?;
            return Ok(false);
        }
        match command {
            "quit" => return Ok(true),
            "status" => {
                self.care = self.store.load_at(clock.now_ms())?;
                log.emit(self.status(host, config, clock))?;
            }
            "feed" | "play" | "rest" => self.care(
                host,
                match command {
                    "feed" => CareAction::Feed,
                    "play" => CareAction::Play,
                    _ => CareAction::Rest,
                },
                value,
                clock,
                log,
            )?,
            "show" | "hide" => {
                effects(
                    host,
                    self.core
                        .update(Event::Intent(Intent::SetVisible(command == "show"))),
                )?;
                log.emit(self.status(host, config, clock))?;
            }
            "stop" => {
                effects(host, self.core.update(Event::Intent(Intent::StopActivity)))?;
                log.emit(self.status(host, config, clock))?;
            }
            "scale" => {
                if let Some(scale) = value
                    .and_then(|value| value.parse::<u16>().ok())
                    .filter(|scale| (50..=150).contains(scale))
                {
                    host.desktop(DesktopCommand::SetScale(scale))?;
                    self.store.set_setting("ax650.scale", &scale.to_string())?;
                    self.scale = scale;
                    log.emit(self.status(host, config, clock))?;
                } else {
                    log.emit(json!({"event":"command_error","error":"scale must be 50..=150"}))?;
                }
            }
            "expr" => {
                let name = value.unwrap();
                if self
                    .manifest
                    .expression_profile
                    .as_ref()
                    .is_some_and(|profile| profile.catalog.contains_key(name))
                {
                    host.avatar(AvatarCommand::PreviewExpression(name.to_owned()))?;
                    log.emit(json!({"event":"expression","name":name}))?;
                } else {
                    log.emit(json!({"event":"command_error","error":"expression is absent from the role pack catalog"}))?;
                }
            }
            "reset" => {
                host.avatar(AvatarCommand::EndPreview)?;
                log.emit(json!({"event":"expression_reset"}))?;
            }
            _ => unreachable!(),
        }
        Ok(false)
    }
}

pub fn run(config: Config) -> Result<()> {
    STOP_SIGNAL.store(0, Ordering::Relaxed);
    install_signals()?;
    let mut log = Log::open(&config.log_dir)?;
    let result = run_logged(&config, &mut log);
    let (completed, error) = match &result {
        Ok(()) => (true, None),
        Err(error) => (false, Some(format!("{error:#}"))),
    };
    let value = json!({"event":"result","completed":completed,"exit_code":if completed {0} else {1},"error":error,"log_dir":log.directory});
    fs::write(
        log.directory.join("result.json"),
        serde_json::to_vec_pretty(&value)?,
    )?;
    log.emit(value)?;
    result
}
fn run_logged(config: &Config, log: &mut Log) -> Result<()> {
    fs::create_dir_all(&config.data_dir)?;
    let _instance = InstanceLock::acquire(&config.data_dir)?;
    let clock = Clock::new();
    let manifest = avatar_pack::read_json::<avatar_pack::Manifest>(&config.model)?;
    manifest.validate()?;
    let mut store = CareStore::open(config.data_dir.join("care.sqlite3"), clock.now_ms())?;
    let care = store.load_at(clock.now_ms())?;
    let scale = config
        .scale
        .or(store
            .setting("ax650.scale")?
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|scale| (50..=150).contains(scale)))
        .unwrap_or(64);
    fs::write(
        log.directory.join("run-info.json"),
        serde_json::to_vec_pretty(
            &json!({"model":config.model,"host":config.host,"data_dir":config.data_dir,"scale":scale,"fps_limit":config.fps,"seconds":config.seconds,"backend":config.backend.name(),"display":std::env::var("DISPLAY").unwrap_or_else(|_| ":0".into()),"scope":"AX650 X11 software renderer with persistent care; no WebView or voice"}),
        )?,
    )?;
    log.emit(json!({"event":"starting","log_dir":log.directory,"host_stderr":log.directory.join("host.stderr.log")}))?;
    let mut command = host_command(config, &log.directory);
    let mut host = Host::start_with_stderr(
        &mut command,
        Duration::from_secs(7),
        Stdio::from(File::create(log.directory.join("host.stderr.log"))?),
    )
    .context("rendering host startup failed; inspect host.stderr.log")?;
    host.set_timeout(Duration::from_secs(2))?;
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let mut app = Application {
        core: PetCore::default(),
        store,
        care,
        manifest,
        touch_session: format!("ax650-{}-{nonce}-{}", std::process::id(), host.id()),
        variation_seed: nonce as u64 ^ (nonce >> 64) as u64,
        scale,
        request_sequence: 0,
    };
    ensure!(
        !host.avatar_capabilities.touch_reactions || !app.manifest.touch_reactions.is_empty(),
        "host advertises touch reactions but role pack has no bindings"
    );
    host.desktop(DesktopCommand::SetScale(scale))?;
    app.store.set_setting("ax650.scale", &scale.to_string())?;
    effects(
        &mut host,
        app.core.update(Event::Tick {
            now_ms: clock.elapsed_ms(),
            needs: app.care.needs,
        }),
    )?;
    let capabilities = host.avatar_capabilities;
    effects(
        &mut host,
        app.core
            .update(Event::Avatar(AvatarEvent::Ready(capabilities))),
    )?;
    log.emit(app.status(&host, config, &clock))?;
    let input = input();
    let started = Instant::now();
    let mut next_poll = Instant::now();
    let mut next_ping = Instant::now();
    let mut next_refresh = Instant::now() + Duration::from_secs(60);
    let reason = loop {
        let signal = STOP_SIGNAL.load(Ordering::Relaxed);
        if signal != 0 {
            break format!("signal_{signal}");
        }
        if config.seconds > 0 && started.elapsed().as_secs() >= config.seconds {
            break "duration_complete".to_owned();
        }
        let mut quit = false;
        let batch_started = Instant::now();
        // Keep a bounded command batch from postponing health checks forever.
        for _ in 0..8 {
            if STOP_SIGNAL.load(Ordering::Relaxed) != 0
                || batch_started.elapsed() >= Duration::from_millis(200)
            {
                break;
            }
            match input.try_recv() {
                Ok(Input::Line(line)) => {
                    if app.command(&line, &mut host, config, &clock, log)? {
                        quit = true;
                        break;
                    }
                }
                Ok(Input::Error(error)) => {
                    log.emit(json!({"event":"stdin_error","error":error,"continuing":true}))?;
                    break;
                }
                Ok(Input::Eof) => {
                    log.emit(json!({"event":"stdin_eof","continuing":true}))?;
                    break;
                }
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            }
        }
        if quit {
            break "quit".to_owned();
        }
        if Instant::now() >= next_ping {
            host.ping().context("rendering host heartbeat failed")?;
            next_ping = Instant::now() + Duration::from_secs(1);
        }
        if Instant::now() >= next_refresh {
            app.care = app.store.load_at(clock.now_ms())?;
            next_refresh = Instant::now() + Duration::from_secs(60);
        }
        effects(
            &mut host,
            app.core.update(Event::Tick {
                now_ms: clock.elapsed_ms(),
                needs: app.care.needs,
            }),
        )?;
        if Instant::now() >= next_poll {
            for event in host.poll().context("rendering host event polling failed")? {
                match event {
                    AvatarEvent::Hit(hit) => app.hit(&mut host, hit, &clock, log)?,
                    AvatarEvent::Fault(error) => bail!("rendering host fault: {error}"),
                    AvatarEvent::Ready(_) => {
                        bail!("unexpected duplicate rendering host Ready event")
                    }
                }
            }
            next_poll = Instant::now()
                + if app.core.state().visible {
                    Duration::from_millis(100)
                } else {
                    Duration::from_secs(1)
                };
        }
        // EOF intentionally ends input only. Finite duration and signal paths
        // remain active when launched with redirected stdin or from a service.
        thread::sleep(Duration::from_millis(20));
    };
    log.emit(json!({"event":"stopping","reason":reason,"host_pid":host.id()}))?;
    app.core.update(Event::Intent(Intent::Quit));
    host.shutdown()
        .context("rendering host did not shut down cleanly")?;
    app.care = app.store.load_at(clock.now_ms())?;
    log.emit(json!({"event":"stopped","reason":reason,"care":app.care}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stdin_memory_bound_rejects_oversized_and_keeps_eof_semantics() {
        assert!(input_line(&mut io::Cursor::new(vec![b'a'; MAX_INPUT_BYTES + 1])).is_err());
        let mut input = io::Cursor::new(b"status\nquit");
        assert_eq!(input_line(&mut input).unwrap().as_deref(), Some("status\n"));
        assert_eq!(input_line(&mut input).unwrap().as_deref(), Some("quit"));
        assert_eq!(input_line(&mut input).unwrap(), None);
    }
}
