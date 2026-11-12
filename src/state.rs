//! Compositor state and the protocol handlers Smithay asks us to implement.

use std::{os::unix::io::OwnedFd, time::Instant};

use fontdue::Font;
use smithay::{
    backend::renderer::{element::memory::MemoryRenderBuffer, utils::on_commit_buffer_handler},
    delegate_compositor, delegate_data_device, delegate_output, delegate_primary_selection, delegate_seat,
    delegate_shm, delegate_xdg_decoration, delegate_xdg_shell,
    desktop::{
        find_popup_root_surface, get_popup_toplevel_coords, PopupKeyboardGrab, PopupKind, PopupManager,
        PopupPointerGrab, PopupUngrabStrategy,
    },
    input::{
        keyboard::KeyboardHandle,
        pointer::{CursorImageStatus, Focus, PointerHandle},
        Seat, SeatHandler, SeatState,
    },
    reexports::{
        wayland_protocols::xdg::{
            decoration::zv1::server::zxdg_toplevel_decoration_v1::Mode as DecoMode, shell::server::xdg_toplevel,
        },
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::{wl_buffer, wl_output, wl_seat, wl_surface::WlSurface},
            Client, DisplayHandle, Resource,
        },
    },
    utils::{Logical, Point, Rectangle, Serial, Size, SERIAL_COUNTER},
    wayland::{
        buffer::BufferHandler,
        compositor::{get_parent, is_sync_subsurface, CompositorClientState, CompositorHandler, CompositorState},
        output::{OutputHandler, OutputManagerState},
        selection::{
            data_device::{
                set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState, ServerDndGrabHandler,
            },
            primary_selection::{set_primary_focus, PrimarySelectionHandler, PrimarySelectionState},
            SelectionHandler,
        },
        shell::xdg::{
            decoration::{XdgDecorationHandler, XdgDecorationState},
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
        },
        shm::{ShmHandler, ShmState},
    },
};

use crate::{
    apps::{self, App},
    canvas,
    cursor::Cursors,
    icons::Icons,
    focus::FocusTarget,
    ui::{StartHit, StartMenu, TaskbarHit, Taskbar, TitleButton, TASKBAR_H},
    wm::{Flag, WinKind, Wm},
};
use smithay::{
    wayland::xwayland_shell::XWaylandShellState,
    xwayland::{X11Surface, X11Wm},
};

pub fn clock_now() -> (String, String) {
    let now = chrono::Local::now();
    (now.format("%-I:%M %p").to_string(), now.format("%-m/%-d/%Y").to_string())
}

/// What the pointer is hovering, for highlight effects.
#[derive(Clone, PartialEq, Default)]
pub enum Hover {
    #[default]
    None,
    Taskbar(TaskbarHit),
    Title(WlSurface, TitleButton),
    Start(StartHit),
}

#[derive(Default)]
pub struct Start {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub scroll: usize,
    pub menu: Option<StartMenu>,
}

pub struct State {
    pub dh: DisplayHandle,
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub xdg_decoration_state: XdgDecorationState,
    pub shm_state: ShmState,
    pub seat_state: SeatState<Self>,
    pub data_device_state: DataDeviceState,
    pub primary_selection_state: PrimarySelectionState,
    pub _output_manager_state: OutputManagerState,
    pub popups: PopupManager,

    pub seat: Seat<Self>,
    pub keyboard: KeyboardHandle<Self>,
    pub pointer: PointerHandle<Self>,
    pub pointer_loc: Point<f64, Logical>,
    pub seen_pointer: bool,
    pub cursor_status: CursorImageStatus,
    pub cursors: Cursors,

    pub font: Option<Font>,
    pub icons: Icons,
    pub apps: Vec<App>,
    pub taskbar: Option<Taskbar>,
    pub start: Start,
    pub wallpaper: Option<(Size<i32, Logical>, MemoryRenderBuffer)>,
    pub hover: Hover,
    pub clock: (String, String),
    /// For double-click-to-maximize on titlebars.
    pub last_title_click: Option<(WlSurface, Instant)>,
    /// Super pressed with nothing else yet: releasing it opens the start menu.
    pub super_tap: bool,

