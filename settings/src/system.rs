use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Clone, Debug)]
pub struct AudioDev {
    pub name: String,
    pub description: String,
    pub volume: u32,
    pub muted: bool,
}

#[derive(Clone, Debug)]
pub struct SinkInput {
    pub index: u32,
    pub app: String,
    pub title: String,
    pub volume: u32,
    pub muted: bool,
}

#[derive(Clone, Debug)]
pub struct Wifi {
    pub ssid: String,
    pub signal: u8,
    pub security: String,
    pub active: bool,
}

#[derive(Clone, Debug)]
pub struct BtDevice {
    pub mac: String,
    pub name: String,
    pub connected: bool,
}

#[derive(Clone, Debug)]
pub struct DiskInfo {
    pub fs: String,
    pub mount: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub pct: f32,
}

/// Keyboard layouts and their variants, from xkb's own rules file.
#[derive(Clone, Debug, Default)]
pub struct XkbInfo {
    /// (code, description), e.g. ("us", "English (US)").
    pub layouts: Vec<(String, String)>,
    /// (layout, variant code, description).
    pub variants: Vec<(String, String, String)>,
}

pub fn load_xkb() -> XkbInfo {
    let text = ["/usr/share/X11/xkb/rules/evdev.lst", "/usr/share/X11/xkb/rules/base.lst"]
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    let mut info = XkbInfo::default();
    let mut section = "";
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("! ") {
            section = name.trim();
            continue;
        }
        let line = line.trim();
        let Some((code, rest)) = line.split_once(char::is_whitespace) else { continue };
        let rest = rest.trim();
        match section {
            "layout" => info.layouts.push((code.to_string(), rest.to_string())),
            // "intl    us: English (US, intl., with dead keys)"
            "variant" => {
                if let Some((layout, desc)) = rest.split_once(':') {
                    info.variants.push((layout.trim().to_string(), code.to_string(), desc.trim().to_string()));
                }
            }
            _ => {}
        }
    }
    info
}

