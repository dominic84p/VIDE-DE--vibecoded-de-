---
title: 'IPC Commands'
description: 'Control socket protocol ($VIDE_IPC) for built-in apps and nested tests'
---

# IPC Commands

VIDE-DE exposes a Unix domain control socket at `$VIDE_IPC` (in `$XDG_RUNTIME_DIR`, e.g. `vide-de-wayland-0.sock`).

## Wire Format

One **newline-terminated plain-text command** per connection.

## Standard Commands

| Command | Arguments | Description |
|---|---|---|
| `snip` | — | Start an interactive region screenshot (used by Snip & Sketchy's **New**) |
| `screenshot` | — | Capture the whole screen |
| `copy-png` | `<path>` | Put a PNG file onto the Wayland clipboard (`image/png`) |
| `windows` | — | Reply with one line per window: `id\tpid\tminimized\tapp_id\ttitle` |
| `focus-window` | `<id>` | Focus the window with the given numeric ID (Task Mangler's **Switch to**) |
| `close-window` | `<id>` | Politely close the window with the given numeric ID (Task Mangler's **End task**) |
| `screen-info` | — | Reply with `<width> <height>` of the output |
| `screencast-start` | — | Increment active screencast client count and redraw |
| `screencast-stop` | — | Decrement active screencast client count |

## Test-Only Commands (`VIBE_TEST_INPUT=1`)

The following commands are **refused** unless `vibe-comp` was started with `VIBE_TEST_INPUT=1` (as set by `scripts/nested-test.sh`). A normal user session never exposes input injection.

| Command | Arguments | Description |
|---|---|---|
| `screenshot` | `<path>` | Save a full-screen PNG directly to `<path>` |
| `pointer-move` | `<X> <Y>` | Move pointer to logical coordinates `(X, Y)` |
| `pointer-click` | `[left\|right\|middle]` | Click a mouse button at the current pointer position |
| `pointer-down` | `[left\|right\|middle]` | Press a mouse button |
| `pointer-up` | `[left\|right\|middle]` | Release a mouse button |
| `pointer-pos` | — | Reply with current pointer coordinates |
| `pointer-state` | — | Dump active grab and which surface holds pointer focus |
| `key` | `<combo>` | Press and release a key combo (e.g. `ctrl+shift+t`, `alt+tab`, `super`) |
| `type` | `<text>` | Type literal text |
| `popups` | — | Dump each open popup's offset, geometry, drawn position, and positioner |
| `stall` | `[seconds]` | Sleep on the main loop (default 4s) to test the health watchdog |