    pub wm: Wm,
    pub xwm: Option<X11Wm>,
    pub xwayland_shell_state: XWaylandShellState,
    /// Xwayland's display number, once it's up (for apps' DISPLAY).
    pub xdisplay: Option<u32>,
    /// X11 windows that asked to be mapped but whose Wayland surface hasn't shown up yet.
    pub x11_pending: Vec<X11Surface>,
    /// X11 menus/tooltips: they position themselves and sit above everything.
    pub x11_overrides: Vec<X11Surface>,
    pub socket_name: String,
    /// Command to launch once the session is fully up (`VIBE_STARTUP`).
    pub startup: Option<String>,
    pub running: bool,
    /// Running on a real TTY rather than nested.
    pub tty: bool,
    /// VT switch requested by a keybind; the TTY backend acts on it.
    pub pending_vt: Option<i32>,
    /// Logical size of the (single, for now) output; input coords map onto this.
    pub output_size: Size<i32, Logical>,
    /// Set whenever anything visible changed. Backends only draw when this is set.
    pub needs_redraw: bool,
    pub start_time: Instant,
}

impl State {
    pub fn new(dh: DisplayHandle, socket_name: String) -> Self {
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, "seat0");
        let keyboard = seat.add_keyboard(Default::default(), 300, 30).unwrap();
        let pointer = seat.add_pointer();

