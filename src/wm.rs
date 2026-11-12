//! Our own window manager: a stack of windows (last = top) with floating
//! move/resize, maximize, minimize and fullscreen.
//!
//! A window's `pos` is where its *geometry* (the visible part, excluding any
//! client-drawn shadow) sits on screen. The surface itself goes at
//! `pos - geometry offset`.

use fontdue::Font;
use smithay::{
    backend::renderer::{
        element::{memory::MemoryRenderBuffer, solid::SolidColorBuffer},
        Color32F,
    },
    desktop::{utils::bbox_from_surface_tree, utils::under_from_surface_tree, PopupManager, WindowSurfaceType},
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel, wayland_server::protocol::wl_surface::WlSurface,
    },
    utils::{Logical, Point, Rectangle, Size},
    wayland::{
        compositor::with_states,
        shell::xdg::{SurfaceCachedState, ToplevelSurface, XdgToplevelSurfaceData},
    },
    xwayland::X11Surface,
};

use crate::{
    icons::Icons,
    render::BorderPiece,
    ui::{self, TitleButton, TitleKey, TITLE_H},
};

pub const BORDER_WIDTH: i32 = 1;
/// Invisible grab margin outside the frame for resizing.
const RESIZE_MARGIN: i32 = 6;
const CORNER: i32 = 14;
const MIN_W: i32 = 160;
const MIN_H: i32 = 80;

pub mod edge {
    // Same bit values as xdg_toplevel::ResizeEdge.
    pub const TOP: u32 = 1;
    pub const BOTTOM: u32 = 2;
    pub const LEFT: u32 = 4;
    pub const RIGHT: u32 = 8;
}

/// A native Wayland (xdg-shell) window or an X11 one via Xwayland.
pub enum WinKind {
    Wayland(ToplevelSurface),
    X11(X11Surface),
}

/// Workspace index (0-based). We support up to 10 workspaces (1-9, 0 = 10).
pub const WORKSPACE_COUNT: usize = 10;

/// Window states we ask clients to adopt.
#[derive(Clone, Copy)]
pub enum Flag {
    Activated,
    Maximized,
    Fullscreen,
    Resizing,
}

pub struct Window {
    pub id: u64,
    pub kind: WinKind,
    surface: WlSurface,
    pub pos: Point<i32, Logical>,
    /// We draw the titlebar (client agreed to server-side decorations).
    pub ssd: bool,
    pub minimized: bool,
    /// Geometry to restore to.
    pub maximized: Option<Rectangle<i32, Logical>>,
    pub fullscreen: Option<Rectangle<i32, Logical>>,
    /// Workspace this window belongs to (0-9).
    pub workspace: usize,
    /// While/after an edge resize: keep the opposite edges pinned as the client resizes.
    anchor: Option<(u32, Rectangle<i32, Logical>)>,
    pub mapped: bool,
    border: [SolidColorBuffer; 4],
    titlebar: Option<(TitleKey, MemoryRenderBuffer)>,
}

impl Window {
    pub fn surface(&self) -> &WlSurface {
        &self.surface
    }

    pub fn x11(&self) -> Option<&X11Surface> {
        match &self.kind {
            WinKind::X11(x) => Some(x),
            WinKind::Wayland(_) => None,
        }
    }

    fn attrs<T>(&self, f: impl FnOnce(&smithay::wayland::shell::xdg::XdgToplevelSurfaceRoleAttributes) -> T) -> T {
        with_states(self.surface(), |s| f(&s.data_map.get::<XdgToplevelSurfaceData>().unwrap().lock().unwrap()))
    }

    pub fn title(&self) -> String {
        let t = match &self.kind {
            WinKind::Wayland(_) => self.attrs(|a| a.title.clone().or_else(|| a.app_id.clone())),
            WinKind::X11(x) => Some(x.title()).filter(|t| !t.is_empty()).or_else(|| Some(x.class())),
        };
        t.filter(|t| !t.is_empty()).unwrap_or_else(|| "Untitled".into())
    }

