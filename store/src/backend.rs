use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum PackageSource {
    Flatpak,
    Pacman,
    Aur,
    Apt,
    Dnf,
}

impl PackageSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Flatpak => "Flathub",
            Self::Pacman => "pacman",
            Self::Aur => "AUR",
            Self::Apt => "apt",
            Self::Dnf => "dnf",
        }
    }

    pub fn badge_rgb(self) -> [u8; 3] {
        match self {
            Self::Flatpak => [74, 134, 232],
            Self::Pacman => [23, 147, 209],
            Self::Aur => [23, 147, 209],
            Self::Apt => [215, 75, 55],
            Self::Dnf => [60, 110, 180],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SourceFilter {
    All,
    Flatpak,
    System,
}

#[derive(Clone, Debug)]
pub struct DetectedBackends {
    pub flatpak: bool,
    pub pacman: bool,
    pub checkupdates: bool,
    pub yay: bool,
    pub paru: bool,
    pub apt: bool,
    pub dnf: bool,
}

impl DetectedBackends {
    pub fn detect() -> Self {
        Self {
            flatpak: has_bin("flatpak"),
            pacman: has_bin("pacman"),
            checkupdates: has_bin("checkupdates"),
            yay: has_bin("yay"),
            paru: has_bin("paru"),
            apt: has_bin("apt-get") && has_bin("apt-cache"),
            dnf: has_bin("dnf"),
        }
    }

    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.flatpak {
            parts.push("Flatpak");
        }
        if self.pacman {
            parts.push("pacman");
        }
        if self.paru {
            parts.push("paru");
        } else if self.yay {
            parts.push("yay");
        }
        if self.apt {
            parts.push("apt");
        }
        if self.dnf {
            parts.push("dnf");
        }
        if parts.is_empty() {
            "No package managers detected".to_string()
        } else {
            parts.join(" + ")
        }
    }

    pub fn native_label(&self) -> &'static str {
        if self.pacman {
            "pacman"
        } else if self.apt {
            "apt"
        } else if self.dnf {
            "dnf"
        } else {
            "System"
        }
    }
}

fn has_bin(name: &str) -> bool {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in path_var.split(':') {
            if Path::new(dir).join(name).is_file() {
                return true;
            }
        }
    }
    false
}

#[derive(Clone, Debug)]
pub struct AppItem {
    pub id: String,
    pub name: String,
    pub summary: String,
    pub version: String,
    pub new_version: Option<String>,
    pub source: PackageSource,
    pub origin: String,
    pub installed: bool,
    pub category: String,
    pub publisher: String,
    pub accent_rgb: [u8; 3],
    pub rating: &'static str,
}

impl AppItem {
    pub fn has_update(&self) -> bool {
        self.new_version.is_some()
    }
}

#[derive(Clone, Debug)]
pub enum BackendEvent {
    LibraryLoaded {
        installed: Vec<AppItem>,
        updates: Vec<AppItem>,
    },
    SearchResults {
        query: String,
        results: Vec<AppItem>,
    },
    TaskStarted {
        title: String,
    },
    TaskLog {
        line: String,
    },
    TaskFinished {
        ok: bool,
        message: String,
    },
}

#[derive(Clone, Debug)]
pub enum StoreAction {
    Install(AppItem),
    UpdateOne(AppItem),
    Uninstall(AppItem),
    UpdateAll {
        has_flatpak_updates: bool,
        has_pacman_updates: bool,
        has_aur_updates: bool,
        has_apt_updates: bool,
        has_dnf_updates: bool,
    },
}