        Self {
            compositor_state: CompositorState::new::<Self>(&dh),
            xdg_shell_state: XdgShellState::new::<Self>(&dh),
            xdg_decoration_state: XdgDecorationState::new::<Self>(&dh),
            shm_state: ShmState::new::<Self>(&dh, vec![]),
            data_device_state: DataDeviceState::new::<Self>(&dh),
            primary_selection_state: PrimarySelectionState::new::<Self>(&dh),
            _output_manager_state: OutputManagerState::new_with_xdg_output::<Self>(&dh),
            popups: PopupManager::default(),
            seat_state,
            seat,
            keyboard,
            pointer,
            pointer_loc: (0.0, 0.0).into(),
            seen_pointer: false,
            cursor_status: CursorImageStatus::default_named(),
            cursors: Cursors::load(),
            font: canvas::load_font(),
            icons: Icons::new(),
            apps: apps::scan(),
            taskbar: None,
            start: Start::default(),
            wallpaper: None,
            hover: Hover::None,
            clock: clock_now(),
            last_title_click: None,
            super_tap: false,
            wm: Wm::default(),
            xwm: None,
            xwayland_shell_state: XWaylandShellState::new::<Self>(&dh),
            xdisplay: None,
            x11_pending: Vec::new(),
            x11_overrides: Vec::new(),
            socket_name,
            startup: None,
            running: true,
            tty: false,
            pending_vt: None,
            output_size: (1280, 800).into(),
            needs_redraw: true,
            start_time: Instant::now(),
            dh,
        }
    }

    pub fn output_rect(&self) -> Rectangle<i32, Logical> {
        Rectangle::from_size(self.output_size)
    }

    /// The screen minus the taskbar: where maximized windows go.
    pub fn work_area(&self) -> Rectangle<i32, Logical> {
        Rectangle::from_size((self.output_size.w, self.output_size.h - TASKBAR_H).into())
    }

    /// Give keyboard focus to `surface`, raise and un-minimize its window, mark it activated.
    pub fn focus(&mut self, surface: Option<WlSurface>, serial: Serial) {
        if let Some(s) = &surface {
            self.wm.raise(s);
            if let Some(w) = self.wm.find_mut(s) {
                w.minimized = false;
            }
        }
        for w in &self.wm.windows {
            let active = Some(w.surface()) == surface.as_ref();
            w.configure(None, &[(Flag::Activated, active)]);
        }
        // X11 windows need the X server told about focus and stacking too.
        let target = surface.map(|s| match self.wm.find(&s).and_then(|w| w.x11()) {
            Some(x) => {
                if let Some(xwm) = self.xwm.as_mut() {
                    let _ = xwm.raise_window(x);
                }
                FocusTarget::X11(x.clone())
            }
            None => FocusTarget::Wayland(s),
        });
        let kb = self.keyboard.clone();
        kb.set_focus(self, target, serial);
        self.needs_redraw = true;
    }

    pub fn focused(&self) -> Option<WlSurface> {
        self.keyboard.current_focus().and_then(|f| f.surface())
    }

    pub fn focus_top(&mut self) {
        let top = self.wm.top().map(|w| w.surface().clone());
        self.focus(top, SERIAL_COUNTER.next_serial());
    }

    pub fn minimize(&mut self, surface: &WlSurface) {
        if let Some(w) = self.wm.find_mut(surface) {
            w.minimized = true;
        }
        if self.focused().as_ref() == Some(surface) {
            self.focus_top();
        }
        self.needs_redraw = true;
    }

    /// Taskbar click: restore if minimized, minimize if already focused, else focus.
    pub fn activate_from_taskbar(&mut self, surface: &WlSurface) {
        let Some(w) = self.wm.find(surface) else { return };
        if !w.minimized && self.focused().as_ref() == Some(surface) {
            self.minimize(surface);
        } else {
            self.focus(Some(surface.clone()), SERIAL_COUNTER.next_serial());
        }
    }

    pub fn show_desktop(&mut self) {
        for w in &mut self.wm.windows {
            w.minimized = true;
        }
        self.focus(None, SERIAL_COUNTER.next_serial());
    }

    /// Dragging a maximized window off its titlebar restores it under the pointer, like Windows.
    pub fn begin_move(&mut self, surface: &WlSurface) {
        let pos = self.pointer_loc;
        if let Some(w) = self.wm.find(surface) {
            if let Some(restore) = w.maximized {
                let g = w.geometry();
                let frac = ((pos.x - g.loc.x as f64) / g.size.w.max(1) as f64).clamp(0.0, 1.0);
                self.wm.unmaximize(surface);
                if let Some(w) = self.wm.find_mut(surface) {
                    // Keep the pointer at the same spot along the (now narrower) titlebar.
                    w.pos.x = (pos.x - frac * restore.size.w as f64) as i32;
                    w.pos.y = pos.y as i32 + if w.ssd { crate::ui::TITLE_H / 2 } else { 0 };
                }
            }
        }
        self.wm.start_move(surface, pos);
    }

    fn unconstrain_popup(&self, popup: &PopupSurface) {
        let kind = PopupKind::Xdg(popup.clone());
        let Ok(root) = find_popup_root_surface(&kind) else { return };
        let Some(w) = self.wm.find(&root) else { return };
        let mut target = self.output_rect();
        target.loc -= w.surface_loc();
        target.loc -= get_popup_toplevel_coords(&kind);
        popup.with_pending_state(|s| s.geometry = s.positioner.get_unconstrained_geometry(target));
    }

    // ------------------------------------------------------------ launching

    pub fn spawn(&self, cmd: &str) {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(cmd);
        self.app_env(&mut c);
        if let Err(e) = c.spawn() {
            tracing::warn!("failed to spawn {cmd}: {e}");
        }
    }

    pub fn run_startup(&mut self) {
        if let Some(cmd) = self.startup.take() {
            self.spawn(&cmd);
        }
    }

    pub fn launch(&self, app: &App) {
        tracing::info!("launching {} ({})", app.name, app.exec);
        if app.terminal {
            self.spawn(&format!("{} -e {}", crate::input::TERMINAL, app.exec));
        } else {
            self.spawn(&app.exec);
        }
    }

    /// Environment that steers toolkits onto our Wayland socket.
    fn app_env(&self, c: &mut std::process::Command) {
        c.env("WAYLAND_DISPLAY", &self.socket_name)
            .env("XDG_SESSION_TYPE", "wayland")
            .env("QT_QPA_PLATFORM", "wayland;xcb")
            .env("GDK_BACKEND", "wayland,x11")
            .env("MOZ_ENABLE_WAYLAND", "1")
            .env("ELECTRON_OZONE_PLATFORM_HINT", "auto")
            .env("_JAVA_AWT_WM_NONREPARENTING", "1");
        // X11-only apps go through our Xwayland, never the host's X server.
        match self.xdisplay {
            Some(n) => c.env("DISPLAY", format!(":{n}")),
            None => c.env_remove("DISPLAY"),
        };
        // Let Qt/KDE apps pick up the user's KDE theme and icons.
        if std::env::var_os("XDG_CURRENT_DESKTOP").is_none() {
            c.env("XDG_CURRENT_DESKTOP", "vibe:KDE");
        }
    }

    pub fn filtered_apps(&self) -> Vec<usize> {
        self.apps.iter().enumerate().filter(|(_, a)| apps::matches(a, &self.start.query)).map(|(i, _)| i).collect()
    }

    pub fn toggle_start(&mut self) {
        self.start.open = !self.start.open;
        self.start.query.clear();
        self.start.selected = 0;
        self.start.scroll = 0;
        self.needs_redraw = true;
    }
}

