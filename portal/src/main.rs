use std::{
    collections::HashMap,
    ffi::c_void,
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use eframe::egui::{self, Color32, FontId, RichText, Stroke};
use zbus::{
    blocking::Connection,
    interface,
    zvariant::{OwnedObjectPath, OwnedValue, Value},
};

unsafe extern "C" {
    fn pw_cast_start(width: u32, height: u32, fps: u32) -> *mut c_void;
    fn pw_cast_node_id(ctx: *mut c_void) -> u32;
    fn pw_cast_push_rgba(ctx: *mut c_void, rgba: *const u8, w: u32, h: u32);
    fn pw_cast_stop(ctx: *mut c_void);
}

struct PwHandle(*mut c_void);
unsafe impl Send for PwHandle {}
unsafe impl Sync for PwHandle {}

struct ActiveCast {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for ActiveCast {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        ipc_send("screencast-stop");
    }
}

fn ipc_socket() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("VIDE_IPC") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    let dir = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
    let rd = std::fs::read_dir(dir).ok()?;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("vide-de-") && name.ends_with(".sock") {
            return Some(e.path());
        }
    }
    None
}

fn ipc_send(cmd: &str) -> Option<String> {
    let sock = ipc_socket()?;
    let mut stream = UnixStream::connect(sock).ok()?;
    let _ = stream.set_read_timeout(Some(Duration::from_millis(250)));
    stream.write_all(format!("{cmd}\n").as_bytes()).ok()?;
    let mut out = String::new();
    let mut reader = BufReader::new(&stream);
    let _ = reader.read_line(&mut out);
    Some(out.trim().to_string())
}

fn screen_size() -> (u32, u32) {
    if let Some(s) = ipc_send("screen-info")
        && let Some((w, h)) = s.split_once(' ')
        && let (Ok(w), Ok(h)) = (w.parse::<u32>(), h.parse::<u32>())
        && w > 0
        && h > 0
    {
        return (w, h);
    }
    (1920, 1080)
}

fn perms_path() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::var("HOME").map(PathBuf::from).unwrap_or_default().join(".config"));
    base.join("vide-de/screencast-perms.conf")
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Perm {
    Allow,
    Deny,
}

fn check_perm(app_id: &str) -> Option<Perm> {
    let key = if app_id.is_empty() { "unknown" } else { app_id };
    let txt = std::fs::read_to_string(perms_path()).ok()?;
    for line in txt.lines() {
        let line = line.trim();
        if let Some((k, v)) = line.split_once('=')
            && k.trim() == key
        {
            return match v.trim() {
                "allow" => Some(Perm::Allow),
                "deny" => Some(Perm::Deny),
                _ => None,
            };
        }
    }
    None
}

fn save_perm(app_id: &str, perm: Perm) {
    let key = if app_id.is_empty() { "unknown" } else { app_id };
    let path = perms_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut lines: Vec<String> = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim().starts_with(&format!("{key}=")))
        .map(String::from)
        .collect();
    let val = match perm {
        Perm::Allow => "allow",
        Perm::Deny => "deny",
    };
    lines.push(format!("{key}={val}"));
    let _ = std::fs::write(&path, format!("{}\n", lines.join("\n")));
}