pub fn spawn_refresh_library(
    backends: DetectedBackends,
    tx: Sender<BackendEvent>,
    ctx: eframe::egui::Context,
) {
    std::thread::spawn(move || {
        let mut installed = Vec::new();
        let mut updates = Vec::new();

        if backends.flatpak {
            let fp_updates = load_flatpak_updates();
            let update_ids: HashSet<String> = fp_updates.iter().map(|u| u.id.clone()).collect();
            let mut fp_installed = load_flatpak_installed();
            for item in &mut fp_installed {
                if let Some(up) = fp_updates.iter().find(|u| u.id == item.id) {
                    item.new_version = up.new_version.clone();
                }
            }
            for up in fp_updates {
                updates.push(up);
            }
            for item in fp_installed {
                if !update_ids.contains(&item.id) {
                    installed.push(item);
                } else {
                    // Keep in installed list too so Library shows everything installed
                    installed.push(item);
                }
            }
        }

        if backends.pacman {
            let sys_updates = load_pacman_updates(&backends);
            let mut sys_installed = load_pacman_installed();
            for item in &mut sys_installed {
                if let Some(up) = sys_updates.iter().find(|u| u.id == item.id) {
                    item.new_version = up.new_version.clone();
                }
            }
            updates.extend(sys_updates);
            installed.extend(sys_installed);
        } else if backends.apt {
            let sys_updates = load_apt_updates();
            updates.extend(sys_updates);
        } else if backends.dnf {
            let sys_updates = load_dnf_updates();
            updates.extend(sys_updates);
        }

        installed.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        updates.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        let _ = tx.send(BackendEvent::LibraryLoaded { installed, updates });
        ctx.request_repaint();
    });
}

pub fn spawn_search(
    backends: DetectedBackends,
    query: String,
    filter: SourceFilter,
    installed_ids: HashSet<String>,
    tx: Sender<BackendEvent>,
    ctx: eframe::egui::Context,
) {
    std::thread::spawn(move || {
        let q = query.trim().to_string();
        let mut results = Vec::new();
        if q.is_empty() {
            let _ = tx.send(BackendEvent::SearchResults { query: q, results });
            ctx.request_repaint();
            return;
        }

        if backends.flatpak && matches!(filter, SourceFilter::All | SourceFilter::Flatpak) {
            results.extend(search_flatpak(&q, &installed_ids));
        }

        if matches!(filter, SourceFilter::All | SourceFilter::System) {
            if backends.pacman {
                results.extend(search_pacman(&q, &installed_ids));
                if backends.paru {
                    results.extend(search_aur("paru", &q, &installed_ids));
                } else if backends.yay {
                    results.extend(search_aur("yay", &q, &installed_ids));
                }
            } else if backends.apt {
                results.extend(search_apt(&q, &installed_ids));
            } else if backends.dnf {
                results.extend(search_dnf(&q, &installed_ids));
            }
        }

        // Deduplicate by (source, id)
        let mut seen = HashSet::new();
        results.retain(|item| seen.insert((item.source, item.id.clone())));

        let _ = tx.send(BackendEvent::SearchResults { query: q, results });
        ctx.request_repaint();
    });
}

pub fn spawn_action(
    backends: DetectedBackends,
    action: StoreAction,
    tx: Sender<BackendEvent>,
    ctx: eframe::egui::Context,
) {
    std::thread::spawn(move || {
        let title = match &action {
            StoreAction::Install(item) => format!("Installing {}...", item.name),
            StoreAction::UpdateOne(item) => format!("Updating {}...", item.name),
            StoreAction::Uninstall(item) => format!("Uninstalling {}...", item.name),
            StoreAction::UpdateAll { .. } => "Updating all apps and packages...".to_string(),
        };
        let _ = tx.send(BackendEvent::TaskStarted { title: title.clone() });
        ctx.request_repaint();

        let ok = match action {
            StoreAction::Install(item) => run_item_command(&backends, &item, ItemOp::Install, &tx, &ctx),
            StoreAction::UpdateOne(item) => run_item_command(&backends, &item, ItemOp::Update, &tx, &ctx),
            StoreAction::Uninstall(item) => run_item_command(&backends, &item, ItemOp::Uninstall, &tx, &ctx),
            StoreAction::UpdateAll {
                has_flatpak_updates,
                has_pacman_updates,
                has_aur_updates,
                has_apt_updates,
                has_dnf_updates,
            } => {
                let mut all_ok = true;
                if has_flatpak_updates && backends.flatpak {
                    all_ok &= stream_cmd(
                        "flatpak",
                        &["update", "-y", "--noninteractive"],
                        &tx,
                        &ctx,
                    );
                }
                if has_pacman_updates && backends.pacman {
                    all_ok &= stream_cmd(
                        "pkexec",
                        &["pacman", "-Syu", "--noconfirm"],
                        &tx,
                        &ctx,
                    );
                }
                if has_aur_updates {
                    if backends.paru {
                        all_ok &= stream_cmd("paru", &["-Sua", "--noconfirm"], &tx, &ctx);
                    } else if backends.yay {
                        all_ok &= stream_cmd("yay", &["-Sua", "--noconfirm"], &tx, &ctx);
                    }
                }
                if has_apt_updates && backends.apt {
                    all_ok &= stream_cmd(
                        "pkexec",
                        &["sh", "-c", "apt-get update && apt-get upgrade -y"],
                        &tx,
                        &ctx,
                    );
                }
                if has_dnf_updates && backends.dnf {
                    all_ok &= stream_cmd("pkexec", &["dnf", "upgrade", "-y"], &tx, &ctx);
                }
                all_ok
            }
        };

        let message = if ok {
            "Completed successfully.".to_string()
        } else {
            "Operation failed. Check activity log for details.".to_string()
        };
        let _ = tx.send(BackendEvent::TaskFinished { ok, message });
        ctx.request_repaint();
    });
}