    pub fn app_id(&self) -> String {
        match &self.kind {
            WinKind::Wayland(_) => self.attrs(|a| a.app_id.clone().unwrap_or_default()),
            WinKind::X11(x) => x.class(),
        }
    }

    fn declared_geometry(&self) -> Option<Rectangle<i32, Logical>> {
        match &self.kind {
            WinKind::Wayland(_) => with_states(self.surface(), |s| s.cached_state.get::<SurfaceCachedState>().current().geometry),
            // X11 windows have no shadow trickery: the whole surface is the window.
            WinKind::X11(x) => Some(Rectangle::from_size(x.geometry().size)),
        }
    }

    fn size_limits(&self) -> (Size<i32, Logical>, Size<i32, Logical>) {
        match &self.kind {
            WinKind::Wayland(_) => with_states(self.surface(), |s| {
                let mut c = s.cached_state.get::<SurfaceCachedState>();
                let cur = c.current();
                (cur.min_size, cur.max_size)
            }),
            WinKind::X11(x) => (x.min_size().unwrap_or_default(), x.max_size().unwrap_or_default()),
        }
    }

    pub fn close(&self) {
        match &self.kind {
            WinKind::Wayland(t) => t.send_close(),
            WinKind::X11(x) => {
                let _ = x.close();
            }
        }
    }

    /// Ask the client for a new size and/or states. X11 windows are also told
    /// their position, since X clients place their own menus by it.
    pub fn configure(&self, size: Option<Size<i32, Logical>>, flags: &[(Flag, bool)]) {
        match &self.kind {
            WinKind::Wayland(t) => {
                t.with_pending_state(|s| {
                    if let Some(size) = size {
                        s.size = Some(size);
                    }
                    for &(flag, on) in flags {
                        let st = match flag {
                            Flag::Activated => xdg_toplevel::State::Activated,
                            Flag::Maximized => xdg_toplevel::State::Maximized,
                            Flag::Fullscreen => xdg_toplevel::State::Fullscreen,
                            Flag::Resizing => xdg_toplevel::State::Resizing,
                        };
                        if on {
                            s.states.set(st);
                        } else {
                            s.states.unset(st);
                        }
                    }
                });
                if t.is_initial_configure_sent() {
                    t.send_pending_configure();
                }
            }
            WinKind::X11(x) => {
                for &(flag, on) in flags {
                    let _ = match flag {
                        Flag::Activated => x.set_activated(on),
                        Flag::Maximized => x.set_maximized(on),
                        Flag::Fullscreen => x.set_fullscreen(on),
                        Flag::Resizing => Ok(()),
                    };
                }
                let size = size.unwrap_or_else(|| x.geometry().size);
                let _ = x.configure(Rectangle::new(self.pos, size));
            }
        }
    }

    /// Geometry relative to the surface origin.
    fn local_geometry(&self) -> Rectangle<i32, Logical> {
        self.declared_geometry().unwrap_or_else(|| bbox_from_surface_tree(self.surface(), (0, 0)))
    }

    pub fn surface_loc(&self) -> Point<i32, Logical> {
        self.pos - self.local_geometry().loc
    }

    pub fn geometry(&self) -> Rectangle<i32, Logical> {
        Rectangle::new(self.pos, self.local_geometry().size)
    }

    pub fn decorated(&self) -> bool {
        self.ssd && self.fullscreen.is_none()
    }

    /// Geometry plus our titlebar, if we draw one.
    pub fn frame(&self) -> Rectangle<i32, Logical> {
        let mut g = self.geometry();
        if self.decorated() {
            g.loc.y -= TITLE_H;
            g.size.h += TITLE_H;
        }
        g
    }

    pub fn titlebar_rect(&self) -> Option<Rectangle<i32, Logical>> {
        self.decorated().then(|| {
            let g = self.geometry();
            Rectangle::new((g.loc.x, g.loc.y - TITLE_H).into(), (g.size.w, TITLE_H).into())
        })
    }

