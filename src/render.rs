//! Building the element list for a frame. Backends hand these to a damage
//! tracker (nested) or a DRM compositor (TTY) and only changed regions get redrawn.

use std::sync::Mutex;

use smithay::{
    backend::renderer::{
        element::{
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            utils::RescaleRenderElement,
            render_elements,
            solid::{SolidColorBuffer, SolidColorRenderElement},
            surface::{render_elements_from_surface_tree, WaylandSurfaceRenderElement},
            Kind,
        },
        gles::GlesRenderer,
        Color32F,
    },
    desktop::PopupManager,
    input::pointer::{CursorImageAttributes, CursorImageStatus},
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, Rectangle},
    wayland::compositor::{with_states, with_surface_tree_downward, SurfaceAttributes, TraversalAction},
};

use smithay::wayland::shell::wlr_layer::Layer;

use crate::{
    state::{Hover, State},
    ui::{self, StartKey, TaskbarKey, MENU_W, TASKBAR_H},
};

pub const CLEAR: Color32F = Color32F::new(0.0, 0.1, 0.24, 1.0);
pub const BORDER_ACTIVE: Color32F = Color32F::new(0.0, 0.47, 0.84, 1.0);
pub const BORDER_IDLE: Color32F = Color32F::new(0.6, 0.6, 0.6, 1.0);

render_elements! {
    pub Elem<=GlesRenderer>;
    Surface=WaylandSurfaceRenderElement<GlesRenderer>,
    Solid=SolidColorRenderElement,
    /// CPU-painted pictures: cursor, taskbar, start menu, titlebars, wallpaper.
    Image=MemoryRenderBufferRenderElement<GlesRenderer>,
    /// A window drawn shrunk (Alt+Tab thumbnails).
    Thumb=RescaleRenderElement<WaylandSurfaceRenderElement<GlesRenderer>>,
}

pub type BorderPiece<'a> = (&'a SolidColorBuffer, Point<i32, Logical>);

fn image(renderer: &mut GlesRenderer, buf: &MemoryRenderBuffer, at: Point<i32, Logical>, kind: Kind) -> Option<Elem> {
    MemoryRenderBufferRenderElement::from_buffer(renderer, at.to_f64().to_physical(1.0), buf, None, None, None, kind)
        .ok()
        .map(Elem::Image)
}

fn surface_tree(renderer: &mut GlesRenderer, s: &WlSurface, at: Point<i32, Logical>, kind: Kind) -> Vec<Elem> {
    render_elements_from_surface_tree(renderer, s, at.to_physical(1), 1.0, 1.0, kind)
        .into_iter()
        .map(Elem::Surface)
        .collect()
}

/// A window drawn live, shrunk to fit `area` (never enlarged) and centred in it.
fn live_thumbnail(renderer: &mut GlesRenderer, state: &State, s: &WlSurface, area: Rectangle<i32, Logical>) -> Vec<Elem> {
    let Some(w) = state.wm.find(s) else { return Vec::new() };
    let g = w.geometry();
    if g.size.w <= 0 || g.size.h <= 0 {
        return Vec::new();
    }
    let scale = (area.size.w as f64 / g.size.w as f64).min(area.size.h as f64 / g.size.h as f64).min(1.0);
    let (tw, th) = ((g.size.w as f64 * scale) as i32, (g.size.h as f64 * scale) as i32);
    // Shift by the geometry offset so client shadows sit outside the thumbnail.
    let off = w.surface_loc() - w.pos;
    let at = area.loc + Point::from(((area.size.w - tw) / 2, (area.size.h - th) / 2));
    // Draw at full size anchored at `at`, then shrink everything around that point.
    let origin_px = at.to_physical(1);
    render_elements_from_surface_tree(renderer, s, (at + off).to_physical(1), 1.0, 1.0, Kind::Unspecified)
        .into_iter()
        .map(|e| Elem::Thumb(RescaleRenderElement::from_element(e, origin_px, scale)))
        .collect()
}

/// Everything on screen, front to back.
pub fn elements(renderer: &mut GlesRenderer, state: &mut State) -> Vec<Elem> {
    elements_with(renderer, state, true)
}

