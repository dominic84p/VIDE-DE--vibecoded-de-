//! Notifications: Action Center panel + real `org.freedesktop.Notifications` service.
//!
//! We run the D-Bus notification service on its own thread so client notifications
//! don't block compositor frames. The Action Center panel opens from the taskbar clock:
//! Windows 10's full-height panel with notifications on top and the quick action tiles
//! (Network, All settings, Bluetooth, Night light...) at the bottom.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc, Arc,
    },
};

use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    reexports::calloop::channel::Sender as LoopSender,
    utils::{Logical, Point, Rectangle},
};
use zbus::{
    blocking::connection,
    fdo::{RequestNameFlags, RequestNameReply},
    object_server::SignalEmitter,
    zvariant::OwnedValue,
};

use crate::{
    canvas::{hex, measure, Canvas, Rgba},
    state::State,
    ui::TASKBAR_H,
};

const ACTION_CENTER_W: i32 = 396;
const HEADER_H: i32 = 44;
const ACCENT: Rgba = hex(0x0078d7);
const ACCENT_HOVER: Rgba = hex(0x1a86dd);
const BG: Rgba = hex(0x1f1f1f);
const BORDER: Rgba = hex(0x3c3c3c);
const CARD: Rgba = hex(0x2b2b2b);
const CARD_HOVER: Rgba = hex(0x353535);
const BUTTON: Rgba = hex(0x4c4c4c);
const TILE: Rgba = hex(0x3b3b3b);
const TILE_HOVER: Rgba = hex(0x4a4a4a);
const LINK: Rgba = hex(0x8ec5f5);
const WHITE: Rgba = hex(0xffffff);
const DIM: Rgba = hex(0xa0a0a0);
const BODY: Rgba = hex(0xc8c8c8);
const TILE_W: i32 = (ACTION_CENTER_W - 24 - 12) / 4;
const TILE_H: i32 = 58;

/// Windows 10's quick action tiles, in its default order. The first row shows
/// collapsed; Expand shows the rest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuickAction {
    Network,
    AllSettings,
    Bluetooth,
    NightLight,
    FocusAssist,
    Airplane,
    ScreenSnip,
    Hotspot,
}

const QUICK_ACTIONS: [(QuickAction, &str); 8] = [
    (QuickAction::Network, "Network"),
    (QuickAction::AllSettings, "All settings"),
    (QuickAction::Bluetooth, "Bluetooth"),
    (QuickAction::NightLight, "Night light"),
    (QuickAction::FocusAssist, "Focus assist"),
    (QuickAction::Airplane, "Airplane mode"),
    (QuickAction::ScreenSnip, "Screen snip"),
    (QuickAction::Hotspot, "Mobile hotspot"),
];

fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
    Rectangle::new((x, y).into(), (w, h).into())
}

#[derive(Clone, PartialEq, Debug)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub app_icon: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub timestamp: String,
    /// freedesktop urgency hint: 0 low, 1 normal, 2 critical.
    pub urgency: u8,
    /// `expire_timeout` as sent: -1 server's choice, 0 never, else milliseconds.
    pub timeout_ms: i32,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub enum ActionCenterHit {
    #[default]
    Nothing,
    ClearAll,
    Card(u32),
    Close(u32),
    Action(u32, String),
    Tile(QuickAction),
    Expand,
    Manage,
}

#[derive(Clone, PartialEq)]
pub struct ActionCenterKey {
    pub notifications: Vec<Notification>,
    pub hover: ActionCenterHit,
    pub height: i32,
    pub expanded: bool,
    /// Which quick action tiles are lit, in QUICK_ACTIONS order.
    pub on: [bool; 8],
}

pub struct ActionCenterPanel {
    pub key: ActionCenterKey,
    pub buffer: MemoryRenderBuffer,
    pub hits: Vec<(Rectangle<i32, Logical>, ActionCenterHit)>,
}

#[derive(Default)]
pub struct ActionCenter {
    pub open: bool,
    pub notifications: Vec<Notification>,
    pub panel: Option<ActionCenterPanel>,
    pub hover: ActionCenterHit,
    /// All the quick actions showing, not just the first row.
    pub expanded: bool,
    pub focus_assist: bool,
}

