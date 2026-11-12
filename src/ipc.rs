//! A tiny control socket for our own apps: `$VIDE_IPC` (in $XDG_RUNTIME_DIR), one
//! command per connection, newline-terminated. Things only the compositor can do:
//!
//! - `snip`: start a region screenshot (Snip & Sketchy's "New")
//! - `screenshot`: capture the whole screen
//! - `copy-png <path>`: put a PNG on the clipboard
//! - `windows`: replies with one line per window, `id<TAB>pid<TAB>minimized<TAB>app_id<TAB>title`
//!   (Task Mangler's "Apps" are the processes that own a window)
//! - `focus-window <id>` / `close-window <id>`: Task Mangler's "Switch to" and polite End task

use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use smithay::{
    reexports::calloop::{generic::Generic, Interest, LoopHandle, Mode, PostAction},
    utils::SERIAL_COUNTER,
    wayland::selection::data_device::set_data_device_selection,
};

use crate::state::State;

pub fn start(handle: &LoopHandle<'static, State>, state: &mut State) {
    let dir = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    let path = dir.join(format!("vide-de-{}.sock", state.socket_name));
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => return tracing::warn!("control socket disabled: {e}"),
    };
    let _ = listener.set_nonblocking(true);
    let res = handle.insert_source(Generic::new(listener, Interest::READ, Mode::Level), |_, listener, state| {
        while let Ok((stream, _)) = listener.accept() {
            handle_client(state, stream);
        }
        Ok(PostAction::Continue)
    });
    match res {
        Ok(_) => {
            tracing::info!("control socket at {}", path.display());
            state.ipc_path = Some(path);
        }
        Err(e) => tracing::warn!("control socket disabled: {e}"),
    }
}

fn handle_client(state: &mut State, stream: UnixStream) {
    // Our apps write one short line and hang up; don't let a stuck one block us.
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let line = line.trim();
    let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));
    match cmd {
        "snip" => state.start_screenshot(true),
        "screenshot" => state.start_screenshot(false),
        "screencast-start" => {
            state.screencast_clients += 1;
            state.needs_redraw = true;
        }
        "screencast-stop" => {
            state.screencast_clients = state.screencast_clients.saturating_sub(1);
        }
        "screen-info" => {
            let msg = format!("{} {}\n", state.output_size.w, state.output_size.h);
            let _ = (&stream).write_all(msg.as_bytes());
        }
        "copy-png" => match std::fs::read(arg) {
            Ok(bytes) => {
                set_data_device_selection(&state.dh, &state.seat, vec!["image/png".into()], Arc::new(bytes));
            }
            Err(e) => tracing::warn!("copy-png {arg}: {e}"),
        },
        "windows" => {
            let mut out = String::new();
            for w in &state.wm.windows {
                let pid = state.window_pid(w.surface()).unwrap_or(0);
                // Tabs and newlines would break the line format; titles rarely have them.
                let clean = |s: String| s.replace(['\t', '\n'], " ");
                out += &format!("{}\t{pid}\t{}\t{}\t{}\n", w.id, w.minimized as u8, clean(w.app_id()), clean(w.title()));
            }
            let _ = (&stream).write_all(out.as_bytes());
        }
        "focus-window" | "close-window" => {
            let id: u64 = arg.parse().unwrap_or(u64::MAX);
            let Some(s) = state.wm.windows.iter().find(|w| w.id == id).map(|w| w.surface().clone()) else { return };
            if cmd == "focus-window" {
                state.focus(Some(s), SERIAL_COUNTER.next_serial());
                state.needs_redraw = true;
            } else if let Some(w) = state.wm.find(&s) {
                w.close();
            }
        }
        other => tracing::warn!("control socket: unknown command {other:?}"),
    }
}
