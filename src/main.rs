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
mod render;
mod screenshot;
mod sni;
mod state;
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
            unsafe { display.get_mut().dispatch_clients(state).unwrap() };
            Ok(PostAction::Continue)
        },
    )?;

    // Panel clock: check once a second, redraw only when the minute changes.
    event_loop.handle().insert_source(Timer::from_duration(Duration::from_secs(1)), |_, _, state| {
        let now = state::clock_now();
        if now != state.clock {
            state.clock = now;
            state.needs_redraw = true;
        }
        state.reload_config_if_changed();
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
            match ev {
                sni::Event::Items(items) => state.sni_items = items,
                sni::Event::Menu { id, entries } => state.open_tray_menu(id, entries),
            }
            state.needs_redraw = true;
        }
    })?;
    state.sni_tx = Some(sni::start(tray_tx));

    // X11 window icons, read off the main thread (see xwayland::fetch_window_icon).
    let (icon_tx, icon_rx) = smithay::reexports::calloop::channel::channel::<(String, icons::Icon)>();
    event_loop.handle().insert_source(icon_rx, |event, _, state| {
        if let smithay::reexports::calloop::channel::Event::Msg((class, icon)) = event {
            tracing::info!("window icon for {class}: {}x{}", icon.w, icon.h);
            state.icons.set_custom(&class, icon);
            state.taskbar = None;
            state.needs_redraw = true;
        }
    })?;
    state.x11_icon_tx = Some(icon_tx);

    // Testing aids: open the start menu or Task View at launch, and/or have the
    // compositor screenshot itself after a few seconds (lands in ~/Pictures/Screenshots).
    if std::env::var_os("VIBE_DEBUG_START").is_some() {
        state.toggle_start();
    }
    if std::env::var_os("VIBE_DEBUG_DESKTOPS").is_some() {
        state.toggle_desktops();
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