pub fn launch_app(item: &AppItem) {
    match item.source {
        PackageSource::Flatpak => {
            let _ = Command::new("flatpak")
                .args(["run", &item.id])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
        _ => {
            // Try gtk-launch first, fallback to direct binary
            let launched = Command::new("gtk-launch")
                .arg(&item.id)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .is_ok();
            if !launched {
                let _ = Command::new(&item.id)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn();
            }
        }
    }
}

enum ItemOp {
    Install,
    Update,
    Uninstall,
}

fn run_item_command(
    backends: &DetectedBackends,
    item: &AppItem,
    op: ItemOp,
    tx: &Sender<BackendEvent>,
    ctx: &eframe::egui::Context,
) -> bool {
    match item.source {
        PackageSource::Flatpak => match op {
            ItemOp::Install => {
                let origin = if item.origin.is_empty() {
                    "flathub"
                } else {
                    item.origin.as_str()
                };
                stream_cmd(
                    "flatpak",
                    &["install", "-y", "--noninteractive", origin, &item.id],
                    tx,
                    ctx,
                )
            }
            ItemOp::Update => stream_cmd(
                "flatpak",
                &["update", "-y", "--noninteractive", &item.id],
                tx,
                ctx,
            ),
            ItemOp::Uninstall => stream_cmd(
                "flatpak",
                &["uninstall", "-y", "--noninteractive", &item.id],
                tx,
                ctx,
            ),
        },
        PackageSource::Pacman => match op {
            ItemOp::Install | ItemOp::Update => {
                stream_cmd("pkexec", &["pacman", "-S", "--noconfirm", &item.id], tx, ctx)
            }
            ItemOp::Uninstall => {
                stream_cmd("pkexec", &["pacman", "-Rns", "--noconfirm", &item.id], tx, ctx)
            }
        },
        PackageSource::Aur => {
            let helper = if backends.paru { "paru" } else { "yay" };
            match op {
                ItemOp::Install | ItemOp::Update => {
                    stream_cmd(helper, &["-S", "--noconfirm", &item.id], tx, ctx)
                }
                ItemOp::Uninstall => {
                    stream_cmd("pkexec", &["pacman", "-Rns", "--noconfirm", &item.id], tx, ctx)
                }
            }
        }
        PackageSource::Apt => match op {
            ItemOp::Install => {
                stream_cmd("pkexec", &["apt-get", "install", "-y", &item.id], tx, ctx)
            }
            ItemOp::Update => stream_cmd(
                "pkexec",
                &["apt-get", "install", "--only-upgrade", "-y", &item.id],
                tx,
                ctx,
            ),
            ItemOp::Uninstall => {
                stream_cmd("pkexec", &["apt-get", "remove", "-y", &item.id], tx, ctx)
            }
        },
        PackageSource::Dnf => match op {
            ItemOp::Install => stream_cmd("pkexec", &["dnf", "install", "-y", &item.id], tx, ctx),
            ItemOp::Update => stream_cmd("pkexec", &["dnf", "upgrade", "-y", &item.id], tx, ctx),
            ItemOp::Uninstall => stream_cmd("pkexec", &["dnf", "remove", "-y", &item.id], tx, ctx),
        },
    }
}

fn stream_cmd(
    program: &str,
    args: &[&str],
    tx: &Sender<BackendEvent>,
    ctx: &eframe::egui::Context,
) -> bool {
    let _ = tx.send(BackendEvent::TaskLog {
        line: format!("$ {} {}", program, args.join(" ")),
    });
    ctx.request_repaint();

    let mut child = match Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = tx.send(BackendEvent::TaskLog {
                line: format!("Failed to start {program}: {e}"),
            });
            ctx.request_repaint();
            return false;
        }
    };

    if let Some(stdout) = child.stdout.take() {
        let tx2 = tx.clone();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx2.send(BackendEvent::TaskLog { line });
                ctx2.request_repaint();
            }
        });
    }

    if let Some(stderr) = child.stderr.take() {
        let tx2 = tx.clone();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx2.send(BackendEvent::TaskLog { line });
                ctx2.request_repaint();
            }
        });
    }

    match child.wait() {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

fn load_flatpak_installed() -> Vec<AppItem> {
    let out = Command::new("flatpak")
        .args(["list", "--app", "--columns=application,name,version,origin"])
        .output();
    let Ok(out) = out else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut items = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            continue;
        }
        let id = cols[0].trim().to_string();
        let name = cols[1].trim().to_string();
        if id.is_empty() || name.is_empty() {
            continue;
        }
        let version = cols.get(2).map(|s| s.trim()).unwrap_or("").to_string();
        let origin = cols.get(3).map(|s| s.trim()).unwrap_or("flathub").to_string();
        items.push(AppItem {
            accent_rgb: color_from_str(&name),
            id,
            name,
            summary: format!("Flatpak application ({origin})"),
            version,
            new_version: None,
            source: PackageSource::Flatpak,
            origin,
            installed: true,
            category: "Apps".to_string(),
            publisher: "Flathub".to_string(),
            rating: "4.6",
        });
    }
    items
}

