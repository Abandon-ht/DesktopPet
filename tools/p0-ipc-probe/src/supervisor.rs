//! Blocking supervisor; run on a dedicated worker, never the GUI event loop.
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{BufReader, Write},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

static SESSION: AtomicU64 = AtomicU64::new(1);

pub struct Host {
    child: Child,
    requests: Option<SyncSender<Value>>,
    responses: Receiver<Result<Value>>,
    worker: Option<JoinHandle<()>>,
    session: String,
    sequence: u64,
    timeout: Duration,
    stopped: bool,
}

impl Host {
    pub fn start(command: &mut Command, timeout: Duration) -> Result<Self> {
        ensure!(!timeout.is_zero(), "timeout must be positive");
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("spawn host")?;
        let mut input = child.stdin.take().context("missing stdin")?;
        let mut output = BufReader::new(child.stdout.take().context("missing stdout")?);
        let (tx, rx) = mpsc::sync_channel::<Value>(1);
        let (reply_tx, reply_rx) = mpsc::sync_channel(1);
        // Both writes and reads are off the caller thread: a host that stops
        // reading stdin must not prevent the supervisor from enforcing a deadline.
        let worker = thread::spawn(move || {
            while let Ok(request) = rx.recv() {
                let result = (|| -> Result<Value> {
                    let mut bytes = serde_json::to_vec(&request)?;
                    bytes.push(b'\n');
                    ensure!(bytes.len() <= crate::MAX_FRAME, "request too large");
                    input.write_all(&bytes)?;
                    input.flush()?;
                    let bytes = crate::frame(&mut output)?.context("host EOF")?;
                    serde_json::from_slice(&bytes).context("invalid host JSON")
                })();
                let failed = result.is_err();
                if reply_tx.send(result).is_err() || failed {
                    break;
                }
            }
        });
        let mut host = Self {
            child,
            requests: Some(tx),
            responses: reply_rx,
            worker: Some(worker),
            session: format!(
                "{}-{}",
                std::process::id(),
                SESSION.fetch_add(1, Ordering::Relaxed)
            ),
            sequence: 0,
            timeout,
            stopped: false,
        };
        let ready = host.exchange("hello", "ready")?;
        ensure!(
            ready["payload"]["max_frame_bytes"].as_u64() == Some(crate::MAX_FRAME as u64),
            "unsupported frame limit"
        );
        let capabilities = ready["payload"]["capabilities"]
            .as_array()
            .context("missing capabilities")?;
        ensure!(
            ["ping", "shutdown"]
                .iter()
                .all(|name| capabilities.iter().any(|v| v.as_str() == Some(name))),
            "missing host capabilities"
        );
        Ok(host)
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    fn exchange(&mut self, kind: &str, expected: &str) -> Result<Value> {
        ensure!(!self.stopped, "host is stopped");
        self.sequence = self.sequence.checked_add(1).context("sequence exhausted")?;
        let request_id = format!("r-{}", self.sequence);
        let request = json!({"protocol_version":1,"session_id":self.session,"sequence":self.sequence,"request_id":request_id,"type":kind,"payload":{}});
        let result = (|| -> Result<Value> {
            self.requests
                .as_ref()
                .context("host closed")?
                .try_send(request)
                .context("request queue unavailable")?;
            let reply = self
                .responses
                .recv_timeout(self.timeout)
                .context("host response timeout or disconnected")??;
            ensure!(
                reply["protocol_version"].as_u64() == Some(1),
                "host protocol mismatch"
            );
            ensure!(
                reply["session_id"].as_str() == Some(&self.session),
                "host session mismatch"
            );
            ensure!(
                reply["sequence"].as_u64() == Some(self.sequence),
                "host sequence mismatch"
            );
            ensure!(
                reply["request_id"].as_str() == Some(&request_id),
                "host request mismatch"
            );
            ensure!(
                reply["type"].as_str() == Some(expected) && reply["payload"].is_object(),
                "unexpected host response"
            );
            Ok(reply)
        })();
        if result.is_err() {
            self.terminate();
        }
        result
    }

    pub fn ping(&mut self) -> Result<()> {
        self.exchange("ping", "pong").map(|_| ())
    }

    pub fn shutdown(mut self) -> Result<()> {
        self.exchange("shutdown", "stopped")?;
        self.requests.take();
        let deadline = Instant::now() + self.timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                self.stopped = true;
                ensure!(status.success(), "host shutdown failed: {status}");
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!("host exit timeout");
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn terminate(&mut self) {
        self.requests.take();
        if !self.stopped {
            let _ = self.child.kill();
            let _ = self.child.wait();
            self.stopped = true;
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.terminate();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Rolling failure count, including startup failures. Success does not erase
/// recent failures, so a host that alternates success/crash cannot restart forever.
pub struct RestartBudget {
    failures: VecDeque<Instant>,
    base_delay: Duration,
}

impl RestartBudget {
    pub fn new(base_delay: Duration) -> Self {
        Self {
            failures: VecDeque::new(),
            base_delay,
        }
    }

    pub fn failure(&mut self, now: Instant) -> Option<Duration> {
        while self
            .failures
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(60))
        {
            self.failures.pop_front();
        }
        self.failures.push_back(now);
        (self.failures.len() < 3).then(|| {
            self.base_delay
                .saturating_mul(1 << (self.failures.len() - 1))
        })
    }
}

/// Finite health-check run for P0. Each restart creates a fresh protocol session.
pub fn supervise(
    mut command: impl FnMut() -> Command,
    timeout: Duration,
    interval: Duration,
    checks: usize,
    base_delay: Duration,
) -> Result<usize> {
    ensure!(checks > 0, "checks must be positive");
    let mut budget = RestartBudget::new(base_delay);
    let mut attempts = 0;
    loop {
        attempts += 1;
        let result = (|| -> Result<()> {
            let mut host = Host::start(&mut command(), timeout)?;
            for _ in 0..checks {
                thread::sleep(interval);
                host.ping()?;
            }
            host.shutdown()
        })();
        match result {
            Ok(()) => return Ok(attempts),
            Err(error) => {
                eprintln!("supervisor attempt {attempts}: {error:#}");
                let delay = budget
                    .failure(Instant::now())
                    .context("restart budget exhausted: 3 failures within 60 seconds")?;
                thread::sleep(delay);
            }
        }
    }
}
