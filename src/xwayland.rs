//! Xwayland: runs an X server so X11-only apps (Steam, many games, older
//! toolkits) work. Their windows go through the same WM as Wayland ones.

use std::{
    os::unix::io::OwnedFd,
    process::Stdio,
    time::{Duration, Instant},
};

use smithay::{
    delegate_xwayland_shell,
    reexports::{calloop::LoopHandle, wayland_server::protocol::wl_surface::WlSurface},
    utils::{Logical, Point, Rectangle, SERIAL_COUNTER},
    wayland::{
        selection::SelectionTarget,
        xwayland_shell::{XWaylandShellHandler, XWaylandShellState},
    },
    xwayland::{
        xwm::{Reorder, ResizeEdge, XwmId},
        X11Surface, X11Wm, XWayland, XWaylandEvent, XwmHandler,
    },
};

use smithay::reexports::calloop::{
    channel::Sender,
    timer::{TimeoutAction, Timer},
};

use crate::{
    icons::Icon,
    state::State,
    wm::{edge, WinKind},
};

pub fn start(handle: &LoopHandle<'static, State>, state: &mut State) {
    spawn(handle, state, true);
}

/// Start Xwayland. The first start also runs session startup once X is up (or failed),
/// so startup apps get a DISPLAY; a restart after a crash doesn't run it again.
fn spawn(handle: &LoopHandle<'static, State>, state: &mut State, first: bool) {
    let startup = move |state: &mut State| {
        if first {
            state.run_startup();
        }
    };
    let (xwayland, client) = match XWayland::spawn(
        &state.dh,
        None,
        std::iter::empty::<(String, String)>(),
        true,
        Stdio::null(),
        Stdio::null(),
        |_| (),
    ) {
        Ok(x) => x,
        Err(e) => {
            tracing::warn!("Xwayland not started ({e}); X11 apps won't run");
            startup(state);
            return;
        }
    };

    let h = handle.clone();
    let res = handle.insert_source(xwayland, move |event, _, state| match event {
        XWaylandEvent::Ready { x11_socket, display_number } => match X11Wm::start_wm(h.clone(), x11_socket, client.clone()) {
            Ok(wm) => {
                tracing::info!("Xwayland ready on :{display_number}");
                state.xwm = Some(wm);
                state.xdisplay = Some(display_number);
                if !first {
                    // A new server may have a new display number; D-Bus-activated apps need it too.
                    state.spawn("dbus-update-activation-environment --systemd DISPLAY 2>/dev/null || dbus-update-activation-environment DISPLAY");
                }
                startup(state);
            }
            Err(e) => {
                tracing::error!("failed to start X11 window manager: {e}");
                startup(state);
            }
        },
        XWaylandEvent::Error => {
            tracing::error!("Xwayland failed to start");
            startup(state);
        }
    });
    if let Err(e) = res {
        tracing::error!("failed to watch Xwayland: {e}");
        startup(state);
    }
}

impl State {
    /// Start managing an X11 window once it's both mapped and has its Wayland surface.
    fn manage_x11(&mut self, window: X11Surface) {
        if window.is_override_redirect() || !self.x11_pending.contains(&window) {
            return;
        }
        let Some(surface) = window.wl_surface() else { return };
        self.x11_pending.retain(|w| w != &window);
        if self.wm.find(&surface).is_some() {
            return;
        }
        // We draw a titlebar unless the app draws its own (or it's a popup-ish window).
        let ssd = !window.is_decorated() && !window.is_popup();
        let g = window.geometry();
        let pos = (g.loc != Point::from((0, 0))).then_some(g.loc);
        // Apps without an installed .desktop file (Minecraft, terminal-launched apps) carry their icon on the window.
        if let (Some(display), Some(tx)) = (self.xdisplay, self.x11_icon_tx.clone()) {
            let class = if window.class().is_empty() { window.instance() } else { window.class() };
            fetch_window_icon(display, window.window_id(), class, tx);
        }
        self.wm.add(WinKind::X11(window), surface.clone(), pos, ssd);
        if let Some(w) = self.wm.find(&surface) {
            w.configure(None, &[]);
        }
        self.focus(Some(surface), SERIAL_COUNTER.next_serial());
    }

