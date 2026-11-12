//! Building the element list for a frame. Backends hand these to a damage
//! tracker (nested) or a DRM compositor (TTY) and only changed regions get redrawn.

use std::sync::Mutex;

use smithay::{
    backend::renderer::{
        element::{
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
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
    utils::{Logical, Point},
    wayland::compositor::{with_states, with_surface_tree_downward, SurfaceAttributes, TraversalAction},
};

use crate::{
    state::{Hover, State},
    ui::{self, StartKey, TaskItem, TaskbarKey, MENU_W, TASKBAR_H},
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

/// Everything on screen, front to back.
pub fn elements(renderer: &mut GlesRenderer, state: &mut State) -> Vec<Elem> {
    let mut out = Vec::new();
    let (ow, oh) = (state.output_size.w, state.output_size.h);
    let focused = state.focused();

    // Cursor on top of everything.
    let ptr = state.pointer_loc.to_i32_round();
    match &state.cursor_status {
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

    // X11 menus and tooltips, above everything but the cursor.
    for x in state.x11_overrides.iter().rev() {
        if let Some(s) = x.wl_surface() {
            out.extend(surface_tree(renderer, &s, x.geometry().loc, Kind::Unspecified));
        }
    }

    // Start menu.
    if state.start.open {
        let height = (oh - TASKBAR_H).min(620);
        let items: Vec<(String, String)> =
            state.filtered_apps().into_iter().map(|i| (state.apps[i].name.clone(), state.apps[i].icon.clone())).collect();
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
        };
        if state.start.menu.as_ref().is_none_or(|m| m.key != key) {
            state.start.menu = Some(ui::draw_start_menu(state.font.as_ref(), &mut state.icons, key));
        }
        let menu = state.start.menu.as_ref().unwrap();
        out.extend(image(renderer, &menu.buffer, (0, oh - TASKBAR_H - height).into(), Kind::Unspecified));
        let _ = MENU_W;
    }

    // Taskbar, unless a fullscreen window is in front.
    let fullscreen_top = state.wm.top().is_some_and(|w| w.fullscreen.is_some() && Some(w.surface()) == focused.as_ref());
    if !fullscreen_top {
        let mut items: Vec<_> = state.wm.windows.iter().filter(|w| w.mapped).collect();
        items.sort_by_key(|w| w.id);
        let key = TaskbarKey {
            width: ow,
            items: items
                .iter()
                .map(|w| TaskItem {
                    surface: w.surface().clone(),
                    title: w.title(),
                    app_id: crate::apps::icon_for_app_id(&state.apps, &w.app_id()),
                    focused: !w.minimized && Some(w.surface()) == focused.as_ref(),
                })
                .collect(),
            time: state.clock.0.clone(),
            date: state.clock.1.clone(),
            hover: match &state.hover {
                Hover::Taskbar(h) => Some(h.clone()),
                _ => None,
            },
            start_open: state.start.open,
        };
        if state.taskbar.as_ref().is_none_or(|t| t.key != key) {
            state.taskbar = Some(ui::draw_taskbar(state.font.as_ref(), &mut state.icons, state.logo.as_ref(), key));
        }
        let bar = state.taskbar.as_ref().unwrap();
        out.extend(image(renderer, &bar.buffer, (0, oh - TASKBAR_H).into(), Kind::Unspecified));
    }

    // Windows, top to bottom.
    let font = state.font.as_ref();
    let icons = &mut state.icons;
    let apps = &state.apps;
    for w in state.wm.windows.iter_mut().rev().filter(|w| !w.minimized && w.mapped) {
        let active = Some(w.surface()) == focused.as_ref();
        let sloc = w.surface_loc();
        for (popup, offset) in PopupManager::popups_for_surface(w.surface()) {
            let ploc = sloc + offset - popup.geometry().loc;
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

    // Wallpaper at the very back.
    if state.wallpaper.as_ref().is_none_or(|(s, _)| *s != state.output_size) {
        state.wallpaper = Some((state.output_size, ui::draw_wallpaper(ow, oh)));
    }
    let (_, wp) = state.wallpaper.as_ref().unwrap();
    out.extend(image(renderer, wp, (0, 0).into(), Kind::Unspecified));

    out
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
    if let CursorImageStatus::Surface(s) = &state.cursor_status {
        send(s);
    }
}
