---
title: 'IPC Socket (ipc.rs)'
description: 'Control socket for snip, screenshot, copy-png commands'
---

# IPC Socket (`ipc.rs`)

Unix domain socket at `$VIDE_IPC` (default `~/.local/share/vide-de/ipc.sock`) for external control.

## Protocol

```rust
// Client → Compositor
enum IpcCommand {
    Snip { rect: Option<Rectangle> },           // Start region snip
    Screenshot { output: Option<String> },      // Fullscreen screenshot
    CopyPng { path: PathBuf },                  // Copy PNG to clipboard
    GetVersion,                                 // Get compositor version
    GetWindows,                                 // List windows
    SetWorkspace { idx: usize },                // Switch workspace
    Launch { desktop_id: String },              // Launch app
}

// Compositor → Client (responses)
enum IpcResponse {
    Ok { data: Option<Value> },
    Err { code: i32, message: String },
    Event { name: String, data: Value },        // Async events
}
```

## Wire Format (JSON Lines)

```json
// Request
{"cmd": "snip", "rect": null}

// Response
{"ok": true, "data": null}

// Event (async)
{"event": "window_created", "data": {"id": 42, "title": "Firefox"}}
```

## Server Implementation

```rust
struct IpcServer {
    listener: UnixListener,
    sender: mpsc::Sender<IpcCommand>,
}

impl IpcServer {
    fn accept_loop(&mut self) {
        for stream in self.listener.incoming() {
            let mut stream = stream?;
            let (rd, wr) = stream.split();
            
            // Read JSON lines
            let reader = BufReader::new(rd);
            for line in reader.lines() {
                let cmd: IpcCommand = serde_json::from_str(&line)?;
                self.sender.send(cmd)?;
            }
        }
    }
}
```

## Client Usage

```bash
# Start region snip
echo '{"cmd": "snip"}' | socat - UNIX-CONNECT:$VIDE_IPC

# Fullscreen screenshot
echo '{"cmd": "screenshot"}' | socat - UNIX-CONNECT:$VIDE_IPC

# Copy PNG to clipboard
echo '{"cmd": "copy-png", "path": "/tmp/shot.png"}' | socat - UNIX-CONNECT:$VIDE_IPC

# Get version
echo '{"cmd": "get-version"}' | socat - UNIX-CONNECT:$VIDE_IPC
```

## Snip Flow

```
User presses Win+Shift+S
       │
       ▼
input.rs: start_region_snip()
       │
       ▼
render.rs: show snip overlay (dim + selection)
       │
       ▼
User selects region
       │
       ▼
screenshot.rs: capture region → PNG bytes
       │
       ▼
IPC: send Snip { rect, image } to $VIDE_IPC
       │
       ▼
snip-and-sketchy receives → opens with image
```

## Environment Variable

```bash
# Default: ~/.local/share/vide-de/ipc.sock
export VIDE_IPC=/custom/path/ipc.sock
```

## Security

- Socket permissions: `0600` (user only)
- No authentication (local user only)
- Commands limited to compositor actions
- No arbitrary code execution

## Related Files

- `screenshot.rs` — Snip/screenshot implementation
- `snip/` — Receives snip via IPC
- `main.rs` — IPC server startup
- `state.rs` — Command handling