    pub fn resizable(&self) -> bool {
        self.maximized.is_none() && self.fullscreen.is_none()
    }

    /// The titlebar picture, repainted only when what it shows changed.
    pub fn titlebar(
        &mut self,
        font: Option<&Font>,
        icons: &mut Icons,
        apps: &[crate::apps::App],
        active: bool,
        hover: Option<TitleButton>,
    ) -> Option<(&MemoryRenderBuffer, Point<i32, Logical>)> {
        let rect = self.titlebar_rect()?;
        let key = TitleKey {
            title: self.title(),
            app_id: crate::apps::icon_for_app_id(apps, &self.app_id()),
            width: rect.size.w,
            active,
            maximized: self.maximized.is_some(),
            hover,
        };
        if self.titlebar.as_ref().is_none_or(|(k, _)| *k != key) {
            let buf = ui::draw_titlebar(font, icons, &key);
            self.titlebar = Some((key, buf));
        }
        self.titlebar.as_ref().map(|(_, b)| (b, rect.loc))
    }

    pub fn border(&mut self, color: Color32F) -> [BorderPiece<'_>; 4] {
        let g = self.frame();
        let b = BORDER_WIDTH;
        let (x, y, w, h) = (g.loc.x, g.loc.y, g.size.w, g.size.h);
        let rects = [
            ((x - b, y - b), (w + 2 * b, b)),
            ((x - b, y + h), (w + 2 * b, b)),
            ((x - b, y), (b, h)),
            ((x + w, y), (b, h)),
        ];
        let [t, bo, l, r] = &mut self.border;
        let bufs = [t, bo, l, r];
        let mut i = 0;
        bufs.map(|buf| {
            let (loc, size) = rects[i];
            i += 1;
            buf.update(size, color);
            (&*buf, Point::from(loc))
        })
    }

}

/// What the pointer is over.
pub enum Hit {
    /// A client surface (toplevel, subsurface or popup), with its global origin.
    Client(WlSurface, Point<f64, Logical>),
    Titlebar(WlSurface),
    Button(WlSurface, TitleButton),
    Edge(WlSurface, u32),
}

pub enum Grab {
    Move { surface: WlSurface, offset: Point<f64, Logical> },
    Resize { surface: WlSurface, edges: u32, start_pointer: Point<f64, Logical>, start: Rectangle<i32, Logical> },
}

impl Grab {
    pub fn surface(&self) -> &WlSurface {
        match self {
            Grab::Move { surface, .. } | Grab::Resize { surface, .. } => surface,
        }
    }
}

#[derive(Default)]
pub struct Wm {
    /// Bottom to top.
    pub windows: Vec<Window>,
    pub grab: Option<Grab>,
    next_id: u64,
    /// Current workspace (0-9).
    pub current_workspace: usize,
}

impl Wm {
    /// Manage a new window. `pos` is where the client asked to be, if anywhere.
    pub fn add(&mut self, kind: WinKind, surface: WlSurface, pos: Option<Point<i32, Logical>>, ssd: bool) {
        // Cascade new windows so they never perfectly overlap.
        let step = (self.next_id % 8) as i32;
        self.next_id += 1;
        let mut pos = pos.unwrap_or_else(|| Point::from((90 + step * 30, 50 + step * 30)));
        if ssd {
            pos.y = pos.y.max(TITLE_H);
        }
        self.windows.push(Window {
            id: self.next_id,
            kind,
            surface,
            pos,
            ssd,
            minimized: false,
            maximized: None,
            fullscreen: None,
            workspace: self.current_workspace,
            anchor: None,
            mapped: false,
            border: Default::default(),
            titlebar: None,
        });
    }

    pub fn remove(&mut self, surface: &WlSurface) {
        self.windows.retain(|w| w.surface() != surface);
        if self.grab.as_ref().is_some_and(|g| g.surface() == surface) {
            self.grab = None;
        }
    }