/// One row of the portal permission store (`flatpak permissions`). The data column is never
/// read: for files it holds paths from the user's disk.
#[derive(Clone, Debug)]
pub struct PermEntry {
    pub table: String,
    pub object: String,
    /// Empty for apps that aren't sandboxed.
    pub app: String,
    /// "yes", "no", "ask", or table-specific.
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct AppEntry {
    pub name: String,
    pub exec: String,
    pub comment: String,
}

#[derive(Clone, Debug)]
pub struct NetIface {
    pub device: String,
    pub kind: String,
    pub state: String,
    pub connection: String,
    pub ip: String,
}

#[derive(Clone, Debug, Default)]
pub struct SystemSpecs {
    pub hostname: String,
    pub username: String,
    pub cpu: String,
    pub cores: usize,
    pub ram_gb: f64,
    pub kernel: String,
    pub gpu: String,
    pub session: String,
    pub wayland_display: String,
    pub timezone: String,
    pub ntp_enabled: bool,
    pub locale: String,
}

#[derive(Clone, Debug)]
pub struct ServiceStatus {
    pub name: &'static str,
    pub state: ServiceState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceState {
    Running,
    Stopped,
    /// systemctl couldn't be asked (no user bus, say): not the same as stopped.
    Unknown,
}

#[derive(Default)]
pub struct System {
    pub sinks: Vec<AudioDev>,
    pub default_sink: String,
    pub sources: Vec<AudioDev>,
    pub default_source: String,
    pub sink_inputs: Vec<SinkInput>,
    pub wifi_on: bool,
    pub wifi: Vec<Wifi>,
    pub wifi_target: Option<String>,
    pub wifi_password: String,
    pub bt_on: bool,
    pub bt_devices: Vec<BtDevice>,
    pub net_ifaces: Vec<NetIface>,
    pub disks: Vec<DiskInfo>,
    pub apps: Vec<AppEntry>,
    pub apps_loaded: bool,
    pub specs: SystemSpecs,
    pub specs_loaded: bool,
    pub services: Vec<ServiceStatus>,
    pub log_tail: String,
    pub privacy: Vec<PermEntry>,
    pub xkb: Option<XkbInfo>,
    pub timezones: Vec<String>,
    /// Flatpak application id -> the name people know it by.
    pub app_names: std::collections::HashMap<String, String>,
    pub default_browser: String,
    pub default_files: String,
    pub default_text: String,
    pub default_image: String,
    pub power_profile: Option<String>,
    pub checked: Option<Instant>,
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum RefreshKind {
    Sound,
    Network,
    Bluetooth,
    Storage,
    Power,
    Apps,
    Defaults,
    About,
    Time,
    Update,
    Privacy,
    None,
}

impl System {
    pub fn refresh(&mut self, kind: RefreshKind) {
        if kind == RefreshKind::Apps && !self.apps_loaded {
            self.refresh_apps();
            self.apps_loaded = true;
            return;
        }
        if !self.specs_loaded && matches!(kind, RefreshKind::About | RefreshKind::Time | RefreshKind::Update) {
            self.refresh_specs();
            self.specs_loaded = true;
        }
        let interval = match kind {
            RefreshKind::Network | RefreshKind::Bluetooth => Duration::from_secs(3),
            RefreshKind::Sound => Duration::from_secs(1),
            RefreshKind::Storage | RefreshKind::Power | RefreshKind::Update | RefreshKind::Time | RefreshKind::Privacy => Duration::from_secs(4),
            RefreshKind::Defaults | RefreshKind::About => Duration::from_secs(10),
            RefreshKind::Apps | RefreshKind::None => return,
        };
        if self.checked.is_some_and(|t| t.elapsed() < interval) {
            return;
        }
        self.checked = Some(Instant::now());
        match kind {
            RefreshKind::Sound => self.refresh_sound(),
            RefreshKind::Network => self.refresh_network(),
            RefreshKind::Bluetooth => self.refresh_bluetooth(),
            RefreshKind::Storage => self.refresh_storage(),
            RefreshKind::Power => self.refresh_power(),
            RefreshKind::Defaults => self.refresh_defaults(),
            RefreshKind::About | RefreshKind::Time => self.refresh_specs(),
            RefreshKind::Update => self.refresh_update(),
            RefreshKind::Privacy => self.refresh_privacy(),
            RefreshKind::Apps | RefreshKind::None => {}
        }
    }

    pub fn invalidate(&mut self) {
        self.checked = None;
    }

    fn refresh_sound(&mut self) {
        self.default_sink = run("pactl", &["get-default-sink"]).unwrap_or_default().trim().to_string();
        self.default_source = run("pactl", &["get-default-source"]).unwrap_or_default().trim().to_string();

        self.sinks.clear();
        if let Some(json) = run("pactl", &["-f", "json", "list", "sinks"])
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&json)
            && let Some(arr) = v.as_array()
        {
            for s in arr {
                self.sinks.push(AudioDev {
                    name: s["name"].as_str().unwrap_or("").to_string(),
                    description: s["description"].as_str().unwrap_or("Output device").to_string(),
                    volume: volume_percent(&s["volume"]),
                    muted: s["mute"].as_bool().unwrap_or(false),
                });
            }
        }

        self.sources.clear();
        if let Some(json) = run("pactl", &["-f", "json", "list", "sources"])
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&json)
            && let Some(arr) = v.as_array()
        {
            for s in arr {
                let name = s["name"].as_str().unwrap_or("");
                if name.ends_with(".monitor") {
                    continue;
                }
                self.sources.push(AudioDev {
                    name: name.to_string(),
                    description: s["description"].as_str().unwrap_or("Input device").to_string(),
                    volume: volume_percent(&s["volume"]),
                    muted: s["mute"].as_bool().unwrap_or(false),
                });
            }
        }