fn load_flatpak_updates() -> Vec<AppItem> {
    let out = Command::new("flatpak")
        .args([
            "remote-ls",
            "--updates",
            "--columns=application,name,version,origin",
        ])
        .output();
    let Ok(out) = out else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut items = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 2 {
            continue;
        }
        let id = cols[0].trim().to_string();
        let name = cols[1].trim().to_string();
        if id.is_empty() {
            continue;
        }
        let new_ver = cols
            .get(2)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| Some("Update available".to_string()));
        let origin = cols.get(3).map(|s| s.trim()).unwrap_or("flathub").to_string();
        items.push(AppItem {
            accent_rgb: color_from_str(&name),
            id,
            name: if name.is_empty() {
                "Flatpak Package".to_string()
            } else {
                name
            },
            summary: "Flatpak update available".to_string(),
            version: String::new(),
            new_version: new_ver,
            source: PackageSource::Flatpak,
            origin,
            installed: true,
            category: "Updates".to_string(),
            publisher: "Flathub".to_string(),
            rating: "4.7",
        });
    }
    items
}

fn load_pacman_installed() -> Vec<AppItem> {
    let out = Command::new("pacman").args(["-Qe"]).output();
    let Ok(out) = out else { return Vec::new() };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut items = Vec::new();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        let Some(id) = parts.next() else { continue };
        let ver = parts.next().unwrap_or("");
        items.push(AppItem {
            accent_rgb: color_from_str(id),
            id: id.to_string(),
            name: pretty_pkg_name(id),
            summary: "Explicitly installed system package".to_string(),
            version: ver.to_string(),
            new_version: None,
            source: PackageSource::Pacman,
            origin: "local".to_string(),
            installed: true,
            category: "System".to_string(),
            publisher: "Arch / System Repo".to_string(),
            rating: "4.5",
        });
    }
    items
}

