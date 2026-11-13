---
title: 'Testing'
description: 'Test procedures, nested testing, TTY testing, debugging'
---

# Testing Procedures

## Nested Testing (Primary)

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

- [Building](/development/building/) — Build instructions
- [Architecture](/architecture/overview/) — Component overview
- [NVIDIA Guide](/wiki/nvidia-guide/) — NVIDIA-specific testing