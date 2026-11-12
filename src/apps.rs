//! Installed applications, read from freedesktop `.desktop` files.

use std::{collections::HashSet, path::PathBuf};

#[derive(Clone, Debug)]
pub struct App {
    /// Desktop file id without `.desktop`, e.g. `org.kde.konsole`.
    pub id: String,
    pub wm_class: String,
    pub name: String,
    pub exec: String,
    pub icon: String,
    pub terminal: bool,
}

pub fn scan() -> Vec<App> {
    let dirs: Vec<PathBuf> = crate::icons::data_dirs().into_iter().map(|d| d.join("applications")).collect();

    // Earlier dirs override later ones with the same desktop-file id.
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "desktop") {
                continue;
            }
            let id = path.file_name().unwrap().to_string_lossy().into_owned();
            if !seen.insert(id) {
                continue;
            }
            if let Some(mut app) = std::fs::read_to_string(&path).ok().and_then(|s| parse(&s)) {
                app.id = path.file_stem().unwrap().to_string_lossy().into_owned();
                apps.push(app);
            }
        }
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    tracing::info!("found {} apps", apps.len());
    apps
}

fn parse(text: &str) -> Option<App> {
    let mut in_entry = false;
    let (mut name, mut exec, mut icon, mut wm_class) = (None, None, String::new(), String::new());
    let (mut terminal, mut hidden, mut is_app) = (false, false, false);
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        match k.trim() {
            "Name" => name = Some(v.trim().to_string()),
            "Exec" => exec = Some(v.trim().to_string()),
            "Icon" => icon = v.trim().to_string(),
            "StartupWMClass" => wm_class = v.trim().to_string(),
            "Terminal" => terminal = v.trim() == "true",
            "NoDisplay" | "Hidden" => hidden |= v.trim() == "true",
            "Type" => is_app = v.trim() == "Application",
            // We show KDE's apps (we stand in for its shell) and our own.
            "OnlyShowIn" => hidden |= !v.split(';').any(|d| matches!(d, "KDE" | "VIDE-DE" | "")),
            "NotShowIn" => hidden |= v.split(';').any(|d| d == "VIDE-DE"),
            _ => {}
        }
    }
    if hidden || !is_app {
        return None;
    }
    Some(App { id: String::new(), wm_class, name: name?, exec: strip_field_codes(&exec?), icon, terminal })
}

/// `firefox %u` → `firefox`. We launch apps without file arguments.
fn strip_field_codes(exec: &str) -> String {
    let mut out = String::new();
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.next() {
                Some('%') => out.push('%'),
                _ => {}
            }
        } else {
            out.push(c);
        }
    }
    // Flatpak's file-forwarding markers (`@@u`, `@@`) wrap file arguments we never pass.
    out.split_whitespace().filter(|w| !w.starts_with("@@")).collect::<Vec<_>>().join(" ")
}

/// Case-insensitive match on name or command.
pub fn matches(app: &App, query: &str) -> bool {
    let q = query.to_lowercase();
    app.name.to_lowercase().contains(&q) || app.exec.to_lowercase().contains(&q)
}

/// The icon name for a running window's app_id, via its desktop entry if we can find one.
pub fn icon_for_app_id(apps: &[App], app_id: &str) -> String {
    if app_id.is_empty() {
        return String::new();
    }
    let lower = app_id.to_lowercase();
    apps.iter()
        .find(|a| a.id == app_id || a.id.to_lowercase() == lower || a.wm_class.to_lowercase() == lower)
        .or_else(|| apps.iter().find(|a| a.id.rsplit('.').next().is_some_and(|l| l.to_lowercase() == lower)))
        .map(|a| a.icon.clone())
        .unwrap_or_else(|| app_id.to_string())
}

/// Register our own apps (About VIDE-DE) like any installed app: a .desktop file and
/// an icon in the user's data dir, so our start menu and search index them. Marked
/// OnlyShowIn=VIDE-DE so other desktops don't list them.
pub fn install_builtin() {
    let Ok(home) = std::env::var("HOME") else { return };
    let Some(bin_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) else { return };
    let about = bin_dir.join("vide-about");
    if !about.exists() {
        return;
    }
    let share = PathBuf::from(&home).join(".local/share");
    let icon = share.join("icons/hicolor/512x512/apps/vide-de.png");
    let desktop = share.join("applications/vide-about.desktop");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=About VIDE-DE\nComment=System information for the vibecoded desktop\n\
         Exec=\"{}\"\nIcon=vide-de\nStartupWMClass=vide-about\nCategories=System;Settings;\nOnlyShowIn=VIDE-DE;\n",
        about.display()
    );
    let write = |path: &PathBuf, bytes: &[u8]| {
        // Only touch the file when it changed, so we don't churn other launchers' caches.
        if std::fs::read(path).ok().as_deref() != Some(bytes) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = std::fs::write(path, bytes) {
                tracing::warn!("couldn't install {}: {e}", path.display());
            }
        }
    };
    write(&icon, include_bytes!("../assets/logo-512.png"));
    write(&desktop, entry.as_bytes());
}
