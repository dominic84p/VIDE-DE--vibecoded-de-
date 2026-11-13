---
title: 'Gaming Guide'
description: 'Gaming on VIDE-DE: XWayland, Vulkan, Proton, performance'
---

# Gaming Guide

VIDE-DE supports gaming through XWayland with full GPU acceleration.

## Requirements

### Compositor

- `linux-dmabuf` feedback working (see [NVIDIA Guide](/wiki/nvidia-guide/))
- `relative-pointer` + `pointer-constraints` protocols
- `zwp_idle_inhibit_v1` for fullscreen games

### System

```bash
# Vulkan
sudo pacman -S vulkan-radeon vulkan-intel vulkan-nvidia
# or appropriate for your GPU

# Wine/Proton
sudo pacman -S wine proton-ge-custom

# Steam
sudo pacman -S steam
```

## XWayland Games

### Native Linux Games

Work out of the box if dmabuf works.

```bash
# Launch from Start menu or terminal
./game_executable
```

### Steam (Proton)

1. Install Steam
2. Enable "Proton Experimental" in Settings → Compatibility
3. Games launch via XWayland automatically

### Wine Games

```bash
# Use Wine with XWayland
WINEPREFIX=~/my_prefix wine setup.exe

# For better performance, use:
__NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia wine game.exe
```

## Critical Protocols for Gaming

### Relative Pointer (`zwp_relative_pointer_v1`)

```rust
// Converts X11 pointer grabs (Minecraft mouse-look) to relative motion
// Without this: cursor escapes window, camera spins wildly
```

### Pointer Constraints (`zwp_pointer_constraints_v1`)

```rust
// Confines/locks cursor to window
// Lock follows pointer focus, released on Super / focus change
fn release_pointer(&mut self) {
    self.pointer_lock = None;
    self.show_cursor();
}
```

### Idle Inhibit (`zwp_idle_inhibit_v1`)

```rust
// Prevents screen blanking during gameplay
// Automatically activated for fullscreen XWayland surfaces
```

## Performance Tuning

### NVIDIA

See [NVIDIA Guide](/wiki/nvidia-guide/) — critical for gaming.

### AMD/Intel

Generally works out of the box. Ensure:
- `mesa` latest
- `vulkan-radeon` / `vulkan-intel` installed
- `RADV_PERFTEST=aco` for AMD (optional)

### Environment Variables

```bash
# For all GPUs
export MESA_VK_DEVICE_SELECT=10de:1234  # Force GPU (PCI ID)

# NVIDIA specific
export __NV_PRIME_RENDER_OFFLOAD=1
export __GLX_VENDOR_LIBRARY_NAME=nvidia
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/nvidia_icd.json

# AMD specific
export RADV_PERFTEST=aco,gpl
export AMD_DEBUG=w32ge,w32cs

# General
export vblank_mode=0  # Disable vsync (for benchmarks)
```

## Game-Specific

### Minecraft

```bash
# Java edition
java -jar minecraft.jar

# Ensure relative pointer works (cursor locks to window)
# If cursor escapes: check zwp_relative_pointer_v1 in logs
```

### Steam Games

```bash
# Launch options (per game)
PROTON_ENABLE_NVAPI=1 DXVK_ASYNC=1 %command%

# Or globally
export PROTON_ENABLE_NVAPI=1
export DXVK_ASYNC=1
```

### Lutris

```bash
# Install lutris
sudo pacman -S lutris

# Games configure Wine prefixes automatically
```

### Heroic Games Launcher (Epic/GOG)

```bash
# Flatpak
flatpak install flathub com.heroicgameslauncher.hgl
```

## Troubleshooting

| Issue | Fix |
|-------|-----|
| Game runs at 1 FPS | dmabuf not working — check [NVIDIA Guide](/wiki/nvidia-guide/) |
| Mouse escapes window | `relative-pointer` not working — check logs |
| Cursor visible in game | `pointer-constraints` not working |
| Screen blanks during game | `idle-inhibit` not working |
| Game crashes on launch | Check `RUST_LOG=vibe_comp::xwayland=debug` |
| No audio | PipeWire/WirePlumber running? `systemctl --user status pipewire` |
| Input lag | Disable compositor vsync? Not yet configurable |

## Debug Logging

```bash
RUST_LOG=vibe_comp::xwayland=debug,vibe_comp::input=debug \
./target/release/vibe-comp --nested 2> /tmp/vibe.log

# Look for:
# - "relative_pointer"
# - "pointer_constraints"
# - "idle_inhibit"
# - "dmabuf feedback"
```

## Known Working Games

| Game | Platform | Notes |
|------|----------|-------|
| Minecraft | Java/Proton | Relative pointer works |
| CS2 | Steam/Proton | Fullscreen works |
| Cyberpunk 2077 | Steam/Proton | Needs good GPU |
| Elden Ring | Steam/Proton | Works |
| Dota 2 | Native/Proton | Vulkan native |
| Factorio | Native | Works great |
| Stardew Valley | Native | Works |

## Not Working / Issues

| Game | Issue |
|------|-------|
| Some anti-cheat games | Kernel anti-cheat blocks Wine/Proton |
| VR games | No OpenXR support yet |

## Related

- [NVIDIA Guide](/wiki/nvidia-guide/)
- [XWayland](/components/xwayland/)
- [Input Handling](/components/input-handling/)
- [Troubleshooting](/wiki/troubleshooting/)