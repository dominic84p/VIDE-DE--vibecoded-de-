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
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(format!("{home}/.local/share/applications")));
        dirs.push(PathBuf::from(format!("{home}/.local/share/flatpak/exports/share/applications")));
    }
    dirs.push("/var/lib/flatpak/exports/share/applications".into());
    dirs.push("/usr/local/share/applications".into());
    dirs.push("/usr/share/applications".into());

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
            "OnlyShowIn" => hidden |= !v.split(';').any(|d| d == "KDE" || d.is_empty()),
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
