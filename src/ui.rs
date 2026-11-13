//! Our chrome, styled after Windows 10: bottom taskbar, start menu, light
//! titlebars with min/max/close. Layout and look live here; painting goes
//! through `canvas`. Every picture is keyed on what it shows and only
//! repainted when that key changes.

use fontdue::Font;
use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Rectangle},
};

use crate::{
    canvas::{ellipsize, hex, measure, Canvas, Rgba},
    icons::Icons,
};

const fn rgba(c: u32, a: u8) -> Rgba {
    let h = hex(c);
    Rgba(h.0, h.1, h.2, a)
}

// Palette (the shell's dark/light colours live in `theme`; titlebars are white in both, like Windows 10)
pub use crate::theme::ACCENT;
use crate::theme::{th, ON_ACCENT};
const TITLE_BG: Rgba = hex(0xffffff);
const TITLE_TEXT: Rgba = hex(0x000000);
const TITLE_TEXT_IDLE: Rgba = hex(0x999999);
const BTN_HOVER: Rgba = hex(0xe5e5e5);
const CLOSE_HOVER: Rgba = hex(0xe81123);

pub const TASKBAR_H: i32 = 40;
pub const TITLE_H: i32 = 30;
pub const TITLE_BTN_W: i32 = 46;
pub const START_W: i32 = 48;
const SEARCH_W: i32 = 300;
/// Right edge of the search box: where the Task View button sits.
pub const SEARCH_END: i32 = START_W + SEARCH_W;
pub const TASK_W: i32 = 48;
/// Where the first taskbar button starts (after Start, search and Task View).
pub const TASK_BUTTONS_X: i32 = SEARCH_END + TASK_W + 4;
const CLOCK_W: i32 = 88;
const NOTIFY_W: i32 = 40;
const TRAY_W: i32 = 34;
const SHOW_DESKTOP_W: i32 = 6;
pub const MENU_W: i32 = 680;
const MENU_ROW: i32 = 36;
const MENU_TOP: i32 = 8;
/// Start's app list column, between the rail and the tiles.
const LIST_W: i32 = 284;
const TILE: i32 = 100;
const RAIL_W: i32 = 48;

// ---------------------------------------------------------------- taskbar

#[derive(Clone, PartialEq, Debug)]
pub enum TaskbarHit {
    Start,
    Search,
    Volume,
    Brightness,
    Network,
    /// The clock: opens the Action Center too, for want of a calendar.
    Clock,
    /// The Action Center button right of the clock.
    ActionCenter,
    /// An app's tray icon, by its tray id.
    TrayApp(String),
    /// A taskbar button, by index into `TaskbarKey::items`.
    Button(usize),
    TaskView,
    ShowDesktop,
    Nothing,
}

#[derive(Clone, PartialEq)]
pub struct TaskItem {
    /// Windows behind this button, oldest first. None: a pinned app that isn't running.
    pub windows: Vec<WlSurface>,
    pub title: String,
    /// Icon name (an app_id we look up variations of).
    pub icon: String,
    /// Desktop-file id, when pinned.
    pub pinned: Option<String>,
    pub focused: bool,
}

#[derive(Clone, PartialEq)]
pub struct TaskbarKey {
    pub width: i32,
    pub items: Vec<TaskItem>,
    pub time: String,
    pub date: String,
    pub hover: Option<TaskbarHit>,
    pub start_open: bool,
    /// Volume percent and mute, for the tray icon.
    pub volume: (u32, bool),
    /// What the network icon shows: Wi-Fi bars, wired, off, airplane mode.
    pub net: crate::tray::NetIcon,
    pub flyout: Option<crate::tray::Flyout>,
    pub current_workspace: usize,
    pub taskview_open: bool,
    /// App tray icons: (tray id, something that changes when the icon does).
    pub tray_apps: Vec<(String, String)>,
    /// The query while the search pane is open.
    pub search_text: Option<String>,
    /// Action Center button: notifications waiting, and how many arrived unseen.
    pub notifications: (bool, usize),
    pub action_center_open: bool,
}

pub struct Taskbar {
    pub key: TaskbarKey,
    pub buffer: MemoryRenderBuffer,
    /// Hit rects relative to the taskbar's top-left.
    pub hits: Vec<(Rectangle<i32, Logical>, TaskbarHit)>,
}