fn strip_markup(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    let mut tag_buf = String::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if in_tag {
            if c == '>' {
                in_tag = false;
                let t = tag_buf.trim().to_ascii_lowercase();
                if t == "br" || t == "br/" || t == "br /" {
                    out.push('\n');
                }
                tag_buf.clear();
            } else {
                tag_buf.push(c);
            }
            continue;
        }
        if c == '<' {
            in_tag = true;
            tag_buf.clear();
        } else if c == '&' {
            let mut ent = String::new();
            let mut closed = false;
            while let Some(&nc) = chars.peek() {
                if nc == ';' {
                    chars.next();
                    closed = true;
                    break;
                }
                if ent.len() >= 8 || nc.is_whitespace() || nc == '&' || nc == '<' {
                    break;
                }
                ent.push(chars.next().unwrap());
            }
            if closed {
                match ent.as_str() {
                    "amp" => out.push('&'),
                    "lt" => out.push('<'),
                    "gt" => out.push('>'),
                    "quot" => out.push('"'),
                    "apos" | "#39" => out.push('\''),
                    _ => {
                        out.push('&');
                        out.push_str(&ent);
                        out.push(';');
                    }
                }
            } else {
                out.push('&');
                out.push_str(&ent);
            }
        } else {
            out.push(c);
        }
    }
    if in_tag {
        out.push('<');
        out.push_str(&tag_buf);
    }
    out
}

#[derive(Clone, Debug)]
pub enum NotifEvent {
    Added(Notification),
    Closed(u32),
}

#[derive(Clone, Debug)]
pub enum NotifCmd {
    Close(u32),
    InvokeAction(u32, String),
}

pub type NotifSender = mpsc::Sender<NotifCmd>;

struct Server {
    events: LoopSender<NotifEvent>,
    next_id: Arc<AtomicU32>,
}

#[zbus::interface(name = "org.freedesktop.Notifications")]
impl Server {
    fn get_capabilities(&self) -> Vec<&'static str> {
        vec!["actions", "body", "persistence"]
    }

    fn notify(
        &mut self,
        app_name: String,
        replaces_id: u32,
        app_icon: String,
        summary: String,
        body: String,
        actions: Vec<String>,
        hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
    ) -> u32 {
        let id = if replaces_id != 0 {
            replaces_id
        } else {
            let n = self.next_id.fetch_add(1, Ordering::Relaxed);
            if n == 0 {
                self.next_id.fetch_add(1, Ordering::Relaxed)
            } else {
                n
            }
        };

        let mut action_pairs = Vec::new();
        let mut it = actions.into_iter();
        while let Some(key) = it.next() {
            let label = it.next().unwrap_or_default();
            action_pairs.push((key, label));
        }

        let notif = Notification {
            id,
            app_name: if app_name.is_empty() { "Notification".to_string() } else { app_name },
            app_icon,
            summary: strip_markup(&summary),
            body: strip_markup(&body),
            actions: action_pairs,
            timestamp: chrono::Local::now().format("%H:%M").to_string(),
            urgency: hints.get("urgency").and_then(|v| v.downcast_ref::<u8>().ok()).unwrap_or(1),
            timeout_ms: expire_timeout,
        };

        let _ = self.events.send(NotifEvent::Added(notif));
        id
    }

    fn close_notification(
        &mut self,
        id: u32,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) {
        let _ = self.events.send(NotifEvent::Closed(id));
        let _ = zbus::block_on(Self::notification_closed(&emitter, id, 3));
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        ("vide-de", "vide", "0.1.0", "1.2")
    }

    #[zbus(signal)]
    async fn notification_closed(emitter: &SignalEmitter<'_>, id: u32, reason: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_invoked(emitter: &SignalEmitter<'_>, id: u32, action_key: &str) -> zbus::Result<()>;
}

pub fn start(events: LoopSender<NotifEvent>) -> NotifSender {
    let (tx, rx) = mpsc::channel();
    // The receiver outlives a crash, so queued commands survive the restart.
    crate::supervisor::spawn_service("notifications", move || {
        if let Err(e) = run(&events, &rx) {
            tracing::warn!("notifications: disabled ({e})");
        }
    });
    tx
}

fn run(events: &LoopSender<NotifEvent>, cmds: &mpsc::Receiver<NotifCmd>) -> zbus::Result<()> {
    let next_id = Arc::new(AtomicU32::new(1));
    let server = Server { events: events.clone(), next_id };
    let conn = connection::Builder::session()?
        .serve_at("/org/freedesktop/Notifications", server)?
        .build()?;

    let reply = conn.request_name_with_flags(
        "org.freedesktop.Notifications",
        RequestNameFlags::DoNotQueue.into(),
    )?;
    if reply != RequestNameReply::PrimaryOwner {
        return Err(zbus::Error::Failure(
            "another notification daemon already owns org.freedesktop.Notifications".into(),
        ));
    }
    tracing::info!("notifications: org.freedesktop.Notifications up");

    loop {
        match cmds.recv() {
            Ok(NotifCmd::Close(id)) => {
                if let Ok(iface) = conn.object_server().interface::<_, Server>("/org/freedesktop/Notifications") {
                    let _ = zbus::block_on(Server::notification_closed(iface.signal_emitter(), id, 2));
                }
            }
            Ok(NotifCmd::InvokeAction(id, action)) => {
                if let Ok(iface) = conn.object_server().interface::<_, Server>("/org/freedesktop/Notifications") {
                    let _ = zbus::block_on(Server::action_invoked(iface.signal_emitter(), id, &action));
                }
            }
            Err(_) => return Ok(()),
        }
    }
}