fn load_pacman_updates(backends: &DetectedBackends) -> Vec<AppItem> {
    let mut items = Vec::new();
    let cmd_out = if backends.checkupdates {
        Command::new("checkupdates").output()
    } else {
        Command::new("pacman").args(["-Qu"]).output()
    };
    if let Ok(out) = cmd_out {
        let text = String::from_utf8_lossy(&out.stdout);
        for line in text.lines() {
            // format: pkg old_ver -> new_ver
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 && parts[2] == "->" {
                let id = parts[0].to_string();
                items.push(AppItem {
                    accent_rgb: color_from_str(&id),
                    name: pretty_pkg_name(&id),
                    id,
                    summary: "System package update".to_string(),
                    version: parts[1].to_string(),
                    new_version: Some(parts[3].to_string()),
                    source: PackageSource::Pacman,
                    origin: "repo".to_string(),
                    installed: true,
                    category: "Updates".to_string(),
                    publisher: "System Repo".to_string(),
                    rating: "4.8",
                });
            }
        }
    }

    if backends.paru || backends.yay {
        let helper = if backends.paru { "paru" } else { "yay" };
        if let Ok(out) = Command::new(helper).args(["-Qua"]).output() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 && parts[2] == "->" {
                    let id = parts[0].to_string();
                    items.push(AppItem {
                        accent_rgb: color_from_str(&id),
                        name: pretty_pkg_name(&id),
                        id,
                        summary: "AUR package update".to_string(),
                        version: parts[1].to_string(),
                        new_version: Some(parts[3].to_string()),
                        source: PackageSource::Aur,
                        origin: "aur".to_string(),
                        installed: true,
                        category: "Updates".to_string(),
                        publisher: "AUR".to_string(),
                        rating: "4.4",
                    });
                }
            }
        }
    }
    items
}

fn load_apt_updates() -> Vec<AppItem> {
    let Ok(out) = Command::new("apt").args(["list", "--upgradable"]).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut items = Vec::new();
    for line in text.lines().skip(1) {
        let Some((id_part, rest)) = line.split_once('/') else {
            continue;
        };
        let new_ver = rest.split_whitespace().nth(1).unwrap_or("update").to_string();
        let id = id_part.trim().to_string();
        items.push(AppItem {
            accent_rgb: color_from_str(&id),
            name: pretty_pkg_name(&id),
            id,
            summary: "APT package update".to_string(),
            version: String::new(),
            new_version: Some(new_ver),
            source: PackageSource::Apt,
            origin: "apt".to_string(),
            installed: true,
            category: "Updates".to_string(),
            publisher: "Debian / Ubuntu".to_string(),
            rating: "4.5",
        });
    }
    items
}

fn load_dnf_updates() -> Vec<AppItem> {
    let Ok(out) = Command::new("dnf").args(["check-update", "-q"]).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut items = Vec::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 3 {
            let id = parts[0].split('.').next().unwrap_or(parts[0]).to_string();
            items.push(AppItem {
                accent_rgb: color_from_str(&id),
                name: pretty_pkg_name(&id),
                id,
                summary: "DNF package update".to_string(),
                version: String::new(),
                new_version: Some(parts[1].to_string()),
                source: PackageSource::Dnf,
                origin: parts[2].to_string(),
                installed: true,
                category: "Updates".to_string(),
                publisher: "Fedora".to_string(),
                rating: "4.5",
            });
        }
    }
    items
}