impl CompositorHandler for State {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(&self, client: &'a Client) -> &'a CompositorClientState {
        // Xwayland's connection carries Smithay's own client data, not ours.
        if let Some(x) = client.get_data::<smithay::xwayland::XWaylandClientData>() {
            return &x.compositor_state;
        }
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        on_commit_buffer_handler::<Self>(surface);
        self.needs_redraw = true;

        if !is_sync_subsurface(surface) {
            let mut root = surface.clone();
            while let Some(parent) = get_parent(&root) {
                root = parent;
            }
            self.wm.on_commit(&root);
        }

        // xdg-shell: the first commit of a toplevel or popup is answered with a configure.
        if let Some(WinKind::Wayland(t)) = self.wm.find(surface).map(|w| &w.kind) {
            if !t.is_initial_configure_sent() {
                t.send_configure();
            }
        }
        self.popups.commit(surface);
        if let Some(PopupKind::Xdg(p)) = self.popups.find_popup(surface) {
            if !p.is_initial_configure_sent() {
                let _ = p.send_configure();
            }
        }
    }
}

impl XdgShellHandler for State {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let wl = surface.wl_surface().clone();
        self.wm.add(WinKind::Wayland(surface), wl.clone(), None, false);
        self.focus(Some(wl), SERIAL_COUNTER.next_serial());
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let was_focused = self.focused().as_ref() == Some(surface.wl_surface());
        self.wm.remove(surface.wl_surface());
        if was_focused || self.focused().is_none() {
            self.focus_top();
        }
        self.needs_redraw = true;
    }