impl State {
    /// Docked to the right edge, from the top of the screen down to the taskbar.
    pub fn action_center_rect(&self) -> Rectangle<i32, Logical> {
        let h = self.output_size.h - TASKBAR_H;
        Rectangle::new((self.output_size.w - ACTION_CENTER_W, 0).into(), (ACTION_CENTER_W, h).into())
    }

    pub fn toggle_action_center(&mut self) {
        self.action_center.open = !self.action_center.open;
        if self.action_center.open {
            self.action_center.hover = ActionCenterHit::Nothing;
            self.toasts.list.clear();
            self.toasts.panel = None;
            self.close_flyout();
            // Bluetooth / airplane state for the tiles.
            self.refresh_network();
        }
        self.needs_redraw = true;
    }

    fn quick_action(&mut self, q: QuickAction) {
        let settings = |state: &mut State, page: &str| {
            if let Some(s) = crate::input::own_app("vide-settings") {
                state.spawn(&format!("{s} --page {page}"));
            }
            state.close_action_center();
        };
        match q {
            QuickAction::Network => {
                self.close_action_center();
                self.toggle_flyout(crate::tray::Flyout::Network);
            }
            QuickAction::AllSettings => settings(self, "personalization"),
            QuickAction::Bluetooth => {
                let on = !self.tray.net.bt_on;
                self.set_bluetooth(on);
            }
            QuickAction::NightLight => {
                self.config.night_light = !self.config.night_light;
                if let Err(e) = self.config.save() {
                    tracing::warn!("couldn't save night light: {e}");
                }
            }
            QuickAction::FocusAssist => self.action_center.focus_assist = !self.action_center.focus_assist,
            QuickAction::Airplane => {
                let on = !self.tray.net.airplane;
                self.set_airplane(on);
            }
            QuickAction::ScreenSnip => {
                self.close_action_center();
                self.start_screenshot(true);
            }
            QuickAction::Hotspot => {
                let on = !self.tray.net.hotspot;
                self.set_hotspot(on);
            }
        }
        self.action_center.panel = None;
        self.needs_redraw = true;
    }

    pub fn close_action_center(&mut self) {
        if self.action_center.open {
            self.action_center.open = false;
            self.action_center.hover = ActionCenterHit::Nothing;
            self.needs_redraw = true;
        }
    }

    pub fn handle_notif_event(&mut self, ev: NotifEvent) {
        match ev {
            NotifEvent::Added(notif) => {
                let notif_for_toast = notif.clone();
                if let Some(pos) = self.action_center.notifications.iter().position(|n| n.id == notif.id) {
                    self.action_center.notifications[pos] = notif;
                } else {
                    self.action_center.notifications.insert(0, notif);
                    if self.action_center.notifications.len() > 50 {
                        self.action_center.notifications.pop();
                    }
                }
                self.action_center.panel = None;
                self.push_toast(notif_for_toast);
                self.needs_redraw = true;
            }
            NotifEvent::Closed(id) => {
                self.toasts.list.retain(|t| t.notif.id != id);
                self.toasts.panel = None;
                self.action_center.notifications.retain(|n| n.id != id);
                self.action_center.panel = None;
                self.needs_redraw = true;
            }
        }
    }

