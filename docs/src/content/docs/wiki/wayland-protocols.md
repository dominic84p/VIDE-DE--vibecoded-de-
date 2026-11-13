---
title: 'Wayland Protocols'
description: 'Wayland protocols implemented and used by VIDE-DE'
---

# Wayland Protocols

VIDE-DE implements and uses these Wayland protocols via Smithay 0.7.

## Core Protocols (Required)

| Protocol | Version | Purpose | Handler |
|----------|---------|---------|---------|
| `wl_compositor` | 4 | Surface creation | `CompositorState` |
| `wl_subcompositor` | 1 | Sub-surfaces | `CompositorState` |
| `wl_shm` | 1 | Shared memory buffers | `ShmState` |
| `wl_data_device_manager` | 3 | Clipboard, DnD. A copy outlives the app that made it (`clipboard.rs`), our own copies (screenshots) reach X11 apps too, and the focused app is offered the clipboard again shortly after focus and on Ctrl+V / Shift+Insert (apps that make their clipboard device late would otherwise miss it) | `DataDeviceState` |
| `wl_seat` | 7 | Input devices | `SeatState` |
| `wl_output` | 4 | Output management | `OutputState` |

## Shell Protocols

| Protocol | Version | Purpose | Handler |
|----------|---------|---------|---------|
| `xdg_wm_base` | 2 | Toplevel/popup windows | `XdgShellState` |
| `xdg_decoration` | 1 | Server-side decorations | `DecorationState` |
| `zxdg_exporter_v2` / `zxdg_importer_v2` | 1 | xdg-foreign: portal dialogs attach to the app that opened them | `XdgForeignState` |
| `wlr_layer_shell` | 1 | Desktop, panels, overlays | `LayerShellState` |
| `wlr_foreign_toplevel` | 2 | Window management for external tools | (Not implemented) |

## Input Protocols

| Protocol | Version | Purpose |
|----------|---------|---------|
| `wl_pointer` | 7 | Mouse/touchpad |
| `wl_keyboard` | 7 | Keyboard |
| `wl_touch` | 7 | Touchscreen |
| `zwp_relative_pointer_v1` | 1 | Relative pointer (gaming) |
| `zwp_pointer_constraints_v1` | 1 | Pointer lock/confine (gaming) |
| `zwp_idle_inhibit_v1` | 1 | Inhibit idle (video players) |
| `zwp_text_input_v3` | 1 | IME/text input |

## Desktop Protocols

| Protocol | Version | Purpose |
|----------|---------|---------|
| `ext_workspace` | 1 | Workspace management |
| `ext_foreign_toplevel_list` | 1 | External window list |
| `wlr_output_power_management` | 1 | Output DPMS |
| `wlr_screencopy_v1` | 1 | Screen capture (for snip) |
| `wlr_data_control_v1` | 1 | Clipboard for privileged clients |

## XWayland

| Protocol | Purpose |
|----------|---------|
| `xwayland` | X11 support via XWayland |

## Smithay-Provided Implementations

We use Smithay's implementations for:

```rust
// In State::new()
let compositor_state = CompositorState::new(&event_loop.handle());
let xdg_shell_state = XdgShellState::new(&event_loop.handle());
let layer_shell_state = LayerShellState::new(&event_loop.handle());
let output_state = OutputState::new(&event_loop.handle());
let seat_state = SeatState::new(&event_loop.handle());
let shm_state = ShmState::new(&event_loop.handle(), vec![]);
let data_device_state = DataDeviceState::new(&event_loop.handle());
let decoration_state = DecorationState::new(&event_loop.handle());
let popup_state = PopupState::new(&event_loop.handle());
```

## Handler Traits Implemented

```rust
impl CompositorHandler for State { ... }
impl XdgShellHandler for State { ... }
impl LayerShellHandler for State { ... }
impl OutputHandler for State { ... }
impl SeatHandler for State { ... }
impl ShmHandler for State { ... }
impl DataDeviceHandler for State { ... }
impl DecorationHandler for State { ... }
impl PopupHandler for State { ... }
```

## XDG Shell Details

### Toplevel Events

```rust
fn xdg_toplevel_new(&mut self, surface: WlSurface) {
    let window = Window::new(WinKind::Wayland, surface);
    self.windows.push(window);
}

fn xdg_toplevel_close(&mut self, surface: WlSurface) {
    self.remove_window(surface);
}

fn xdg_toplevel_configure(&mut self, surface: WlSurface, configure: Configure) {
    // Handle client-acknowledged configure
}
```

### Decoration Negotiation

```rust
fn decoration_request(&mut self, surface: WlSurface, mode: DecorationMode) {
    if let Some(window) = self.window_for_surface(&surface) {
        window.ssd = mode == DecorationMode::Server;
        self.needs_redraw = true;
    }
}
```

## Layer Shell Details

```rust
fn layer_shell_new(&mut self, surface: LayerSurface) {
    match surface.layer() {
        Layer::Background => self.desktop_layer = Some(surface),
        Layer::Bottom => self.panels.push(surface),
        Layer::Top => self.overlays.push(surface),
        Layer::Overlay => self.lock_screens.push(surface),
    }
}

fn layer_shell_configure(&mut self, surface: LayerSurface, configure: LayerConfigure) {
    // Apply exclusive_zone, anchor, size
    // Update workspace layout for exclusive zones
}
```

## Screencopy (for Snip)

```rust
// Used by screenshot.rs for region capture
fn screencopy_frame(&mut self, output: WlOutput, cursor: bool) -> ScreencopyFrame {
    // Captures output frame for PrtSc / Win+Shift+S
}
```

## Protocols NOT Implemented

| Protocol | Reason |
|----------|--------|
| `xdg_activation_v1` | Not needed yet |
| `fractional_scale_v1` | Not needed |
| `viewporter` | Not needed |
| `wp_presentation` | Not needed |
| `linux_dmabuf_v1` | Used via Smithay internal |
| `wlr_foreign_toplevel` | For external tools (task switchers) |

## Version Selection

We use Smithay 0.7 which supports these protocol versions. When updating Smithay, check protocol version bumps.

## Custom Protocol Extensions

None currently. All IPC goes through `$VIDE_IPC` Unix socket (JSON Lines).

## Related

- [Architecture](/vide-de/architecture/overview/)
- [Components](/vide-de/components/window-manager/)
- [Layer Shell](/vide-de/components/layers/)
- [XWayland](/vide-de/components/xwayland/)
- [Screenshots](/vide-de/components/screenshot/)