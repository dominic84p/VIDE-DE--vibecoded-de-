//! Notifications: Action Center panel + real `org.freedesktop.Notifications` service.
//!
//! We run the D-Bus notification service on its own thread so client notifications
//! don't block compositor frames. The Action Center panel opens from the taskbar clock.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc, Arc,
    },
};

use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    reexports::calloop::channel::Sender as LoopSender,
    utils::{Logical, Rectangle},
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

const ACTION_CENTER_W: i32 = 420;
const ACTION_CENTER_H: i32 = 520;
const HEADER_H: i32 = 44;
const ACCENT: Rgba = hex(0x76b9ed);
const BG: Rgba = hex(0x1f1f1f);
const BORDER: Rgba = hex(0x3c3c3c);
const CARD: Rgba = hex(0x2b2b2b);
const CARD_HOVER: Rgba = hex(0x353535);
const WHITE: Rgba = hex(0xffffff);
const DIM: Rgba = hex(0xa0a0a0);
const RED: Rgba = hex(0xe81123);

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
}

#[derive(Clone, PartialEq, Debug, Default)]
pub enum ActionCenterHit {
    #[default]
    Nothing,
    ClearAll,
    Card(u32),
    Close(u32),
    Action(u32, String),
}

#[derive(Clone, PartialEq)]
pub struct ActionCenterKey {
    pub notifications: Vec<Notification>,
    pub hover: ActionCenterHit,
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
        _hints: HashMap<String, OwnedValue>,
        _expire_timeout: i32,
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
            summary,
            body,
            actions: action_pairs,
            timestamp: chrono::Local::now().format("%H:%M").to_string(),
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
    std::thread::Builder::new()
        .name("notifications".into())
        .spawn(move || {
            if let Err(e) = run(events, rx) {
                tracing::warn!("notifications: disabled ({e})");
            }
        })
        .ok();
    tx
}