fn ask_user_permission(app_id: &str) -> bool {
    if let Some(p) = check_perm(app_id) {
        return p == Perm::Allow;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let key = if app_id.is_empty() { "An application" } else { app_id };
    Command::new(exe)
        .args(["--prompt", key])
        .status()
        .is_ok_and(|s| s.success())
}

struct PortalSession {
    path: OwnedObjectPath,
    sessions: Arc<Mutex<HashMap<OwnedObjectPath, ActiveCast>>>,
    conn: Connection,
}

#[interface(name = "org.freedesktop.impl.portal.Session")]
impl PortalSession {
    fn close(&self) {
        self.sessions.lock().unwrap().remove(&self.path);
        let conn = self.conn.clone();
        let path = self.path.clone();
        std::thread::spawn(move || {
            let _ = conn.object_server().remove::<PortalSession, _>(&path);
        });
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}

struct ScreenCastPortal {
    conn: Connection,
    sessions: Arc<Mutex<HashMap<OwnedObjectPath, ActiveCast>>>,
}

#[interface(name = "org.freedesktop.impl.portal.ScreenCast")]
impl ScreenCastPortal {
    fn create_session(
        &self,
        _handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        _app_id: String,
        _options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        let sess = PortalSession {
            path: session_handle.clone(),
            sessions: self.sessions.clone(),
            conn: self.conn.clone(),
        };
        let _ = self.conn.object_server().at(&session_handle, sess);
        (0, HashMap::new())
    }

    fn select_sources(
        &self,
        _handle: OwnedObjectPath,
        _session_handle: OwnedObjectPath,
        _app_id: String,
        _options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        (0, HashMap::new())
    }

    fn start(
        &self,
        _handle: OwnedObjectPath,
        session_handle: OwnedObjectPath,
        _parent_window: String,
        app_id: String,
        _options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        if !ask_user_permission(&app_id) {
            return (1, HashMap::new());
        }

        let (w, h) = screen_size();
        ipc_send("screencast-start");

        let ctx_ptr = unsafe { pw_cast_start(w, h, 30) };
        if ctx_ptr.is_null() {
            ipc_send("screencast-stop");
            return (2, HashMap::new());
        }
        let node_id = unsafe { pw_cast_node_id(ctx_ptr) };
        if node_id == 0 {
            unsafe { pw_cast_stop(ctx_ptr) };
            ipc_send("screencast-stop");
            return (2, HashMap::new());
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = stop.clone();
        let pw = PwHandle(ctx_ptr);
        let thread = std::thread::spawn(move || {
            let pw = pw;
            let dir = std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir());
            let frame_path = dir.join("vide-screencast.rgba");
            let mut last_buf: Vec<u8> = Vec::new();
            while !stop_bg.load(Ordering::Relaxed) {
                if let Ok(mut f) = std::fs::File::open(&frame_path) {
                    let mut hdr = [0u8; 8];
                    if f.read_exact(&mut hdr).is_ok() {
                        let fw = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
                        let fh = u32::from_le_bytes(hdr[4..8].try_into().unwrap());
                        let need = (fw as usize) * (fh as usize) * 4;
                        if need > 0 && fw == w && fh == h {
                            last_buf.resize(need, 0);
                            if f.read_exact(&mut last_buf).is_ok() {
                                unsafe { pw_cast_push_rgba(pw.0, last_buf.as_ptr(), fw, fh) };
                            }
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(33));
            }
            unsafe { pw_cast_stop(pw.0) };
        });

        self.sessions.lock().unwrap().insert(
            session_handle,
            ActiveCast { stop, thread: Some(thread) },
        );

        let mut props: HashMap<String, OwnedValue> = HashMap::new();
        props.insert("position".into(), Value::from((0i32, 0i32)).try_into().unwrap());
        props.insert("size".into(), Value::from((w as i32, h as i32)).try_into().unwrap());
        props.insert("source_type".into(), Value::from(1u32).try_into().unwrap());

        let streams: Vec<(u32, HashMap<String, OwnedValue>)> = vec![(node_id, props)];
        let mut results: HashMap<String, OwnedValue> = HashMap::new();
        results.insert("streams".into(), Value::from(streams).try_into().unwrap());
        (0, results)
    }

    #[zbus(property)]
    fn available_source_types(&self) -> u32 {
        1 // Monitor
    }

    #[zbus(property)]
    fn available_cursor_modes(&self) -> u32 {
        3 // Hidden (1) | Embedded (2)
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        4
    }
}

struct ScreenshotPortal;

#[interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl ScreenshotPortal {
    fn screenshot(
        &self,
        _handle: OwnedObjectPath,
        _app_id: String,
        _parent_window: String,
        _options: HashMap<String, OwnedValue>,
    ) -> (u32, HashMap<String, OwnedValue>) {
        ipc_send("screenshot");
        (0, HashMap::new())
    }

    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }
}

struct PromptApp {
    app_id: String,
    remember: bool,
    allowed: Arc<AtomicBool>,
    decided: Arc<AtomicBool>,
    size: (u32, u32),
}

impl eframe::App for PromptApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut visuals = egui::Visuals::light();
        visuals.window_fill = Color32::WHITE;
        visuals.panel_fill = Color32::WHITE;
        visuals.widgets.inactive.corner_radius = 0.0.into();
        visuals.widgets.hovered.corner_radius = 0.0.into();
        visuals.widgets.active.corner_radius = 0.0.into();
        ctx.set_visuals(visuals);

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::WHITE).inner_margin(22.0))
            .show(ui, |ui| {
                ui.label(RichText::new("Share your screen").font(FontId::proportional(20.0)).color(Color32::from_rgb(0x11, 0x11, 0x11)));
                ui.add_space(6.0);
                ui.label(
                    RichText::new(format!("\"{}\" wants to view the contents of your screen.", self.app_id))
                        .size(13.5)
                        .color(Color32::from_rgb(0x44, 0x44, 0x44)),
                );
                ui.add_space(14.0);
                egui::Frame::new()
                    .fill(Color32::from_rgb(0xf4, 0xf4, 0xf4))
                    .stroke(Stroke::new(1.5, Color32::from_rgb(0x00, 0x78, 0xd7)))
                    .inner_margin(12.0)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new("Entire Screen").size(14.5).color(Color32::from_rgb(0x11, 0x11, 0x11)));
                        ui.label(
                            RichText::new(format!("Primary Display ({} × {})", self.size.0, self.size.1))
                                .size(12.0)
                                .color(Color32::from_rgb(0x60, 0x60, 0x60)),
                        );
                    });
                ui.add_space(12.0);
                ui.checkbox(&mut self.remember, "Remember my choice for this app");
                ui.add_space(16.0);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_sized([92.0, 30.0], egui::Button::new("Deny")).clicked() {
                        if self.remember {
                            save_perm(&self.app_id, Perm::Deny);
                        }
                        self.allowed.store(false, Ordering::Relaxed);
                        self.decided.store(true, Ordering::Relaxed);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    ui.add_space(8.0);
                    let share = egui::Button::new(RichText::new("Share").color(Color32::WHITE))
                        .fill(Color32::from_rgb(0x00, 0x78, 0xd7));
                    if ui.add_sized([92.0, 30.0], share).clicked() {
                        if self.remember {
                            save_perm(&self.app_id, Perm::Allow);
                        }
                        self.allowed.store(true, Ordering::Relaxed);
                        self.decided.store(true, Ordering::Relaxed);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
    }
}