/// Like `elements`, optionally without the cursor (screenshots leave it out).
pub fn elements_with(renderer: &mut GlesRenderer, state: &mut State, cursor: bool) -> Vec<Elem> {
    let mut out = Vec::new();
    let (ow, oh) = (state.output_size.w, state.output_size.h);
    let focused = state.focused();

    // Cursor on top of everything.
    let ptr = state.pointer_loc.to_i32_round();
    let status = if cursor { state.cursor_status.clone() } else { CursorImageStatus::Hidden };
    match &status {
        CursorImageStatus::Hidden => {}
        CursorImageStatus::Surface(surface) => {
            let hotspot = with_states(surface, |s| {
                s.data_map.get::<Mutex<CursorImageAttributes>>().map(|a| a.lock().unwrap().hotspot).unwrap_or_default()
            });
            out.extend(surface_tree(renderer, surface, ptr - hotspot, Kind::Cursor));
        }
        CursorImageStatus::Named(icon) => {
            let (buf, hot) = state.cursors.get(*icon);
            out.extend(image(renderer, buf, ptr - *hot, Kind::Cursor));
        }
    }

    // Alt+Tab: live window thumbnails on top of the panel picture.
    crate::supervisor::guard("alt-tab switcher", || {
        if let (Some(origin), Some(panel)) = (state.switcher_origin(), state.switcher_picture()) {
            let sw = state.switcher.as_ref().unwrap();
            for (i, s) in sw.order.iter().enumerate() {
                let tile = sw.tile(i);
                let area = Rectangle::new(
                    origin + tile.loc + Point::from(((tile.size.w - (crate::switcher::TILE_W - 20)) / 2, 30)),
                    (crate::switcher::TILE_W - 20, crate::switcher::THUMB_H).into(),
                );
                out.extend(live_thumbnail(renderer, state, s, area));
            }
            out.extend(image(renderer, &panel, origin, Kind::Unspecified));
        }
    });

    // Overlay-layer clients (lock screens, notifications) above all our chrome.
    for (s, at) in state.layer_surfaces(&[Layer::Overlay]) {
        out.extend(surface_tree(renderer, &s, at, Kind::Unspecified));
    }

    // Software brightness: everything below the cursor seen through a black veil.
    if state.tray.brightness < 0.999 {
        let alpha = (1.0 - state.tray.brightness) * 0.85;
        state.dim.update(state.output_size, Color32F::new(0.0, 0.0, 0.0, alpha));
        out.push(Elem::Solid(SolidColorRenderElement::from_buffer(&state.dim, (0, 0), 1.0, 1.0, Kind::Unspecified)));
    }

    // Notification toasts, above the Action Center and flyouts.
    crate::supervisor::guard("toasts", || {
        if let Some((buf, at)) = state.toasts_picture() {
            out.extend(image(renderer, &buf, at, Kind::Unspecified));
        }
    });

    // Task View: live window thumbnails over its picture.
    crate::supervisor::guard("task view", || {
        let desktops_at = state.desktops_rect().loc;
        if let Some(buf) = state.desktops_picture() {
            for (s, area) in state.task_view_thumbs() {
                out.extend(live_thumbnail(renderer, state, &s, area));
            }
            out.extend(image(renderer, &buf, desktops_at, Kind::Unspecified));
        }
    });

    // Action Center (notifications).
    crate::supervisor::guard("action center", || {
        let action_center_at = state.action_center_rect().loc;
        if let Some(buf) = state.action_center_picture() {
            out.extend(image(renderer, &buf, action_center_at, Kind::Unspecified));
        }
    });

    // Tray flyout (volume/brightness slider), above the taskbar.
    crate::supervisor::guard("tray flyout", || {
        let flyout_at = state.flyout_rect().loc;
        if let Some(buf) = state.flyout_picture().cloned() {
            out.extend(image(renderer, &buf, flyout_at, Kind::Unspecified));
        }
    });

    // Snipping overlay: everything dimmed except the selection.
    if let Some(snip) = state.snip.as_mut() {
        let sel = snip.selection(state.pointer_loc);
        for (buf, at) in snip.overlay(Rectangle::from_size(state.output_size), sel) {
            out.push(Elem::Solid(SolidColorRenderElement::from_buffer(buf, at.to_physical(1), 1.0, 1.0, Kind::Unspecified)));
        }
    }

    // Our right-click menu.
    crate::supervisor::guard("context menu", || {
        if let Some(m) = state.menu.as_mut() {
            let pos = m.pos;
            let buf = m.picture(state.font.as_ref()).clone();
            out.extend(image(renderer, &buf, pos, Kind::Unspecified));
        }
    });

    // X11 menus and tooltips, above everything but the cursor.
    for x in state.x11_overrides.iter().rev() {
        if let Some(s) = x.wl_surface() {
            out.extend(surface_tree(renderer, &s, x.geometry().loc, Kind::Unspecified));
        }
    }

    // Drag-and-drop icon following the cursor.
    if let Some(icon) = &state.dnd_icon {
        let loc = state.pointer_loc.to_i32_round() + Point::from((12, 12));
        out.extend(surface_tree(renderer, icon, loc, Kind::Unspecified));
    }

    // Start menu.
    crate::supervisor::guard("start menu", || {
        if state.start.open {
            let height = (oh - TASKBAR_H).min(620);
            let items = state.start_rows();
            let hover = match &state.hover {
                Hover::Start(h) => Some(h.clone()),
                _ => None,
            };
            let key = StartKey {
                height,
                query: state.start.query.clone(),
                items,
                selected: state.start.selected,
                scroll: state.start.scroll,
                hover,
                power_open: state.start.power_open,
                searching: state.start.searching(),
                filter: state.start.filter,
                kinds: state.start.results.iter().map(|e| state.result_kind(e)).collect(),
                tiles: state.start_tiles().into_iter().map(|(i, g)| (state.apps[i].name.clone(), state.apps[i].icon.clone(), g)).collect(),
                user: std::env::var("USER").unwrap_or_else(|_| "User".into()),
                rail_open: state.start.rail_open,
                preview: if state.start.searching() && !state.start.query.trim().is_empty() { state.start_preview() } else { None },
            };
            if state.start.menu.as_ref().is_none_or(|m| m.key != key) {
                state.start.menu = Some(ui::draw_start_menu(state.font.as_ref(), &mut state.icons, key));
            }
            let menu = state.start.menu.as_ref().unwrap();
            out.extend(image(renderer, &menu.buffer, (0, oh - TASKBAR_H - height).into(), Kind::Unspecified));
            let _ = MENU_W;
        }
    });

    // Taskbar, unless a fullscreen window is in front.
    crate::supervisor::guard("taskbar", || {
        // Fullscreen apps cover it, unless the start menu is up (Super over a game shows it, like Windows).
        let fullscreen_top = state.wm.top().is_some_and(|w| w.fullscreen.is_some() && Some(w.surface()) == focused.as_ref());
        if !fullscreen_top || state.start.open {
            let key = TaskbarKey {
                width: ow,
                items: state.taskbar_items(focused.as_ref()),
                time: state.clock.0.clone(),
                date: state.clock.1.clone(),
                hover: match &state.hover {
                    Hover::Taskbar(h) => Some(h.clone()),
                    _ => None,
                },
                start_open: state.start.open,
                volume: ((state.tray.volume * 100.0).round() as u32, state.tray.muted),
                net: state.tray.icon(),
                flyout: state.tray.flyout,
                current_workspace: state.wm.current_workspace,
                taskview_open: state.desktops.open,
                tray_apps: state.sni_items.iter().map(|i| (i.id.clone(), tray_fingerprint(i))).collect(),
                search_text: (state.start.open && state.start.searching()).then(|| state.start.query.clone()),
                notifications: (!state.action_center.notifications.is_empty(), state.action_center.unseen),
                action_center_open: state.action_center.open,
            };
            if state.taskbar.as_ref().is_none_or(|t| t.key != key) {
                let tray_icons = tray_icons(state);
                state.taskbar = Some(ui::draw_taskbar(state.font.as_ref(), &mut state.icons, state.logo.as_ref(), &tray_icons, key));
            }
            let bar = state.taskbar.as_ref().unwrap();
            out.extend(image(renderer, &bar.buffer, (0, oh - TASKBAR_H).into(), Kind::Unspecified));
        }
    });

    // Top-layer clients (panels, docks) above windows.
    for (s, at) in state.layer_surfaces(&[Layer::Top]) {
        out.extend(surface_tree(renderer, &s, at, Kind::Unspecified));
    }

    // Where a dragged window will snap.
    if let Some(z) = state.wm.snap_preview {
        state.snap_buf.update(z.size, Color32F::new(0.46, 0.73, 0.93, 0.28));
        out.push(Elem::Solid(SolidColorRenderElement::from_buffer(&state.snap_buf, z.loc.to_physical(1), 1.0, 1.0, Kind::Unspecified)));
    }

    // Windows, top to bottom (only current workspace).
    let font = state.font.as_ref();
    let icons = &mut state.icons;
    let apps = &state.apps;
    for w in state.wm.windows.iter_mut().rev().filter(|w| !w.minimized && w.mapped && w.workspace == state.wm.current_workspace) {
        let active = Some(w.surface()) == focused.as_ref();
        let sloc = w.surface_loc();
        for (popup, offset) in PopupManager::popups_for_surface(w.surface()) {
            // Relative to the parent's window geometry (`pos`), not its surface (`sloc`).
            let ploc = w.pos + offset - popup.geometry().loc;
            out.extend(surface_tree(renderer, popup.wl_surface(), ploc, Kind::Unspecified));
        }
        out.extend(surface_tree(renderer, w.surface(), sloc, Kind::Unspecified));

        let hover = match &state.hover {
            Hover::Title(s, b) if s == w.surface() => Some(*b),
            _ => None,
        };
        if let Some((buf, at)) = w.titlebar(font, icons, apps, active, hover) {
            out.extend(image(renderer, buf, at, Kind::Unspecified));
        }
        if w.decorated() && w.maximized.is_none() {
            for (buf, at) in w.border(if active { BORDER_ACTIVE } else { BORDER_IDLE }) {
                out.push(Elem::Solid(SolidColorRenderElement::from_buffer(buf, at.to_physical(1), 1.0, 1.0, Kind::Unspecified)));
            }
        }
    }

    // Bottom and background layers: vide-desktop's icons and wallpaper live here.
    state.arrange_layers();
    for (s, at) in state.layer_surfaces(&[Layer::Bottom, Layer::Background]) {
        out.extend(surface_tree(renderer, &s, at, Kind::Unspecified));
    }
    if state.has_background_layer() {
        return out;
    }

    // Our own wallpaper at the very back, when no desktop program provides one.
    if state.wallpaper.as_ref().is_none_or(|(s, _)| *s != state.output_size) {
        state.wallpaper = Some((state.output_size, ui::draw_wallpaper(ow, oh)));
    }
    let (_, wp) = state.wallpaper.as_ref().unwrap();
    out.extend(image(renderer, wp, (0, 0).into(), Kind::Unspecified));

    // Night light: a warm veil over the whole picture, cursor included, like the
    // monitor's own colour temperature (which desktop panels usually don't have).
    if state.config.night_light && state.config.night_light_strength > 0.0 {
        let alpha = (state.config.night_light_strength as f32 * 0.32).clamp(0.0, 0.4);
        state.night.update(state.output_size, Color32F::new(1.0, 0.72, 0.42, alpha));
        out.push(Elem::Solid(SolidColorRenderElement::from_buffer(&state.night, (0, 0), 1.0, 1.0, Kind::Unspecified)));
    }

    out
}