fn search_flatpak(query: &str, installed_ids: &HashSet<String>) -> Vec<AppItem> {
    let Ok(out) = Command::new("flatpak")
        .args([
            "search",
            "--columns=name,description,application,version,remotes",
            query,
        ])
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut items = Vec::new();
    for line in text.lines().take(35) {
        if line.starts_with("No matches") {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        if cols.len() < 3 {
            continue;
        }
        let name = cols[0].trim().to_string();
        let summary = cols[1].trim().to_string();
        let id = cols[2].trim().to_string();
        if id.is_empty() || name.is_empty() {
            continue;
        }
        let version = cols.get(3).map(|s| s.trim()).unwrap_or("").to_string();
        let origin = cols
            .get(4)
            .map(|s| s.split(',').next().unwrap_or("flathub").trim())
            .unwrap_or("flathub")
            .to_string();
        items.push(AppItem {
            accent_rgb: color_from_str(&name),
            installed: installed_ids.contains(&id),
            id,
            name,
            summary,
            version,
            new_version: None,
            source: PackageSource::Flatpak,
            origin,
            category: "Apps".to_string(),
            publisher: "Flathub".to_string(),
            rating: "4.7",
        });
    }
    items
}

fn search_pacman(query: &str, installed_ids: &HashSet<String>) -> Vec<AppItem> {
    let Ok(out) = Command::new("pacman").args(["-Ss", query]).output() else {
        return Vec::new();
    };
    parse_pacman_ss(&String::from_utf8_lossy(&out.stdout), PackageSource::Pacman, installed_ids)
}

fn search_aur(helper: &str, query: &str, installed_ids: &HashSet<String>) -> Vec<AppItem> {
    let Ok(out) = Command::new(helper)
        .args(["-Ssa", query])
        .output()
    else {
        return Vec::new();
    };
    parse_pacman_ss(&String::from_utf8_lossy(&out.stdout), PackageSource::Aur, installed_ids)
}

fn parse_pacman_ss(
    text: &str,
    source: PackageSource,
    installed_ids: &HashSet<String>,
) -> Vec<AppItem> {
    let mut items = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() && items.len() < 35 {
        let header = lines[i];
        if !header.starts_with(' ') && header.contains('/') {
            let desc = lines
                .get(i + 1)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if let Some((repo, rest)) = header.split_once('/') {
                let mut parts = rest.split_whitespace();
                if let Some(id) = parts.next() {
                    let ver = parts.next().unwrap_or("").to_string();
                    let installed = installed_ids.contains(id) || header.contains("[installed");
                    items.push(AppItem {
                        accent_rgb: color_from_str(id),
                        id: id.to_string(),
                        name: pretty_pkg_name(id),
                        summary: desc,
                        version: ver,
                        new_version: None,
                        source,
                        origin: repo.to_string(),
                        installed,
                        category: "Packages".to_string(),
                        publisher: repo.to_string(),
                        rating: "4.5",
                    });
                }
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    items
}

fn search_apt(query: &str, installed_ids: &HashSet<String>) -> Vec<AppItem> {
    let Ok(out) = Command::new("apt-cache").args(["search", query]).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut items = Vec::new();
    for line in text.lines().take(35) {
        if let Some((id, desc)) = line.split_once(" - ") {
            let id = id.trim().to_string();
            items.push(AppItem {
                accent_rgb: color_from_str(&id),
                installed: installed_ids.contains(&id),
                name: pretty_pkg_name(&id),
                id,
                summary: desc.trim().to_string(),
                version: String::new(),
                new_version: None,
                source: PackageSource::Apt,
                origin: "apt".to_string(),
                category: "Packages".to_string(),
                publisher: "APT Repo".to_string(),
                rating: "4.5",
            });
        }
    }
    items
}

fn search_dnf(query: &str, installed_ids: &HashSet<String>) -> Vec<AppItem> {
    let Ok(out) = Command::new("dnf").args(["search", "-q", query]).output() else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut items = Vec::new();
    for line in text.lines().take(35) {
        if let Some((left, desc)) = line.split_once(" : ") {
            let id = left.split('.').next().unwrap_or(left).trim().to_string();
            items.push(AppItem {
                accent_rgb: color_from_str(&id),
                installed: installed_ids.contains(&id),
                name: pretty_pkg_name(&id),
                id,
                summary: desc.trim().to_string(),
                version: String::new(),
                new_version: None,
                source: PackageSource::Dnf,
                origin: "dnf".to_string(),
                category: "Packages".to_string(),
                publisher: "DNF Repo".to_string(),
                rating: "4.5",
            });
        }
    }
    items
}

pub fn featured_catalog(backends: &DetectedBackends, installed_ids: &HashSet<String>) -> Vec<AppItem> {
    struct Entry {
        fp_id: &'static str,
        native_id: &'static str,
        name: &'static str,
        summary: &'static str,
        category: &'static str,
        publisher: &'static str,
        rgb: [u8; 3],
        rating: &'static str,
    }

    const ENTRIES: &[Entry] = &[
        Entry {
            fp_id: "org.mozilla.firefox",
            native_id: "firefox",
            name: "Firefox",
            summary: "Fast, private and secure web browser from Mozilla",
            category: "Apps",
            publisher: "Mozilla",
            rgb: [230, 96, 30],
            rating: "4.8",
        },
        Entry {
            fp_id: "com.obsproject.Studio",
            native_id: "obs-studio",
            name: "OBS Studio",
            summary: "Free and open source software for video recording and live streaming",
            category: "Apps",
            publisher: "OBS Project",
            rgb: [44, 48, 56],
            rating: "4.9",
        },
        Entry {
            fp_id: "com.valvesoftware.Steam",
            native_id: "steam",
            name: "Steam",
            summary: "Play, discuss, and create games with the world's largest PC gaming platform",
            category: "Gaming",
            publisher: "Valve Corporation",
            rgb: [27, 40, 56],
            rating: "4.8",
        },
        Entry {
            fp_id: "com.discordapp.Discord",
            native_id: "discord",
            name: "Discord",
            summary: "Voice, video, and text chat for communities and friends",
            category: "Apps",
            publisher: "Discord Inc.",
            rgb: [88, 101, 242],
            rating: "4.6",
        },
        Entry {
            fp_id: "org.videolan.VLC",
            native_id: "vlc",
            name: "VLC media player",
            summary: "Plays almost every video and audio format out of the box",
            category: "Apps",
            publisher: "VideoLAN",
            rgb: [232, 108, 22],
            rating: "4.8",
        },
        Entry {
            fp_id: "org.gimp.GIMP",
            native_id: "gimp",
            name: "GIMP",
            summary: "GNU Image Manipulation Program for photo retouching and image editing",
            category: "Apps",
            publisher: "GIMP Team",
            rgb: [92, 82, 70],
            rating: "4.7",
        },
        Entry {
            fp_id: "org.blender.Blender",
            native_id: "blender",
            name: "Blender",
            summary: "Free and open 3D creation suite for modeling, animation, and rendering",
            category: "Apps",
            publisher: "Blender Foundation",
            rgb: [234, 118, 0],
            rating: "4.9",
        },
        Entry {
            fp_id: "com.spotify.Client",
            native_id: "spotify-launcher",
            name: "Spotify",
            summary: "Stream millions of songs and podcasts on your desktop",
            category: "Apps",
            publisher: "Spotify AB",
            rgb: [29, 185, 84],
            rating: "4.6",
        },
        Entry {
            fp_id: "org.prismlauncher.PrismLauncher",
            native_id: "prismlauncher",
            name: "Prism Launcher",
            summary: "Custom Minecraft launcher with modpack and instance management",
            category: "Gaming",
            publisher: "Prism Launcher",
            rgb: [67, 160, 71],
            rating: "4.9",
        },
        Entry {
            fp_id: "com.visualstudio.code",
            native_id: "code",
            name: "Visual Studio Code",
            summary: "Code editing. Redefined. Built on open source with extensions",
            category: "Apps",
            publisher: "Macrohard",
            rgb: [0, 122, 204],
            rating: "4.8",
        },
        Entry {
            fp_id: "net.lutris.Lutris",
            native_id: "lutris",
            name: "Lutris",
            summary: "Open gaming platform for Linux with Wine, Proton, and emulators",
            category: "Gaming",
            publisher: "Lutris",
            rgb: [215, 138, 36],
            rating: "4.7",
        },
        Entry {
            fp_id: "org.kde.kdenlive",
            native_id: "kdenlive",
            name: "Kdenlive",
            summary: "Non-linear video editor with multi-track timeline and effects",
            category: "Apps",
            publisher: "KDE",
            rgb: [82, 148, 226],
            rating: "4.7",
        },
    ];

    let native_source = if backends.pacman {
        PackageSource::Pacman
    } else if backends.apt {
        PackageSource::Apt
    } else if backends.dnf {
        PackageSource::Dnf
    } else {
        PackageSource::Flatpak
    };

    ENTRIES
        .iter()
        .map(|e| {
            let use_flatpak = backends.flatpak;
            let id = if use_flatpak { e.fp_id } else { e.native_id };
            let source = if use_flatpak {
                PackageSource::Flatpak
            } else {
                native_source
            };
            let installed = installed_ids.contains(e.fp_id) || installed_ids.contains(e.native_id);
            AppItem {
                id: id.to_string(),
                name: e.name.to_string(),
                summary: e.summary.to_string(),
                version: "Latest".to_string(),
                new_version: None,
                source,
                origin: if use_flatpak { "flathub" } else { "repo" }.to_string(),
                installed,
                category: e.category.to_string(),
                publisher: e.publisher.to_string(),
                accent_rgb: e.rgb,
                rating: e.rating,
            }
        })
        .collect()
}

fn pretty_pkg_name(id: &str) -> String {
    id.split(['-', '_'])
        .filter(|s| !s.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn color_from_str(s: &str) -> [u8; 3] {
    const PALETTE: &[[u8; 3]] = &[
        [0, 120, 215],
        [16, 124, 16],
        [136, 23, 152],
        [209, 52, 56],
        [0, 130, 114],
        [202, 80, 16],
        [74, 84, 89],
        [0, 99, 177],
    ];
    let hash = s.bytes().fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b as usize));
    PALETTE[hash % PALETTE.len()]
}