fn run_prompt(app_id: String) -> ! {
    let allowed = Arc::new(AtomicBool::new(false));
    let decided = Arc::new(AtomicBool::new(false));
    let allowed_c = allowed.clone();
    let decided_c = decided.clone();
    let size = screen_size();
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([430.0, 235.0])
            .with_resizable(false)
            .with_always_on_top(),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "VIDE-DE Screen Share",
        opts,
        Box::new(move |cc| {
            vide_fonts::install_egui!(eframe::egui, cc.egui_ctx);
            Ok(Box::new(PromptApp {
                app_id,
                remember: false,
                allowed: allowed_c,
                decided: decided_c,
                size,
            }))
        }),
    );
    let ok = decided.load(Ordering::Relaxed) && allowed.load(Ordering::Relaxed);
    std::process::exit(if ok { 0 } else { 1 });
}

fn main() {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--prompt") {
        let app_id = args.next().unwrap_or_else(|| "An application".into());
        run_prompt(app_id);
    }

    let Ok(conn) = Connection::session() else {
        eprintln!("vide-portal: failed to connect to session D-Bus");
        std::process::exit(1);
    };
    let sessions = Arc::new(Mutex::new(HashMap::new()));
    let screencast = ScreenCastPortal {
        conn: conn.clone(),
        sessions,
    };
    let _ = conn.object_server().at("/org/freedesktop/portal/desktop", screencast);
    let _ = conn.object_server().at("/org/freedesktop/portal/desktop", ScreenshotPortal);
    if let Err(e) = conn.request_name("org.freedesktop.impl.portal.desktop.vide") {
        eprintln!("vide-portal: failed to claim D-Bus name: {e}");
        std::process::exit(1);
    }
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