        self.sink_inputs.clear();
        if let Some(json) = run("pactl", &["-f", "json", "list", "sink-inputs"])
            && let Ok(v) = serde_json::from_str::<serde_json::Value>(&json)
            && let Some(arr) = v.as_array()
        {
            for i in arr {
                let props = &i["properties"];
                let app = props["application.name"]
                    .as_str()
                    .or_else(|| props["media.name"].as_str())
                    .unwrap_or("App")
                    .to_string();
                let title = props["media.name"].as_str().unwrap_or("").to_string();
                let title = if title == app { String::new() } else { title };
                self.sink_inputs.push(SinkInput {
                    index: i["index"].as_u64().unwrap_or(0) as u32,
                    app,
                    title,
                    volume: volume_percent(&i["volume"]),
                    muted: i["mute"].as_bool().unwrap_or(false),
                });
            }
        }
    }

    fn refresh_network(&mut self) {
        self.wifi_on = run("nmcli", &["radio", "wifi"]).is_some_and(|s| s.trim() == "enabled");
        self.wifi.clear();
        if self.wifi_on
            && let Some(out) = run("nmcli", &["-t", "-f", "IN-USE,SIGNAL,SECURITY,SSID", "device", "wifi", "list", "--rescan", "no"])
        {
            for line in out.lines() {
                let mut f = line.splitn(4, ':');
                let in_use = f.next().unwrap_or("").contains('*');
                let signal = f.next().unwrap_or("0").parse().unwrap_or(0);
                let security = f.next().unwrap_or("").replace("\\:", ":");
                let ssid = f.next().unwrap_or("").replace("\\:", ":");
                if ssid.is_empty() || self.wifi.iter().any(|w| w.ssid == ssid) {
                    continue;
                }
                self.wifi.push(Wifi { ssid, signal, security, active: in_use });
            }
            self.wifi.sort_by(|a, b| b.signal.cmp(&a.signal));
        }

        self.net_ifaces.clear();
        if let Some(out) = run("nmcli", &["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device"]) {
            for line in out.lines() {
                let mut f = line.splitn(4, ':');
                let device = f.next().unwrap_or("").to_string();
                let kind = f.next().unwrap_or("").to_string();
                let state = f.next().unwrap_or("").to_string();
                let connection = f.next().unwrap_or("").to_string();
                if device.is_empty() || kind == "loopback" {
                    continue;
                }
                let ip = run("nmcli", &["-g", "IP4.ADDRESS", "device", "show", &device])
                    .unwrap_or_default()
                    .lines()
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                self.net_ifaces.push(NetIface { device, kind, state, connection, ip });
            }
        }
    }

    fn refresh_bluetooth(&mut self) {
        self.bt_on = run("bluetoothctl", &["show"]).is_some_and(|s| s.contains("Powered: yes"));
        self.bt_devices.clear();
        let Some(out) = run("bluetoothctl", &["devices", "Paired"]) else { return };
        for line in out.lines() {
            let Some(rest) = line.strip_prefix("Device ") else { continue };
            let Some((mac, name)) = rest.split_once(' ') else { continue };
            let connected = run("bluetoothctl", &["info", mac]).is_some_and(|s| s.contains("Connected: yes"));
            self.bt_devices.push(BtDevice {
                mac: mac.to_string(),
                name: name.to_string(),
                connected,
            });
        }
    }

    fn refresh_storage(&mut self) {
        self.disks.clear();
        let Some(out) = run("df", &["-B1", "-P", "-T"]) else { return };
        for line in out.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 7 {
                continue;
            }
            let fs = parts[0];
            let fstype = parts[1];
            if matches!(fstype, "tmpfs" | "devtmpfs" | "efivarfs" | "squashfs" | "overlay") || fstype == "fuse" || fstype.starts_with("fuse.") {
                continue;
            }
            let total: u64 = parts[2].parse().unwrap_or(0);
            let used: u64 = parts[3].parse().unwrap_or(0);
            let mount = parts[6..].join(" ");
            if total == 0 {
                continue;
            }
            // Btrfs subvolumes (/, /home, /var/log, swap...) share one device and one set of numbers:
            // show the drive once, under its most general mount.
            if let Some(existing) = self.disks.iter_mut().find(|d| d.fs == fs) {
                if mount == "/" || (existing.mount != "/" && mount.len() < existing.mount.len()) {
                    existing.mount = mount;
                }
                continue;
            }
            let pct = (used as f64 / total as f64 * 100.0) as f32;
            self.disks.push(DiskInfo {
                fs: fs.to_string(),
                mount,
                total_bytes: total,
                used_bytes: used,
                pct,
            });
        }
    }

