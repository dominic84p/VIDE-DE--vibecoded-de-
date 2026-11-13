//! Compositor state and the protocol handlers Smithay asks us to implement.

use std::{os::unix::io::OwnedFd, sync::Arc, time::Instant};

use fontdue::Font;
use smithay::{
    backend::renderer::{element::memory::MemoryRenderBuffer, utils::on_commit_buffer_handler},
    delegate_compositor, delegate_data_control, delegate_data_device, delegate_output,
    delegate_primary_selection, delegate_seat, delegate_shm, delegate_xdg_activation,
    delegate_xdg_decoration, delegate_xdg_shell,
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
            wlr_data_control::{DataControlHandler, DataControlState},
            SelectionHandler, SelectionSource, SelectionTarget,
        },
        shell::xdg::{
            decoration::{XdgDecorationHandler, XdgDecorationState},
            PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
        },
        shm::{ShmHandler, ShmState},
        xdg_activation::{XdgActivationHandler, XdgActivationState, XdgActivationToken, XdgActivationTokenData},
    },
};

use smithay::{
    backend::{
        allocator::{
            dmabuf::Dmabuf,
            format::FormatSet,
            Buffer as _,
        },
        drm::DrmNode,
        renderer::{gles::GlesRenderer, ImportDma},
    },
    delegate_dmabuf, delegate_drm_syncobj, delegate_pointer_constraints, delegate_relative_pointer,
    reexports::calloop::LoopHandle,
    wayland::{
        compositor::{add_blocker, add_pre_commit_hook, with_states},
        drm_syncobj::{DrmSyncobjCachedState, DrmSyncobjHandler, DrmSyncobjState},
        pointer_constraints::{with_pointer_constraint, PointerConstraintsHandler, PointerConstraintsState},
        relative_pointer::RelativePointerManagerState,
    },
    wayland::dmabuf::{DmabufFeedbackBuilder, DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
};

use crate::{
    apps::{self, App},
    canvas,
    cursor::Cursors,
    icons::Icons,
    focus::FocusTarget,
    notifications::{ActionCenter, NotifSender},
    ui::{StartHit, StartMenu, TaskbarHit, Taskbar, TitleButton, TASKBAR_H},
    wm::{Flag, WinKind, Wm},
};
use smithay::{
    wayland::xwayland_shell::XWaylandShellState,
    xwayland::{X11Surface, X11Wm},
};

pub fn clock_now(h24: bool) -> (String, String) {
    let now = chrono::Local::now();
    let time = if h24 { now.format("%H:%M") } else { now.format("%-I:%M %p") };
    (time.to_string(), now.format("%-m/%-d/%Y").to_string())
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
    pub power_open: bool,
    pub menu: Option<StartMenu>,
    /// What the list shows for `query` (see search.rs).
    pub results: Vec<crate::search::Entry>,
    /// Opened from the taskbar search box: the search pane even before typing.
    pub search_mode: bool,
    /// The search pane's tab (index into search::FILTERS).
    pub filter: usize,
    /// Start's left rail is expanded to show its labels (the hamburger button).
    pub rail_open: bool,
}

impl Start {
    /// Typing in Start, or opening search, turns it into Windows' search pane.
    pub fn searching(&self) -> bool {
        self.search_mode || !self.query.trim().is_empty()
    }
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
    pub data_control_state: DataControlState,
    pub xdg_activation_state: XdgActivationState,
    pub _output_manager_state: OutputManagerState,
    pub popups: PopupManager,
    pub layer_shell_state: smithay::wayland::shell::wlr_layer::WlrLayerShellState,
    /// The (single) output; layer surfaces are laid out on it.
    pub output: Option<smithay::output::Output>,
    pub dmabuf_state: DmabufState,
    /// Explicit sync (linux-drm-syncobj). NVIDIA needs it: without it we can show a
    /// client's frame before its GPU finished drawing it (flicker, stale frames, black boxes).
    pub syncobj_state: Option<DrmSyncobjState>,
    pub loop_handle: LoopHandle<'static, State>,
    /// What the renderer can import; checked when clients hand us GPU buffers.
    pub dmabuf_formats: Option<FormatSet>,

    pub seat: Seat<Self>,
    pub keyboard: KeyboardHandle<Self>,
    pub pointer: PointerHandle<Self>,
    pub pointer_loc: Point<f64, Logical>,
    /// The client surface under the pointer and its global origin (for relative motion).
    pub pointer_focus: Option<(WlSurface, Point<f64, Logical>)>,
    pub relative_pointer_state: RelativePointerManagerState,
    pub pointer_constraints_state: PointerConstraintsState,
    pub seen_pointer: bool,
    pub dnd_icon: Option<WlSurface>,
    pub cursor_status: CursorImageStatus,
    pub cursors: Cursors,

    pub font: Option<Font>,
    pub icons: Icons,
    /// Our logo, sized for the start button.
    pub logo: Option<crate::icons::Icon>,
    pub apps: Vec<App>,
    pub taskbar: Option<Taskbar>,
    pub start: Start,
    pub pins: crate::pins::Pins,
    pub task_drag: Option<crate::pins::TaskDrag>,
    pub file_index: crate::search::FileIndex,
    pub menu: Option<crate::menu::Menu>,
    /// Alt+Tab in progress.
    pub switcher: Option<crate::switcher::Switcher>,
    pub snap_buf: smithay::backend::renderer::element::solid::SolidColorBuffer,
    pub tray: crate::tray::Tray,
    /// Settings from ~/.config/vide-de/settings.conf (written by vide-settings).
    pub config: vide_config::Config,
    pub config_mtime: Option<std::time::SystemTime>,
    /// Mouse settings changed; the TTY backend re-applies them to libinput devices.
    pub mouse_config_dirty: bool,
    /// App tray icons (StatusNotifierItem), and the channel to the tray thread.
    pub sni_items: Vec<crate::sni::TrayItem>,
    pub sni_tx: Option<std::sync::mpsc::Sender<crate::sni::Cmd>>,
    /// Where the last tray icon right-click was, to place its menu.
    pub tray_menu_x: i32,
    /// Tray icons by fingerprint, so pixmaps aren't rescaled every frame.
    /// Icon lookup that prefers the dark theme, for the dark taskbar tray.
    pub tray_theme: crate::icons::Icons,
    pub tray_icon_cache: std::collections::HashMap<String, Option<std::rc::Rc<crate::icons::Icon>>>,
    pub desktops: crate::desktops::Desktops,
    /// Full-screen black at partial alpha: our software brightness.
    pub dim: smithay::backend::renderer::element::solid::SolidColorBuffer,
    /// Full-screen warm veil for Settings → Display → Night light.
    pub night: smithay::backend::renderer::element::solid::SolidColorBuffer,
    /// A screenshot to take on the next frame: `Some(None)` = whole screen, else a region.
    pub screenshot_requested: Option<Option<Rectangle<i32, Logical>>>,
    /// Active PipeWire screencast streams requesting frames via `vide-portal`.
    pub screencast_clients: usize,
    pub last_screencast: Option<Instant>,
    /// Win+Shift+S region selection in progress.
    pub snip: Option<crate::screenshot::Snip>,
    pub wallpaper: Option<(Size<i32, Logical>, MemoryRenderBuffer)>,
    pub hover: Hover,
    pub clock: (String, String),
    pub action_center: ActionCenter,
    pub toasts: crate::notifications::Toasts,
    pub notif_tx: Option<NotifSender>,
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
    /// When Xwayland last died, so a server that crashes on every start isn't restarted forever.
    pub xwayland_crashes: std::collections::VecDeque<std::time::Instant>,
    /// Where background threads deliver X11 window icons (class, icon).
    pub x11_icon_tx: Option<smithay::reexports::calloop::channel::Sender<(String, crate::icons::Icon)>>,
    pub socket_name: String,
    /// Our control socket, handed to apps as $VIDE_IPC.
    pub ipc_path: Option<std::path::PathBuf>,
    pub supervisor: crate::supervisor::Supervisor,
    pub pings: crate::health::Pings,
    /// Settings changed the monitor mode or scale; the backend applies it when idle.
    pub display_dirty: bool,
    /// `VIBE_TEST_INPUT=1`: the control socket may inject input and write screenshots to a path.
    /// Off in a normal session, where a socket that can type for you shouldn't exist.
    pub test_control: bool,
    /// Where the next screenshot goes instead of ~/Pictures/Screenshots (test control only).
    pub screenshot_path: Option<std::path::PathBuf>,
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
    pub fn new(dh: DisplayHandle, loop_handle: LoopHandle<'static, State>, socket_name: String) -> Self {
        let config = vide_config::Config::load();
        let clock = clock_now(config.clock_24h);
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(&dh, "seat0");
        let xkb = smithay::input::keyboard::XkbConfig {
            layout: &config.xkb_layout,
            variant: &config.xkb_variant,
            options: (!config.xkb_options.is_empty()).then(|| config.xkb_options.clone()),
            ..Default::default()
        };
        let keyboard = seat
            .add_keyboard(xkb, config.key_repeat_delay, config.key_repeat_rate)
            .unwrap_or_else(|_| seat.add_keyboard(Default::default(), 300, 30).unwrap());
        let pointer = seat.add_pointer();

        let primary_selection_state = PrimarySelectionState::new::<Self>(&dh);
        let data_control_state = DataControlState::new::<Self, _>(&dh, Some(&primary_selection_state), |_| true);

        Self {
            compositor_state: CompositorState::new::<Self>(&dh),
            xdg_shell_state: XdgShellState::new::<Self>(&dh),
            xdg_decoration_state: XdgDecorationState::new::<Self>(&dh),
            shm_state: ShmState::new::<Self>(&dh, vec![]),
            data_device_state: DataDeviceState::new::<Self>(&dh),
            primary_selection_state,
            data_control_state,
            xdg_activation_state: XdgActivationState::new::<Self>(&dh),
            _output_manager_state: OutputManagerState::new_with_xdg_output::<Self>(&dh),
            popups: PopupManager::default(),
            layer_shell_state: smithay::wayland::shell::wlr_layer::WlrLayerShellState::new::<Self>(&dh),
            output: None,
            dmabuf_state: DmabufState::new(),
            syncobj_state: None,
            loop_handle,
            dmabuf_formats: None,
            seat_state,
            seat,
            keyboard,
            pointer,
            pointer_loc: (0.0, 0.0).into(),
            pointer_focus: None,
            relative_pointer_state: RelativePointerManagerState::new::<Self>(&dh),
            pointer_constraints_state: PointerConstraintsState::new::<Self>(&dh),
            seen_pointer: false,
            dnd_icon: None,
            cursor_status: CursorImageStatus::default_named(),
            cursors: Cursors::load(),
            font: canvas::load_font(),
            icons: Icons::new(),
            logo: crate::icons::logo(24),
            apps: apps::scan(),
            taskbar: None,
            start: Start::default(),
            pins: crate::pins::Pins::load(),
            task_drag: None,
            file_index: Default::default(),
            menu: None,
            switcher: None,
            snap_buf: Default::default(),
            tray: crate::tray::Tray::new(),
            config: vide_config::Config::load(),
            config_mtime: vide_config::Config::mtime(),
            mouse_config_dirty: true,
            sni_items: Vec::new(),
            sni_tx: None,
            tray_menu_x: 0,
            tray_icon_cache: Default::default(),
            tray_theme: crate::icons::Icons::dark(),
            desktops: Default::default(),
            dim: Default::default(),
            night: Default::default(),
            screenshot_requested: None,
            screencast_clients: 0,
            last_screencast: None,
            snip: None,
            wallpaper: None,
            hover: Hover::None,
            clock,
            action_center: ActionCenter::default(),
            toasts: Default::default(),
            notif_tx: None,
            last_title_click: None,
            super_tap: false,
            wm: Wm::default(),
            xwm: None,
            xwayland_shell_state: XWaylandShellState::new::<Self>(&dh),
            xdisplay: None,
            x11_pending: Vec::new(),
            x11_overrides: Vec::new(),
            xwayland_crashes: Default::default(),
            x11_icon_tx: None,
            socket_name,
            ipc_path: None,
            supervisor: Default::default(),
            pings: Default::default(),
            display_dirty: false,
            test_control: std::env::var_os("VIBE_TEST_INPUT").is_some(),
            screenshot_path: None,
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

    /// Offer clients GPU buffer sharing (linux-dmabuf). Without it, GPU apps and
    /// Xwayland copy every frame through RAM: games crawl. `render_node` tells clients
    /// (Xwayland especially) which GPU to render on.
    pub fn init_dmabuf(&mut self, renderer: &GlesRenderer, render_node: Option<DrmNode>) {
        let formats = renderer.dmabuf_formats();
        match render_node.and_then(|n| DmabufFeedbackBuilder::new(n.dev_id(), formats.clone()).build().ok()) {
            Some(feedback) => {
                self.dmabuf_state.create_global_with_default_feedback::<Self>(&self.dh, &feedback);
                tracing::info!("linux-dmabuf v4 on {:?}", render_node.unwrap().dev_path());
            }
            None => {
                self.dmabuf_state.create_global::<Self>(&self.dh, formats.clone());
                tracing::warn!("linux-dmabuf without a render node; Xwayland may fall back to software GL");
            }
        }
        self.dmabuf_formats = Some(formats);
    }

    pub fn output_rect(&self) -> Rectangle<i32, Logical> {
        Rectangle::from_size(self.output_size)
    }

    /// The screen minus the taskbar: where maximized windows go.
    pub fn work_area(&self) -> Rectangle<i32, Logical> {
        Rectangle::from_size((self.output_size.w, self.output_size.h - TASKBAR_H).into())
    }

    /// True while any compositor shell overlay (Start, Task View, tray flyout, menu, Alt+Tab, snip)
    /// is open and should own the mouse instead of an underlying game's pointer lock.
    pub fn shell_overlay_open(&self) -> bool {
        self.start.open
            || self.desktops.open
            || self.tray.flyout.is_some()
            || self.menu.is_some()
            || self.switcher.is_some()
            || self.snip.is_some()
    }

    /// Let go of any game's pointer lock/confine, so the user gets a usable cursor back
    /// (Super, Alt+Tab, focus moving elsewhere). The game re-locks when the pointer returns.
    pub fn release_pointer(&mut self) {
        if let Some((s, _)) = &self.pointer_focus {
            with_pointer_constraint(s, &self.pointer, |c| {
                if let Some(c) = c.filter(|c| c.is_active()) {
                    c.deactivate();
                }
            });
        }
    }

    /// Give keyboard focus to `surface`, raise and un-minimize its window, mark it activated.
    pub fn focus(&mut self, surface: Option<WlSurface>, serial: Serial) {
        if self.pointer_focus.as_ref().map(|(s, _)| s) != surface.as_ref() {
            self.release_pointer();
        }
        let mut restored = false;
        if let Some(s) = &surface {
            self.wm.raise(s);
            if let Some(w) = self.wm.find_mut(s) {
                restored = w.minimized;
                w.minimized = false;
                // Focusing a window on another workspace takes you there, so focus is never invisible.
                let ws = w.workspace;
                self.wm.set_workspace(ws);
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
        // A window restored from minimized may still hold keyboard focus; re-enter so it
        // notices. Only then: a leave/enter on every click into the focused window makes
        // Firefox close its <select> dropdowns (GTK subsurfaces, not xdg popups) before the
        // click lands. See scripts/nested-tests/popup-select-firefox.test.
        if restored && kb.current_focus() == target && target.is_some() {
            kb.set_focus(self, None, serial);
        }
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
        let ws = self.wm.current_workspace;
        for w in self.wm.windows.iter_mut().filter(|w| w.workspace == ws) {
            w.minimized = true;
        }
        self.focus(None, SERIAL_COUNTER.next_serial());
    }

    /// Dragging a maximized window off its titlebar restores it under the pointer, like Windows.
    pub fn begin_move(&mut self, surface: &WlSurface) {
        let pos = self.pointer_loc;
        if let Some(w) = self.wm.find(surface) {
            if let Some(restore) = w.maximized.or(w.snapped) {
                let g = w.geometry();
                let frac = ((pos.x - g.loc.x as f64) / g.size.w.max(1) as f64).clamp(0.0, 1.0);
                self.wm.unmaximize(surface);
                self.wm.unsnap(surface);
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
        let mut target = self.work_area();
        // The popup's geometry is relative to its parent's window geometry, so is the area it must fit.
        target.loc -= w.pos;
        target.loc -= get_popup_toplevel_coords(&kind);
        popup.with_pending_state(|s| s.geometry = s.positioner.get_unconstrained_geometry(target));
    }

    // ------------------------------------------------------------ launching

    pub fn spawn(&self, cmd: &str) {
        if let Some(child) = self.spawn_child(cmd) {
            crate::supervisor::reap(child);
        }
    }

    /// Like `spawn`, but hands back the process so the supervisor can watch it.
    /// `exec` makes the child the program itself, not a shell around it.
    pub fn spawn_child(&self, cmd: &str) -> Option<std::process::Child> {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(cmd);
        self.app_env(&mut c);
        // Flatpaks without `--socket=wayland` don't mount the Wayland socket; forcing
        // ELECTRON_OZONE_PLATFORM_HINT=auto makes Electron crash instead of using X11.
        if cmd.contains("flatpak ") {
            c.env_remove("ELECTRON_OZONE_PLATFORM_HINT");
        }
        match c.spawn() {
            Ok(child) => Some(child),
            Err(e) => {
                tracing::warn!("failed to spawn {cmd}: {e}");
                None
            }
        }
    }

    /// Session is up (Xwayland ready or failed): wire up desktop services, then autostart.
    pub fn run_startup(&mut self) {
        // D-Bus starts portals and the keyring's password prompt on demand; they need
        // to know our display, or they can't show file pickers and dialogs.
        // Run sequentially: update D-Bus/systemd env -> trigger pam_kwallet_init so the
        // SDDM PAM ksecretd unlocks KWallet -> only spawn ksecretd if nothing owns
        // org.freedesktop.secrets (e.g. raw VT login without SDDM).
        self.spawn(
            "dbus-update-activation-environment --systemd WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP \
             XDG_SESSION_TYPE XDG_SESSION_DESKTOP DESKTOP_SESSION QT_QPA_PLATFORM 2>/dev/null \
             || dbus-update-activation-environment WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP \
             XDG_SESSION_TYPE XDG_SESSION_DESKTOP DESKTOP_SESSION QT_QPA_PLATFORM; \
             if [ -n \"$PAM_KWALLET5_LOGIN\" ]; then \
               for k in /usr/lib/pam_kwallet_init /usr/libexec/pam_kwallet_init; do \
                 [ -x \"$k\" ] && \"$k\" && break; \
               done; \
               sleep 0.4; \
             fi; \
             dbus-send --session --print-reply --dest=org.freedesktop.DBus /org/freedesktop/DBus \
             org.freedesktop.DBus.NameHasOwner string:org.freedesktop.secrets | grep -q true \
             || exec ksecretd",
        );
        // Polkit agent: shows the password prompt when mounting a disk (File Exploder's
        // drives) or anything else needs admin rights.
        self.spawn(
            "for a in /usr/lib/polkit-kde-authentication-agent-1 /usr/libexec/polkit-kde-authentication-agent-1 \
             /usr/lib/polkit-gnome/polkit-gnome-authentication-agent-1; do [ -x \"$a\" ] && exec \"$a\"; done",
        );
        // Start our screenshare / screenshot portal backend and reload xdg-desktop-portal
        // so it picks up vide-portals.conf on the current D-Bus/Wayland environment.
        if let Some(portal) = crate::input::own_app("vide-portal") {
            self.supervise("vide-portal", &format!("exec {portal}"));
        }
        self.spawn("systemctl --user try-restart xdg-desktop-portal 2>/dev/null || true");
        // The desktop (wallpaper + icons) is its own program on the background layer;
        // without it we fall back to drawing a wallpaper ourselves.
        if let Some(desktop) = crate::input::own_app("vide-desktop") {
            self.supervise("vide-desktop", &format!("exec {desktop}"));
        }
        if let Some(cmd) = self.startup.take() {
            self.spawn(&cmd);
        }
    }

    pub fn launch(&mut self, app: &App) {
        tracing::info!("launching {} ({})", app.name, app.exec);
        self.pins.launched(&app.id);
        if app.terminal {
            // Konsole-style `-e`; PowerSlop runs whatever $SHELL it's given, so pass the app as one.
            self.spawn(&format!("konsole -e {}", app.exec));
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
        if let Some(p) = &self.ipc_path {
            c.env("VIDE_IPC", p);
        }
        // X11-only apps go through our Xwayland, never the host's X server.
        match self.xdisplay {
            Some(n) => c.env("DISPLAY", format!(":{n}")),
            None => c.env_remove("DISPLAY"),
        };
        // Say who we are, so tools stop guessing from whatever services happen to run.
        // KDE stays listed second so Qt/KDE apps still pick up the user's theme and icons.
        c.env("XDG_CURRENT_DESKTOP", "VIDE-DE:KDE")
            .env("XDG_SESSION_DESKTOP", "vide-de")
            .env("DESKTOP_SESSION", "vide-de");
    }

    /// Recompute the start menu's list. Hidden KDE apps are only left out here; they
    /// still launch from anywhere else (Mod+Enter, taskbar, menus).
    pub fn refresh_start(&mut self) {
        self.start.results = if self.start.query.trim().is_empty() && self.start.search_mode {
            crate::search::top_apps(&self.apps, &self.pins)
        } else {
            let r = crate::search::results(&self.start.query, &self.apps, self.config.hide_kde_apps, &self.pins, &self.file_index);
            if self.start.searching() { crate::search::arrange(r, self.start.filter) } else { r }
        };
        self.start.selected = self.start.results.iter().position(|e| e.selectable()).unwrap_or(0);
        self.start.scroll = 0;
    }

    /// Pick up edits from the Settings app (polled once a second).
    pub fn reload_config_if_changed(&mut self) {
        let mtime = vide_config::Config::mtime();
        if mtime == self.config_mtime {
            return;
        }
        self.config_mtime = mtime;
        let new = vide_config::Config::load();
        if new.mouse_flat != self.config.mouse_flat
            || new.mouse_speed != self.config.mouse_speed
            || new.tap_to_click != self.config.tap_to_click
            || new.natural_scroll != self.config.natural_scroll
            || new.disable_while_typing != self.config.disable_while_typing
        {
            self.mouse_config_dirty = true;
        }
        if new.xkb_layout != self.config.xkb_layout
            || new.xkb_variant != self.config.xkb_variant
            || new.xkb_options != self.config.xkb_options
        {
            let xkb = smithay::input::keyboard::XkbConfig {
                layout: &new.xkb_layout,
                variant: &new.xkb_variant,
                options: (!new.xkb_options.is_empty()).then(|| new.xkb_options.clone()),
                ..Default::default()
            };
            let kb = self.keyboard.clone();
            let _ = kb.set_xkb_config(self, xkb);
        }
        if new.display_mode != self.config.display_mode || new.display_scale != self.config.display_scale {
            self.display_dirty = true;
        }
        if new.key_repeat_delay != self.config.key_repeat_delay
            || new.key_repeat_rate != self.config.key_repeat_rate
        {
            self.keyboard.change_repeat_info(new.key_repeat_rate, new.key_repeat_delay);
        }
        tracing::info!("settings reloaded: {new:?}");
        let clock_format_changed = new.clock_24h != self.config.clock_24h;
        self.config = new;
        if clock_format_changed {
            self.clock = clock_now(self.config.clock_24h);
            self.taskbar = None;
        }
        self.start.menu = None;
        self.refresh_start();
        self.needs_redraw = true;
    }

    pub fn toggle_start(&mut self) {
        self.start.open = !self.start.open;
        if self.start.open {
            self.release_pointer();
            self.file_index.refresh();
        }
        self.start.query.clear();
        self.start.power_open = false;
        self.start.search_mode = false;
        self.start.filter = 0;
        self.start.rail_open = false;
        self.refresh_start();
        self.needs_redraw = true;
    }
}

/// Games lock or confine the pointer (mouse-look); Xwayland maps X11 pointer grabs onto this.
impl PointerConstraintsHandler for State {
    fn new_constraint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>) {
        // Only takes effect while the pointer is over that surface.
        if self.pointer_focus.as_ref().is_some_and(|(s, _)| s == surface) {
            with_pointer_constraint(surface, pointer, |c| {
                if let Some(c) = c {
                    c.activate();
                }
            });
        }
    }

    fn cursor_position_hint(&mut self, surface: &WlSurface, pointer: &PointerHandle<Self>, location: Point<f64, Logical>) {
        // While locked, the app tells us where its cursor "is", so it reappears there on unlock.
        let active = with_pointer_constraint(surface, pointer, |c| c.is_some_and(|c| c.is_active()));
        if let (true, Some((s, origin))) = (active, &self.pointer_focus) {
            if s == surface {
                self.pointer_loc = *origin + location;
            }
        }
    }
}

impl DrmSyncobjHandler for State {
    fn drm_syncobj_state(&mut self) -> Option<&mut DrmSyncobjState> {
        self.syncobj_state.as_mut()
    }
}

impl CompositorHandler for State {
    fn new_surface(&mut self, surface: &WlSurface) {
        // Hold each commit back until the client's GPU work on it is done (its acquire point).
        add_pre_commit_hook::<Self, _>(surface, |state, _dh, surface| {
            let acquire = with_states(surface, |s| {
                s.cached_state.get::<DrmSyncobjCachedState>().pending().acquire_point.clone()
            });
            let Some(acquire) = acquire else { return };
            let Ok((blocker, source)) = acquire.generate_blocker() else { return };
            let Some(client) = surface.client() else { return };
            let res = state.loop_handle.insert_source(source, move |_, _, state| {
                let dh = state.dh.clone();
                state.client_compositor_state(&client).blocker_cleared(state, &dh);
                Ok(())
            });
            if res.is_ok() {
                add_blocker(surface, blocker);
            }
        });
    }

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
            self.layer_commit(&root);
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

    fn client_pong(&mut self, client: smithay::wayland::shell::xdg::ShellClient) {
        self.pong(&client);
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

    fn show_window_menu(&mut self, surface: ToplevelSurface, _seat: wl_seat::WlSeat, _serial: Serial, _location: Point<i32, Logical>) {
        let at = self.pointer_loc.to_i32_round();
        self.open_window_menu(surface.wl_surface(), at);
    }

    fn popup_destroyed(&mut self, surface: PopupSurface) {
        self.popups.cleanup();
        if self.keyboard.current_focus().and_then(|f| f.surface()).as_ref() == Some(surface.wl_surface()) {
            let kind = PopupKind::Xdg(surface);
            if let Ok(root) = find_popup_root_surface(&kind) {
                self.focus(Some(root), SERIAL_COUNTER.next_serial());
            } else {
                self.focus_top();
            }
        }
        self.needs_redraw = true;
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

impl DmabufHandler for State {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(&mut self, _global: &DmabufGlobal, dmabuf: Dmabuf, notifier: ImportNotifier) {
        // The renderer imports for real at draw time; here we only refuse formats it can't do.
        let ok = self.dmabuf_formats.as_ref().is_some_and(|f| f.contains(&dmabuf.format()));
        if ok {
            let _ = notifier.successful::<Self>();
        } else {
            notifier.failed();
        }
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

#[derive(Clone)]
pub enum SelectionOwner {
    Bytes(Arc<Vec<u8>>),
    X11,
}

impl SelectionHandler for State {
    /// Clipboard contents we own ourselves (screenshots, calculator, X11 bridge).
    type SelectionUserData = SelectionOwner;

    fn new_selection(&mut self, ty: SelectionTarget, source: Option<SelectionSource>, _seat: Seat<Self>) {
        if let Some(xwm) = self.xwm.as_mut() {
            let _ = xwm.new_selection(ty, source.map(|s| s.mime_types()));
        }
    }

    fn send_selection(
        &mut self,
        ty: SelectionTarget,
        mime_type: String,
        fd: OwnedFd,
        _seat: Seat<Self>,
        user_data: &Self::SelectionUserData,
    ) {
        match user_data {
            SelectionOwner::Bytes(data) => {
                // A pipe to the pasting app; write off the main thread so a slow reader can't stall us.
                let data = data.clone();
                std::thread::spawn(move || {
                    use std::io::Write;
                    let _ = std::fs::File::from(fd).write_all(&data);
                });
            }
            SelectionOwner::X11 => {
                if let Some(xwm) = self.xwm.as_mut() {
                    let _ = xwm.send_selection(ty, mime_type, fd, self.loop_handle.clone());
                }
            }
        }
    }
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

impl DataControlHandler for State {
    fn data_control_state(&self) -> &DataControlState {
        &self.data_control_state
    }
}

impl XdgActivationHandler for State {
    fn activation_state(&mut self) -> &mut XdgActivationState {
        &mut self.xdg_activation_state
    }

    fn request_activation(
        &mut self,
        _token: XdgActivationToken,
        _token_data: XdgActivationTokenData,
        surface: WlSurface,
    ) {
        if let Some(w) = self.wm.find_mut(&surface) {
            let ws = w.workspace;
            w.minimized = false;
            if ws != self.wm.current_workspace {
                self.switch_desktop(ws);
            }
            self.focus(Some(surface), SERIAL_COUNTER.next_serial());
            self.needs_redraw = true;
        }
    }
}

impl ClientDndGrabHandler for State {
    fn started(
        &mut self,
        _source: Option<smithay::reexports::wayland_server::protocol::wl_data_source::WlDataSource>,
        icon: Option<WlSurface>,
        _seat: Seat<Self>,
    ) {
        self.dnd_icon = icon;
        self.needs_redraw = true;
    }

    fn dropped(
        &mut self,
        _target: Option<WlSurface>,
        _validated: bool,
        _seat: Seat<Self>,
    ) {
        self.dnd_icon = None;
        self.needs_redraw = true;
    }
}
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
    /// "firefox (pid 1234)", set when the client connects, so we can say who left.
    pub who: std::sync::OnceLock<String>,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, reason: DisconnectReason) {
        let who = self.who.get().map(String::as_str).unwrap_or("a client");
        match reason {
            DisconnectReason::ConnectionClosed => tracing::info!("{who} disconnected"),
            // We (or smithay, on a client's bad request) killed it: say exactly why.
            DisconnectReason::ProtocolError(e) => tracing::warn!(
                "{who} was disconnected by a protocol error: {} on {}@{} (code {})",
                e.message,
                e.object_interface,
                e.object_id,
                e.code
            ),
        }
    }
}

delegate_compositor!(State);
delegate_xdg_shell!(State);
delegate_xdg_decoration!(State);
delegate_shm!(State);
delegate_seat!(State);
delegate_data_device!(State);
delegate_primary_selection!(State);
delegate_data_control!(State);
delegate_xdg_activation!(State);
delegate_output!(State);
delegate_dmabuf!(State);
delegate_drm_syncobj!(State);
delegate_relative_pointer!(State);
delegate_pointer_constraints!(State);