    pub fn find(&self, surface: &WlSurface) -> Option<&Window> {
        self.windows.iter().find(|w| w.surface() == surface)
    }

    pub fn find_mut(&mut self, surface: &WlSurface) -> Option<&mut Window> {
        self.windows.iter_mut().find(|w| w.surface() == surface)
    }

    /// Topmost visible window on the current workspace.
    pub fn top(&self) -> Option<&Window> {
        self.windows.iter().rev().find(|w| !w.minimized && w.mapped && w.workspace == self.current_workspace)
    }

    pub fn raise(&mut self, surface: &WlSurface) {
        if let Some(i) = self.windows.iter().position(|w| w.surface() == surface) {
            let w = self.windows.remove(i);
            self.windows.push(w);
        }
    }

    /// Bring the bottom-most visible window to the top. Alt+Tab.
    pub fn cycle(&mut self) -> Option<WlSurface> {
        let i = self.windows.iter().position(|w| !w.minimized && w.mapped && w.workspace == self.current_workspace)?;
        let w = self.windows.remove(i);
        let s = w.surface().clone();
        self.windows.push(w);
        Some(s)
    }

    /// Get all windows on the current workspace, bottom to top.
    pub fn windows_on_current(&self) -> impl Iterator<Item = &Window> {
        let ws = self.current_workspace;
        self.windows.iter().filter(move |w| w.workspace == ws)
    }

    /// Get all windows on the current workspace, top to bottom.
    pub fn windows_on_current_rev(&self) -> impl Iterator<Item = &Window> {
        let ws = self.current_workspace;
        self.windows.iter().rev().filter(move |w| w.workspace == ws)
    }

    /// Get mutable windows on the current workspace, top to bottom.
    pub fn windows_on_current_mut(&mut self) -> impl Iterator<Item = &mut Window> {
        let ws = self.current_workspace;
        self.windows.iter_mut().rev().filter(move |w| w.workspace == ws)
    }

    /// Switch to workspace `ws` (0-9). Returns the new workspace index.
    pub fn set_workspace(&mut self, ws: usize) -> usize {
        self.current_workspace = ws.min(WORKSPACE_COUNT - 1);
        self.current_workspace
    }

    /// Move to the next workspace (with wrap).
    pub fn next_workspace(&mut self) -> usize {
        self.current_workspace = (self.current_workspace + 1) % WORKSPACE_COUNT;
        self.current_workspace
    }

    /// Move to the previous workspace (with wrap).
    pub fn prev_workspace(&mut self) -> usize {
        self.current_workspace = (self.current_workspace + WORKSPACE_COUNT - 1) % WORKSPACE_COUNT;
        self.current_workspace
    }

    /// Move a window to a different workspace.
    pub fn move_window_to_workspace(&mut self, surface: &WlSurface, ws: usize) -> bool {
        let ws = ws.min(WORKSPACE_COUNT - 1);
        if let Some(w) = self.find_mut(surface) {
            w.workspace = ws;
            true
        } else {
            false
        }
    }

    /// Get workspace indicator data for the taskbar.
    pub fn workspace_indicator(&self) -> (usize, [bool; WORKSPACE_COUNT]) {
        let mut has_windows = [false; WORKSPACE_COUNT];
        for w in &self.windows {
            if w.mapped && !w.minimized {
                has_windows[w.workspace] = true;
            }
        }
        (self.current_workspace, has_windows)
    }

    /// Called on every commit of a toplevel's root surface.
    pub fn on_commit(&mut self, surface: &WlSurface) {
        let Some(w) = self.find_mut(surface) else { return };
        w.mapped = true;
        if let Some((edges, start)) = w.anchor {
            let size = w.geometry().size;
            if edges & edge::LEFT != 0 {
                w.pos.x = start.loc.x + start.size.w - size.w;
            }
            if edges & edge::TOP != 0 {
                w.pos.y = start.loc.y + start.size.h - size.h;
            }
        }
    }

