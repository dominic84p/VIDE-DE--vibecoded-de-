---
title: 'System Tray (tray.rs)'
description: 'Volume and brightness flyouts, OSD overlays'
---

# System Tray (`tray.rs`)

Built-in volume and brightness control flyouts — separate from StatusNotifierItem (SNI) which handles *app* tray icons.

## Volume Flyout

```rust
struct VolumeFlyout {
    visible: bool,
    volume: u8,      // 0-100
    muted: bool,
    output: String,  // PipeWire node name
}
```

**Trigger:** Click speaker icon in taskbar, or volume keys (XF86AudioRaiseVolume, etc.)

**Rendering:**
```
┌─────────────────┐
│  [Vol] Volume  75%│  ← Icon + label + percentage
├─────────────────┤
│ ████████████░░  │  ← Visual bar (20 segments)
├─────────────────┤
│  [Mute]  [Mixer]│  ← Buttons
└─────────────────┘
```

## Brightness Flyout

```rust
struct BrightnessFlyout {
    visible: bool,
    brightness: u8,  // 0-100
    device: String,  // Backlight device path
}
```

**Trigger:** Click brightness icon, or brightness keys (XF86MonBrightnessUp, etc.)

**Note:** Desktop monitors have no hardware backlight. Brightness is a **compositor overlay** (`state.dim`).

```rust
// In render.rs - applied as full-screen dimming layer
if state.dim > 0 {
    elements.push(fullscreen_dim_element(state.dim));
}
```

## OSD (On-Screen Display)

Shows briefly when keys pressed:

```
┌──────────────────────┐
│  [Vol] Volume: 75%   │
│  ████████████░░░░░░  │
└──────────────────────┘
```

Auto-hides after 2 seconds.

## Keybindings

| Key | Action |
|-----|--------|
| `XF86AudioRaiseVolume` | Volume +5% |
| `XF86AudioLowerVolume` | Volume -5% |
| `XF86AudioMute` | Toggle mute |
| `XF86MonBrightnessUp` | Brightness +10% |
| `XF86MonBrightnessDown` | Brightness -10% |

## D-Bus Integration

Uses `wpctl` (WirePlumber) for volume, `brightnessctl` for brightness:

```rust
async fn set_volume(volume: u8) {
    Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{}%", volume)])
        .spawn()?;
}

async fn set_brightness(brightness: u8) {
    Command::new("brightnessctl")
        .args(["set", &format!("{}%", brightness)])
        .spawn()?;
}
```

## Configuration

```ini
[system-tray]
volume-step = 5
brightness-step = 10
osd-timeout = 2000  # ms
```

## Related Files

- `sni.rs` — App tray icons (different from system tray)
- `ui.rs` — Flyout rendering via canvas
- `input.rs` — Media key handling
- `state.rs` — `dim` overlay for brightness
- `config/` — Settings for step sizes