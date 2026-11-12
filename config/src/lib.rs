//! VIDE-DE settings: one small `key=value` file at ~/.config/vide-de/settings.conf,
//! written by the Settings app and watched by the compositor.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Leave KDE's own apps out of the Start menu list and search (they stay installed).
    pub hide_kde_apps: bool,
    /// Flat = no mouse acceleration (what gamers want); otherwise libinput's adaptive curve.
    pub mouse_flat: bool,
    /// Pointer speed, -1.0 (slowest) to 1.0 (fastest); 0 is the device default.
    pub mouse_speed: f64,
}

impl Default for Config {
    fn default() -> Self {
        // First run: start from what the user already set up in KDE.
        let (mouse_flat, mouse_speed) = kde_mouse().unwrap_or((true, 0.0));
        Self { hide_kde_apps: false, mouse_flat, mouse_speed }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        let base = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"));
        base.join("vide-de/settings.conf")
    }

    pub fn load() -> Self {
        let mut c = Self::default();
        let Ok(text) = std::fs::read_to_string(Self::path()) else { return c };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "hide_kde_apps" => c.hide_kde_apps = v == "true",
                "mouse_flat" => c.mouse_flat = v == "true",
                "mouse_speed" => c.mouse_speed = v.parse::<f64>().unwrap_or(c.mouse_speed).clamp(-1.0, 1.0),
                _ => {}
            }
        }
        c
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = format!(
            "# VIDE-DE settings. Edited by the Settings app; changes apply live.\n\
             hide_kde_apps={}\nmouse_flat={}\nmouse_speed={:.2}\n",
            self.hide_kde_apps, self.mouse_flat, self.mouse_speed
        );
        // Write-then-rename so the compositor never reads a half-written file.
        let tmp = path.with_extension("conf.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }

    /// Last-modified time, for cheap change polling.
    pub fn mtime() -> Option<std::time::SystemTime> {
        std::fs::metadata(Self::path()).and_then(|m| m.modified()).ok()
    }
}

/// KDE's mouse settings (kcminputrc): profile 1 = flat, 2 = adaptive; speed -1..1.
/// Takes the first pointer device listed.
fn kde_mouse() -> Option<(bool, f64)> {
    let home = std::env::var("HOME").ok()?;
    let rc = std::fs::read_to_string(format!("{home}/.config/kcminputrc")).ok()?;
    let (mut flat, mut speed) = (None, None);
    for line in rc.lines() {
        if let Some(v) = line.strip_prefix("PointerAccelerationProfile=") {
            flat.get_or_insert(v.trim() == "1");
        } else if let Some(v) = line.strip_prefix("PointerAcceleration=") {
            speed = speed.or(v.trim().parse().ok());
        }
    }
    Some((flat.unwrap_or(true), speed.unwrap_or(0.0)))
}