    fn refresh_power(&mut self) {
        self.power_profile = run("powerprofilesctl", &["get"]).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    }

    fn refresh_defaults(&mut self) {
        self.default_browser = run("xdg-mime", &["query", "default", "x-scheme-handler/http"]).unwrap_or_default().trim().to_string();
        self.default_files = run("xdg-mime", &["query", "default", "inode/directory"]).unwrap_or_default().trim().to_string();
        self.default_text = run("xdg-mime", &["query", "default", "text/plain"]).unwrap_or_default().trim().to_string();
        self.default_image = run("xdg-mime", &["query", "default", "image/png"]).unwrap_or_default().trim().to_string();
    }

    fn refresh_apps(&mut self) {
        self.apps.clear();
        let mut dirs = vec![PathBuf::from("/usr/share/applications")];
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(home).join(".local/share/applications"));
        }
        for dir in dirs {
            collect_desktop_entries(&dir, &mut self.apps);
        }
        self.apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        self.apps.dedup_by(|a, b| a.name.eq_ignore_ascii_case(&b.name));
    }

    fn refresh_specs(&mut self) {
        let hostname = std::fs::read_to_string("/etc/hostname")
            .unwrap_or_else(|_| "vide-pc".into())
            .trim()
            .to_string();
        let username = std::env::var("USER").unwrap_or_else(|_| "user".into());

        let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
        let cpu = cpuinfo
            .lines()
            .find_map(|l| l.strip_prefix("model name"))
            .and_then(|s| s.split_once(':'))
            .map(|(_, v)| v.trim().to_string())
            .unwrap_or_else(|| "Unknown CPU".into());
        let cores = cpuinfo.lines().filter(|l| l.starts_with("processor")).count().max(1);

        let meminfo = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
        let mem_kb: f64 = meminfo
            .lines()
            .find_map(|l| l.strip_prefix("MemTotal:"))
            .and_then(|s| s.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap_or(0.0);
        let ram_gb = mem_kb / (1024.0 * 1024.0);

        let kernel = run("uname", &["-r"]).unwrap_or_default().trim().to_string();
        let session = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "wayland".into());
        let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".into());
        let locale = std::env::var("LANG").unwrap_or_else(|_| "en_US.UTF-8".into());

        let gpu = detect_gpu();

        let (timezone, ntp_enabled) = if let Some(td) = run("timedatectl", &["show", "--property=Timezone", "--property=NTP", "--value"]) {
            let mut lines = td.lines();
            let tz = lines.next().unwrap_or("Local").trim().to_string();
            let ntp = lines.next().unwrap_or("no").trim() == "yes";
            (tz, ntp)
        } else {
            ("Local".into(), false)
        };

        self.specs = SystemSpecs {
            hostname,
            username,
            cpu,
            cores,
            ram_gb,
            kernel,
            gpu,
            session,
            wayland_display,
            timezone,
            ntp_enabled,
            locale,
        };
    }

    fn refresh_privacy(&mut self) {
        // `flatpak permissions` prints Table, Object, App, Permissions, Data, tab-separated.
        // Only the first four are used; Data can hold file paths and stays unread.
        let wanted = ["devices", "location", "notifications", "screencast", "documents"];
        self.privacy = run("flatpak", &["permissions"])
            .unwrap_or_default()
            .lines()
            .filter_map(|l| {
                let mut f = l.split('\t');
                let (table, object, app, value) = (f.next()?, f.next()?, f.next()?, f.next()?);
                wanted.contains(&table).then(|| PermEntry { table: table.into(), object: object.into(), app: app.into(), value: value.trim().into() })
            })
            .collect();
        if self.app_names.is_empty() {
            self.app_names = run("flatpak", &["list", "--app", "--columns=application,name"])
                .unwrap_or_default()
                .lines()
                .filter_map(|l| {
                    let (id, name) = l.split_once('\t')?;
                    Some((id.trim().to_string(), name.trim().to_string()))
                })
                .collect();
        }
    }

    fn refresh_update(&mut self) {
        // (unit, runs in the user's systemd?). Each lives in one place; asking the other manager about
        // a unit it doesn't own just says "inactive", which would read as a false "not running".
        let units: [(&'static str, bool); 4] = [("pipewire", true), ("wireplumber", true), ("xdg-desktop-portal", true), ("NetworkManager", false)];
        self.services = units
            .iter()
            .map(|&(name, user)| {
                let mut args = vec!["show", "-p", "ActiveState", "--value", name];
                if user {
                    args.insert(0, "--user");
                }
                // `show` exits 0 with a state even for stopped units, unlike `is-active`, so a failure
                // here means we couldn't ask at all.
                let state = match Command::new("systemctl").args(&args).output().ok().filter(|o| o.status.success()) {
                    Some(o) if String::from_utf8_lossy(&o.stdout).trim() == "active" => ServiceState::Running,
                    Some(_) => ServiceState::Stopped,
                    None => ServiceState::Unknown,
                };
                ServiceStatus { name, state }
            })
            .collect();

        let mut log_text = String::new();
        let candidates = [
            std::env::var("HOME").ok().map(|h| PathBuf::from(h).join("vibe.log")),
            Some(PathBuf::from("/tmp/vibe.log")),
        ];
        for path in candidates.into_iter().flatten() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                log_text = compositor_lines(&content, 30);
                break;
            }
        }
        if log_text.is_empty() {
            log_text = "No ~/vibe.log or /tmp/vibe.log found.".into();
        }
        self.log_tail = log_text;
    }
}

