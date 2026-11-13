---
title: 'Testing'
description: 'Test procedures, nested testing, TTY testing, debugging'
---

# Testing Procedures

## Nested Testing (Primary)

### Automated Nested Test Harness (`scripts/nested-test.sh`)

For anything that opens the compositor, use `scripts/nested-test.sh`:

```bash
./scripts/nested-test.sh scripts/nested-tests/smoke.test
./scripts/nested-test.sh scripts/nested-tests/popup-select-chromium.test
./scripts/nested-test.sh scripts/nested-tests/health.test
```

It automatically handles all safety isolation:
- Creates a private `XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`, and private D-Bus session (so it never collides with the host tray or remounts `/run/user/$UID/doc`).
- Stubs the keyring daemon so apps don't pop password prompts.
- Runs in its own process group (`setsid`) and cleans up only that group on exit.
- Verifies that the host `xdg-document-portal` mount survived unchanged.
- Gives test apps the nested Xwayland's `DISPLAY`, never the host's, so X11 apps can't open on the real screen.
- Sets `VIBE_TEST_INPUT=1` so the test script can inject clicks, keys, and diagnostic commands over `$VIDE_IPC`.

**Test file commands** (used in `scripts/nested-tests/*.test`):
- `run <cmd>` / `run-host <cmd>` — launch an app inside the nested session (or on the host)
- `expect-window <title>` — wait for a window with matching title (supports quoted strings)
- `move <X> <Y>`, `click [left|right|middle]`, `scroll <N>`, `key <combo>`, `type <text>` — inject pointer/keyboard input
- `shot <name>` — save a screenshot to the run's output directory
- `windows`, `ipc <cmd>` — query windows or send any `$VIDE_IPC` command (e.g. `popups`, `pointer-state`, `stall`)
- `stop` / `cont` — `SIGSTOP` / `SIGCONT` a launched client to test freeze detection
- `expect-log <pattern>` — assert a pattern appears in the compositor log

### Manual Nested Test

Run inside current Wayland/X11 desktop as a window.

### Basic Test

```bash
# Build
cargo build --release --workspace

# Run with 12s timeout, log to file
VIBE_STARTUP=konsole timeout 12 ./target/release/vibe-comp --nested 2> /tmp/vibe.log

# Check for errors
grep -i error /tmp/vibe.log
grep -i panic /tmp/vibe.log
```

### Private D-Bus (Required for Tray)

```bash
# Prevents collision with host desktop's tray watcher
dbus-run-session -- env VIBE_STARTUP=konsole timeout 12 ./target/release/vibe-comp --nested 2> /tmp/vibe.log
```

### Private XDG_RUNTIME_DIR (Required for Portals)

```bash
# Prevents xdg-document-portal conflicts with host
XDG_RUNTIME_DIR=/tmp/vt \
WAYLAND_DISPLAY=wayland-1 \
dbus-run-session -- env VIBE_STARTUP=konsole timeout 12 ./target/release/vibe-comp --nested 2> /tmp/vibe.log

# Verify no portal remount:
mount | grep /run/user/$UID/doc  # Before and after test
```

### Debug Flags for Automated Testing

```bash
# Screenshot after 6s (verify rendering)
VIBE_DEBUG_SHOT=1 timeout 12 ./target/release/vibe-comp --nested 2> /tmp/vibe.log
# Check ~/Pictures/Screenshots/ for output

# Open Start menu (verify UI)
VIBE_DEBUG_START=1 timeout 12 ./target/release/vibe-comp --nested

# Open Start menu + type search (verify search)
VIBE_DEBUG_START=text timeout 12 ./target/release/vibe-comp --nested

# Open Task View (verify desktops)
VIBE_DEBUG_DESKTOPS=1 timeout 12 ./target/release/vibe-comp --nested

# Step Alt+Tab (verify switcher)
VIBE_DEBUG_SWITCHER=1 timeout 12 ./target/release/vibe-comp --nested
```

### Process Isolation