    pub fn action_center_click(&mut self, hit: ActionCenterHit) {
        match hit {
            ActionCenterHit::ClearAll => {
                if let Some(tx) = &self.notif_tx {
                    for n in &self.action_center.notifications {
                        let _ = tx.send(NotifCmd::Close(n.id));
                    }
                }
                self.action_center.notifications.clear();
                self.action_center.panel = None;
                self.needs_redraw = true;
            }
            ActionCenterHit::Close(id) => {
                if let Some(tx) = &self.notif_tx {
                    let _ = tx.send(NotifCmd::Close(id));
                }
                self.action_center.notifications.retain(|n| n.id != id);
                self.action_center.panel = None;
                self.needs_redraw = true;
            }
            ActionCenterHit::Action(id, action) => {
                if let Some(tx) = &self.notif_tx {
                    let _ = tx.send(NotifCmd::InvokeAction(id, action));
                }
                self.action_center.notifications.retain(|n| n.id != id);
                self.action_center.panel = None;
                self.needs_redraw = true;
            }
            ActionCenterHit::Card(id) => {
                if let Some(n) = self.action_center.notifications.iter().find(|n| n.id == id) {
                    if n.actions.iter().any(|(k, _)| k == "default") {
                        if let Some(tx) = &self.notif_tx {
                            let _ = tx.send(NotifCmd::InvokeAction(id, "default".to_string()));
                        }
                        self.action_center.notifications.retain(|n| n.id != id);
                        self.action_center.panel = None;
                        self.needs_redraw = true;
                    }
                }
            }
            ActionCenterHit::Tile(q) => self.quick_action(q),
            ActionCenterHit::Expand => {
                self.action_center.expanded = !self.action_center.expanded;
                self.action_center.panel = None;
                self.needs_redraw = true;
            }
            ActionCenterHit::Manage => {
                if let Some(s) = crate::input::own_app("vide-settings") {
                    self.spawn(&s);
                }
                self.close_action_center();
            }
            ActionCenterHit::Nothing => {}
        }
    }

    pub fn action_center_picture(&mut self) -> Option<MemoryRenderBuffer> {
        if !self.action_center.open {
            return None;
        }
        let n = &self.tray.net;
        let on = QUICK_ACTIONS.map(|(q, _)| match q {
            QuickAction::Network => (n.wifi_on || n.wired.is_some()) && !n.airplane,
            QuickAction::Bluetooth => n.bt_on,
            QuickAction::NightLight => self.config.night_light,
            QuickAction::FocusAssist => self.action_center.focus_assist,
            QuickAction::Airplane => n.airplane,
            QuickAction::Hotspot => n.hotspot,
            QuickAction::AllSettings | QuickAction::ScreenSnip => false,
        });
        let key = ActionCenterKey {
            notifications: self.action_center.notifications.clone(),
            hover: self.action_center.hover.clone(),
            height: self.action_center_rect().size.h,
            expanded: self.action_center.expanded,
            on,
        };
        if self.action_center.panel.as_ref().is_none_or(|p| p.key != key) {
            let panel = draw_action_center(self.font.as_ref(), key);
            self.action_center.panel = Some(panel);
        }
        self.action_center.panel.as_ref().map(|p| p.buffer.clone())
    }
}