/// Drop colour codes: the log is written with ANSI escapes.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The last `max` lines the compositor itself wrote (they start with a timestamp), with runs of
/// the same message folded into one. Apps share the log file, and their output would bury these.
fn compositor_lines(log: &str, max: usize) -> String {
    let mut rows: Vec<(String, String, u32)> = Vec::new(); // (time, message, repeats)
    for raw in log.lines() {
        let line = strip_ansi(raw);
        let b = line.as_bytes();
        if line.len() < 30 || b[4] != b'-' || b[10] != b'T' {
            continue;
        }
        let (time, msg) = line.split_once(' ').map(|(t, m)| (t[11..19.min(t.len())].to_string(), m.trim().to_string())).unwrap_or_default();
        if let Some(last) = rows.last_mut().filter(|l| l.1 == msg) {
            last.2 += 1;
        } else {
            rows.push((time, msg, 1));
        }
    }
    let start = rows.len().saturating_sub(max);
    if rows.is_empty() {
        return "Nothing from the compositor yet.".into();
    }
    rows[start..]
        .iter()
        .map(|(t, m, n)| {
            let m: String = m.chars().take(170).collect();
            if *n > 1 { format!("{t}  {m}  (x{n})") } else { format!("{t}  {m}") }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn detect_gpu() -> String {
    if let Ok(nvidia) = std::fs::read_to_string("/proc/driver/nvidia/gpus") {
        let _ = nvidia;
    }
    if let Ok(rd) = std::fs::read_dir("/proc/driver/nvidia/gpus") {
        for e in rd.flatten() {
            if let Ok(info) = std::fs::read_to_string(e.path().join("information"))
                && let Some(model) = info.lines().find_map(|l| l.strip_prefix("Model:"))
            {
                return model.trim().to_string();
            }
        }
    }
    if let Some(out) = run("lspci", &[]) {
        for line in out.lines() {
            if line.contains("VGA compatible controller") || line.contains("3D controller") {
                if let Some((_, rest)) = line.split_once(": ") {
                    return rest.trim().to_string();
                }
            }
        }
    }
    "DRM / Mesa GPU".into()
}

fn collect_desktop_entries(dir: &PathBuf, out: &mut Vec<AppEntry>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_desktop_entries(&path, out);
            continue;
        }
        if path.extension().is_none_or(|e| e != "desktop") {
            continue;
        }
        let Ok(txt) = std::fs::read_to_string(&path) else { continue };
        let mut in_entry = false;
        let mut name = String::new();
        let mut exec = String::new();
        let mut comment = String::new();
        let mut nodisplay = false;
        let mut hidden = false;
        for line in txt.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_entry {
                continue;
            }
            if let Some(v) = line.strip_prefix("Name=") {
                if name.is_empty() {
                    name = v.to_string();
                }
            } else if let Some(v) = line.strip_prefix("Exec=") {
                if exec.is_empty() {
                    exec = v.to_string();
                }
            } else if let Some(v) = line.strip_prefix("Comment=") {
                if comment.is_empty() {
                    comment = v.to_string();
                }
            } else if line == "NoDisplay=true" {
                nodisplay = true;
            } else if line == "Hidden=true" {
                hidden = true;
            }
        }
        if !nodisplay && !hidden && !name.is_empty() && !exec.is_empty() {
            out.push(AppEntry { name, exec, comment });
        }
    }
}

