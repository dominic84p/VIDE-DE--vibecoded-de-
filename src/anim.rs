//! Windoze 10's animations: windows zoom in when they open or come back from the taskbar and
//! shrink away when minimized; Start, the Action Center, flyouts, Task View and menus slide or
//! fade in. Closing a panel is instant, as on Windoze. Settings → Ease of Access → Display →
//! Show animations (`animations=false`) turns them all off.
//!
//! Nothing here keeps its own timer. `render` works out how far along each animation is from
//! when it began, and while any is still running it asks for another frame
//! (`State::needs_redraw`), so the backends draw every vblank until it settles.

use std::time::Instant;

/// Durations, in milliseconds: snappier than Windoze 10's, so nothing ever feels like a wait.
/// Input never waits for them either; a window being minimized is already out of the way.
pub const WINDOW_OPEN: u64 = 130;
pub const WINDOW_MINIMIZE: u64 = 110;
pub const START: u64 = 140;
pub const ACTION_CENTER: u64 = 150;
pub const FLYOUT: u64 = 100;
pub const TASK_VIEW: u64 = 120;
pub const MENU: u64 = 70;

/// Fast start, gentle landing.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Linear progress (0..=1) of an animation that began at `since` and lasts `ms`.
pub fn progress(since: Instant, ms: u64) -> f32 {
    (since.elapsed().as_secs_f32() * 1000.0 / ms as f32).min(1.0)
}

/// Notices something opening, to animate it in.
#[derive(Default)]
pub struct Edge {
    open: bool,
    since: Option<Instant>,
}

impl Edge {
    /// Eased progress (0..=1) of the opening animation for something that is now `open`.
    pub fn update(&mut self, open: bool, ms: u64) -> f32 {
        if open && !self.open {
            self.since = Some(Instant::now());
        }
        self.open = open;
        let Some(since) = self.since.filter(|_| open) else { return 1.0 };
        let p = progress(since, ms);
        if p >= 1.0 {
            self.since = None;
        }
        ease_out(p)
    }
}

#[derive(Default)]
pub struct Anims {
    pub start: Edge,
    pub action_center: Edge,
    pub flyout: Edge,
    pub task_view: Edge,
    pub menu: Edge,
}
