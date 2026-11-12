//! App tray icons (Discord, Steam, ...): the StatusNotifierItem protocol over D-Bus.
//!
//! We play both roles Plasma normally does: the *watcher* apps register their icon
//! with, and the *host* that reads those icons and forwards clicks and menu picks
//! (dbusmenu) back. All D-Bus work runs on its own thread so a slow app can't stall
//! the compositor; it talks to the main loop through channels.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

use smithay::reexports::calloop::channel::Sender as LoopSender;
use zbus::{
    blocking::{connection, Connection, Proxy},
    message::Header,
    object_server::SignalEmitter,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";
const MENU_IFACE: &str = "com.canonical.dbusmenu";

#[derive(Clone, PartialEq, Debug)]
pub struct TrayItem {
    /// `bus_name` + object path; stable while the app runs.
    pub id: String,
    pub title: String,
    pub icon_name: String,
    pub theme_path: String,
    /// Premultiplied RGBA, when the app sends pixels instead of an icon name.
    pub pixmap: Option<(u32, u32, Vec<u8>)>,
}

#[derive(Clone, Debug)]
pub struct MenuEntry {
    pub id: i32,
    pub label: String,
    pub enabled: bool,
    pub separator: bool,
    /// Submenu nesting depth, for indentation.
    pub depth: usize,
}

pub enum Cmd {
    Activate { id: String, x: i32, y: i32 },
    OpenMenu { id: String, x: i32, y: i32 },
    MenuClick { id: String, entry: i32 },
}

pub enum Event {
    Items(Vec<TrayItem>),
    Menu { id: String, entries: Vec<MenuEntry> },
}

/// (bus name, object path) of every registered item.
type Registry = Arc<Mutex<Vec<(String, String)>>>;

struct Watcher {
    items: Registry,
    dirty: Arc<AtomicBool>,
}

#[zbus::interface(name = "org.kde.StatusNotifierWatcher")]
impl Watcher {
    fn register_status_notifier_item(&mut self, service: &str, #[zbus(header)] header: Header<'_>) {
        // Apps pass either a bus name (icon at the default path) or just an object path.
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        let (bus, path) = if service.starts_with('/') {
            (sender, service.to_string())
        } else {
            (service.to_string(), "/StatusNotifierItem".to_string())
        };
        let mut items = self.items.lock().unwrap();
        if !items.iter().any(|(b, p)| *b == bus && *p == path) {
            tracing::info!("tray: {bus}{path} registered");
            items.push((bus, path));
        }
        self.dirty.store(true, Ordering::Relaxed);
    }

    fn register_status_notifier_host(&mut self, _service: &str) {}

    #[zbus(property)]
    fn registered_status_notifier_items(&self) -> Vec<String> {
        self.items.lock().unwrap().iter().map(|(b, p)| format!("{b}{p}")).collect()
    }

    #[zbus(property)]
    fn is_status_notifier_host_registered(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn protocol_version(&self) -> i32 {
        0
    }

    #[zbus(signal)]
    async fn status_notifier_host_registered(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

/// Start the tray thread. Returns where to send clicks; updates arrive on `events`.
pub fn start(events: LoopSender<Event>) -> mpsc::Sender<Cmd> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || {
            if let Err(e) = run(events, rx) {
                tracing::warn!("tray: disabled ({e})");
            }
        })
        .ok();
    tx
}

fn run(events: LoopSender<Event>, cmds: mpsc::Receiver<Cmd>) -> zbus::Result<()> {
    let items: Registry = Default::default();
    let dirty = Arc::new(AtomicBool::new(false));
    let conn = connection::Builder::session()?
        .serve_at("/StatusNotifierWatcher", Watcher { items: items.clone(), dirty: dirty.clone() })?
        .name("org.kde.StatusNotifierWatcher")?
        .build()?;
    tracing::info!("tray: StatusNotifierWatcher up");
    // Apps that started before us wait for this to (re)register.
    if let Ok(iface) = conn.object_server().interface::<_, Watcher>("/StatusNotifierWatcher") {
        let _ = zbus::block_on(Watcher::status_notifier_host_registered(iface.signal_emitter()));
    }

    let mut last: Vec<TrayItem> = Vec::new();
    let mut last_full = Instant::now();
    loop {
        match cmds.recv_timeout(Duration::from_millis(300)) {
            Ok(cmd) => handle(&conn, &events, cmd),
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        // Re-read icons promptly after a registration, and every couple of seconds
        // anyway (icons change, apps quit).
        if dirty.swap(false, Ordering::Relaxed) || last_full.elapsed() > Duration::from_secs(2) {
            last_full = Instant::now();
            let now = refresh(&conn, &items);
            if now != last {
                last = now.clone();
                if events.send(Event::Items(now)).is_err() {
                    return Ok(());
                }
            }
        }
    }
}

fn item_proxy<'a>(conn: &'a Connection, id: &str) -> zbus::Result<Proxy<'a>> {
    let (bus, path) = split_id(id);
    Proxy::new(conn, bus.to_string(), path.to_string(), ITEM_IFACE)
}

fn split_id(id: &str) -> (&str, &str) {
    let i = id.find('/').unwrap_or(id.len());
    (&id[..i], &id[i..])
}

/// Read every registered item; forget the ones whose app is gone.
fn refresh(conn: &Connection, items: &Registry) -> Vec<TrayItem> {
    let list = items.lock().unwrap().clone();
    let mut out = Vec::new();
    let mut dead = Vec::new();
    for (bus, path) in list {
        let id = format!("{bus}{path}");
        let Ok(p) = item_proxy(conn, &id) else { continue };
        // Status "Passive" means the app doesn't want to be shown right now.
        match p.get_property::<String>("Status") {
            Ok(s) if s == "Passive" => continue,
            Ok(_) => {}
            Err(_) => {
                dead.push((bus, path));
                continue;
            }
        }
        let icon_name = p.get_property::<String>("IconName").unwrap_or_default();
        let pixmap = if icon_name.is_empty() {
            p.get_property::<Vec<(i32, i32, Vec<u8>)>>("IconPixmap").ok().and_then(best_pixmap)
        } else {
            None
        };
        out.push(TrayItem {
            id,
            title: p.get_property::<String>("Title").unwrap_or_default(),
            icon_name,
            theme_path: p.get_property::<String>("IconThemePath").unwrap_or_default(),
            pixmap,
        });
    }
    if !dead.is_empty() {
        items.lock().unwrap().retain(|e| !dead.contains(e));
    }
    out
}

/// Pick the pixmap closest to 22px and convert ARGB32 (network order) to premultiplied RGBA.
fn best_pixmap(pixmaps: Vec<(i32, i32, Vec<u8>)>) -> Option<(u32, u32, Vec<u8>)> {
    let (w, h, data) = pixmaps
        .into_iter()
        .filter(|(w, h, d)| *w > 0 && *h > 0 && d.len() >= (*w * *h * 4) as usize)
        .min_by_key(|(w, _, _)| (w - 22).abs())?;
    let mut rgba = Vec::with_capacity(data.len());
    for px in data.chunks_exact(4).take((w * h) as usize) {
        let (a, r, g, b) = (px[0] as u16, px[1] as u16, px[2] as u16, px[3] as u16);
        // Premultiply (sources are straight alpha).
        rgba.extend_from_slice(&[(r * a / 255) as u8, (g * a / 255) as u8, (b * a / 255) as u8, a as u8]);
    }
    Some((w as u32, h as u32, rgba))
}

fn handle(conn: &Connection, events: &LoopSender<Event>, cmd: Cmd) {
    match cmd {
        Cmd::Activate { id, x, y } => {
            let Ok(p) = item_proxy(conn, &id) else { return };
            // Apps that are menu-only (or don't implement Activate) get their menu instead.
            let menu_only = p.get_property::<bool>("ItemIsMenu").unwrap_or(false);
            if menu_only || p.call_method("Activate", &(x, y)).is_err() {
                open_menu(conn, events, &id, x, y);
            }
        }
        Cmd::OpenMenu { id, x, y } => open_menu(conn, events, &id, x, y),
        Cmd::MenuClick { id, entry } => {
            let Some(menu) = menu_path(conn, &id) else { return };
            let (bus, _) = split_id(&id);
            if let Ok(m) = Proxy::new(conn, bus.to_string(), menu, MENU_IFACE) {
                let stamp = 0u32;
                let _ = m.call_method("Event", &(entry, "clicked", Value::I32(0), stamp));
            }
        }
    }
}

fn menu_path(conn: &Connection, id: &str) -> Option<String> {
    let p = item_proxy(conn, id).ok()?;
    let path: OwnedObjectPath = p.get_property("Menu").ok()?;
    let s = path.as_str().to_string();
    (s != "/").then_some(s)
}

/// Fetch the app's dbusmenu and send it to the main loop to draw; apps without one
/// get the plain ContextMenu call and draw their own.
fn open_menu(conn: &Connection, events: &LoopSender<Event>, id: &str, x: i32, y: i32) {
    let Some(menu) = menu_path(conn, id) else {
        if let Ok(p) = item_proxy(conn, id) {
            let _ = p.call_method("ContextMenu", &(x, y));
        }
        return;
    };
    let (bus, _) = split_id(id);
    let Ok(m) = Proxy::new(conn, bus.to_string(), menu, MENU_IFACE) else { return };
    let _ = m.call_method("AboutToShow", &(0i32,));
    type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);
    let reply: zbus::Result<(u32, Layout)> =
        m.call("GetLayout", &(0i32, -1i32, Vec::<String>::new()));
    let Ok((_, (_, _, children))) = reply else { return };
    let mut entries = Vec::new();
    flatten(&children, 0, &mut entries);
    let _ = events.send(Event::Menu { id: id.to_string(), entries });
}

fn flatten(children: &[OwnedValue], depth: usize, out: &mut Vec<MenuEntry>) {
    for child in children {
        let Ok((id, props, kids)) =
            <(i32, HashMap<String, OwnedValue>, Vec<OwnedValue>)>::try_from(child.try_clone().unwrap_or_else(|_| OwnedValue::from(0i32)))
        else {
            continue;
        };
        let get_bool = |k: &str, d: bool| props.get(k).and_then(|v| bool::try_from(v).ok()).unwrap_or(d);
        let get_str = |k: &str| props.get(k).and_then(|v| <&str>::try_from(v).ok()).unwrap_or("").to_string();
        if !get_bool("visible", true) {
            continue;
        }
        let separator = get_str("type") == "separator";
        // GTK-style mnemonics: "_Quit" -> "Quit", "__" -> "_".
        let label = get_str("label").replace("__", "\u{0}").replace('_', "").replace('\u{0}', "_");
        if !separator && label.is_empty() && kids.is_empty() {
            continue;
        }
        out.push(MenuEntry { id, label, enabled: get_bool("enabled", true), separator, depth });
        if !kids.is_empty() {
            flatten(&kids, depth + 1, out);
        }
    }
}