```bash
# Use setsid for clean process group
setsid ./target/release/vibe-comp --nested 2> /tmp/vibe.log &
PGID=$!
sleep 12
kill -TERM -- -$PGID  # Kill entire process group
```

## TTY Testing (Real Hardware)

**Only on spare VT. Never from inside desktop.**

### Procedure

```bash
# 1. Switch to free VT
# Ctrl+Alt+F4 (or F3-F6)

# 2. Log in as normal user

# 3. Run with logging
dbus-run-session -- ./target/release/vibe-comp --tty 2> ~/vibe.log

# 4. Test functionality:
#    - Open terminal (Super+Enter)
#    - Launch apps from Start menu
#    - Switch workspaces (Super+1..0)
#    - Test XWayland app (firefox, etc.)
#    - Test gaming (relative pointer)

# 5. Emergency exit
# Ctrl+Alt+Backspace

# 6. Check logs
cat ~/vibe.log
```

### NVIDIA Testing

```bash
# Ensure kernel params
# nvidia-drm.modeset=1

# Check dmabuf feedback in logs
RUST_LOG=vibe_comp::xwayland=debug,vibe_comp::state=debug dbus-run-session -- ./target/release/vibe-comp --tty 2> ~/vibe.log

# Verify no flicker/stale frames
```

## Test Checklist

### Window Management
- [ ] Open/close windows
- [ ] Move/resize (mouse + keyboard)
- [ ] Maximize/restore
- [ ] Fullscreen toggle
- [ ] Minimize/restore
- [ ] Snap (Win+Arrow)
- [ ] Workspace switch (Super+1..0)
- [ ] Move window to workspace (Super+Shift+1..0)

### Input
- [ ] Keybindings work
- [ ] Start menu typing/search
- [ ] Alt+Tab switcher
- [ ] Pointer focus follows mouse
- [ ] Click-to-focus (if enabled)
- [ ] Media keys (volume/brightness)

### Rendering
- [ ] No visual artifacts
- [ ] Titlebars render correctly (SSD)
- [ ] CSD apps work (no titlebar)
- [ ] Layer shell surfaces (desktop, panels)
- [ ] Snip overlay (Win+Shift+S)
- [ ] Screenshot (PrtSc)

### XWayland
- [ ] X11 apps launch
- [ ] X11 apps get GPU acceleration
- [ ] Games work (relative pointer)
- [ ] Override-redirect windows (menus)

### System Integration
- [ ] Tray icons (SNI)
- [ ] Volume/brightness flyouts
- [ ] Notifications
- [ ] Clipboard
- [ ] Drag and drop

### Built-in Apps
- [ ] Settings launches
- [ ] PowerSlop launches
- [ ] File Exploder launches
- [ ] Task Mangler launches
- [ ] Snip & Sketchy launches
- [ ] About VIDE-DE launches

## Regression Testing

Before each PR, run:

```bash
# 1. Clean build
cargo clean
cargo build --release --workspace
cargo build --release --workspace --features vibe-comp/tty

# 2. Nested test (full)
XDG_RUNTIME_DIR=/tmp/vt WAYLAND_DISPLAY=wayland-1 \
dbus-run-session -- env VIBE_STARTUP=konsole VIBE_DEBUG_SHOT=1 \
timeout 15 ./target/release/vibe-comp --nested 2> /tmp/vibe.log

# 3. Verify screenshot exists and looks correct
ls -la ~/Pictures/Screenshots/

# 4. Check logs
grep -E "(error|panic|warn)" /tmp/vibe.log
```

## Performance Testing

```bash
# Frame time measurement
VIBE_DEBUG_SHOT=1 RUST_LOG=vibe_comp::render=debug \
timeout 30 ./target/release/vibe-comp --nested 2>&1 | grep "frame"

# Memory usage
heaptrack ./target/release/vibe-comp --nested
```

## Related Files

- [Building](/vide-de/development/building/) — Build instructions
- [Architecture](/vide-de/architecture/overview/) — Component overview
- [NVIDIA Guide](/vide-de/wiki/nvidia-guide/) — NVIDIA-specific testing