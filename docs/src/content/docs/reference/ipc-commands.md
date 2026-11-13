---
title: 'IPC Commands'
description: 'IPC socket protocol for external control'
---

# IPC Commands

Unix domain socket at `$VIDE_IPC` (default `~/.local/share/vide-de/ipc.sock`).

## Wire Format

JSON Lines (one JSON object per line).

### Request

```json
{"cmd": "command_name", "arg1": "value1", "arg2": "value2"}
```

### Response

```json
{"ok": true, "data": {...}}
{"ok": false, "error": "error message"}
```

### Events (Async)

```json
{"event": "event_name", "data": {...}}
```

## Commands

### `snip` — Region Snip

Start interactive region snip.

```json
{"cmd": "snip", "rect": null}
```

**Response:**
```json
{"ok": true}
```

**Events:**
```json
{"event": "snip_completed", "data": {"rect": {"x": 100, "y": 100, "width": 800, "height": 600}, "image_path": "/home/user/Pictures/Screenshots/Snip_2024-01-15_14-30-00.png"}}
```

### `screenshot` — Fullscreen Screenshot

```json
{"cmd": "screenshot", "output": "HDMI-A-1"}
```

**Parameters:**
- `output` (optional): Output name for single-monitor capture

**Response:**
```json
{"ok": true, "data": {"path": "/home/user/Pictures/Screenshots/Screenshot_2024-01-15_14-30-00.png"}}
```

### `copy-png` — Copy PNG to Clipboard

```json
{"cmd": "copy-png", "path": "/tmp/image.png"}
```

**Response:**
```json
{"ok": true}
```

### `get-version` — Compositor Version

```json
{"cmd": "get-version"}
```

**Response:**
```json
{"ok": true, "data": {"version": "0.1.0", "git_commit": "abc123", "smithay_version": "0.7"}}
```

### `get-windows` — List Windows

```json
{"cmd": "get-windows"}
```

**Response:**
```json
{"ok": true, "data": {"windows": [
    {"id": 1, "title": "Firefox", "app_id": "firefox", "workspace": 0, "maximized": false, "focused": true},
    {"id": 2, "title": "Terminal", "app_id": "konsole", "workspace": 0, "maximized": false, "focused": false}
]}}
```

### `set-workspace` — Switch Workspace

```json
{"cmd": "set-workspace", "idx": 2}
```

**Response:**
```json
{"ok": true}
```

### `launch` — Launch Application

```json
{"cmd": "launch", "desktop_id": "firefox.desktop"}
```

**Response:**
```json
{"ok": true}
```

## Events

| Event | Data | Description |
|-------|------|-------------|
| `window_created` | `{id, title, app_id, workspace}` | New window |
| `window_destroyed` | `{id}` | Window closed |
| `window_focused` | `{id}` | Focus changed |
| `workspace_changed` | `{idx}` | Workspace switched |
| `snip_completed` | `{rect, image_path}` | Snip finished |

## Client Example

### Bash + socat

```bash
# Snip
echo '{"cmd": "snip"}' | socat - UNIX-CONNECT:$VIDE_IPC

# Screenshot
echo '{"cmd": "screenshot"}' | socat - UNIX-CONNECT:$VIDE_IPC

# Get windows
echo '{"cmd": "get-windows"}' | socat - UNIX-CONNECT:$VIDE_IPC
```

### Python

```python
import socket
import json

def ipc_cmd(cmd, **kwargs):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect("/home/user/.local/share/vide-de/ipc.sock")
    
    msg = {"cmd": cmd, **kwargs}
    sock.sendall((json.dumps(msg) + "\n").encode())
    
    response = sock.recv(4096).decode()
    sock.close()
    return json.loads(response)

# Usage
ipc_cmd("snip")
ipc_cmd("screenshot")
ipc_cmd("get-windows")
```

### Rust

```rust
use std::os::unix::net::UnixStream;
use std::io::{Write, BufRead, BufReader};

fn ipc_cmd(cmd: &str, args: serde_json::Value) -> Result<serde_json::Value> {
    let mut stream = UnixStream::connect(&std::env::var("VIDE_IPC")?)?;
    let msg = json!({ "cmd": cmd, ..args });
    writeln!(stream, "{}", msg)?;
    
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(serde_json::from_str(&line)?)
}
```

## Environment Variable

```bash
export VIDE_IPC=/custom/path/ipc.sock
# Default: ~/.local/share/vide-de/ipc.sock
```

## Security

- Socket mode: `0600` (user only)
- No authentication (local user only)
- Commands limited to compositor actions
- No arbitrary code execution

## Related Files

- [IPC Socket](/components/ipc/) — Implementation
- [Screenshots](/components/screenshot/) — Snip/screenshot details
- [Snip App](/wiki/built-in-apps/#snip--sketchy) — Receives snip via IPC