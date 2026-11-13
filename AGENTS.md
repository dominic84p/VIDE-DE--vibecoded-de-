# AGENTS.md

Guidance for AI agents working on this repo. Read
[CONTRIBUTING.md](CONTRIBUTING.md) first: all code here is AI-written, and you
are expected to build, test, commit and open PRs yourself.

## What this is

A Wayland compositor + desktop shell in Rust on **Smithay 0.7**. Smithay
provides protocol plumbing and backends; everything user-visible (window
management, input routing, taskbar, start menu, titlebars, rendering order)
is ours. Don't pull in `smithay::desktop::Space` or another layout engine:
the WM in `src/wm.rs` is deliberately our own.

## Layout

- `main.rs`: event loop (calloop), client socket, clock timer, picks backend.
- `winit.rs` / `tty.rs`: nested and real-hardware backends. Both just call
  `render::elements()` and present the result. `tty.rs` is behind the `tty`
  cargo feature (needs libseat).
- `state.rs`: `State` plus Wayland protocol handlers (compositor, xdg-shell,
  decorations, seat, selection, popups).
- `wm.rs`: windows (`WinKind::Wayland` or `WinKind::X11`), stacking,
  move/resize grabs, maximize/minimize/fullscreen, hit-testing.
- `input.rs`: keybinds, start-menu typing, pointer routing to chrome vs clients.
- `render.rs`: builds the front-to-back element list for a frame.
- `ui.rs` + `canvas.rs`: all chrome is painted on the CPU into
  `MemoryRenderBuffer`s, cached by a key and repainted only when the key changes.
- `xwayland.rs`, `focus.rs`: X11 support and the keyboard focus enum.
- `apps.rs`, `icons.rs`, `cursor.rs`: desktop entries, icon themes, xcursor themes.
- `menu.rs`: right-click menus. `tray.rs`: volume/brightness. `screenshot.rs`: PrtSc and snip.
- `sni.rs`: app tray icons. We are the StatusNotifierWatcher + host, on a D-Bus thread
  talking to the main loop over channels. `desktops.rs`: Task View panel.
- `about/`, `settings/`, `powerslop/`, `exploder/`, `mangler/`: our own apps
  (`vide-about`, `vide-settings`, `powerslop`, `file-exploder`, `task-mangler`), separate egui programs. `apps::install_builtin` registers them as
  .desktop entries on startup. PowerSlop uses egui 0.31 (egui_term's version).
  Our apps are Windows puns styled like their Windows originals.
- `health.rs`: why a freeze leaves a trace. Pings every app's xdg client and logs the ones that stop
  answering (and puts "(Not responding)" in their titlebar); a watchdog thread logs when our own main
  loop or a TTY page flip stalls. Grep `~/vibe.log` for `answering pings`, `main loop stuck`, `page flip`,
  `protocol error`.
- `layers.rs`: wlr-layer-shell, so separate programs can be the desktop, panels or
  overlays. `desktop/` is `vide-desktop`, our desktop (wallpaper + icons) as an SCTK
  client on the background layer; the compositor starts it. Keep desktop features
  there, not in the compositor. Wallpapers: `assets/wallpapers/`, installed by
  `apps::install_builtin` to `~/.local/share/vide-de/wallpapers`.
- `snip/`: `snip-and-sketchy`. The compositor only captures (`screenshot.rs`); markup,
  saving and copying live in the app. `ipc.rs` is a control socket (`$VIDE_IPC`) for
  things only the compositor can do: `snip`, `screenshot`, `copy-png <path>`.
- `config/`: `vide-config`, the settings file shared by the compositor and Settings
  (`~/.config/vide-de/settings.conf`, polled once a second).

## Conventions

- **A window's `pos` is its geometry origin**, not its surface origin. Use
  `surface_loc()` for drawing and hit-testing surfaces; this keeps
  client-side shadows from shifting windows.
- **Ask clients for changes through `Window::configure(size, flags)`**, which
  covers both xdg and X11. Don't call `with_pending_state` directly outside it.
- **Set `state.needs_redraw = true`** whenever something visible changes.
  Backends only render when it's set; there's no busy loop.
- **Element lists are front to back**: cursor first, wallpaper last.
- **Respect client decoration choices**: `w.ssd` is true only when we draw
  the titlebar. Apps that request client-side decorations keep their own.
- Match the surrounding style. Comments explain *why*, not what.

## Testing

- **Use `scripts/nested-test.sh` for anything that opens the compositor.** It runs `vibe-comp --nested`
  isolated from the human's session (own runtime dir, D-Bus and config, own process group, a check
  that the document portal mount survived) and drives it with a small test file: `run` an app,
  `click X Y`, `key ctrl+l`, `type text`, `shot NAME`. See the header of the script, and
  `scripts/nested-tests/` for examples. Injection works through the control socket, which only
  accepts it when the compositor was started with `VIBE_TEST_INPUT=1` (the script sets it); a normal
  session never exposes input injection. The rules below are why the script exists; if you must
  run the compositor by hand, follow them.