    /// Topmost thing under `point`.
    pub fn hit(&self, point: Point<f64, Logical>) -> Option<Hit> {
        for w in self.windows.iter().rev().filter(|w| !w.minimized && w.mapped && w.workspace == self.current_workspace) {
            let sloc = w.surface_loc();
            for (popup, offset) in PopupManager::popups_for_surface(w.surface()) {
                let ploc = sloc + offset - popup.geometry().loc;
                if let Some((s, o)) = under_from_surface_tree(popup.wl_surface(), point, ploc, WindowSurfaceType::ALL) {
                    return Some(Hit::Client(s, o.to_f64()));
                }
            }
            if let Some((s, o)) = under_from_surface_tree(w.surface(), point, sloc, WindowSurfaceType::ALL) {
                return Some(Hit::Client(s, o.to_f64()));
            }
            let frame = w.frame();
            let s = w.surface().clone();
            if let Some(tb) = w.titlebar_rect() {
                if tb.to_f64().contains(point) {
                    if w.resizable() && point.y < (tb.loc.y + 4) as f64 {
                        return Some(Hit::Edge(s, edges_at(frame, point)));
                    }
                    return Some(match ui::title_button_at(tb.size.w, point.x as i32 - tb.loc.x) {
                        Some(b) => Hit::Button(s, b),
                        None => Hit::Titlebar(s),
                    });
                }
            }
            if w.resizable() {
                let mut outer = frame;
                outer.loc -= (RESIZE_MARGIN, RESIZE_MARGIN).into();
                outer.size += (RESIZE_MARGIN * 2, RESIZE_MARGIN * 2).into();
                if outer.to_f64().contains(point) && !frame.to_f64().contains(point) {
                    return Some(Hit::Edge(s, edges_at(frame, point)));
                }
            }
        }
        None
    }

    pub fn start_move(&mut self, surface: &WlSurface, pointer: Point<f64, Logical>) {
        if let Some(w) = self.find_mut(surface) {
            w.anchor = None;
            self.grab = Some(Grab::Move { surface: surface.clone(), offset: pointer - w.pos.to_f64() });
        }
    }

    pub fn start_resize(&mut self, surface: &WlSurface, edges: u32, pointer: Point<f64, Logical>) {
        if let Some(w) = self.find_mut(surface) {
            if !w.resizable() || edges == 0 {
                return;
            }
            let start = w.geometry();
            w.anchor = Some((edges, start));
            self.grab = Some(Grab::Resize { surface: surface.clone(), edges, start_pointer: pointer, start });
        }
    }

    /// Pointer moved during a grab. `top_limit` keeps titlebars below the screen top.
    pub fn update_grab(&mut self, pointer: Point<f64, Logical>, top_limit: i32) {
        let Some(grab) = &self.grab else { return };
        match grab {
            Grab::Move { surface, offset } => {
                let mut pos: Point<i32, Logical> = (pointer - *offset).to_i32_round();
                let surface = surface.clone();
                if let Some(w) = self.find_mut(&surface) {
                    let title = if w.decorated() { TITLE_H } else { 0 };
                    pos.y = pos.y.max(top_limit + title);
                    w.pos = pos;
                    if w.x11().is_some() {
                        w.configure(None, &[]);
                    }
                }
            }
            Grab::Resize { surface, edges, start_pointer, start } => {
                let (edges, start) = (*edges, *start);
                let delta = (pointer - *start_pointer).to_i32_round::<i32>();
                let surface = surface.clone();
                let Some(w) = self.find_mut(&surface) else { return };
                let mut size = start.size;
                if edges & edge::LEFT != 0 {
                    size.w -= delta.x;
                }
                if edges & edge::RIGHT != 0 {
                    size.w += delta.x;
                }
                if edges & edge::TOP != 0 {
                    size.h -= delta.y;
                }
                if edges & edge::BOTTOM != 0 {
                    size.h += delta.y;
                }
                let (min, max) = w.size_limits();
                size.w = size.w.max(min.w.max(MIN_W));
                size.h = size.h.max(min.h.max(MIN_H));
                if max.w > 0 {
                    size.w = size.w.min(max.w);
                }
                if max.h > 0 {
                    size.h = size.h.min(max.h);
                }
                w.configure(Some(size), &[(Flag::Resizing, true)]);
            }
        }
    }

