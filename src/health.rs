//! Knowing when something is stuck, and saying so in the log. A frozen app and a frozen
//! compositor look the same from the chair, and without this `~/vibe.log` can't tell them apart.
//!
//! - Pings: every few seconds each app's xdg client is asked to answer. One that doesn't within
//!   a few seconds is logged by name and pid, and its titlebar says "(Not responding)".
//! - Watchdog: a thread that notices when our own main loop stops making progress and says
//!   which phase it is stuck in, and when a display page flip never completes.

use std::{
    sync::{
        OnceLock,
        atomic::{AtomicU64, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use smithay::{
    utils::SERIAL_COUNTER,
    wayland::shell::{PingError, xdg::ShellClient},
};

use crate::{state::State, wm::WinKind};

const PING_EVERY: Duration = Duration::from_secs(5);
/// How long an app may take to answer before we call it stuck.
const GRACE: Duration = Duration::from_secs(3);

struct Ping {
    client: ShellClient,
    label: String,
    sent: Option<Instant>,
    /// When we first called it stuck, and the last long-wait milestone we logged.
    stuck_since: Option<Instant>,
    milestone: u64,
}

#[derive(Default)]
pub struct Pings {
    entries: Vec<Ping>,
    last_round: Option<Instant>,
}

impl State {
    /// Called every second.
    pub fn ping_tick(&mut self) {
        let now = Instant::now();
        let new_round = self.pings.last_round.is_none_or(|t| now.duration_since(t) >= PING_EVERY);
        if new_round {
            self.pings.last_round = Some(now);
            self.enroll_clients();
        }

        for e in &mut self.pings.entries {
            if new_round && e.sent.is_none() {
                // Fails if one is still pending: that is the stuck case, handled below.
                if !matches!(e.client.send_ping(SERIAL_COUNTER.next_serial()), Err(PingError::PingAlreadyPending(_))) {
                    e.sent = Some(now);
                }
            }
            let Some(sent) = e.sent else { continue };
            let waited = now.duration_since(sent);
            if waited < GRACE {
                continue;
            }
            let since = *e.stuck_since.get_or_insert(now - (waited - GRACE));
            if e.milestone == 0 {
                tracing::warn!("{} isn't answering pings ({}s)", e.label, waited.as_secs());
                e.milestone = 1;
            } else {
                // Again at 15 s, 60 s, then every minute: enough to see how long, not a flood.
                let secs = now.duration_since(since).as_secs();
                let next = [15, 60].get(e.milestone as usize - 1).copied().unwrap_or(60 * (e.milestone - 1));
                if secs >= next {
                    tracing::warn!("{} still not answering after {}s", e.label, waited.as_secs());
                    e.milestone += 1;
                }
            }
        }
        self.pings.entries.retain(|e| e.client.alive());
        self.mark_unresponsive();
    }

    /// An app answered.
    pub fn pong(&mut self, client: &ShellClient) {
        if let Some(e) = self.pings.entries.iter_mut().find(|e| e.client == *client) {
            if e.stuck_since.take().is_some() {
                let waited = e.sent.map(|s| s.elapsed().as_secs()).unwrap_or_default();
                tracing::info!("{} is answering again after {waited}s", e.label);
            }
            e.sent = None;
            e.milestone = 0;
        }
        self.mark_unresponsive();
    }

    /// Make sure every window's client is on the list, with a name for the log.
    fn enroll_clients(&mut self) {
        let mut found = Vec::new();
        for w in &self.wm.windows {
            if let WinKind::Wayland(top) = &w.kind {
                let client = top.client();
                if self.pings.entries.iter().all(|e| e.client != client) && found.iter().all(|(c, _): &(ShellClient, String)| *c != client) {
                    let pid = self.window_pid(w.surface()).map(|p| p.to_string()).unwrap_or_else(|| "?".into());
                    let name = Some(w.app_id()).filter(|a| !a.is_empty()).unwrap_or_else(|| w.title());
                    found.push((client, format!("{name} (pid {pid})")));
                }
            }
        }
        for (client, label) in found {
            self.pings.entries.push(Ping { client, label, sent: None, stuck_since: None, milestone: 0 });
        }
    }

    fn mark_unresponsive(&mut self) {
        let stuck: Vec<&ShellClient> = self.pings.entries.iter().filter(|e| e.stuck_since.is_some()).map(|e| &e.client).collect();
        let mut changed = false;
        for w in &mut self.wm.windows {
            let now_stuck = matches!(&w.kind, WinKind::Wayland(top) if stuck.contains(&&top.client()));
            if w.unresponsive != now_stuck {
                w.unresponsive = now_stuck;
                changed = true;
            }
        }
        if changed {
            self.needs_redraw = true;
        }
    }
}

// ------------------------------------------------------------------ watchdog

#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Phase {
    /// Waiting for events, or running their callbacks.
    Dispatch,
    Render,
    Flush,
}
const PHASES: [&str; 3] = ["dispatching events", "rendering a frame", "flushing to clients"];

fn started() -> Instant {
    static START: OnceLock<Instant> = OnceLock::new();
    *START.get_or_init(Instant::now)
}
fn now_ms() -> u64 {
    started().elapsed().as_millis() as u64
}

static BEAT: AtomicU64 = AtomicU64::new(0);
static PHASE: AtomicU8 = AtomicU8::new(0);
/// When the page flip in flight was queued (ms since start), or 0 for none.
static FLIP_SINCE: AtomicU64 = AtomicU64::new(0);

/// The main loop reached `phase`. It must do this often: every second at least, since the
/// clock timer wakes it.
pub fn enter(phase: Phase) {
    PHASE.store(phase as u8, Ordering::Relaxed);
    BEAT.store(now_ms(), Ordering::Relaxed);
}

// Only the TTY backend has page flips to watch.
#[cfg_attr(not(feature = "tty"), allow(dead_code))]
pub fn flip_queued() {
    FLIP_SINCE.store(now_ms().max(1), Ordering::Relaxed);
}

#[cfg_attr(not(feature = "tty"), allow(dead_code))]
pub fn flip_done() {
    FLIP_SINCE.store(0, Ordering::Relaxed);
}

pub fn start() {
    enter(Phase::Dispatch);
    let _ = std::thread::Builder::new().name("watchdog".into()).spawn(|| {
        let (mut reported_stall, mut reported_flip) = (false, false);
        loop {
            std::thread::sleep(Duration::from_millis(500));
            let now = now_ms();

            let quiet = now.saturating_sub(BEAT.load(Ordering::Relaxed));
            let phase = PHASES[(PHASE.load(Ordering::Relaxed) as usize).min(PHASES.len() - 1)];
            if quiet >= 3000 {
                if !reported_stall || quiet % 10_000 < 500 {
                    tracing::warn!("main loop stuck for {}s while {phase}", quiet / 1000);
                }
                reported_stall = true;
            } else if reported_stall {
                tracing::info!("main loop is moving again");
                reported_stall = false;
            }

            let flip = FLIP_SINCE.load(Ordering::Relaxed);
            if flip != 0 && now.saturating_sub(flip) >= 1500 {
                if !reported_flip {
                    tracing::warn!("page flip not completed after {}ms: the display may be stalled", now - flip);
                }
                reported_flip = true;
            } else if reported_flip {
                tracing::info!("page flips are completing again");
                reported_flip = false;
            }
        }
    });
}
