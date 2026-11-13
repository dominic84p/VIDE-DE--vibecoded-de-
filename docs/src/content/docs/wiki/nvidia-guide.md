---
title: 'NVIDIA Guide'
description: 'NVIDIA-specific setup, troubleshooting, and workarounds'
---

# NVIDIA Guide

VIDE-DE works on NVIDIA but requires specific configuration due to driver limitations.

## Kernel Parameters

**Required:**
```bash
# /etc/default/grub
GRUB_CMDLINE_LINUX_DEFAULT="... nvidia-drm.modeset=1"
# Then: sudo grub-mkconfig -o /boot/grub/grub.cfg
```

## Driver Version

- **Minimum:** 550+ (explicit sync support)
- **Recommended:** 560+ (better Wayland support)
- **Avoid:** 535 and older (no explicit sync)

## Known Issues & Workarounds

### 1. Hardware Cursors Unreliable

**Problem:** Cursor flickers, disappears, or shows wrong image.

**Fix:** TTY backend forces software cursor:
```rust
// tty.rs - always composite, never scanout
FrameFlags::empty()  // Instead of FrameFlags::DEFAULT
```

### 2. Overlay Planes Unreliable

**Problem:** Direct scanout of client buffers skips sync wait.

**Fix:** Always composite via GPU:
```rust
// tty.rs - render with empty flags
backend.render(elements, FrameFlags::empty());
```

### 3. Explicit Sync Required (dmabuf)

**Problem:** Without `linux-drm-syncobj`, frames shown before GPU finishes → flicker, stale frames, black rectangles.

**Fix:** Pre-commit hook in `State::new_surface`:
```rust
fn new_surface(&mut self, surface: WlSurface) {
    surface.with_pending_state(|state| {
        state.acquire_point = Some(AcquirePoint::default());
    });
}
```

**Verify in logs:**
```bash
RUST_LOG=vibe_comp::state=debug ./target/release/vibe-comp --tty 2> ~/vibe.log
# Look for: "dmabuf feedback", "acquire point", "syncobj"
```

### 4. XWayland GPU Acceleration

**Problem:** Without linux-dmabuf feedback, XWayland can't use GPU → games at single-digit FPS.

**Fix:** `State::init_dmabuf()` MUST run in every backend:
```rust
fn init_dmabuf(&mut self) {
    let feedback = DmabufFeedback::new(
        self.renderer.device(),
        self.renderer.format_modifiers(),
    );
    self.xwayland.as_mut().unwrap().set_dmabuf_feedback(feedback);
}
```

### 5. G-Sync / VRR

**Status:** Not explicitly supported. Works if compositor doesn't interfere.

### 6. Multiple GPUs (Optimus/Prime)

**Problem:** Laptop with iGPU + dGPU.

**Fix:** 
- Use `prime-run` or `optimus-manager`
- Set `__NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia`
- Or configure `VIDE_TTY_DEVICE=/dev/dri/card1` (dGPU)

## Testing Checklist

```bash
# 1. Verify modeset
cat /sys/module/nvidia_drm/parameters/modeset
# Should be "Y"

# 2. Check dmabuf
ls /dev/dri/
# Should have renderD128, card0, card1

# 3. Run with debug
RUST_LOG=vibe_comp::xwayland=debug,vibe_comp::state=debug \
dbus-run-session -- ./target/release/vibe-comp --tty 2> ~/vibe.log

# 4. Check logs for:
# - "dmabuf feedback created"
# - "acquire point set"
# - "syncobj"
# - No "failed to import dmabuf"
```

## Environment Variables

```bash
# Force NVIDIA as primary
__NV_PRIME_RENDER_OFFLOAD=1
__GLX_VENDOR_LIBRARY_NAME=nvidia

# Debug
WAYLAND_DEBUG=1
LIBVA_DRIVER_NAME=nvidia
VDPAU_DRIVER=nvidia
```

## XWayland Games

**Working:**
- Steam (Proton) games
- Native Linux games (Vulkan/OpenGL)
- Wine games (with proper setup)

**Requirements:**
- `linux-dmabuf` feedback working
- `relative-pointer` + `pointer-constraints` protocols
- `zwp_idle_inhibit_v1` for fullscreen games

## Troubleshooting

| Symptom | Likely Cause | Fix |
|---------|--------------|-----|
| Black screen | No modeset | `nvidia-drm.modeset=1` |
| Flickering | No explicit sync | Check acquire point hook |
| Stale frames | No syncobj | Driver 550+, check logs |
| Games slow | No dmabuf | `init_dmabuf()` in backend |
| Cursor issues | HW cursor | Forced software (auto) |
| X11 apps crash | XWayland sync | Update driver |

## Recommended Settings

```ini
# ~/.config/vide-de/settings.conf
[xwayland]
enabled = true
lazy-start = true
```

## Related

- [Troubleshooting](/wiki/troubleshooting/)
- [Architecture](/architecture/overview/)
- [XWayland](/components/xwayland/)
- [Testing](/development/testing/)