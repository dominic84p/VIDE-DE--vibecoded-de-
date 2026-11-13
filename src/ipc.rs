//! A tiny control socket for our own apps: `$VIDE_IPC` (in $XDG_RUNTIME_DIR), one
//! command per connection, newline-terminated. Things only the compositor can do:
//!
//! - `snip`: start a region screenshot (Snip & Sketchy's "New")
//! - `screenshot`: capture the whole screen
//! - `copy-png <path>`: put a PNG on the clipboard
//! - with `VIBE_TEST_INPUT=1` only: `pointer-move X Y`, `pointer-click [left|right|middle]`, `pointer-down`/`pointer-up`,
//!   `key ctrl+shift+t`, `type text`, `pointer-pos`, and `screenshot /path.png` (for the nested test script)
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
        "screenshot" => {
            // `screenshot /some/file.png` is for tests only.
            if !arg.is_empty() {
                if !state.test_control {
                    return tracing::warn!("control socket: screenshot <path> needs VIBE_TEST_INPUT");
                }
                state.screenshot_path = Some(PathBuf::from(arg));
            }
            state.start_screenshot(false)
        }
        "stall" => {
            if !state.test_control {
                return tracing::warn!("control socket: stall needs VIBE_TEST_INPUT");
            }
            // Wedge the main loop on purpose, to prove the watchdog notices.
            std::thread::sleep(Duration::from_secs(arg.parse().unwrap_or(4)));
        }
        "popups" => {
            if !state.test_control {
                return tracing::warn!("control socket: popups needs VIBE_TEST_INPUT");
            }
            let _ = (&stream).write_all(state.describe_popups().as_bytes());
        }
        "pointer-move" | "pointer-click" | "pointer-down" | "pointer-up" | "key" | "type" | "pointer-pos" | "pointer-state" => {
            if !state.test_control {
                return tracing::warn!("control socket: {cmd} needs VIBE_TEST_INPUT");
            }
            if let Some(reply) = state.inject(cmd, arg) {
                let _ = (&stream).write_all(reply.as_bytes());
            }
        }
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
                set_data_device_selection(&state.dh, &state.seat, vec!["image/png".into()], crate::state::SelectionOwner::Bytes(Arc::new(bytes)));
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

/// Evdev key codes for the names tests use.
fn evdev_key(name: &str) -> Option<u32> {
    Some(match name {
        "esc" | "escape" => 1,
        "backspace" => 14,
        "tab" => 15,
        "enter" | "return" => 28,
        "ctrl" => 29,
        "shift" => 42,
        "alt" => 56,
        "space" => 57,
        "super" | "win" => 125,
        "up" => 103,
        "left" => 105,
        "right" => 106,
        "down" => 108,
        "-" | "minus" => 12,
        "=" | "equal" => 13,
        "," | "comma" => 51,
        "." | "dot" => 52,
        "/" | "slash" => 53,
        "0" => 11,
        d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => 1 + (d.as_bytes()[0] - b'0') as u32,
        c if c.len() == 1 && c.as_bytes()[0].is_ascii_lowercase() => {
            // Letters follow the keyboard rows, not the alphabet.
            const ROWS: [(&str, u32); 3] = [("qwertyuiop", 16), ("asdfghjkl", 30), ("zxcvbnm", 44)];
            ROWS.iter().find_map(|(row, first)| row.find(c).map(|i| first + i as u32))?
        }
        _ => return None,
    })
}

impl State {
    fn now_ms(&self) -> u32 {
        self.start_time.elapsed().as_millis() as u32
    }

    fn key_event(&mut self, evdev: u32, down: bool) {
        use smithay::backend::input::KeyState;
        let t = self.now_ms();
        // xkb keycodes are evdev codes plus 8.
        self.on_key(smithay::input::keyboard::Keycode::new(evdev + 8), if down { KeyState::Pressed } else { KeyState::Released }, t);
    }