    fn forget_x11(&mut self, window: &X11Surface) {
        self.x11_pending.retain(|w| w != window);
        self.x11_overrides.retain(|w| w != window);
        let managed = self.wm.windows.iter().find(|w| w.x11() == Some(window)).map(|w| w.surface().clone());
        if let Some(s) = managed {
            let was_focused = self.focused().as_ref() == Some(&s);
            self.wm.remove(&s);
            if was_focused {
                self.focus_top();
            }
        }
        self.needs_redraw = true;
    }

    fn x11_surface(&self, window: &X11Surface) -> Option<WlSurface> {
        window.wl_surface().filter(|s| self.wm.find(s).is_some())
    }
}

/// Read `_NET_WM_ICON` on a background thread: Xwayland is served by our own main
/// thread, so a blocking X request from it could deadlock.
fn fetch_window_icon(display: u32, window: u32, class: String, tx: Sender<(String, Icon)>) {
    std::thread::spawn(move || {
        // Games often set the icon a moment after mapping; try a few times.
        for attempt in 0..3 {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(1500));
            }
            if let Some(icon) = read_net_wm_icon(display, window) {
                let _ = tx.send((class, icon));
                return;
            }
        }
    });
}

fn read_net_wm_icon(display: u32, window: u32) -> Option<Icon> {
    use x11rb::{
        connection::Connection as _,
        protocol::xproto::{AtomEnum, ConnectionExt as _},
        rust_connection::RustConnection,
    };
    let (conn, _) = RustConnection::connect(Some(&format!(":{display}"))).ok()?;
    let atom = conn.intern_atom(false, b"_NET_WM_ICON").ok()?.reply().ok()?.atom;
    let reply = conn.get_property(false, window, atom, AtomEnum::CARDINAL, 0, 1 << 22).ok()?.reply().ok()?;
    let data: Vec<u32> = reply.value32()?.collect();
    let _ = conn.flush();

    // A list of (width, height, width*height ARGB pixels); take the one closest to 48px.
    let (mut best, mut i): (Option<(usize, usize, usize)>, usize) = (None, 0);
    while i + 2 <= data.len() {
        let (w, h) = (data[i] as usize, data[i + 1] as usize);
        if w == 0 || h == 0 || i + 2 + w * h > data.len() {
            break;
        }
        if best.is_none_or(|(bw, _, _)| (w as i64 - 48).abs() < (bw as i64 - 48).abs()) {
            best = Some((w, h, i + 2));
        }
        i += 2 + w * h;
    }
    let (w, h, start) = best?;
    let mut rgba = Vec::with_capacity(w * h * 4);
    for &px in &data[start..start + w * h] {
        let (a, r, g, b) = (px >> 24, (px >> 16) & 0xff, (px >> 8) & 0xff, px & 0xff);
        rgba.extend_from_slice(&[(r * a / 255) as u8, (g * a / 255) as u8, (b * a / 255) as u8, a as u8]);
    }
    Some(Icon { w: w as u32, h: h as u32, rgba })
}