fn draw_action_center(
    font: Option<&fontdue::Font>,
    key: ActionCenterKey,
) -> ActionCenterPanel {
    let (w, h) = (ACTION_CENTER_W, key.height);
    let mut c = Canvas::new(w, h);
    c.fill(BG);
    c.rect(0, 0, 1, h, BORDER);
    let mut hits = vec![(rect(0, 0, w, h), ActionCenterHit::Nothing)];
    let hover = |x: &ActionCenterHit| key.hover == *x;
    let Some(f) = font else { return ActionCenterPanel { key, buffer: c.into_buffer(), hits } };

    // Bottom up: tiles, then the Expand / Clear all row above them.
    let rows = if key.expanded { 2 } else { 1 };
    let tiles_top = h - 12 - rows * TILE_H - (rows - 1) * 4;
    let links_y = tiles_top - 34;
    for (i, (q, label)) in QUICK_ACTIONS.iter().enumerate().take(rows as usize * 4) {
        let x = 12 + (i as i32 % 4) * (TILE_W + 4);
        let y = tiles_top + (i as i32 / 4) * (TILE_H + 4);
        let hit = ActionCenterHit::Tile(*q);
        let fill = match (key.on[i], hover(&hit)) {
            (true, true) => ACCENT_HOVER,
            (true, false) => ACCENT,
            (false, true) => TILE_HOVER,
            (false, false) => TILE,
        };
        c.rect(x, y, TILE_W, TILE_H, fill);
        let (gx, gy) = (x as f32 + 18.0, y as f32 + 17.0);
        match q {
            QuickAction::Network => crate::tray::wifi(&mut c, gx, gy, true),
            QuickAction::AllSettings => crate::tray::glyph(&mut c, GEAR.into(), gx, gy, 18),
            QuickAction::Bluetooth => crate::tray::bluetooth(&mut c, gx, gy),
            QuickAction::NightLight => crate::tray::sun(&mut c, gx, gy, WHITE),
            QuickAction::FocusAssist => crate::tray::glyph(&mut c, MOON.into(), gx, gy, 18),
            QuickAction::Airplane => crate::tray::airplane(&mut c, gx, gy),
            QuickAction::ScreenSnip => crate::tray::glyph(&mut c, SNIP.into(), gx, gy, 18),
            QuickAction::Hotspot => crate::tray::hotspot(&mut c, gx, gy),
        }
        c.text(f, 12.0, x + 8, y + 34, 18, WHITE, &crate::canvas::ellipsize(f, 12.0, label, TILE_W - 12));
        hits.push((rect(x, y, TILE_W, TILE_H), hit));
    }
    let expand = if key.expanded { "Collapse" } else { "Expand" };
    let ew = measure(f, 12.0, expand);
    c.text(f, 12.0, 14, links_y, 24, if hover(&ActionCenterHit::Expand) { WHITE } else { LINK }, expand);
    hits.push((rect(10, links_y, ew + 8, 24), ActionCenterHit::Expand));
    if !key.notifications.is_empty() {
        let clear = "Clear all notifications";
        let cw = measure(f, 12.0, clear);
        c.text(f, 12.0, w - 14 - cw, links_y, 24, if hover(&ActionCenterHit::ClearAll) { WHITE } else { LINK }, clear);
        hits.push((rect(w - 18 - cw, links_y, cw + 8, 24), ActionCenterHit::ClearAll));
    }

    // Top: "Manage notifications".
    let manage = "Manage notifications";
    let mw = measure(f, 12.0, manage);
    c.text(f, 12.0, w - 14 - mw, 10, 24, if hover(&ActionCenterHit::Manage) { WHITE } else { LINK }, manage);
    hits.push((rect(w - 18 - mw, 10, mw + 8, 24), ActionCenterHit::Manage));

    if key.notifications.is_empty() {
        let t = "No new notifications";
        c.text(f, 14.0, (w - measure(f, 14.0, t)) / 2, HEADER_H + 10, 24, BODY, t);
        hits.reverse();
        return ActionCenterPanel { key, buffer: c.into_buffer(), hits };
    }

    // Notifications, grouped under their app's name like Windows.
    let (card_x, card_w) = (12, w - 24);
    let mut y = HEADER_H;
    let mut last_app: Option<&str> = None;
    for n in &key.notifications {
        let has_actions = !n.actions.is_empty();
        let body_lines: Vec<&str> = n.body.lines().filter(|l| !l.trim().is_empty()).take(2).collect();
        let card_h = 36 + body_lines.len() as i32 * 18 + if has_actions { 40 } else { 0 } + 8;
        let header = last_app != Some(n.app_name.as_str());
        let need = card_h + if header { 30 } else { 0 };
        if y + need > links_y - 8 {
            break;
        }
        if header {
            c.text(f, 13.0, card_x + 2, y + 4, 22, WHITE, &crate::canvas::ellipsize(f, 13.0, &n.app_name, card_w - 8));
            y += 30;
            last_app = Some(&n.app_name);
        }
        let card_hit = ActionCenterHit::Card(n.id);
        let card_hover = hover(&card_hit) || matches!(&key.hover, ActionCenterHit::Close(id) | ActionCenterHit::Action(id, _) if *id == n.id);
        c.rect(card_x, y, card_w, card_h, if card_hover { CARD_HOVER } else { CARD });
        hits.push((rect(card_x, y, card_w, card_h), card_hit));

        c.text(f, 14.0, card_x + 12, y + 8, 22, WHITE, &crate::canvas::ellipsize(f, 14.0, &n.summary, card_w - 90));
        for (i, line) in body_lines.iter().enumerate() {
            c.text(f, 13.0, card_x + 12, y + 32 + i as i32 * 18, 18, BODY, &crate::canvas::ellipsize(f, 13.0, line, card_w - 24));
        }
        // Time, or the close X while hovered, top right.
        let cx = card_x + card_w - 28;
        if card_hover {
            let close_hover = hover(&ActionCenterHit::Close(n.id));
            if close_hover {
                c.rect(cx, y + 6, 22, 22, BUTTON);
            }
            let (x0, y0) = (cx as f32 + 7.0, y as f32 + 13.0);
            c.line(x0, y0, x0 + 8.0, y0 + 8.0, 1.2, WHITE);
            c.line(x0 + 8.0, y0, x0, y0 + 8.0, 1.2, WHITE);
            hits.push((rect(cx, y + 6, 22, 22), ActionCenterHit::Close(n.id)));
        } else {
            let tw = measure(f, 11.0, &n.timestamp);
            c.text(f, 11.0, card_x + card_w - 12 - tw, y + 8, 22, DIM, &n.timestamp);
        }
        if has_actions {
            let actions: Vec<_> = n.actions.iter().filter(|(k, _)| k != "default").take(3).collect();
            let count = actions.len().max(1) as i32;
            let bw = (card_w - 24 - (count - 1) * 4) / count;
            for (i, (action_key, label)) in actions.into_iter().enumerate() {
                let bx = card_x + 12 + i as i32 * (bw + 4);
                let by = y + card_h - 40;
                let hit = ActionCenterHit::Action(n.id, action_key.clone());
                c.rect(bx, by, bw, 32, if hover(&hit) { hex(0x5c5c5c) } else { BUTTON });
                let lw = measure(f, 12.0, label);
                c.text(f, 12.0, bx + (bw - lw).max(8) / 2, by, 32, WHITE, &crate::canvas::ellipsize(f, 12.0, label, bw - 12));
                hits.push((rect(bx, by, bw, 32), hit));
            }
        }
        y += card_h + 4;
    }

    // Later entries win in hit tests, so specific rects go before the background.
    hits.reverse();
    ActionCenterPanel { key, buffer: c.into_buffer(), hits }
}

