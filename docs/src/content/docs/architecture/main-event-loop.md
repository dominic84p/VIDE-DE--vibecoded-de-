---
title: 'Main Event Loop (main.rs)'
description: 'Event loop, backend selection, and compositor initialization'
---

# Main Event Loop (`main.rs`)

The entry point for `vibe-comp`. Sets up the calloop event loop, selects the backend (nested vs TTY), initializes the compositor state, and runs the main loop.

## Responsibilities

1. **Parse CLI args** — `--nested`, `--tty`, `--version`, etc.
2. **Initialize logging** — `tracing` with `RUST_LOG` support
3. **Select backend** — `WinitBackend` (nested) or `TtyBackend` (real hardware)
4. **Create `State`** — Compositor state with all protocol handlers
5. **Start IPC socket** — Unix socket at `$VIDE_IPC` for external control
6. **Run event loop** — calloop dispatch + render timer

## Backend Selection

```rust
// main.rs
let backend = if args.nested {
    Backend::Winit(WinitBackend::new()?)
} else if args.tty {
    Backend::Tty(TtyBackend::new()?)
} else {
    // Default to nested for safety
    Backend::Winit(WinitBackend::new()?)
};
```

## Event Loop Structure

```rust
// Simplified main loop
let mut event_loop = EventLoop::try_new()?;

// Insert State and other globals
let state = State::new(&event_loop.handle(), backend)?;
event_loop.handle().insert(state);

// IPC socket
let ipc_socket = IpcSocket::new(&event_loop.handle())?;
event_loop.handle().insert(ipc_socket);

// Render timer (~60fps)
let render_timer = event_loop.handle().insert_source(
    Timer::from_duration(Duration::from_millis(16)),
    move |_, _, state| {
        state.needs_redraw = true;
        state.backend.schedule_render();
        TimeoutAction::ToDuration(Duration::from_millis(16))
    }
)?;

// Start desktop process (layer-shell client)
start_desktop_process(&event_loop.handle())?;

// Run
event_loop.run(None, &mut state, |state| {
    // Wayland dispatch happens here
})?;
```

## Key Components Initialized

| Component | Purpose |
|-----------|---------|
| `State` | All Wayland protocol handlers, window list, input state |
| `Backend` | Abstracts winit/tty rendering |
| `IpcSocket` | Control socket for `snip`, `screenshot`, `copy-png` |
| `DesktopProcess` | Spawns `vide-desktop` as layer-shell client |

## Debugging

```bash
# Verbose logging
RUST_LOG=trace cargo run --release -- --nested

# Specific modules
RUST_LOG=vibe_comp::wm=debug,vibe_comp::input=debug cargo run --release -- --nested
```

## Related Files

- `winit.rs` — Nested backend implementation
- `tty.rs` — TTY/DRM/KMS backend implementation
- `state.rs` — Compositor state and protocol handlers
- `ipc.rs` — IPC socket protocol