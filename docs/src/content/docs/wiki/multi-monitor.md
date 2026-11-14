---
title: 'Multi-Monitor Setup'
description: 'Using several monitors with VIDE-DE'
---

# Multi-Monitor Setup

## Detection

On the TTY, every connected monitor is used, each on its own CRTC. Plug one in or pull one
out and udev tells us; the monitor list is rescanned and windows on a monitor that went away
come back to the primary. Monitors are rescanned after switching back from another VT too.
Nothing to configure.

A GPU has a limited number of CRTCs (usually 3–4); a monitor beyond that is logged
(`no free CRTC for ...`) and left dark.

## Layout

- The **primary** monitor is the first one found. It sits at the top-left of the layout and
  carries the shell: taskbar, Start, Action Center, tray flyouts, toasts, Task View and
  desktop icons, like Windoze's default "show taskbar on main display only".
- The other monitors line up **to its right**, top-aligned, in connector order.
- There is no arrangement UI yet, and Settings → Display's resolution applies to the primary
  only; the others use their preferred mode. The **scale** applies to all of them.

## Windows

| Action | Where |
|--------|-------|
| New window | Opens on the monitor the pointer is on |
| Maximize (Win+Up, titlebar double-click) | Fills the monitor the window is on (minus the taskbar on the primary) |
| Snap (Win+Left/Right, drag to an edge) | Halves of the monitor it's on; dragging snaps on the monitor under the pointer |
| Fullscreen | The monitor the window is on, or the one the app asked for |
| Moving | Drag a window across monitors freely |

Apps are told which monitor their window is on (`wl_surface.enter`), so their own menus and
fullscreen requests land there.

## Pointer

The pointer moves across all monitors. Where monitors have different heights, it stops at
the edge rather than entering the empty space beside the shorter one.

## Desktop and wallpaper

`vide-desktop` (icons and the chosen wallpaper) is on the primary. Other monitors show the
built-in VIDE-DE gradient wallpaper for now.

## Screenshots

PrtSc captures every monitor in one image, at full physical resolution. Win+Shift+S can
select a region on any monitor.

## Testing without the hardware

The nested compositor can pretend to have several monitors by splitting its window:

```bash
VIBE_NESTED_MONITORS=2 scripts/nested-test.sh my.test
```

## Logs

```bash
grep -E "monitor |output |no free CRTC|monitors changed" ~/vibe.log
```

## Not yet

- Taskbar on every monitor
- Choosing the primary and arranging monitors in Settings
- Per-monitor resolution and scale
- A wallpaper from `vide-desktop` on secondary monitors

## Related

- [Configuration](/vide-de/configuration/settings)
- [Layer Shell](/vide-de/components/layers/)
- [XWayland](/vide-de/components/xwayland/)