// ------------------------------------------------------------------ toasts

const TOAST_W: i32 = 364;
const TOAST_MARGIN: i32 = 12;
const TOAST_GAP: i32 = 8;
const MAX_TOASTS: usize = 3;
const TOAST_BTN_H: i32 = 32;

pub struct Toast {
    pub notif: Notification,
    since: Instant,
    /// `None` stays until dismissed (critical, or the app asked for no timeout).
    ttl: Option<Duration>,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub enum ToastHit {
    #[default]
    Nothing,
    Body(u32),
    Close(u32),
    Action(u32, String),
}

impl ToastHit {
    fn id(&self) -> Option<u32> {
        match self {
            ToastHit::Nothing => None,
            ToastHit::Body(id) | ToastHit::Close(id) | ToastHit::Action(id, _) => Some(*id),
        }
    }
}

#[derive(Clone, PartialEq)]
struct ToastKey {
    items: Vec<Notification>,
    hover: ToastHit,
}

struct ToastPanel {
    key: ToastKey,
    buffer: MemoryRenderBuffer,
    hits: Vec<(Rectangle<i32, Logical>, ToastHit)>,
    size: (i32, i32),
}

/// Popups for new notifications, stacked bottom-right above the taskbar.
#[derive(Default)]
pub struct Toasts {
    /// Oldest first, so the newest sits lowest, next to the taskbar.
    pub list: Vec<Toast>,
    pub hover: ToastHit,
    panel: Option<ToastPanel>,
}

fn toast_card_h(n: &Notification) -> i32 {
    let lines = n.body.lines().filter(|l| !l.trim().is_empty()).take(2).count() as i32;
    let actions = n.actions.iter().any(|(k, _)| k != "default");
    // Header row, title, body; the button row (if any) sits flush at the bottom edge.
    52 + lines * 18 + if actions { 10 + TOAST_BTN_H } else { 12 }
}

impl State {
    /// A new notification: show it as a toast, unless Focus assist is on (it still lands
    /// in the Action Center, which is the point of Focus assist).
    fn push_toast(&mut self, notif: Notification) {
        if self.action_center.focus_assist || !self.config.toasts {
            return;
        }
        let ttl = match (notif.urgency, notif.timeout_ms) {
            (2, _) | (_, 0) => None,
            (_, ms) if ms > 0 => Some(Duration::from_millis(ms as u64).max(Duration::from_secs(2))),
            _ => Some(Duration::from_secs(self.config.toast_seconds as u64)),
        };
        let toast = Toast { notif, since: Instant::now(), ttl };
        // The same id replaces its toast in place (progress updates, chat messages).
        if let Some(t) = self.toasts.list.iter_mut().find(|t| t.notif.id == toast.notif.id) {
            *t = toast;
        } else {
            self.toasts.list.push(toast);
            if self.toasts.list.len() > MAX_TOASTS {
                self.toasts.list.remove(0);
            }
        }
        self.toasts.panel = None;
    }

    /// Called every second: dismiss toasts that ran out, except the one under the pointer.
    pub fn toast_tick(&mut self) {
        if self.toasts.list.is_empty() {
            return;
        }
        let now = Instant::now();
        let hovered = self.toasts.hover.id();
        for t in &mut self.toasts.list {
            if Some(t.notif.id) == hovered {
                t.since = now;
            }
        }
        let before = self.toasts.list.len();
        self.toasts.list.retain(|t| t.ttl.is_none_or(|d| now.duration_since(t.since) < d));
        if self.toasts.list.len() != before {
            self.toasts.hover = ToastHit::Nothing;
            self.toasts.panel = None;
            self.needs_redraw = true;
        }
    }