impl XwmHandler for State {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        self.xwm.as_mut().expect("X11 event before the X11 WM started")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    /// Xwayland died. Its windows are gone with it; without a new server no X11 app
    /// (Steam, games, Electron apps on X11) can start until the next login.
    fn disconnected(&mut self, _xwm: XwmId) {
        tracing::warn!("Xwayland died");
        self.xwm = None;
        self.xdisplay = None;
        for w in std::mem::take(&mut self.x11_overrides).into_iter().chain(std::mem::take(&mut self.x11_pending)) {
            self.forget_x11(&w);
        }
        let dead: Vec<X11Surface> = self.wm.windows.iter().filter_map(|w| w.x11().cloned()).collect();
        for w in &dead {
            self.forget_x11(w);
        }

        let now = Instant::now();
        self.xwayland_crashes.push_back(now);
        while self.xwayland_crashes.front().is_some_and(|t| now.duration_since(*t) > Duration::from_secs(60)) {
            self.xwayland_crashes.pop_front();
        }
        if self.xwayland_crashes.len() > 5 {
            return tracing::error!("Xwayland keeps crashing; not restarting it");
        }
        let delay = Duration::from_secs(self.xwayland_crashes.len() as u64);
        let handle = self.loop_handle.clone();
        let _ = self.loop_handle.insert_source(Timer::from_duration(delay), move |_, _, state| {
            tracing::info!("restarting Xwayland");
            spawn(&handle, state, false);
            TimeoutAction::Drop
        });
    }
    fn new_override_redirect_window(&mut self, _xwm: XwmId, _window: X11Surface) {}