pub fn draw_taskbar(
    font: Option<&Font>,
    icons: &mut Icons,
    logo: Option<&crate::icons::Icon>,
    tray_icons: &[Option<std::rc::Rc<crate::icons::Icon>>],
    key: TaskbarKey,
) -> Taskbar {
    let w = key.width;
    let mut c = Canvas::new(w, TASKBAR_H);
    let mut hits = Vec::new();
    let hovered = |h: &TaskbarHit| key.hover.as_ref() == Some(h);
    c.fill(th().taskbar);

    // Start button: our logo (a chevron if it somehow failed to load).
    let start_bg = if key.start_open { th().active } else if hovered(&TaskbarHit::Start) { th().hover } else { th().taskbar };
    c.rect(0, 0, START_W, TASKBAR_H, start_bg);
    match logo {
        Some(l) => c.icon(l, (START_W - l.w as i32) / 2, (TASKBAR_H - l.h as i32) / 2),
        None => {
            let mark = if key.start_open || hovered(&TaskbarHit::Start) { th().accent_light } else { th().text };
            c.line(16.0, 14.0, 24.0, 26.0, 2.5, mark);
            c.line(24.0, 26.0, 32.0, 14.0, 2.5, mark);
        }
    }
    hits.push((rect(0, 0, START_W, TASKBAR_H), TaskbarHit::Start));

    // Search box. While the search pane is open it turns white and shows the query.
    let sx = START_W;
    match &key.search_text {
        Some(q) => {
            c.rect(sx, 0, SEARCH_W, TASKBAR_H, hex(0xffffff));
            magnifier(&mut c, sx as f32 + 22.0, TASKBAR_H as f32 / 2.0, hex(0x000000), hex(0xffffff));
            if let Some(f) = font {
                let t = if q.is_empty() { String::new() } else { ellipsize(f, 13.0, q, SEARCH_W - 60) };
                let tw = if t.is_empty() { 0 } else { c.text(f, 13.0, sx + 42, 0, TASKBAR_H, hex(0x000000), &t) };
                c.rect(sx + 43 + tw, 11, 1, TASKBAR_H - 22, hex(0x000000));
                if q.is_empty() {
                    c.text(f, 13.0, sx + 48, 0, TASKBAR_H, hex(0x767676), "Type here to search");
                }
            }
        }
        None => {
            c.rect(sx, 0, SEARCH_W, TASKBAR_H, if hovered(&TaskbarHit::Search) { th().search_hover } else { th().search_bg });
            // White on a light taskbar needs an edge to read as a box.
            if crate::theme::light() {
                c.outline(sx, 0, SEARCH_W, TASKBAR_H, th().divider);
            }
            magnifier(&mut c, sx as f32 + 22.0, TASKBAR_H as f32 / 2.0, th().text, th().search_bg);
            if let Some(f) = font {
                c.text(f, 13.0, sx + 42, 0, TASKBAR_H, th().text_dim, "Type here to search");
            }
        }
    }
    hits.push((rect(sx, 0, SEARCH_W, TASKBAR_H), TaskbarHit::Search));

    // Task View: Windows 10 overlapping desktop frames with the current workspace badge.
    let tv_x = SEARCH_END;
    let tv_bg = if key.taskview_open { th().active } else if hovered(&TaskbarHit::TaskView) { th().hover } else { th().taskbar };
    c.rect(tv_x, 0, TASK_W, TASKBAR_H, tv_bg);
    let (cx, cy) = (tv_x + TASK_W / 2, TASKBAR_H / 2);
    crate::tray::glyph(
        &mut c,
        r##"<rect x="2.5" y="5.5" width="11.5" height="9" rx="0.6" fill="none" stroke="#fff" stroke-width="1.2"/><path d="M5.5 5.5V3.5h12v9h-2" fill="none" stroke="#fff" stroke-width="1.2" stroke-linejoin="round"/><path d="M5 17h10" stroke="#fff" stroke-width="1.2" stroke-linecap="round"/>"##.into(),
        cx as f32,
        cy as f32,
        20,
    );
    if let Some(f) = font {
        let n = (key.current_workspace + 1).to_string();
        let tw = measure(f, 10.0, &n);
        c.round_rect(cx + 5, cy + 3, tw + 6, 12, 3, ACCENT);
        c.text(f, 10.0, cx + 8, cy + 3, 12, ON_ACCENT, &n);
    }
    if key.taskview_open {
        c.rect(tv_x, TASKBAR_H - 2, TASK_W, 2, th().accent_light);
    }
    hits.push((rect(tv_x, 0, TASK_W, TASKBAR_H), TaskbarHit::TaskView));

    // Window buttons.
    let right_edge = w - CLOCK_W - NOTIFY_W - SHOW_DESKTOP_W;
    const TRAY_ITEMS: i32 = 3;
    let apps_x = right_edge - TRAY_W * TRAY_ITEMS - TRAY_W * key.tray_apps.len() as i32;
    let mut x = TASK_BUTTONS_X;
    for (i, item) in key.items.iter().enumerate() {
        if x + TASK_W > apps_x {
            break;
        }
        let hit = TaskbarHit::Button(i);
        let bg = if item.focused { th().active } else if hovered(&hit) { th().hover } else { th().taskbar };
        c.rect(x, 0, TASK_W, TASKBAR_H, bg);
        match app_icon(icons, &item.icon, 24) {
            Some(icon) => c.icon(&icon, x + (TASK_W - 24) / 2, (TASKBAR_H - 24) / 2),
            None => {
                // No icon: first letter of the title in a tile.
                c.round_rect(x + 12, 8, 24, 24, 4, ACCENT);
                if let (Some(f), Some(ch)) = (font, item.title.chars().next()) {
                    let s = ch.to_uppercase().to_string();
                    let tw = measure(f, 14.0, &s);
                    c.text(f, 14.0, x + 24 - tw / 2, 8, 24, ON_ACCENT, &s);
                }
            }
        }
        // Open-window underline; full width when focused, none for a pinned app that isn't running.
        if item.focused {
            c.rect(x, TASKBAR_H - 2, TASK_W, 2, th().accent_light);
        } else if !item.windows.is_empty() {
            c.rect(x + 6, TASKBAR_H - 2, TASK_W - 12, 2, th().accent_light);
        }
        // Several windows: a second, offset line, like Windows' stacked-button edge.
        if item.windows.len() > 1 {
            c.rect(x + TASK_W - 3, 4, 1, TASKBAR_H - 8, th().divider);
        }
        hits.push((rect(x, 0, TASK_W, TASKBAR_H), hit));
        x += TASK_W;
    }

    // App tray icons (Discord, Steam, ...), left of our own.
    for (i, (id, _)) in key.tray_apps.iter().enumerate() {
        let bx = apps_x + i as i32 * TRAY_W;
        let hit = TaskbarHit::TrayApp(id.clone());
        if hovered(&hit) {
            c.rect(bx, 0, TRAY_W, TASKBAR_H, th().hover);
        }
        match tray_icons.get(i).and_then(|o| o.as_ref()) {
            Some(icon) => c.icon(icon, bx + (TRAY_W - icon.w as i32) / 2, (TASKBAR_H - icon.h as i32) / 2),
            None => c.round_rect(bx + 9, 12, 16, 16, 3, ACCENT),
        }
        hits.push((rect(bx, 0, TRAY_W, TASKBAR_H), hit));
    }
    let tray_x = apps_x + TRAY_W * key.tray_apps.len() as i32;

    // Tray: network, brightness, volume, just left of the clock. Bluetooth lives in
    // the Action Center's quick actions, as in Windows 10.
    use crate::tray::{net_icon, speaker, sun, Flyout};
    for (i, (hit, open)) in [
        (TaskbarHit::Network, key.flyout == Some(Flyout::Network)),
        (TaskbarHit::Brightness, key.flyout == Some(Flyout::Brightness)),
        (TaskbarHit::Volume, key.flyout == Some(Flyout::Volume)),
    ]
    .into_iter()
    .enumerate()
    {
        let bx = tray_x + i as i32 * TRAY_W;
        if open {
            c.rect(bx, 0, TRAY_W, TASKBAR_H, th().active);
        } else if hovered(&hit) {
            c.rect(bx, 0, TRAY_W, TASKBAR_H, th().hover);
        }
        let (cx, cy) = (bx as f32 + TRAY_W as f32 / 2.0, TASKBAR_H as f32 / 2.0);
        match hit {
            TaskbarHit::Volume => speaker(&mut c, cx - 5.0, cy, key.volume.1, key.volume.0 as f32 / 100.0, th().text),
            TaskbarHit::Brightness => sun(&mut c, cx, cy, th().text),
            _ => net_icon(&mut c, cx, cy, key.net),
        }
        hits.push((rect(bx, 0, TRAY_W, TASKBAR_H), hit));
    }

    // Clock: clicking it opens the Action Center.
    if hovered(&TaskbarHit::Clock) {
        c.rect(right_edge, 0, CLOCK_W, TASKBAR_H, th().hover);
    }
    if let Some(f) = font {
        let cx = right_edge + CLOCK_W / 2;
        let tw = measure(f, 12.0, &key.time);
        c.text(f, 12.0, cx - tw / 2, 3, TASKBAR_H / 2, th().text, &key.time);
        let dw = measure(f, 12.0, &key.date);
        c.text(f, 12.0, cx - dw / 2, TASKBAR_H / 2 - 3, TASKBAR_H / 2, th().text, &key.date);
    }
    hits.push((rect(right_edge, 0, CLOCK_W, TASKBAR_H), TaskbarHit::Clock));

    // Action Center button: a speech bubble, lined when there are notifications, with a
    // count badge for ones that came in unseen.
    let nx = right_edge + CLOCK_W;
    if key.action_center_open {
        c.rect(nx, 0, NOTIFY_W, TASKBAR_H, th().active);
    } else if hovered(&TaskbarHit::ActionCenter) {
        c.rect(nx, 0, NOTIFY_W, TASKBAR_H, th().hover);
    }
    let (any, unseen) = key.notifications;
    let lines = if any { r##"<path d="M6 7.5h8M6 10.5h8" stroke="#fff" stroke-width="1.1"/>"## } else { "" };
    crate::tray::glyph(
        &mut c,
        format!(r##"<path d="M2.5 3.5h15v10h-7l-4 3.5v-3.5h-4z" fill="none" stroke="#fff" stroke-width="1.15" stroke-linejoin="round"/>{lines}"##),
        (nx + NOTIFY_W / 2) as f32,
        (TASKBAR_H / 2) as f32,
        20,
    );
    if unseen > 0 {
        if let Some(f) = font {
            let n = if unseen > 9 { "9+".to_string() } else { unseen.to_string() };
            let tw = measure(f, 10.0, &n).max(8);
            let (bx, by) = (nx + NOTIFY_W / 2 + 2, 7);
            c.round_rect(bx, by, tw + 8, 15, 7, th().text);
            c.text(f, 10.0, bx + 4 + (tw - measure(f, 10.0, &n)) / 2, by, 15, th().taskbar, &n);
        }
    }
    hits.push((rect(nx, 0, NOTIFY_W, TASKBAR_H), TaskbarHit::ActionCenter));

    // Show-desktop sliver at the far right.
    let dx = w - SHOW_DESKTOP_W;
    c.rect(dx, 0, 1, TASKBAR_H, th().divider);
    if hovered(&TaskbarHit::ShowDesktop) {
        c.rect(dx + 1, 0, SHOW_DESKTOP_W - 1, TASKBAR_H, th().hover);
    }
    hits.push((rect(dx, 0, SHOW_DESKTOP_W, TASKBAR_H), TaskbarHit::ShowDesktop));

    Taskbar { key, buffer: c.into_buffer(), hits }
}

/// Look an app's icon up by its app_id, trying the usual name variations.
pub fn app_icon(icons: &mut Icons, app_id: &str, size: u32) -> Option<std::rc::Rc<crate::icons::Icon>> {
    let last = app_id.rsplit('.').next().unwrap_or(app_id);
    [app_id.to_string(), app_id.to_lowercase(), last.to_lowercase()]
        .iter()
        .find_map(|n| icons.get(n, size))
}

fn magnifier(c: &mut Canvas, x: f32, y: f32, fg: Rgba, _bg: Rgba) {
    let col = format!("#{:02x}{:02x}{:02x}", fg.0, fg.1, fg.2);
    let svg = format!(
        r##"<circle cx="8.5" cy="8.5" r="5.2" fill="none" stroke="{col}" stroke-width="1.35"/><path d="M12.3 12.3l4.7 4.7" stroke="{col}" stroke-width="1.45" stroke-linecap="round"/>"##
    );
    crate::tray::glyph(c, svg, x, y, 18);
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
    Rectangle::new((x, y).into(), (w, h).into())
}

// ---------------------------------------------------------------- titlebar

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum TitleButton {
    Minimize,
    Maximize,
    Close,
}

#[derive(Clone, PartialEq)]
pub struct TitleKey {
    pub title: String,
    pub app_id: String,
    pub width: i32,
    pub active: bool,
    pub maximized: bool,
    pub hover: Option<TitleButton>,
}

/// Which titlebar button is at `x` (relative to the titlebar's left), if any.
pub fn title_button_at(width: i32, x: i32) -> Option<TitleButton> {
    let from_right = width - x;
    match from_right {
        r if r <= 0 => None,
        r if r <= TITLE_BTN_W => Some(TitleButton::Close),
        r if r <= TITLE_BTN_W * 2 => Some(TitleButton::Maximize),
        r if r <= TITLE_BTN_W * 3 => Some(TitleButton::Minimize),
        _ => None,
    }
}

pub fn draw_titlebar(font: Option<&Font>, icons: &mut Icons, key: &TitleKey) -> MemoryRenderBuffer {
    let w = key.width;
    let mut c = Canvas::new(w, TITLE_H);
    c.fill(TITLE_BG);

    let mut tx = 10;
    if let Some(icon) = app_icon(icons, &key.app_id, 16) {
        c.icon(&icon, 8, (TITLE_H - 16) / 2);
        tx = 32;
    }
    if let Some(f) = font {
        let t = ellipsize(f, 12.0, &key.title, w - tx - TITLE_BTN_W * 3 - 8);
        c.text(f, 12.0, tx, 0, TITLE_H, if key.active { TITLE_TEXT } else { TITLE_TEXT_IDLE }, &t);
    }

    let glyph = if key.active { TITLE_TEXT } else { TITLE_TEXT_IDLE };
    for (i, btn) in [TitleButton::Minimize, TitleButton::Maximize, TitleButton::Close].into_iter().enumerate() {
        let bx = w - TITLE_BTN_W * (3 - i as i32);
        let hover = key.hover == Some(btn);
        let mut fg = glyph;
        if hover {
            let bg = if btn == TitleButton::Close { CLOSE_HOVER } else { BTN_HOVER };
            c.rect(bx, 0, TITLE_BTN_W, TITLE_H, bg);
            if btn == TitleButton::Close {
                fg = hex(0xffffff);
            }
        }
        let (cx, cy) = (bx + TITLE_BTN_W / 2, TITLE_H / 2);
        match btn {
            TitleButton::Minimize => c.rect(cx - 5, cy, 10, 1, fg),
            TitleButton::Maximize if key.maximized => {
                // "Restore": two overlapping squares.
                c.outline(cx - 5, cy - 3, 8, 8, fg);
                c.rect(cx - 3, cy - 5, 8, 1, fg);
                c.rect(cx + 4, cy - 5, 1, 8, fg);
            }
            TitleButton::Maximize => c.outline(cx - 5, cy - 5, 10, 10, fg),
            TitleButton::Close => {
                let (x, y) = (cx as f32, cy as f32);
                c.line(x - 5.0, y - 5.0, x + 5.0, y + 5.0, 1.1, fg);
                c.line(x + 5.0, y - 5.0, x - 5.0, y + 5.0, 1.1, fg);
            }
        }
    }
    c.into_opaque_buffer()
}

// ---------------------------------------------------------------- start menu

#[derive(Clone, PartialEq, Debug)]
pub enum StartHit {
    /// Index into the filtered list.
    Item(usize),
    /// A search pane tab (index into search::FILTERS).
    Filter(usize),
    /// An action in the search pane's preview, by id (see State::start_preview).
    Action(u8),
    /// A Start tile (index into StartKey::tiles).
    Tile(usize),
    /// The left rail: hamburger, account, Documents, Pictures, Settings.
    Rail(RailItem),
    Power,
    PowerAction(PowerAction),
    Nothing,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum RailItem {
    Expand,
    Account,
    Documents,
    Pictures,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PowerAction {
    SignOut,
    Restart,
    ShutDown,
}

impl PowerAction {
    const ALL: [PowerAction; 3] = [PowerAction::SignOut, PowerAction::Restart, PowerAction::ShutDown];

    fn label(self) -> &'static str {
        match self {
            PowerAction::SignOut => "Sign out",
            PowerAction::Restart => "Restart",
            PowerAction::ShutDown => "Shut down",
        }
    }
}

#[derive(Clone, PartialEq)]
pub enum StartRow {
    Header(String),
    /// (label, icon name, dim detail text on the right)
    Item(String, String, String),
}

#[derive(Clone, PartialEq)]
pub struct StartKey {
    pub height: i32,
    pub query: String,
    pub items: Vec<StartRow>,
    pub selected: usize,
    pub scroll: usize,
    pub hover: Option<StartHit>,
    /// The power button's Sign out / Restart / Shut down list is showing.
    pub power_open: bool,
    /// Windows' search pane instead of the app list.
    pub searching: bool,
    pub filter: usize,
    /// Per item, what kind of thing it is ("App"...), for the search pane.
    pub kinds: Vec<String>,
    pub preview: Option<Preview>,
    /// Tiles: (name, icon, group: 0 Productivity, 1 Explore).
    pub tiles: Vec<(String, String, u8)>,
    pub user: String,
    pub rail_open: bool,
}

/// The search pane's right side: the highlighted result, big, and its actions.
#[derive(Clone, PartialEq, Debug)]
pub struct Preview {
    pub name: String,
    pub icon: String,
    pub kind: String,
    pub location: String,
    /// (action id, label, icon name)
    pub actions: Vec<(u8, String, String)>,
}

/// The search pane, wider than Start since it has a preview column.
pub const SEARCH_PANEL_W: i32 = 760;
const RESULTS_W: i32 = 360;

pub struct StartMenu {
    pub key: StartKey,
    pub buffer: MemoryRenderBuffer,
    pub hits: Vec<(Rectangle<i32, Logical>, StartHit)>,
}

pub fn start_menu_rows(height: i32) -> usize {
    ((height - MENU_TOP - 8) / MENU_ROW).max(1) as usize
}

pub fn draw_start_menu(font: Option<&Font>, icons: &mut Icons, key: StartKey) -> StartMenu {
    if key.searching {
        return draw_search(font, icons, key);
    }
    let h = key.height;
    let mut c = Canvas::new(MENU_W, h);
    let mut hits = vec![(rect(0, 0, MENU_W, h), StartHit::Nothing)];
    c.rect(0, 0, MENU_W, h, th().start);
    let Some(f) = font else { return StartMenu { key, buffer: c.into_buffer(), hits } };

    // App list: Most used, then A-Z under letter headers.
    let lx = RAIL_W;
    let rows = start_menu_rows(h);
    for (row, (i, item)) in key.items.iter().enumerate().skip(key.scroll).take(rows).enumerate() {
        let y = MENU_TOP + row as i32 * MENU_ROW;
        let (name, icon) = match item {
            StartRow::Header(t) => {
                // Single letters sit on the left like Windows' jump headers; others are titles.
                let size = if t.chars().count() == 1 { 15.0 } else { 13.0 };
                c.text(f, size, lx + 14, y + 4, MENU_ROW - 4, th().text, t);
                continue;
            }
            StartRow::Item(n, i, _) => (n, i),
        };
        let hit = StartHit::Item(i);
        if key.hover.as_ref() == Some(&hit) {
            c.rect(lx, y, LIST_W, MENU_ROW, th().hover);
        }
        match icons.get(icon, 24).or_else(|| app_icon(icons, icon, 24)) {
            Some(ic) => c.icon(&ic, lx + 12, y + (MENU_ROW - 24) / 2),
            None => c.rect(lx + 12, y + 6, 24, 24, ACCENT),
        }
        c.text(f, 13.0, lx + 48, y, MENU_ROW, th().text, &ellipsize(f, 13.0, name, LIST_W - 60));
        hits.push((rect(lx, y, LIST_W, MENU_ROW), hit));
    }

    // Tiles: group titles, then medium tiles three to a row.
    let tx0 = RAIL_W + LIST_W + 12;
    let mut y = 8;
    let mut group = None;
    let mut col = 0;
    for (i, (name, icon, g)) in key.tiles.iter().enumerate() {
        if group != Some(*g) {
            if group.is_some() {
                y += TILE + 12;
            }
            group = Some(*g);
            col = 0;
            c.text(f, 13.0, tx0, y, 32, th().text, if *g == 0 { "Productivity" } else { "Explore" });
            y += 36;
        } else if col == 3 {
            col = 0;
            y += TILE + 4;
        }
        if y + TILE > h - 8 {
            break;
        }
        let x = tx0 + col * (TILE + 4);
        let hit = StartHit::Tile(i);
        let hover = key.hover.as_ref() == Some(&hit);
        c.rect(x, y, TILE, TILE, if hover { hex(0x1a86dd) } else { ACCENT });
        if hover {
            c.outline(x, y, TILE, TILE, rgba(0xffffff, 0x99));
            c.outline(x + 1, y + 1, TILE - 2, TILE - 2, rgba(0xffffff, 0x99));
        }
        match icons.get(icon, 40).or_else(|| app_icon(icons, icon, 40)) {
            Some(ic) => c.icon(&ic, x + (TILE - ic.w as i32) / 2, y + 22),
            None => c.rect(x + 30, y + 22, 40, 40, hex(0x005a9e)),
        }
        c.text(f, 12.0, x + 8, y + TILE - 26, 20, ON_ACCENT, &ellipsize(f, 12.0, name, TILE - 14));
        hits.push((rect(x, y, TILE, TILE), hit));
        col += 1;
    }

    // Left rail, drawn last so its expanded form covers the list.
    let rail_w = if key.rail_open { 256 } else { RAIL_W };
    c.rect(0, 0, rail_w, h, th().start_rail);
    if key.rail_open {
        c.rect(rail_w, 0, 1, h, th().divider);
    }
    let items: [(Option<RailItem>, i32, String); 6] = [
        (Some(RailItem::Expand), 6, "START".into()),
        (Some(RailItem::Account), h - 206, key.user.clone()),
        (Some(RailItem::Documents), h - 166, "Documents".into()),
        (Some(RailItem::Pictures), h - 126, "Pictures".into()),
        (Some(RailItem::Settings), h - 86, "Settings".into()),
        (None, h - 46, "Power".into()),
    ];
    for (item, y, label) in items {
        let hit = item.map(StartHit::Rail).unwrap_or(StartHit::Power);
        let active = item.is_none() && key.power_open;
        if active {
            c.rect(0, y, rail_w, 40, th().active);
        } else if key.hover.as_ref() == Some(&hit) {
            c.rect(0, y, rail_w, 40, th().hover);
        }
        let (gx, gy) = (RAIL_W as f32 / 2.0, y as f32 + 20.0);
        let body = match item {
            Some(RailItem::Expand) => r##"<path d="M3 5.5h14M3 10h14M3 14.5h14" stroke="#fff" stroke-width="1.25" stroke-linecap="round"/>"##,
            Some(RailItem::Account) => r##"<circle cx="10" cy="10" r="7.8" fill="none" stroke="#fff" stroke-width="1.2"/><circle cx="10" cy="7.8" r="2.6" fill="none" stroke="#fff" stroke-width="1.2"/><path d="M5 15.3c1.2-2.2 2.9-3.3 5-3.3s3.8 1.1 5 3.3" fill="none" stroke="#fff" stroke-width="1.2" stroke-linecap="round"/>"##,
            Some(RailItem::Documents) => r##"<path d="M4.5 2.5h7l4 4v11h-11zM11.5 2.5v4h4M7 9.5h6M7 12.5h6M7 15.5h4" fill="none" stroke="#fff" stroke-width="1.15" stroke-linejoin="round" stroke-linecap="round"/>"##,
            Some(RailItem::Pictures) => r##"<rect x="2.5" y="4" width="15" height="12" rx="0.6" fill="none" stroke="#fff" stroke-width="1.2"/><path d="M2.8 13.5l3.8-3.8 3.2 3.2 2.4-2.4 5 5" fill="none" stroke="#fff" stroke-width="1.2" stroke-linejoin="round"/><circle cx="13.2" cy="7.4" r="1.2" fill="#fff"/>"##,
            Some(RailItem::Settings) => r##"<circle cx="10" cy="10" r="2.7" fill="none" stroke="#fff" stroke-width="1.2"/><path d="M8.7 2.5h2.6l.4 1.8a5.7 5.7 0 0 1 1.4.6l1.5-1 1.8 1.8-1 1.5c.25.45.45.92.6 1.4l1.8.4v2.6l-1.8.4a5.7 5.7 0 0 1-.6 1.4l1 1.5-1.8 1.8-1.5-1a5.7 5.7 0 0 1-1.4.6l-.4 1.8H8.7l-.4-1.8a5.7 5.7 0 0 1-1.4-.6l-1.5 1-1.8-1.8 1-1.5a5.7 5.7 0 0 1-.6-1.4l-1.8-.4V8.7l1.8-.4c.15-.48.35-.95.6-1.4l-1-1.5 1.8-1.8 1.5 1c.45-.25.92-.45 1.4-.6z" fill="none" stroke="#fff" stroke-width="1.15" stroke-linejoin="round"/>"##,
            None => r##"<path d="M6.2 5.2a6.4 6.4 0 1 0 7.6 0" fill="none" stroke="#fff" stroke-width="1.3" stroke-linecap="round"/><path d="M10 2.5v6.8" stroke="#fff" stroke-width="1.3" stroke-linecap="round"/>"##,
        };
        crate::tray::glyph(&mut c, body.into(), gx, gy, 18);
        if key.rail_open {
            let size = if item == Some(RailItem::Expand) { 12.0 } else { 13.0 };
            c.text(f, size, RAIL_W + 4, y, 40, th().text, &ellipsize(f, size, &label, rail_w - RAIL_W - 16));
        }
        hits.push((rect(0, y, rail_w, 40), hit));
    }
    if key.rail_open {
        // Clicks beside the expanded rail just fold it away.
        hits.push((rect(rail_w, 0, MENU_W - rail_w, h), StartHit::Rail(RailItem::Expand)));
    }

    if key.power_open {
        let (lw, row) = (170, 36);
        let ly = h - 46 - row * PowerAction::ALL.len() as i32 - 4;
        c.rect(RAIL_W - 4, ly - 4, lw + 8, row * PowerAction::ALL.len() as i32 + 8, th().menu);
        c.outline(RAIL_W - 4, ly - 4, lw + 8, row * PowerAction::ALL.len() as i32 + 8, th().border);
        for (i, a) in PowerAction::ALL.into_iter().enumerate() {
            let y = ly + i as i32 * row;
            let hit = StartHit::PowerAction(a);
            if key.hover.as_ref() == Some(&hit) {
                c.rect(RAIL_W, y, lw, row, th().hover);
            }
            c.text(f, 13.0, RAIL_W + 14, y, row, th().text, a.label());
            hits.push((rect(RAIL_W, y, lw, row), hit));
        }
    }

    // Later entries win in hit tests, so reverse: specific rects before the background.
    hits.reverse();
    StartMenu { key, buffer: c.into_buffer(), hits }
}

// ---------------------------------------------------------------- search pane

/// Windows 10's search pane: tabs across the top, results on the left with the top
/// one under "Best match", and the highlighted result previewed on the right.
fn draw_search(font: Option<&Font>, icons: &mut Icons, key: StartKey) -> StartMenu {
    let (w, h) = (SEARCH_PANEL_W, key.height);
    let mut c = Canvas::new(w, h);
    let mut hits = vec![(rect(0, 0, w, h), StartHit::Nothing)];
    c.rect(0, 0, w, h, th().start_rail);
    let Some(f) = font else { return StartMenu { key, buffer: c.into_buffer(), hits } };

    // Tabs.
    let mut x = 20;
    for (i, label) in crate::search::FILTERS.iter().enumerate() {
        let tw = measure(f, 13.0, label);
        let hit = StartHit::Filter(i);
        let active = key.filter == i;
        let col = if active || key.hover.as_ref() == Some(&hit) { th().text } else { th().text_dim };
        c.text(f, 13.0, x, 8, 34, col, label);
        if active {
            c.rect(x, 42, tw, 3, ACCENT);
        }
        hits.push((rect(x - 8, 4, tw + 16, 42), hit));
        x += tw + 28;
    }
    c.rect(0, 47, w, 1, th().divider);

    // Empty query: "Top apps", as tiles.
    if key.query.trim().is_empty() {
        c.text(f, 15.0, 24, 64, 26, th().text, "Top apps");
        let (tw, tile_h) = (166, 104);
        for (i, item) in key.items.iter().enumerate() {
            let StartRow::Item(name, icon, _) = item else { continue };
            let (tx, ty) = (24 + (i as i32 % 4) * (tw + 8), 100 + (i as i32 / 4) * (tile_h + 8));
            if ty + tile_h > h {
                break;
            }
            let hit = StartHit::Item(i);
            c.rect(tx, ty, tw, tile_h, if key.hover.as_ref() == Some(&hit) { th().card_hover } else { th().card });
            match icons.get(icon, 32).or_else(|| app_icon(icons, icon, 32)) {
                Some(ic) => c.icon(&ic, tx + (tw - 32) / 2, ty + 20),
                None => c.round_rect(tx + (tw - 32) / 2, ty + 20, 32, 32, 4, ACCENT),
            }
            let n = ellipsize(f, 12.0, name, tw - 16);
            c.text(f, 12.0, tx + (tw - measure(f, 12.0, &n)) / 2, ty + 64, 24, th().text, &n);
            hits.push((rect(tx, ty, tw, tile_h), hit));
        }
        if key.items.is_empty() {
            c.text(f, 13.0, 24, 100, 24, th().text_dim, "Apps you open will show up here.");
        }
        hits.reverse();
        return StartMenu { key, buffer: c.into_buffer(), hits };
    }

    // Results.
    c.rect(0, 48, RESULTS_W, h - 48, th().start);
    let mut y = 56;
    let mut best = false;
    for (i, item) in key.items.iter().enumerate().skip(key.scroll) {
        if y >= h {
            break;
        }
        match item {
            StartRow::Header(t) => {
                best = t == "Best match";
                c.text(f, 14.0, 20, y + 4, 26, th().text, t);
                y += 32;
            }
            StartRow::Item(name, icon, detail) => {
                let rh = if best { 72 } else { 40 };
                let hit = StartHit::Item(i);
                if i == key.selected {
                    c.rect(0, y, RESULTS_W, rh, th().active);
                } else if key.hover.as_ref() == Some(&hit) {
                    c.rect(0, y, RESULTS_W, rh, th().hover);
                }
                let size = if best { 40 } else { 24 };
                let ix = 20;
                match icons.get(icon, size as u32).or_else(|| app_icon(icons, icon, size as u32)) {
                    Some(ic) => c.icon(&ic, ix, y + (rh - size) / 2),
                    None => c.round_rect(ix, y + (rh - size) / 2, size, size, 4, ACCENT),
                }
                let tx = ix + size + 14;
                let tw = RESULTS_W - tx - 12;
                let sub = if best { key.kinds.get(i).cloned().unwrap_or_default() } else { detail.clone() };
                if sub.is_empty() {
                    c.text(f, if best { 15.0 } else { 13.0 }, tx, y, rh, th().text, &ellipsize(f, 13.0, name, tw));
                } else {
                    let top = if best { y + 14 } else { y + 2 };
                    c.text(f, if best { 15.0 } else { 13.0 }, tx, top, 20, th().text, &ellipsize(f, 14.0, name, tw));
                    c.text(f, 12.0, tx, top + 20, 18, th().text_dim, &ellipsize(f, 12.0, &sub, tw));
                }
                hits.push((rect(0, y, RESULTS_W, rh), hit));
                best = false;
                y += rh;
            }
        }
    }
    if key.items.is_empty() {
        c.text(f, 13.0, 20, 60, 24, th().text_dim, "No results found");
    }

    // Preview.
    let px = RESULTS_W;
    let pw = w - px;
    c.rect(px, 48, pw, h - 48, th().start);
    c.rect(px, 48, 1, h - 48, th().divider);
    if let Some(p) = &key.preview {
        let cx = px + pw / 2;
        match icons.get(&p.icon, 64).or_else(|| app_icon(icons, &p.icon, 64)) {
            Some(ic) => c.icon(&ic, cx - 32, 76),
            None => c.round_rect(cx - 32, 76, 64, 64, 8, ACCENT),
        }
        let name = ellipsize(f, 20.0, &p.name, pw - 48);
        c.text(f, 20.0, cx - measure(f, 20.0, &name) / 2, 152, 30, th().text, &name);
        c.text(f, 13.0, cx - measure(f, 13.0, &p.kind) / 2, 184, 22, th().text_dim, &p.kind);
        let mut ay = 214;
        if !p.location.is_empty() {
            let l = ellipsize(f, 12.0, &p.location, pw - 48);
            c.text(f, 12.0, cx - measure(f, 12.0, &l) / 2, 206, 20, th().text_dim, &l);
            ay += 18;
        }
        c.rect(px + 24, ay, pw - 48, 1, th().divider);
        ay += 10;
        for (id, label, icon) in &p.actions {
            let hit = StartHit::Action(*id);
            if key.hover.as_ref() == Some(&hit) {
                c.rect(px + 16, ay, pw - 32, 36, th().hover);
            }
            action_glyph(&mut c, icon, (px + 40) as f32, (ay + 18) as f32);
            c.text(f, 13.0, px + 60, ay, 36, th().text, label);
            hits.push((rect(px + 16, ay, pw - 32, 36), hit));
            ay += 36;
        }
    }

    hits.reverse();
    StartMenu { key, buffer: c.into_buffer(), hits }
}

/// White line icons for the preview's actions, like Windows' MDL2 glyphs.
fn action_glyph(c: &mut Canvas, icon: &str, x: f32, y: f32) {
    const S: &str = r##"fill="none" stroke="#fff" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round""##;
    let body = match icon {
        "document-open" => format!(r##"<path d="M8 4H3.5v12.5H16V12M11 3.5h5.5V9M16.5 3.5L9 11" {S}/>"##),
        "folder-open" => format!(r##"<path d="M2.5 5.5h5l1.5 1.5h8.5v9h-15z" {S}/>"##),
        "window-pin" => format!(r##"<path d="M12 2.5l5.5 5.5-2.5 1-3 3 .5 3.5-2 1-7.5-7.5 1-2 3.5.5 3-3zM7 13l-4.5 4.5" {S}/>"##),
        "edit-copy" => format!(r##"<path d="M6.5 6.5h10v10h-10zM3.5 13.5v-10h10" {S}/>"##),
        _ => format!(r##"<circle cx="10" cy="10" r="7" {S}/><path d="M3 10h14M10 3c2.5 2 2.5 12 0 14M10 3c-2.5 2-2.5 12 0 14" {S}/>"##),
    };
    crate::tray::glyph(c, body, x, y, 16);
}

// ---------------------------------------------------------------- wallpaper

pub fn draw_wallpaper(w: i32, h: i32) -> MemoryRenderBuffer {
    let mut c = Canvas::new(w, h);
    c.wallpaper(hex(0x001a3d), hex(0x0b4b8f), rgba(0x49b4ff, 0x90), w as f32 * 0.68, h as f32 * 0.42, w as f32 * 0.55);
    c.into_opaque_buffer()
}
