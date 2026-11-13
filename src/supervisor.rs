//! Keeping one broken piece from taking the session down. The compositor hosts a lot
//! of shell code (taskbar, tray, notifications) and a panic in any of it used to end
//! everything. `guard` contains a panic to the piece that raised it, `spawn_service`
//! restarts a crashed D-Bus thread, and `Supervisor` restarts helper programs
//! (vide-desktop, vide-portal) that die.

use std::{
    collections::VecDeque,
    panic::{catch_unwind, AssertUnwindSafe},
    process::Child,
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::state::State;

/// More crashes than this inside `WINDOW` and we stop retrying.
const MAX_CRASHES: usize = 5;
const WINDOW: Duration = Duration::from_secs(60);

/// Panics in `guard` this fast mean a loop that will never recover (a callback that
/// re-panics every tick). Better to exit than to spin and spam the log forever.
const STORM_PANICS: usize = 100;
const STORM_WINDOW: Duration = Duration::from_secs(5);

static RECENT_PANICS: Mutex<VecDeque<Instant>> = Mutex::new(VecDeque::new());

/// Run `f`; if it panics, log it and return `None` instead of unwinding further.
/// The panic message itself is already on stderr from the default hook.
pub fn guard<R>(what: &str, f: impl FnOnce() -> R) -> Option<R> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => Some(r),
        Err(_) => {
            tracing::error!("{what} panicked; carrying on without it");
            let now = Instant::now();
            let mut recent = RECENT_PANICS.lock().unwrap_or_else(|e| e.into_inner());
            recent.push_back(now);
            while recent.front().is_some_and(|t| now.duration_since(*t) > STORM_WINDOW) {
                recent.pop_front();
            }
            if recent.len() >= STORM_PANICS {
                tracing::error!("{STORM_PANICS} panics in {STORM_WINDOW:?}; giving up");
                std::process::exit(1);
            }
            None
        }
    }
}

/// Run a service on its own thread and rerun it if it panics. A normal return
/// (including "another daemon owns the name") is final.
pub fn spawn_service(name: &'static str, mut run: impl FnMut() + Send + 'static) {
    let res = std::thread::Builder::new().name(name.into()).spawn(move || {
        let mut crashes: VecDeque<Instant> = VecDeque::new();
        loop {
            if guard(name, &mut run).is_some() {
                return;
            }
            let now = Instant::now();
            crashes.push_back(now);
            while crashes.front().is_some_and(|t| now.duration_since(*t) > WINDOW) {
                crashes.pop_front();
            }
            if crashes.len() > MAX_CRASHES {
                tracing::error!("{name}: crashing repeatedly, disabled");
                return;
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    });
    if let Err(e) = res {
        tracing::warn!("{name}: couldn't start thread: {e}");
    }
}

/// Collect a launched program's exit status once it ends. Without this every program we
/// start, once it exits, stays in the process table as a zombie until we do.
pub fn reap(mut child: Child) {
    let _ = std::thread::Builder::new().name("reap".into()).spawn(move || {
        let _ = child.wait();
    });
}

struct Watched {
    name: &'static str,
    cmd: String,
    child: Option<Child>,
    crashes: VecDeque<Instant>,
    retry_at: Option<Instant>,
    gave_up: bool,
}

#[derive(Default)]
pub struct Supervisor {
    items: Vec<Watched>,
}

impl State {
    /// Start a helper program and keep it running: restarted when it crashes, left alone
    /// when it exits cleanly.
    pub fn supervise(&mut self, name: &'static str, cmd: &str) {
        let child = self.spawn_child(cmd);
        self.supervisor.items.push(Watched {
            name,
            cmd: cmd.to_string(),
            child,
            crashes: VecDeque::new(),
            retry_at: None,
            gave_up: false,
        });
    }

    /// Called every second: notice helpers that died and bring them back.
    pub fn supervisor_tick(&mut self) {
        let mut items = std::mem::take(&mut self.supervisor.items);
        let now = Instant::now();
        for w in &mut items {
            if let Some(child) = w.child.as_mut() {
                match child.try_wait() {
                    Ok(None) | Err(_) => continue,
                    Ok(Some(status)) => {
                        w.child = None;
                        // A crashed desktop or panel leaves a hole in the layers; redraw to show our fallback.
                        self.needs_redraw = true;
                        if status.success() {
                            tracing::info!("{} exited", w.name);
                            w.gave_up = true;
                            continue;
                        }
                        tracing::warn!("{} died ({status})", w.name);
                        w.crashes.push_back(now);
                        while w.crashes.front().is_some_and(|t| now.duration_since(*t) > WINDOW) {
                            w.crashes.pop_front();
                        }
                        if w.crashes.len() > MAX_CRASHES {
                            tracing::error!("{} keeps crashing; not restarting it", w.name);
                            w.gave_up = true;
                        } else {
                            w.retry_at = Some(now + Duration::from_secs(w.crashes.len() as u64));
                        }
                    }
                }
            } else if !w.gave_up && w.retry_at.is_some_and(|t| now >= t) {
                tracing::info!("restarting {}", w.name);
                w.retry_at = None;
                w.child = self.spawn_child(&w.cmd);
                if w.child.is_none() {
                    w.gave_up = true;
                }
            }
        }
        // Anything supervised while we were iterating (not expected) is kept too.
        items.append(&mut self.supervisor.items);
        self.supervisor.items = items;
    }
}
