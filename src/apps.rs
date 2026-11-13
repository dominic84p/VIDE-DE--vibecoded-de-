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
    /// Part of KDE (org.kde.* or Categories has KDE), for the "hide KDE apps" setting.
    pub kde: bool,
}

pub fn scan() -> Vec<App> {
    let dirs: Vec<PathBuf> = crate::icons::data_dirs().into_iter().map(|d| d.join("applications")).collect();

    // Earlier dirs override later ones with the same desktop-file id.
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        scan_dir(&dir, 0, &dir, &mut seen, &mut apps);
    }
    apps.sort_by_key(|a| a.name.to_lowercase());
    tracing::info!("found {} apps", apps.len());
    apps
}

fn scan_dir(dir: &PathBuf, depth: u32, root: &PathBuf, seen: &mut HashSet<String>, apps: &mut Vec<App>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            if depth < 2 {
                scan_dir(&path, depth + 1, root, seen, apps);
            }
            continue;
        }
        if path.extension().is_none_or(|x| x != "desktop") {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(&path);
        let id = rel.to_string_lossy().replace('/', "-");
        if !seen.insert(id) {
            continue;
        }
        if let Some(mut app) = std::fs::read_to_string(&path).ok().and_then(|s| parse(&s)) {
            app.id = rel.with_extension("").to_string_lossy().replace('/', "-");
            app.kde |= app.id.starts_with("org.kde.");
            apps.push(app);
        }
    }
}

fn parse(text: &str) -> Option<App> {
    let mut in_entry = false;
    let (mut name, mut exec, mut icon, mut wm_class) = (None, None, String::new(), String::new());
    let (mut terminal, mut hidden, mut is_app, mut kde) = (false, false, false, false);
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
            "Categories" => kde |= v.split(';').any(|c| c == "KDE"),
            // We show KDE's apps (we stand in for its shell) and our own.
            "OnlyShowIn" => hidden |= !v.split(';').any(|d| matches!(d, "KDE" | "VIDE-DE" | "")),
            "NotShowIn" => hidden |= v.split(';').any(|d| d == "VIDE-DE"),
            _ => {}
        }
    }
    if hidden || !is_app {
        return None;
    }
    Some(App { id: String::new(), wm_class, name: name?, exec: strip_field_codes(&exec?), icon, terminal, kde })
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

/// Lowercase letters and digits only: "Leather Client", "leather-client" and
/// "leather_client" all become "leatherclient".
fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The installed app a running window belongs to, judged by its app_id / X11 class.
pub fn match_app<'a>(apps: &'a [App], app_id: &str) -> Option<&'a App> {
    if app_id.is_empty() {
        return None;
    }
    let want = norm(app_id);
    let last = |id: &str| norm(id.rsplit('.').next().unwrap_or(id));
    let exe = |exec: &str| {
        let mut parts = exec.split_whitespace().map(|s| s.trim_matches('"'));
        let first = loop {
            let Some(tok) = parts.next() else { return String::new() };
            let base = tok.rsplit('/').next().unwrap_or(tok);
            if base == "env" || (tok.contains('=') && !tok.starts_with('/')) {
                continue;
            }
            break base;
        };
        norm(first.split('.').next().unwrap_or(first))
    };
    // Most to least specific, so a weak match never beats a strong one.
    apps.iter()
        .find(|a| norm(&a.id) == want || (!a.wm_class.is_empty() && norm(&a.wm_class) == want))
        .or_else(|| apps.iter().find(|a| last(&a.id) == want || last(app_id) == norm(&a.id)))
        .or_else(|| apps.iter().find(|a| norm(&a.name) == want))
        .or_else(|| apps.iter().find(|a| !a.exec.contains("flatpak run") && exe(&a.exec) == want))
}

/// The icon name for a running window's app_id, via its desktop entry if we can find one.
/// Without one, the app_id itself: the theme may have it, or the window supplied its own.
pub fn icon_for_app_id(apps: &[App], app_id: &str) -> String {
    match_app(apps, app_id)
        .map(|a| a.icon.clone())
        .filter(|i| !i.is_empty())
        .unwrap_or_else(|| app_id.to_string())
}