const TRAY_ICON: u32 = 20;

fn tray_fingerprint(i: &crate::sni::TrayItem) -> String {
    match &i.pixmap {
        // Cheap content hash: size plus a sparse sample of the pixels.
        Some((w, h, px)) => {
            let sample: u64 = px.iter().step_by(37).map(|&b| b as u64).sum();
            format!("px:{w}x{h}:{sample}")
        }
        None => format!("{}|{}", i.theme_path, i.icon_name),
    }
}

/// Load each app tray icon: from the app's own icon folder, the theme, or its raw pixels.
fn tray_icons(state: &mut State) -> Vec<Option<std::rc::Rc<crate::icons::Icon>>> {
    let mut out = Vec::new();
    for item in &state.sni_items {
        let fp = tray_fingerprint(item);
        if !state.tray_icon_cache.contains_key(&fp) {
            let raw_icon = match &item.pixmap {
                Some((w, h, px)) => Some(crate::icons::from_rgba(*w, *h, px.clone(), 48)),
                None => {
                    let in_app_dir = (!item.theme_path.is_empty())
                        .then(|| {
                            ["png", "svg"].iter().find_map(|ext| {
                                state.tray_theme.get(&format!("{}/{}.{ext}", item.theme_path, item.icon_name), 48)
                            })
                        })
                        .flatten();
                    in_app_dir
                        .or_else(|| state.tray_theme.get(&item.icon_name, 48))
                        .map(|rc| crate::icons::from_rgba(rc.w, rc.h, rc.rgba.clone(), 48))
                }
            };
            // Share the tray icon with the window/taskbar icon cache as a custom fallback,
            // so apps launched from a terminal without a .desktop file still show an icon.
            if let Some(ref ic) = raw_icon {
                let copy = |i: &crate::icons::Icon| crate::icons::Icon { w: i.w, h: i.h, rgba: i.rgba.clone() };
                let sni_id = item.id.split('/').next().unwrap_or(&item.id);
                if !sni_id.is_empty() && !sni_id.starts_with(':') {
                    state.icons.set_custom(sni_id, copy(ic));
                }
                if !item.title.is_empty() {
                    state.icons.set_custom(&item.title, copy(ic));
                }
                if !item.icon_name.is_empty() {
                    state.icons.set_custom(&item.icon_name, copy(ic));
                }
                if !item.proc_name.is_empty() {
                    state.icons.set_custom(&item.proc_name, copy(ic));
                }
            }
            let icon = raw_icon.map(|ic| {
                let scaled = crate::icons::from_rgba(ic.w, ic.h, ic.rgba, TRAY_ICON);
                std::rc::Rc::new(lighten_dark_glyph(&scaled).unwrap_or(scaled))
            });
            state.tray_icon_cache.insert(fp.clone(), icon);
        }
        out.push(state.tray_icon_cache[&fp].clone());
    }
    out
}

