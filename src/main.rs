mod apps;
mod canvas;
mod cursor;
mod desktops;
mod focus;
mod icons;
mod input;
mod ipc;
mod layers;
mod menu;
mod notifications;
mod pins;
mod render;
mod screenshot;
mod search;
mod sni;
mod switcher;
mod state;
mod supervisor;
mod tray;
#[cfg(feature = "tty")]
mod tty;
mod ui;
mod winit;
mod wm;
mod xwayland;

use std::{sync::Arc, time::Duration};

use smithay::{
    reexports::{
        calloop::{
            generic::Generic,
            timer::{TimeoutAction, Timer},
            EventLoop, Interest, Mode, PostAction,
        },
        wayland_server::Display,
    },
    wayland::socket::ListeningSocketSource,
};

use state::{ClientState, State};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let mut event_loop: EventLoop<'static, State> = EventLoop::try_new()?;
    let display: Display<State> = Display::new()?;
    let dh = display.handle();

    // Clients connect here.
    let socket = ListeningSocketSource::new_auto()?;
    let socket_name = socket.socket_name().to_string_lossy().into_owned();
    event_loop.handle().insert_source(socket, |stream, _, state| {
        if let Err(e) = state.dh.insert_client(stream, Arc::new(ClientState::default())) {
            tracing::warn!("rejected client: {e}");
        }
    })?;

    // Wake up whenever a client sends us something.
    event_loop.handle().insert_source(
        Generic::new(display, Interest::READ, Mode::Level),
        |_, display, state| {
            // Safety: we never drop the display while the source is alive.
            // A panic in one protocol handler shouldn't end every client's session.
            supervisor::guard("wayland dispatch", || unsafe { display.get_mut().dispatch_clients(state).unwrap() });
            state.popups.cleanup();
            Ok(PostAction::Continue)
        },
    )?;

    // Panel clock: check once a second, redraw only when the minute changes.
    event_loop.handle().insert_source(Timer::from_duration(Duration::from_secs(1)), |_, _, state| {
        supervisor::guard("clock tick", || {
            let now = state::clock_now();
            if now != state.clock {
                state.clock = now;
                state.needs_redraw = true;
            }
            state.reload_config_if_changed();
        });
        supervisor::guard("supervisor", || state.supervisor_tick());
        TimeoutAction::ToDuration(Duration::from_secs(1))
    })?;

    tracing::info!("listening on {socket_name}");
    apps::install_builtin();
    let mut state = State::new(dh, event_loop.handle(), socket_name);

    // Optional autostart, e.g. `VIBE_STARTUP=konsole cargo run`. Runs once Xwayland is up.
    state.startup = std::env::var("VIBE_STARTUP").ok();
    ipc::start(&event_loop.handle(), &mut state);
    xwayland::start(&event_loop.handle(), &mut state);

    // App tray icons: a D-Bus thread feeding us through a channel.
    let (tray_tx, tray_rx) = smithay::reexports::calloop::channel::channel();
    event_loop.handle().insert_source(tray_rx, |event, _, state| {
        if let smithay::reexports::calloop::channel::Event::Msg(ev) = event {
            supervisor::guard("tray event", || match ev {
                sni::Event::Items(items) => state.sni_items = items,
                sni::Event::Menu { id, entries } => state.open_tray_menu(id, entries),
            });
            state.needs_redraw = true;
        }
    })?;
    state.sni_tx = Some(sni::start(tray_tx));

    // X11 window icons, read off the main thread (see xwayland::fetch_window_icon).
    let (icon_tx, icon_rx) = smithay::reexports::calloop::channel::channel::<(String, icons::Icon)>();
    event_loop.handle().insert_source(icon_rx, |event, _, state| {
        if let smithay::reexports::calloop::channel::Event::Msg((class, icon)) = event {
            tracing::info!("window icon for {class}: {}x{}", icon.w, icon.h);
            supervisor::guard("window icon", || state.icons.set_custom(&class, icon));
            state.taskbar = None;
            state.needs_redraw = true;
        }
    })?;
    state.x11_icon_tx = Some(icon_tx);

    // Notifications: real org.freedesktop.Notifications service + Action Center.
    let (notif_tx, notif_rx) = smithay::reexports::calloop::channel::channel::<notifications::NotifEvent>();
    event_loop.handle().insert_source(notif_rx, |event, _, state| {
        if let smithay::reexports::calloop::channel::Event::Msg(ev) = event {
            supervisor::guard("notification event", || state.handle_notif_event(ev));
        }
    })?;
    state.notif_tx = Some(notifications::start(notif_tx));
    // Network and radio state for the tray icon, read off the main thread.
    state.start_network_polling();

    // Testing aids: open the start menu or Task View at launch, and/or have the
    // compositor screenshot itself after a few seconds (lands in ~/Pictures/Screenshots).
    if let Ok(q) = std::env::var("VIBE_DEBUG_START") {
        state.toggle_start();
        // Anything but "1" is typed into search.
        if q != "1" {
            state.start.query = q;
            state.refresh_start();
        }
    }
    if std::env::var_os("VIBE_DEBUG_DESKTOPS").is_some() {
        state.toggle_desktops();
    }
    // "network", "network-key", "search" or "action-center": open that panel a moment in, once the radios are read.
    if let Ok(which) = std::env::var("VIBE_DEBUG_OPEN") {
        event_loop.handle().insert_source(Timer::from_duration(Duration::from_secs(3)), move |_, _, state| {
            match which.as_str() {
                "network" => state.toggle_flyout(tray::Flyout::Network),
                // The security key prompt, on the second network listed.
                "network-key" => {
                    state.toggle_flyout(tray::Flyout::Network);
                    state.tray.expanded = state.tray.net.networks.get(1).map(|n| n.ssid.clone());
                    state.tray.password = Some("hunter2".into());
                    state.tray.password_error = true;
                }
                "search" => {
                    state.toggle_start();
                    state.start.search_mode = true;
                    state.refresh_start();
                }
                _ => state.toggle_action_center(),
            }
            TimeoutAction::Drop
        })?;
    }
    if std::env::var_os("VIBE_DEBUG_SWITCHER").is_some() {
        event_loop.handle().insert_source(Timer::from_duration(Duration::from_secs(5)), |_, _, state| {
            state.switcher_step(false);
            TimeoutAction::Drop
        })?;
    }
    if std::env::var_os("VIBE_DEBUG_SHOT").is_some() {
        event_loop.handle().insert_source(Timer::from_duration(Duration::from_secs(6)), |_, _, state| {
            state.screenshot_requested = Some(None);
            TimeoutAction::Drop
        })?;
    }

    // Nested if we're inside another desktop, real hardware otherwise. `--tty` / `--nested` force it.
    let args: Vec<String> = std::env::args().collect();
    let inside_desktop = std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some();
    let want_tty = args.iter().any(|a| a == "--tty") || (!inside_desktop && !args.iter().any(|a| a == "--nested"));

    if want_tty {
        #[cfg(feature = "tty")]
        return tty::run(&mut event_loop, &mut state);
        #[cfg(not(feature = "tty"))]
        return Err("built without TTY support; rebuild with `--features tty`".into());
    }
    winit::run(&mut event_loop, &mut state)
}