pub fn launch_desktop_exec(exec: &str) {
    let cleaned = exec
        .replace("%U", "")
        .replace("%u", "")
        .replace("%F", "")
        .replace("%f", "")
        .replace("%i", "")
        .replace("%c", "")
        .replace("%k", "");
    let _ = Command::new("sh").args(["-c", &cleaned]).spawn();
}

pub fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn spawn(cmd: &str, args: &[&str]) {
    // Wait for it in a thread: a child nobody waits for stays a zombie after it exits.
    if let Ok(mut child) = Command::new(cmd).args(args).spawn() {
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
}

pub fn volume_percent(v: &serde_json::Value) -> u32 {
    v.as_object()
        .and_then(|m| m.values().next())
        .and_then(|c| c["value_percent"].as_str())
        .and_then(|s| s.trim_end_matches('%').parse().ok())
        .unwrap_or(100)
}

/// Connected DRM connectors and their modes, from sysfs.
pub fn load_timezones() -> Vec<String> {
    run("timedatectl", &["list-timezones"]).unwrap_or_default().lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

pub fn display_modes() -> Vec<(String, Vec<u32>)> {
    let mut out: Vec<(String, Vec<u32>)> = Vec::new();
    let Ok(rd) = std::fs::read_dir("/sys/class/drm") else { return out };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with("card") || !name.contains('-') {
            continue;
        }
        let dir = e.path();
        if std::fs::read_to_string(dir.join("status")).is_ok_and(|s| s.trim() != "connected") {
            continue;
        }
        let Ok(modes) = std::fs::read_to_string(dir.join("modes")) else { continue };
        for line in modes.lines() {
            let m = line.trim();
            if m.is_empty() || out.iter().any(|(x, _)| x == m) {
                continue;
            }
            out.push((m.to_string(), vec![60, 75, 100, 120, 144, 165, 240]));
        }
    }
    out.sort();
    out
}

pub fn fmt_res(r: &str) -> String {
    match r.split_once('x') {
        Some((w, h)) => format!("{w} × {h}"),
        None => r.to_string(),
    }
}

pub fn fmt_bytes(b: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    if b as f64 >= GB {
        format!("{:.1} GB", b as f64 / GB)
    } else {
        format!("{:.0} MB", b as f64 / MB)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn services_report_running_when_systemd_says_active() {
        let mut s = System::default();
        s.refresh_update();
        // Only meaningful where systemd runs; elsewhere every state is Unknown.
        for svc in &s.services {
            eprintln!("{} -> {:?}", svc.name, svc.state);
        }
    }

    #[test]
    fn log_lines_are_filtered_and_folded() {
        let log = "Warning: some app noise\n\
                   \u{1b}[2m2026-11-13T03:22:10.1Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m vibe_comp: one\n\
                   \u{1b}[2m2026-11-13T03:22:11.2Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m vibe_comp: same\n\
                   \u{1b}[2m2026-11-13T03:22:12.3Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m vibe_comp: same\n\
                   info: Roblox: more app noise\n";
        let out = compositor_lines(log, 10);
        assert!(!out.contains("Roblox") && !out.contains("app noise"));
        assert!(out.contains("(x2)"), "{out}");
        assert_eq!(out.lines().count(), 2, "{out}");
    }
}