- Build both configs: `cargo build --release` and `cargo build --release --features tty`.
- Test nested from inside a desktop. It keeps running until you stop it, so
  wrap it in `timeout`:

  ```bash
  VIBE_STARTUP=konsole timeout 12 ./target/release/vibe-comp --nested 2> /tmp/vibe.log
  ```

  `VIBE_DEBUG_START=1` opens the start menu at launch, for screenshots (`=text` also types a search).
- **Never run `--tty` from inside a running desktop session.** It competes for
  the GPU and seat. The human has to test TTY mode from a spare VT; ask them
  to, and read `~/vibe.log` afterwards.
- **Run nested tests on a private D-Bus**: `dbus-run-session -- env ... vibe-comp --nested`.
  On the host's bus our tray watcher would collide with the host desktop's. And never
  request a D-Bus name with `ReplaceExisting`: zbus's builder `.name()` does by default,
  and that once stole and killed Plasma's tray.
- **Give nested tests their own `XDG_RUNTIME_DIR` too, set before the bus starts**:
  `XDG_RUNTIME_DIR=/tmp/vt WAYLAND_DISPLAY=/run/user/$UID/wayland-1 dbus-run-session -- ...`
  (the dir must be mode 700 and its path short: Unix sockets cap at about 108 characters).
  Our apps make D-Bus start portals, and an activated service inherits the *bus's*
  environment, not your script's. If that is the real runtime dir, a second
  `xdg-document-portal` remounts `/run/user/$UID/doc` and the human's Flatpak apps stop
  launching (`bwrap: Can't find source path .../doc/by-app/...`). Repair:
  `systemctl --user restart xdg-document-portal`. Check `mount | grep /run/user/$UID/doc`
  before and after a run.
- **Never `pkill` a test by name.** It matches the human's own session, which is the
  compositor they are using right now. Start the run with `setsid` so it has its own
  process group, and signal only that group (`kill -TERM -- -$PGID`).
- `VIBE_DEBUG_SHOT=1` makes the compositor screenshot itself after 6 s, so you can check
  a nested run without capturing the user's screen.
- If you screenshot the screen to check the nested window, the user's own
  windows may be in front. Don't capture or describe their private apps.

## Gotchas we already hit

- **A popup's offset is relative to its parent's *window geometry* (`w.pos`), not its surface
  origin (`surface_loc()`).** Apps with client-side shadows (Chromium, Electron, GTK) have the two
  apart, so popups were drawn offset by the shadow inset. Use `w.pos + offset - popup.geometry().loc`
  to draw, hit-test and unconstrain.
- **Never re-focus the parent window on a click that lands inside one of its popups.** Chromium's
  dropdowns and permission prompts take keyboard focus; handing it back to the parent makes them
  close before the click is delivered, so nothing in them could be selected. See
  `scripts/nested-tests/popup-select-chromium.test`.
- **NVIDIA:** hardware cursor and overlay planes are unreliable, and direct
  scanout of client buffers skips our sync wait, so `tty.rs` renders with
  `FrameFlags::empty()` (always composite). Don't switch back to `DEFAULT`.
- **NVIDIA needs explicit sync** once clients share GPU buffers (dmabuf):
  without `linux-drm-syncobj` we show frames before the GPU finished them
  (flicker, duplicated/stale frames, black rectangles). The pre-commit hook in
  `State::new_surface` blocks each commit on its acquire point.
- **Xwayland's client** carries `XWaylandClientData`, not our `ClientState`;
  `client_compositor_state` must check for both.
- **X11 windows only enter the WM once mapped *and* associated with a
  `wl_surface`** (`manage_x11`). Override-redirect windows (menus, tooltips)
  live in `state.x11_overrides` and draw above everything.
- **X11 keyboard focus** needs the X server told too; that's why
  `KeyboardFocus` is `FocusTarget` rather than `WlSurface`.
- **Single-instance apps** (Electron, Firefox) hand off to a copy already
  running in another session. That's expected, not a bug in us.
- **Without linux-dmabuf (with a render-node feedback), Xwayland can't use the GPU**
  and games drop to single-digit FPS. `State::init_dmabuf` must run in every backend.
- **Games need relative-pointer + pointer-constraints.** Xwayland turns X11 pointer
  grabs (Minecraft mouse-look) into these; without them the cursor escapes. Locks
  follow pointer focus and are released on Super / focus change (`release_pointer`).
- **Brightness is a compositor overlay** (`state.dim`), since desktop monitors have no backlight.
- **Debug builds are slow** because of the CPU-painted UI; `profile.dev` sets
  `opt-level = 1`. Use release builds for real sessions.

## Commits and PRs

Per CONTRIBUTING.md you make them. Keep commits focused, describe *why* in
the message, and put your token count in the PR description (rule 4).

Then, if your model isn't in the README's **AI Agent Pokédex** yet, add it as the
next numbered row. A new model version gets a new number.