    /// Hidden while the Action Center shows the same notifications, and under a fullscreen
    /// app (the taskbar is hidden there too).
    fn toasts_visible(&self) -> bool {
        if self.toasts.list.is_empty() || self.action_center.open {
            return false;
        }
        let focused = self.focused();
        let fullscreen = self.wm.top().is_some_and(|w| w.fullscreen.is_some() && Some(w.surface()) == focused.as_ref());
        self.start.open || !fullscreen
    }

    fn toast_origin(&self, size: (i32, i32)) -> Point<i32, Logical> {
        (self.output_size.w - size.0 - TOAST_MARGIN, self.output_size.h - TASKBAR_H - TOAST_MARGIN - size.1).into()
    }

    pub fn toasts_picture(&mut self) -> Option<(MemoryRenderBuffer, Point<i32, Logical>)> {
        if !self.toasts_visible() {
            return None;
        }
        let key = ToastKey { items: self.toasts.list.iter().map(|t| t.notif.clone()).collect(), hover: self.toasts.hover.clone() };
        if self.toasts.panel.as_ref().is_none_or(|p| p.key != key) {
            self.toasts.panel = Some(draw_toasts(self.font.as_ref(), &mut self.icons, key));
        }
        let p = self.toasts.panel.as_ref()?;
        Some((p.buffer.clone(), self.toast_origin(p.size)))
    }

    /// What's under `pos` in the toast stack. The gaps between cards are see-through, so
    /// they report `None` and the click goes on to whatever is behind.
    pub fn toast_at(&self, pos: Point<f64, Logical>) -> Option<ToastHit> {
        if !self.toasts_visible() {
            return None;
        }
        let p = self.toasts.panel.as_ref()?;
        let local = (pos - self.toast_origin(p.size).to_f64()).to_i32_round();
        p.hits.iter().find(|(r, _)| r.contains(local)).map(|(_, h)| h.clone())
    }

    pub fn set_toast_hover(&mut self, hit: ToastHit) {
        if self.toasts.hover != hit {
            self.toasts.hover = hit;
            self.toasts.panel = None;
        }
    }

