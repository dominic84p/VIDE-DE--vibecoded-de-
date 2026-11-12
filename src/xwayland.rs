//! Xwayland: runs an X server so X11-only apps (Steam, many games, older
//! toolkits) work. Their windows go through the same WM as Wayland ones.

use std::{os::unix::io::OwnedFd, process::Stdio};

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

use crate::{
    state::State,
    wm::{edge, WinKind},
};

pub fn start(handle: &LoopHandle<'static, State>, state: &mut State) {
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
            state.run_startup();
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
                state.run_startup();
            }
            Err(e) => {
                tracing::error!("failed to start X11 window manager: {e}");
                state.run_startup();
            }
        },
        XWaylandEvent::Error => {
            tracing::error!("Xwayland failed to start");
            state.run_startup();
        }
    });
    if let Err(e) = res {
        tracing::error!("failed to watch Xwayland: {e}");
        state.run_startup();
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

impl XwmHandler for State {
    fn xwm_state(&mut self, _xwm: XwmId) -> &mut X11Wm {
        self.xwm.as_mut().expect("X11 event before the X11 WM started")
    }

    fn new_window(&mut self, _xwm: XwmId, _window: X11Surface) {}
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

    fn configure_notify(&mut self, _xwm: XwmId, _window: X11Surface, _geometry: Rectangle<i32, Logical>, _above: Option<u32>) {
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

    fn send_selection(&mut self, _xwm: XwmId, _selection: SelectionTarget, _mime_type: String, _fd: OwnedFd) {}
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
