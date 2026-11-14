//! Several monitors side by side. The primary one (the first found) sits at (0, 0) and
//! carries the shell: taskbar, Start, flyouts, toasts, desktop icons, as on Windoze by
//! default. The others line up to its right, so no window ever needs a negative position
//! (X11 can't have one). Windows live on any of them and maximize, snap and go fullscreen
//! on the one they're on.
//!
//! Everything else keeps using `output_size` / `output_rect()` / `work_area()`, which are
//! the primary's.

use smithay::{
    output::{Mode, Output, Scale},
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle, Size},
};

use crate::state::State;

pub struct Monitor {
    pub output: Output,
    /// Where it sits in the global (logical) layout.
    pub geo: Rectangle<i32, Logical>,
}

impl State {
    /// A monitor came up (or the nested window was split into several).
    pub fn add_monitor(&mut self, output: &Output, mode: Mode) {
        if self.monitors.iter().any(|m| m.output == *output) {
            return self.set_output_mode(output, mode);
        }
        if self.monitors.is_empty() {
            self.output = Some(output.clone());
        }
        self.monitors.push(Monitor { output: output.clone(), geo: Rectangle::default() });
        tracing::info!("monitor {}: {}x{}", output.name(), mode.size.w, mode.size.h);
        self.set_output_mode(output, mode);
    }

    /// A monitor went away: its windows come back to the others.
    pub fn remove_monitor(&mut self, output: &Output) {
        self.monitors.retain(|m| m.output != *output);
        self.output = self.monitors.first().map(|m| m.output.clone());
        tracing::info!("monitor {} removed", output.name());
        self.relayout();
    }

    /// Put `output` in `mode` at the Settings scale, then lay all monitors out again.
    pub fn set_output_mode(&mut self, output: &Output, mode: Mode) {
        let scale = self.display_scale();
        output.change_current_state(Some(mode), None, Some(Scale::Fractional(scale)), None);
        output.set_preferred(mode);
        let changed = scale != self.output_scale;
        self.output_scale = scale;
        if changed {
            tracing::info!("display scale: {:.0}%", scale * 100.0);
            crate::canvas::set_scale(scale);
            self.repaint_shell();
            for w in &mut self.wm.windows {
                w.titlebar = None;
            }
            self.rescale_surfaces();
        }
        self.relayout();
    }

    /// Line the monitors up left to right, primary first, and fit everything to the result.
    fn relayout(&mut self) {
        let mut x = 0;
        for m in &mut self.monitors {
            let Some(mode) = m.output.current_mode() else { continue };
            let size: Size<i32, Logical> = mode.size.to_f64().to_logical(m.output.current_scale().fractional_scale()).to_i32_round();
            m.geo = Rectangle::new((x, 0).into(), size);
            m.output.change_current_state(None, None, None, Some(m.geo.loc));
            x += size.w;
        }
        if let Some(primary) = self.monitors.first() {
            self.output_size = primary.geo.size;
        }
        self.pointer_loc = self.clamp_to_monitors(self.pointer_loc, self.pointer_loc);
        self.arrange_layers();
        let screens: Vec<_> = self.monitors.iter().map(|m| (self.work_area_on(m.geo), m.geo)).collect();
        if !screens.is_empty() {
            self.wm.refit(&screens);
        }
        // Cached chrome was painted for the old size.
        self.taskbar = None;
        self.start.menu = None;
        self.wallpapers.clear();
        self.needs_redraw = true;
    }

    /// The monitor `p` is on, else the primary.
    pub fn monitor_at(&self, p: Point<f64, Logical>) -> Rectangle<i32, Logical> {
        self.monitors.iter().map(|m| m.geo).find(|g| g.to_f64().contains(p)).unwrap_or_else(|| self.output_rect())
    }

    /// The part of `screen` windows may fill: all of it, except the taskbar on the primary.
    pub fn work_area_on(&self, screen: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
        if screen.loc == Point::from((0, 0)) { Rectangle::from_size((screen.size.w, screen.size.h - crate::ui::TASKBAR_H).into()) } else { screen }
    }

    /// The monitor a window is on (the one holding its centre), else the pointer's.
    pub fn screen_of(&self, s: &WlSurface) -> Rectangle<i32, Logical> {
        let Some(w) = self.wm.find(s) else { return self.monitor_at(self.pointer_loc) };
        let g = w.geometry();
        let centre = g.loc.to_f64() + g.size.to_f64().downscale(2.0).to_point();
        self.monitors.iter().map(|m| m.geo).find(|m| m.to_f64().contains(centre)).unwrap_or_else(|| self.monitor_at(self.pointer_loc))
    }

    pub fn work_area_of(&self, s: &WlSurface) -> Rectangle<i32, Logical> {
        self.work_area_on(self.screen_of(s))
    }

    /// The box around every monitor: what the nested window shows, and what PrtSc captures.
    pub fn layout_rect(&self) -> Rectangle<i32, Logical> {
        self.monitors.iter().map(|m| m.geo).reduce(|a, b| a.merge(b)).unwrap_or_else(|| self.output_rect())
    }

    /// Keep the pointer on a monitor. Moving off every one (into the gap beside a shorter
    /// monitor) stops at the edge of the one it was on.
    pub fn clamp_to_monitors(&self, from: Point<f64, Logical>, to: Point<f64, Logical>) -> Point<f64, Logical> {
        if self.monitors.iter().any(|m| m.geo.to_f64().contains(to)) {
            return to;
        }
        let g = self.monitor_at(from).to_f64();
        Point::from((to.x.clamp(g.loc.x, g.loc.x + g.size.w - 1.0), to.y.clamp(g.loc.y, g.loc.y + g.size.h - 1.0)))
    }
}