/// Our own apps, registered like any installed app (a .desktop file in the user's
/// data dir) so the start menu, search and taskbar treat them normally.
/// (binary, desktop id, name, comment, icon, categories)
const BUILTIN: &[(&str, &str, &str, &str, &str, &str)] = &[
    ("vide-about", "vide-about", "About VIDE-DE", "System information for the vibecoded desktop", "vide-de", "System;"),
    ("vide-settings", "vide-settings", "Settings", "Mouse, Start menu and other VIDE-DE options", "preferences-system", "Settings;System;"),
    ("powerslop", "powerslop", "PowerSlop", "A shell, but vibecoded", "utilities-terminal", "System;TerminalEmulator;"),
    ("task-mangler", "task-mangler", "Task Mangler", "See what's running. Like Task Manager, but it mangles", "utilities-system-monitor", "System;Monitor;"),
    ("snip-and-sketchy", "snip-and-sketchy", "Snip & Sketchy", "Mark up your screenshots. Like Snip & Sketch, but sketchier", "applets-screenshooter", "Utility;Graphics;"),
    ("file-exploder", "file-exploder", "File Exploder", "Your files. Like File Explorer, but it might go off", "system-file-manager", "System;FileManager;"),
    ("window-shopping", "window-shopping", "Window Shopping", "Get apps, Flatpaks, and updates for your PC", "system-software-install", "System;PackageManager;"),
];

/// Our bundled wallpapers, installed for vide-desktop and the Settings picker.
const WALLPAPERS: &[(&str, &[u8])] = &[
    ("bloom.png", include_bytes!("../assets/wallpapers/bloom.png")),
    ("aurora.png", include_bytes!("../assets/wallpapers/aurora.png")),
    ("sunset.png", include_bytes!("../assets/wallpapers/sunset.png")),
    ("midnight.png", include_bytes!("../assets/wallpapers/midnight.png")),
    ("ember.png", include_bytes!("../assets/wallpapers/ember.png")),
    ("vibe.png", include_bytes!("../assets/wallpapers/vibe.png")),
];

/// Our notification sounds (made by scripts/make-sounds.py).
const SOUNDS: &[(&str, &[u8])] = &[
    ("ding.wav", include_bytes!("../assets/sounds/ding.wav")),
    ("chime.wav", include_bytes!("../assets/sounds/chime.wav")),
    ("pop.wav", include_bytes!("../assets/sounds/pop.wav")),
    ("bell.wav", include_bytes!("../assets/sounds/bell.wav")),
    ("glass.wav", include_bytes!("../assets/sounds/glass.wav")),
];

/// Install the built-ins that exist next to our binary. Marked OnlyShowIn=VIDE-DE so
/// other desktops don't list them. Files are only rewritten when they changed.
pub fn install_builtin() {
    let Ok(home) = std::env::var("HOME") else { return };
    let Some(bin_dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) else { return };
    let share = PathBuf::from(&home).join(".local/share");
    let write = |path: &PathBuf, bytes: &[u8]| {
        if std::fs::read(path).ok().as_deref() != Some(bytes) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(e) = std::fs::write(path, bytes) {
                tracing::warn!("couldn't install {}: {e}", path.display());
            }
        }
    };
    write(&share.join("icons/hicolor/512x512/apps/vide-de.png"), include_bytes!("../assets/logo-512.png"));
    let walls = vide_config::wallpapers_dir();
    for (name, bytes) in WALLPAPERS {
        write(&walls.join(name), bytes);
    }
    let sounds = vide_config::sounds_dir();
    for (name, bytes) in SOUNDS {
        write(&sounds.join(name), bytes);
    }
    for (bin, id, name, comment, icon, categories) in BUILTIN {
        let exe = bin_dir.join(bin);
        if !exe.exists() {
            continue;
        }
        let entry = format!(
            "[Desktop Entry]\nType=Application\nName={name}\nComment={comment}\nExec=\"{}\"\nIcon={icon}\n\
             StartupWMClass={id}\nCategories={categories}\nOnlyShowIn=VIDE-DE;\n",
            exe.display()
        );
        write(&share.join(format!("applications/{id}.desktop")), entry.as_bytes());
    }
}