    pub fn end_grab(&mut self) {
        if let Some(Grab::Resize { surface, .. }) = self.grab.take() {
            if let Some(w) = self.find(&surface) {
                w.configure(None, &[(Flag::Resizing, false)]);
            }
        }
    }

    pub fn maximize(&mut self, surface: &WlSurface, area: Rectangle<i32, Logical>) {
        let Some(w) = self.find_mut(surface) else { return };
        if w.maximized.is_some() || w.fullscreen.is_some() {
            return;
        }
        w.maximized = Some(restorable(w.geometry()));
        w.anchor = None;
        let (pos, size) = if w.ssd {
            (area.loc + Point::from((0, TITLE_H)), Size::from((area.size.w, area.size.h - TITLE_H)))
        } else {
            (area.loc, area.size)
        };
        w.pos = pos;
        w.configure(Some(size), &[(Flag::Maximized, true)]);
    }

    pub fn unmaximize(&mut self, surface: &WlSurface) {
        let Some(w) = self.find_mut(surface) else { return };
        let Some(r) = w.maximized.take() else { return };
        w.pos = r.loc;
        w.configure(Some(r.size), &[(Flag::Maximized, false)]);
    }

    pub fn toggle_maximize(&mut self, surface: &WlSurface, area: Rectangle<i32, Logical>) {
        if self.find(surface).is_some_and(|w| w.maximized.is_some()) {
            self.unmaximize(surface);
        } else {
            self.maximize(surface, area);
        }
    }

    pub fn fullscreen(&mut self, surface: &WlSurface, output: Rectangle<i32, Logical>) {
        let Some(w) = self.find_mut(surface) else { return };
        if w.fullscreen.is_some() {
            return;
        }
        w.fullscreen = Some(restorable(w.geometry()));
        w.pos = output.loc;
        w.configure(Some(output.size), &[(Flag::Fullscreen, true)]);
    }

    pub fn unfullscreen(&mut self, surface: &WlSurface) {
        let Some(w) = self.find_mut(surface) else { return };
        let Some(r) = w.fullscreen.take() else { return };
        let size = w.maximized.map(|_| None).unwrap_or(Some(r.size));
        if w.maximized.is_none() {
            w.pos = r.loc;
        }
        w.configure(size, &[(Flag::Fullscreen, false)]);
    }
}

/// A geometry worth restoring to: unmapped windows report 0×0.
fn restorable(g: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    if g.size.w < MIN_W || g.size.h < MIN_H {
        Rectangle::new(g.loc, (900, 600).into())
    } else {
        g
    }
}

/// Which edges of `frame` a point just outside (or on the top strip of) it is near.
fn edges_at(frame: Rectangle<i32, Logical>, p: Point<f64, Logical>) -> u32 {
    let (x0, y0) = (frame.loc.x as f64, frame.loc.y as f64);
    let (x1, y1) = (x0 + frame.size.w as f64, y0 + frame.size.h as f64);
    let c = CORNER as f64;
    let mut e = 0;
    if p.x < x0 + if p.y < y0 + c || p.y > y1 - c { c } else { 0.0 } {
        e |= edge::LEFT;
    }
    if p.x >= x1 - if p.y < y0 + c || p.y > y1 - c { c } else { 0.0 } {
        e |= edge::RIGHT;
    }
    if p.y < y0 + if p.x < x0 + c || p.x > x1 - c { c } else { 4.0 } {
        e |= edge::TOP;
    }
    if p.y >= y1 - if p.x < x0 + c || p.x > x1 - c { c } else { 0.0 } {
        e |= edge::BOTTOM;
    }
    e
}