    fn new_popup(&mut self, surface: PopupSurface, positioner: PositionerState) {
        surface.with_pending_state(|s| {
            s.geometry = positioner.get_geometry();
            s.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        if let Err(e) = self.popups.track_popup(PopupKind::Xdg(surface)) {
            tracing::warn!("failed to track popup: {e:?}");
        }
    }

    fn reposition_request(&mut self, surface: PopupSurface, positioner: PositionerState, token: u32) {
        surface.with_pending_state(|s| {
            s.geometry = positioner.get_geometry();
            s.positioner = positioner;
        });
        self.unconstrain_popup(&surface);
        surface.send_repositioned(token);
    }

    fn grab(&mut self, surface: PopupSurface, seat: wl_seat::WlSeat, serial: Serial) {
        let Some(seat) = Seat::<Self>::from_resource(&seat) else { return };
        let kind = PopupKind::Xdg(surface);
        let Ok(root) = find_popup_root_surface(&kind) else { return };
        let Ok(mut grab) = self.popups.grab_popup(FocusTarget::Wayland(root), kind, &seat, serial) else { return };

        if let Some(kb) = seat.get_keyboard() {
            if kb.is_grabbed() && !(kb.has_grab(serial) || kb.has_grab(grab.previous_serial().unwrap_or(serial))) {
                grab.ungrab(PopupUngrabStrategy::All);
                return;
            }
            kb.set_focus(self, grab.current_grab(), serial);
            kb.set_grab(self, PopupKeyboardGrab::new(&grab), serial);
        }
        if let Some(ptr) = seat.get_pointer() {
            if ptr.is_grabbed() && !(ptr.has_grab(serial) || ptr.has_grab(grab.previous_serial().unwrap_or_else(|| grab.serial()))) {
                grab.ungrab(PopupUngrabStrategy::All);
                return;
            }
            ptr.set_grab(self, PopupPointerGrab::new(&grab), serial, Focus::Keep);
        }
    }

    // Client-side-decorated apps (GTK, Electron, games) drive moves/resizes themselves.
    fn move_request(&mut self, surface: ToplevelSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        self.begin_move(surface.wl_surface());
    }

    fn resize_request(&mut self, surface: ToplevelSurface, _seat: wl_seat::WlSeat, _serial: Serial, edges: xdg_toplevel::ResizeEdge) {
        let pos = self.pointer_loc;
        self.wm.start_resize(surface.wl_surface(), edges as u32, pos);
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        let area = self.work_area();
        self.wm.maximize(surface.wl_surface(), area);
        if !surface.is_initial_configure_sent() {
            return;
        }
        self.needs_redraw = true;
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        self.wm.unmaximize(surface.wl_surface());
        self.needs_redraw = true;
    }

    fn fullscreen_request(&mut self, surface: ToplevelSurface, _output: Option<wl_output::WlOutput>) {
        let rect = self.output_rect();
        self.wm.fullscreen(surface.wl_surface(), rect);
        self.needs_redraw = true;
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        self.wm.unfullscreen(surface.wl_surface());
        self.needs_redraw = true;
    }

    fn minimize_request(&mut self, surface: ToplevelSurface) {
        self.minimize(surface.wl_surface());
    }
}

impl SeatHandler for State {
    type KeyboardFocus = FocusTarget;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<Self> {
        &mut self.seat_state
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&FocusTarget>) {
        let client = focused.and_then(|f| f.surface()).and_then(|s| self.dh.get_client(s.id()).ok());
        set_data_device_focus(&self.dh, seat, client.clone());
        set_primary_focus(&self.dh, seat, client);
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, image: CursorImageStatus) {
        self.cursor_status = image;
        self.needs_redraw = true;
    }
}

impl BufferHandler for State {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for State {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

impl SelectionHandler for State {
    type SelectionUserData = ();
}

impl DataDeviceHandler for State {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl PrimarySelectionHandler for State {
    fn primary_selection_state(&self) -> &PrimarySelectionState {
        &self.primary_selection_state
    }
}

impl ClientDndGrabHandler for State {}
impl ServerDndGrabHandler for State {
    fn send(&mut self, _mime_type: String, _fd: OwnedFd, _seat: Seat<Self>) {}
}

impl OutputHandler for State {}

/// Apps that want to draw their own titlebar (Chromium, Firefox, Electron) get to;
/// everyone else gets ours.
impl XdgDecorationHandler for State {
    fn new_decoration(&mut self, toplevel: ToplevelSurface) {
        self.set_decoration(&toplevel, DecoMode::ServerSide);
    }

    fn request_mode(&mut self, toplevel: ToplevelSurface, mode: DecoMode) {
        self.set_decoration(&toplevel, mode);
    }

    fn unset_mode(&mut self, toplevel: ToplevelSurface) {
        self.set_decoration(&toplevel, DecoMode::ServerSide);
    }
}

impl State {
    fn set_decoration(&mut self, toplevel: &ToplevelSurface, mode: DecoMode) {
        toplevel.with_pending_state(|s| s.decoration_mode = Some(mode));
        if toplevel.is_initial_configure_sent() {
            toplevel.send_pending_configure();
        }
        if let Some(w) = self.wm.find_mut(toplevel.wl_surface()) {
            w.ssd = mode == DecoMode::ServerSide;
        }
        self.needs_redraw = true;
    }
}

#[derive(Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}

delegate_compositor!(State);
delegate_xdg_shell!(State);
delegate_xdg_decoration!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_data_device!(State);
delegate_primary_selection!(State);
delegate_output!(State);
