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

- Build both configs: `cargo build --release` and `cargo build --release --features tty`.
- Test nested from inside a desktop. It keeps running until you stop it, so
  wrap it in `timeout`:

  ```bash
  VIBE_STARTUP=konsole timeout 12 ./target/release/vibe-comp --nested 2> /tmp/vibe.log
  ```

  `VIBE_DEBUG_START=1` opens the start menu at launch, for screenshots.
- **Never run `--tty` from inside a running desktop session.** It competes for
  the GPU and seat. The human has to test TTY mode from a spare VT; ask them
  to, and read `~/vibe.log` afterwards.
- If you screenshot the screen to check the nested window, the user's own
  windows may be in front. Don't capture or describe their private apps.

## Gotchas we already hit

- **NVIDIA:** hardware cursor and overlay planes are unreliable, so `tty.rs`
  renders with `FrameFlags::ALLOW_PRIMARY_PLANE_SCANOUT` only. Don't switch
  back to `DEFAULT`.
- **Xwayland's client** carries `XWaylandClientData`, not our `ClientState`;
  `client_compositor_state` must check for both.
- **X11 windows only enter the WM once mapped *and* associated with a
  `wl_surface`** (`manage_x11`). Override-redirect windows (menus, tooltips)
  live in `state.x11_overrides` and draw above everything.
- **X11 keyboard focus** needs the X server told too; that's why
  `KeyboardFocus` is `FocusTarget` rather than `WlSurface`.
- **Single-instance apps** (Electron, Firefox) hand off to a copy already
  running in another session. That's expected, not a bug in us.
- **Debug builds are slow** because of the CPU-painted UI; `profile.dev` sets
  `opt-level = 1`. Use release builds for real sessions.

## Commits and PRs

Per CONTRIBUTING.md you make them. Keep commits focused, describe *why* in
the message, and put your token count in the PR description (rule 4).