    /// Input for tests, through the same handlers real devices use. Gated by `test_control`.
    pub fn inject(&mut self, cmd: &str, arg: &str) -> Option<String> {
        use smithay::backend::input::ButtonState;
        let t = self.now_ms();
        match cmd {
            "pointer-pos" => return Some(format!("{} {}\n", self.pointer_loc.x.round(), self.pointer_loc.y.round())),
            "pointer-state" => {
                let kind = |s: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface| {
                    if self.popups.find_popup(s).is_some() { "popup" } else { "other" }
                };
                let ours = self.pointer_focus.as_ref().map(|(s, o)| format!("{} origin={:?}", kind(s), o));
                let theirs = self.pointer.current_focus().map(|s| kind(&s).to_string());
                return Some(format!(
                    "grabbed={} ours={:?} smithay_focus={:?} at=({:.0},{:.0})\n",
                    self.pointer.is_grabbed(),
                    ours,
                    theirs,
                    self.pointer_loc.x,
                    self.pointer_loc.y
                ));
            }
            "pointer-move" => {
                let mut it = arg.split_whitespace().filter_map(|v| v.parse::<f64>().ok());
                if let (Some(x), Some(y)) = (it.next(), it.next()) {
                    self.pointer_to((x, y).into(), t);
                }
            }
            "pointer-click" | "pointer-down" | "pointer-up" => {
                let button = match arg.split_whitespace().next() {
                    Some("right") => 0x111,
                    Some("middle") => 0x112,
                    _ => 0x110,
                };
                if cmd != "pointer-up" {
                    self.on_button(button, ButtonState::Pressed, t);
                }
                if cmd != "pointer-down" {
                    self.on_button(button, ButtonState::Released, t + 1);
                }
            }
            "key" => {
                // `key ctrl+shift+t`: modifiers down, the key, then everything back up.
                let parts: Vec<&str> = arg.split('+').collect();
                let codes: Vec<u32> = parts.iter().filter_map(|p| evdev_key(p.trim())).collect();
                if codes.len() == parts.len() {
                    for c in &codes {
                        self.key_event(*c, true);
                    }
                    for c in codes.iter().rev() {
                        self.key_event(*c, false);
                    }
                } else {
                    tracing::warn!("control socket: unknown key {arg:?}");
                }
            }
            "type" => {
                for ch in arg.chars() {
                    let (shift, name) = if ch.is_ascii_uppercase() { (true, ch.to_ascii_lowercase().to_string()) } else if ch == ' ' { (false, "space".into()) } else { (false, ch.to_string()) };
                    let Some(code) = evdev_key(&name) else { continue };
                    if shift {
                        self.key_event(42, true);
                    }
                    self.key_event(code, true);
                    self.key_event(code, false);
                    if shift {
                        self.key_event(42, false);
                    }
                }
            }
            _ => {}
        }
        self.needs_redraw = true;
        None
    }
}

impl State {
    /// For the nested-test script: every popup with the numbers that decide where it is drawn and
    /// where clicks land on it.
    pub fn describe_popups(&self) -> String {
        use smithay::desktop::PopupManager;
        let mut out = String::new();
        for w in &self.wm.windows {
            let sloc = w.surface_loc();
            out += &format!("window {} pos={:?} surface_loc={:?} geometry={:?}\n", w.app_id(), w.pos, sloc, w.geometry());
            for (popup, offset) in PopupManager::popups_for_surface(w.surface()) {
                let g = popup.geometry();
                let drawn = w.pos + offset - g.loc;
                let (pending_geo, anchor) = match &popup {
                    smithay::desktop::PopupKind::Xdg(p) => p.with_pending_state(|s| (s.geometry, s.positioner.anchor_rect)),
                    _ => Default::default(),
                };
                out += &format!("  popup offset={offset:?} geometry={g:?} drawn_at={drawn:?} pending_geometry={pending_geo:?} anchor_rect={anchor:?}\n");
            }
        }
        out
    }
}