fn run(events: LoopSender<NotifEvent>, cmds: mpsc::Receiver<NotifCmd>) -> zbus::Result<()> {
    let next_id = Arc::new(AtomicU32::new(1));
    let server = Server { events, next_id };
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
    pub fn action_center_rect(&self) -> Rectangle<i32, Logical> {
        let x = self.output_size.w - ACTION_CENTER_W - 8;
        Rectangle::new(
            (x, self.output_size.h - TASKBAR_H - ACTION_CENTER_H - 8).into(),
            (ACTION_CENTER_W, ACTION_CENTER_H).into(),
        )
    }

    pub fn toggle_action_center(&mut self) {
        self.action_center.open = !self.action_center.open;
        if self.action_center.open {
            self.action_center.hover = ActionCenterHit::Nothing;
        }
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
                if let Some(pos) = self.action_center.notifications.iter().position(|n| n.id == notif.id) {
                    self.action_center.notifications[pos] = notif;
                } else {
                    self.action_center.notifications.insert(0, notif);
                    if self.action_center.notifications.len() > 50 {
                        self.action_center.notifications.pop();
                    }
                }
                self.action_center.panel = None;
                self.needs_redraw = true;
            }
            NotifEvent::Closed(id) => {
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
            ActionCenterHit::Nothing => {}
        }
    }

    pub fn action_center_picture(&mut self) -> Option<MemoryRenderBuffer> {
        if !self.action_center.open {
            return None;
        }
        let key = ActionCenterKey {
            notifications: self.action_center.notifications.clone(),
            hover: self.action_center.hover.clone(),
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
    let mut c = Canvas::new(ACTION_CENTER_W, ACTION_CENTER_H);
    c.fill(BG);
    c.outline(0, 0, ACTION_CENTER_W, ACTION_CENTER_H, BORDER);

    let mut hits = vec![(rect(0, 0, ACTION_CENTER_W, ACTION_CENTER_H), ActionCenterHit::Nothing)];

    // Header.
    if let Some(f) = font {
        c.text(f, 14.0, 16, 12, 24, WHITE, "Notifications");
        c.rect(8, HEADER_H - 1, ACTION_CENTER_W - 16, 1, BORDER);

        if !key.notifications.is_empty() {
            let clear_text = "Clear all";
            let cw = measure(f, 12.0, clear_text);
            let cx = ACTION_CENTER_W - 16 - cw;
            let clear_hover = key.hover == ActionCenterHit::ClearAll;
            let col = if clear_hover { WHITE } else { DIM };
            c.text(f, 12.0, cx, 14, 20, col, clear_text);
            hits.push((rect(cx - 8, 8, cw + 16, 28), ActionCenterHit::ClearAll));
        }
    }

    if key.notifications.is_empty() {
        if let Some(f) = font {
            c.text(f, 13.0, 16, HEADER_H + 24, 24, DIM, "No new notifications");
        }
        return ActionCenterPanel { key, buffer: c.into_buffer(), hits };
    }

    // Cards list.
    let card_x = 12;
    let card_w = ACTION_CENTER_W - 24;
    let mut y = HEADER_H + 10;

    for n in &key.notifications {
        let has_actions = !n.actions.is_empty();
        let card_h = if has_actions { 96 } else { 72 };
        if y + card_h > ACTION_CENTER_H - 8 {
            break;
        }

        let is_card_hover = key.hover == ActionCenterHit::Card(n.id);
        let bg = if is_card_hover { CARD_HOVER } else { CARD };
        c.round_rect(card_x, y, card_w, card_h, 4, bg);
        c.outline(card_x, y, card_w, card_h, BORDER);

        // Entire card is a hit target.
        hits.push((rect(card_x, y, card_w, card_h), ActionCenterHit::Card(n.id)));

        if let Some(f) = font {
            // App name.
            c.text(f, 11.0, card_x + 12, y + 8, 16, ACCENT, &n.app_name);

            // Timestamp.
            let tw = measure(f, 11.0, &n.timestamp);
            let close_btn_x = card_x + card_w - 24;
            c.text(f, 11.0, close_btn_x - tw - 8, y + 8, 16, DIM, &n.timestamp);

            // Close button (x).
            let close_hover = key.hover == ActionCenterHit::Close(n.id);
            if close_hover {
                c.round_rect(close_btn_x, y + 6, 18, 18, 2, RED);
            }
            c.text(f, 12.0, close_btn_x + 5, y + 6, 18, WHITE, "×");
            hits.push((rect(close_btn_x - 2, y + 4, 22, 22), ActionCenterHit::Close(n.id)));

            // Summary.
            let summary_display = if n.summary.len() > 40 {
                format!("{}...", &n.summary[..37])
            } else {
                n.summary.clone()
            };
            c.text(f, 13.0, card_x + 12, y + 26, 18, WHITE, &summary_display);

            // Body.
            let body_first_line = n.body.lines().next().unwrap_or("");
            let body_display = if body_first_line.len() > 50 {
                format!("{}...", &body_first_line[..47])
            } else {
                body_first_line.to_string()
            };
            c.text(f, 12.0, card_x + 12, y + 46, 18, DIM, &body_display);

            // Actions buttons.
            if has_actions {
                let mut ax = card_x + 12;
                for (action_key, label) in n.actions.iter().take(3) {
                    let lw = measure(f, 11.0, label);
                    let bw = (lw + 16).max(40);
                    if ax + bw > card_x + card_w - 12 {
                        break;
                    }
                    let act_hover = key.hover == ActionCenterHit::Action(n.id, action_key.clone());
                    let btn_bg = if act_hover { CARD_HOVER } else { CARD };
                    c.round_rect(ax, y + 68, bw, 20, 2, btn_bg);
                    c.outline(ax, y + 68, bw, 20, BORDER);
                    c.text(f, 11.0, ax + 8, y + 70, 16, WHITE, label);
                    hits.push((
                        rect(ax, y + 68, bw, 20),
                        ActionCenterHit::Action(n.id, action_key.clone()),
                    ));
                    ax += bw + 8;
                }
            }
        }

        y += card_h + 8;
    }

    ActionCenterPanel { key, buffer: c.into_buffer(), hits }
}

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