    fn map_window_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Err(e) = window.set_mapped(true) {
            tracing::warn!("failed to map X11 window: {e}");
            return;
        }
        if !self.x11_pending.contains(&window) {
            self.x11_pending.push(window.clone());
        }
        self.manage_x11(window);
    }

    fn mapped_override_redirect_window(&mut self, _xwm: XwmId, window: X11Surface) {
        if !self.x11_overrides.contains(&window) {
            self.x11_overrides.push(window);
        }
        self.needs_redraw = true;
    }

    fn unmapped_window(&mut self, _xwm: XwmId, window: X11Surface) {
        self.forget_x11(&window);
        if !window.is_override_redirect() {
            let _ = window.set_mapped(false);
        }
    }

    fn destroyed_window(&mut self, _xwm: XwmId, window: X11Surface) {
        self.forget_x11(&window);
    }

    fn configure_request(
        &mut self,
        _xwm: XwmId,
        window: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _reorder: Option<Reorder>,
    ) {
        let mut g = window.geometry();
        if let Some(w) = w {
            g.size.w = w as i32;
        }
        if let Some(h) = h {
            g.size.h = h as i32;
        }
        match self.x11_surface(&window) {
            // Managed: grant the size, but where it sits is our call.
            Some(s) => {
                if let Some(win) = self.wm.find(&s) {
                    win.configure(Some(g.size), &[]);
                }
            }
            None => {
                if let Some(x) = x {
                    g.loc.x = x;
                }
                if let Some(y) = y {
                    g.loc.y = y;
                }
                let _ = window.configure(g);
            }
        }
    }

    fn configure_notify(&mut self, _xwm: XwmId, window: X11Surface, _geometry: Rectangle<i32, Logical>, above: Option<u32>) {
        if window.is_override_redirect() {
            if !window.is_mapped() {
                self.x11_overrides.retain(|w| w != &window);
            } else if let Some(pos) = self.x11_overrides.iter().position(|w| w == &window) {
                let w = self.x11_overrides.remove(pos);
                let insert_at = above
                    .and_then(|id| self.x11_overrides.iter().position(|o| o.window_id() == id))
                    .map_or(0, |idx| idx + 1);
                self.x11_overrides.insert(insert_at, w);
            }
        }
        self.needs_redraw = true;
    }

    fn resize_request(&mut self, _xwm: XwmId, window: X11Surface, _button: u32, resize_edge: ResizeEdge) {
        if let Some(s) = self.x11_surface(&window) {
            let e = match resize_edge {
                ResizeEdge::Top => edge::TOP,
                ResizeEdge::Bottom => edge::BOTTOM,
                ResizeEdge::Left => edge::LEFT,
                ResizeEdge::Right => edge::RIGHT,
                ResizeEdge::TopLeft => edge::TOP | edge::LEFT,
                ResizeEdge::TopRight => edge::TOP | edge::RIGHT,
                ResizeEdge::BottomLeft => edge::BOTTOM | edge::LEFT,
                ResizeEdge::BottomRight => edge::BOTTOM | edge::RIGHT,
            };
            let pos = self.pointer_loc;
            self.wm.start_resize(&s, e, pos);
        }
    }

    fn move_request(&mut self, _xwm: XwmId, window: X11Surface, _button: u32) {
        if let Some(s) = self.x11_surface(&window) {
            self.begin_move(&s);
        }
    }

    fn maximize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(s) = self.x11_surface(&window) {
            let area = self.work_area();
            self.wm.maximize(&s, area);
        }
    }

    fn unmaximize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(s) = self.x11_surface(&window) {
            self.wm.unmaximize(&s);
        }
    }

    fn fullscreen_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(s) = self.x11_surface(&window) {
            let rect = self.output_rect();
            self.wm.fullscreen(&s, rect);
        }
    }

    fn unfullscreen_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(s) = self.x11_surface(&window) {
            self.wm.unfullscreen(&s);
        }
    }

    fn minimize_request(&mut self, _xwm: XwmId, window: X11Surface) {
        if let Some(s) = self.x11_surface(&window) {
            self.minimize(&s);
        }
    }

    fn allow_selection_access(&mut self, _xwm: XwmId, _selection: SelectionTarget) -> bool {
        matches!(self.keyboard.current_focus(), Some(crate::focus::FocusTarget::X11(_)))
    }

    fn send_selection(&mut self, _xwm: XwmId, selection: SelectionTarget, mime_type: String, fd: OwnedFd) {
        match selection {
            SelectionTarget::Clipboard => {
                // A clipboard we own ourselves (a screenshot): Smithay's client-selection request
                // refuses those, so hand the bytes over directly.
                let ours = smithay::wayland::selection::data_device::current_data_device_selection_userdata(&self.seat)
                    .and_then(|u| match &*u {
                        crate::state::SelectionOwner::Bytes(data) => Some(data.clone()),
                        crate::state::SelectionOwner::X11 => None,
                    });
                match ours {
                    Some(data) => crate::state::write_clipboard(data, fd),
                    None => {
                        let _ = smithay::wayland::selection::data_device::request_data_device_client_selection(
                            &self.seat,
                            mime_type,
                            fd,
                        );
                    }
                }
            }
            SelectionTarget::Primary => {
                let _ = smithay::wayland::selection::primary_selection::request_primary_client_selection(
                    &self.seat,
                    mime_type,
                    fd,
                );
            }
        }
    }

    fn new_selection(&mut self, _xwm: XwmId, selection: SelectionTarget, mime_types: Vec<String>) {
        match selection {
            SelectionTarget::Clipboard => {
                smithay::wayland::selection::data_device::set_data_device_selection(
                    &self.dh,
                    &self.seat,
                    mime_types,
                    crate::state::SelectionOwner::X11,
                );
            }
            SelectionTarget::Primary => {
                smithay::wayland::selection::primary_selection::set_primary_selection(
                    &self.dh,
                    &self.seat,
                    mime_types,
                    crate::state::SelectionOwner::X11,
                );
            }
        }
    }

    fn cleared_selection(&mut self, _xwm: XwmId, selection: SelectionTarget) {
        match selection {
            SelectionTarget::Clipboard => {
                smithay::wayland::selection::data_device::clear_data_device_selection(&self.dh, &self.seat);
            }
            SelectionTarget::Primary => {
                smithay::wayland::selection::primary_selection::clear_primary_selection(&self.dh, &self.seat);
            }
        }
    }
}

impl XWaylandShellHandler for State {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell_state
    }

    fn surface_associated(&mut self, _xwm: XwmId, _surface: WlSurface, window: X11Surface) {
        self.manage_x11(window);
        self.needs_redraw = true;
    }
}

delegate_xwayland_shell!(State);