    pub fn toast_click(&mut self, hit: ToastHit) {
        let invoke = |state: &mut State, id: u32, action: &str| {
            if let Some(tx) = &state.notif_tx {
                let _ = tx.send(NotifCmd::InvokeAction(id, action.to_string()));
            }
            state.action_center.notifications.retain(|n| n.id != id);
            state.action_center.panel = None;
        };
        let id = match hit {
            ToastHit::Nothing => return,
            // The X only hides the popup; the notification stays in the Action Center.
            ToastHit::Close(id) => id,
            ToastHit::Action(id, action) => {
                invoke(self, id, &action);
                id
            }
            ToastHit::Body(id) => {
                if self.toasts.list.iter().any(|t| t.notif.id == id && t.notif.actions.iter().any(|(k, _)| k == "default")) {
                    invoke(self, id, "default");
                }
                id
            }
        };
        self.toasts.list.retain(|t| t.notif.id != id);
        self.toasts.hover = ToastHit::Nothing;
        self.toasts.panel = None;
        self.needs_redraw = true;
    }
}

fn draw_toasts(font: Option<&fontdue::Font>, icons: &mut crate::icons::Icons, key: ToastKey) -> ToastPanel {
    let h = key.items.iter().map(toast_card_h).sum::<i32>() + (key.items.len() as i32 - 1).max(0) * TOAST_GAP;
    let mut c = Canvas::new(TOAST_W, h);
    let mut hits = Vec::new();
    let Some(f) = font else { return ToastPanel { key, buffer: c.into_buffer(), hits, size: (TOAST_W, h) } };
    let hover = |x: &ToastHit| key.hover == *x;

    // Windows 10's look: a flat dark card, the app's small icon and name on top, the
    // title and text under it, and the X only while the pointer is over the toast.
    let mut y = 0;
    for n in &key.items {
        let ch = toast_card_h(n);
        let over = key.hover.id() == Some(n.id);
        c.rect(0, y, TOAST_W, ch, BG);
        c.outline(0, y, TOAST_W, ch, BORDER);
        hits.push((rect(0, y, TOAST_W, ch), ToastHit::Body(n.id)));

        let icon = [n.app_icon.as_str(), n.app_name.as_str()]
            .into_iter()
            .filter(|id| !id.is_empty())
            .find_map(|id| crate::ui::app_icon(icons, id, 16));
        let mut tx = 12;
        if let Some(icon) = &icon {
            c.icon(icon, 12, y + 10);
            tx = 12 + 16 + 8;
        }
        c.text(f, 12.0, tx, y + 8, 18, BODY, &crate::canvas::ellipsize(f, 12.0, &n.app_name, TOAST_W - tx - 44));
        c.text(f, 14.0, 12, y + 30, 22, WHITE, &crate::canvas::ellipsize(f, 14.0, &n.summary, TOAST_W - 24));
        let lines: Vec<&str> = n.body.lines().filter(|l| !l.trim().is_empty()).take(2).collect();
        for (i, line) in lines.iter().enumerate() {
            c.text(f, 13.0, 12, y + 52 + i as i32 * 18, 18, BODY, &crate::canvas::ellipsize(f, 13.0, line, TOAST_W - 24));
        }

        if over {
            let close = ToastHit::Close(n.id);
            let (cx, cy, cs) = (TOAST_W - 32, y + 1, 30);
            if hover(&close) {
                c.rect(cx, cy, cs, cs, BUTTON);
            }
            let (x0, y0) = (cx as f32 + 10.0, cy as f32 + 10.0);
            c.line(x0, y0, x0 + 10.0, y0 + 10.0, 1.2, WHITE);
            c.line(x0 + 10.0, y0, x0, y0 + 10.0, 1.2, WHITE);
            hits.push((rect(cx, cy, cs, cs), close));
        }

        // Full-width buttons with a thin gap between them, flush with the card's edges.
        let actions: Vec<_> = n.actions.iter().filter(|(k, _)| k != "default").take(3).collect();
        if !actions.is_empty() {
            let count = actions.len() as i32;
            let gap = 2;
            let bw = (TOAST_W - 2 - (count - 1) * gap) / count;
            let by = y + ch - TOAST_BTN_H - 1;
            for (i, (action_key, label)) in actions.into_iter().enumerate() {
                let bx = 1 + i as i32 * (bw + gap);
                let hit = ToastHit::Action(n.id, action_key.clone());
                c.rect(bx, by, bw, TOAST_BTN_H, if hover(&hit) { hex(0x5c5c5c) } else { TILE });
                let lw = measure(f, 12.0, label);
                c.text(f, 12.0, bx + (bw - lw).max(8) / 2, by, TOAST_BTN_H, WHITE, &crate::canvas::ellipsize(f, 12.0, label, bw - 12));
                hits.push((rect(bx, by, bw, TOAST_BTN_H), hit));
            }
        }
        y += ch + TOAST_GAP;
    }
    // Later entries win in hit tests, so the X and buttons go in front of the card.
    hits.reverse();
    ToastPanel { key, buffer: c.into_buffer(), hits, size: (TOAST_W, h) }
}

const GEAR: &str = r##"<circle cx="10" cy="10" r="2.6" fill="none" stroke="#fff" stroke-width="1.3"/><path d="M10 2.5v2.2M10 15.3v2.2M2.5 10h2.2M15.3 10h2.2M4.7 4.7l1.6 1.6M13.7 13.7l1.6 1.6M4.7 15.3l1.6-1.6M13.7 6.3l1.6-1.6" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/><circle cx="10" cy="10" r="5.4" fill="none" stroke="#fff" stroke-width="1.3"/>"##;
const MOON: &str = r##"<path d="M14.5 13.2A6.5 6.5 0 0 1 7.3 3.4a6.5 6.5 0 1 0 7.2 9.8z" fill="none" stroke="#fff" stroke-width="1.3" stroke-linejoin="round"/>"##;
const SNIP: &str = r##"<path d="M3 3h3M8.5 3h3M14 3h3v3M17 8.5v3M17 14v3h-3M11.5 17h-3M6 17H3v-3M3 11.5v-3M3 6V3" stroke="#fff" stroke-width="1.3" stroke-linecap="round"/><path d="M10 7v6M7 10h6" stroke="#fff" stroke-width="1.3" stroke-linecap="round"/>"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_replace_and_close() {
        let mut ac = ActionCenter::default();
        let notif1 = Notification {
            id: 1,
            app_name: "TestApp".into(),
            app_icon: "".into(),
            summary: "Hello".into(),
            body: "World".into(),
            actions: vec![("default".into(), "Open".into())],
            timestamp: "12:00".into(),
            urgency: 1,
            timeout_ms: -1,
        };

        // Add
        ac.notifications.push(notif1.clone());
        assert_eq!(ac.notifications.len(), 1);
        assert_eq!(ac.notifications[0].summary, "Hello");

        // Replace
        let mut notif2 = notif1.clone();
        notif2.summary = "Updated Hello".into();
        if let Some(pos) = ac.notifications.iter().position(|n| n.id == notif2.id) {
            ac.notifications[pos] = notif2;
        }
        assert_eq!(ac.notifications.len(), 1);
        assert_eq!(ac.notifications[0].summary, "Updated Hello");

        // Close
        ac.notifications.retain(|n| n.id != 1);
        assert!(ac.notifications.is_empty());
    }
}