/// Tray icons drawn for light menu bars (black "template" glyphs, like Claude's) vanish
/// on our dark taskbar. If an icon is nearly all dark and colourless, redraw it in white,
/// keeping its shape (alpha). Colourful icons are left alone.
fn lighten_dark_glyph(icon: &crate::icons::Icon) -> Option<crate::icons::Icon> {
    let (mut n, mut lum, mut sat) = (0u32, 0.0f32, 0.0f32);
    for px in icon.rgba.chunks_exact(4) {
        let a = px[3] as f32 / 255.0;
        if a < 0.25 {
            continue;
        }
        // Un-premultiply to judge the real colour.
        let (r, g, b) = (px[0] as f32 / 255.0 / a, px[1] as f32 / 255.0 / a, px[2] as f32 / 255.0 / a);
        lum += 0.2126 * r + 0.7152 * g + 0.0722 * b;
        sat += r.max(g).max(b) - r.min(g).min(b);
        n += 1;
    }
    if n == 0 || lum / n as f32 > 0.3 || sat / n as f32 > 0.15 {
        return None;
    }
    let rgba = icon.rgba.chunks_exact(4).flat_map(|px| [px[3], px[3], px[3], px[3]]).collect();
    Some(crate::icons::Icon { w: icon.w, h: icon.h, rgba })
}

/// Tell every visible surface it was just presented so it can draw its next frame.
pub fn send_frames(state: &State, time: u32) {
    let send = |surface: &WlSurface| {
        with_surface_tree_downward(
            surface,
            (),
            |_, _, &()| TraversalAction::DoChildren(()),
            |_, states, &()| {
                for cb in states.cached_state.get::<SurfaceAttributes>().current().frame_callbacks.drain(..) {
                    cb.done(time);
                }
            },
            |_, _, &()| true,
        );
    };
    for w in &state.wm.windows {
        send(w.surface());
        for (popup, _) in PopupManager::popups_for_surface(w.surface()) {
            send(popup.wl_surface());
        }
    }
    for x in &state.x11_overrides {
        if let Some(s) = x.wl_surface() {
            send(&s);
        }
    }
    for (s, _) in state.layer_surfaces(&[Layer::Overlay, Layer::Top, Layer::Bottom, Layer::Background]) {
        send(&s);
    }
    if let CursorImageStatus::Surface(s) = &state.cursor_status {
        send(s);
    }
}
