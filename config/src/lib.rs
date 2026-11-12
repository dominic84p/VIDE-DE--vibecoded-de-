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
    /// Desktop wallpaper image (drawn by vide-desktop, scaled to fill).
    pub wallpaper: PathBuf,
    /// "Ultra Performance Mode": let fullscreen apps' frames go straight to the screen
    /// (direct scanout) instead of being composited. Off by default: some NVIDIA setups
    /// flicker with it.
    pub direct_scanout: bool,
    /// Monitor mode as "WIDTHxHEIGHT@HZ" (e.g. "2560x1440@144"); empty = the monitor's
    /// preferred mode. Applies to every monitor that supports it.
    pub display_mode: String,
    /// UI scale for outputs, 1.0 = 100%.
    pub display_scale: f64,
    /// Night light: warm the screen to cut blue light.
    pub night_light: bool,
    /// Night light warmth, 0.0 (none) to 1.0 (strongest).
    pub night_light_strength: f64,
    /// Lock the screen after this many idle minutes; 0 = never.
    pub idle_lock_minutes: u32,
}

impl Default for Config {
    fn default() -> Self {
        // First run: start from what the user already set up in KDE.
        let (mouse_flat, mouse_speed) = kde_mouse().unwrap_or((true, 0.0));
        Self {
            hide_kde_apps: false,
            mouse_flat,
            mouse_speed,
            wallpaper: wallpapers_dir().join("bloom.png"),
            direct_scanout: false,
            display_mode: String::new(),
            display_scale: 1.0,
            night_light: false,
            night_light_strength: 0.5,
            idle_lock_minutes: 0,
        }
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
                "wallpaper" if !v.is_empty() => c.wallpaper = PathBuf::from(v),
                "direct_scanout" => c.direct_scanout = v == "true",
                "display_mode" => c.display_mode = v.to_string(),
                "display_scale" => c.display_scale = v.parse::<f64>().unwrap_or(1.0).clamp(0.5, 3.0),
                "night_light" => c.night_light = v == "true",
                "night_light_strength" => c.night_light_strength = v.parse::<f64>().unwrap_or(0.5).clamp(0.0, 1.0),
                "idle_lock_minutes" => c.idle_lock_minutes = v.parse().unwrap_or(0),
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
             hide_kde_apps={}\nmouse_flat={}\nmouse_speed={:.2}\nwallpaper={}\ndirect_scanout={}\n\
             display_mode={}\ndisplay_scale={:.2}\nnight_light={}\nnight_light_strength={:.2}\nidle_lock_minutes={}\n",
            self.hide_kde_apps,
            self.mouse_flat,
            self.mouse_speed,
            self.wallpaper.display(),
            self.direct_scanout,
            self.display_mode,
            self.display_scale,
            self.night_light,
            self.night_light_strength,
            self.idle_lock_minutes
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

/// Where VIDE-DE installs its bundled wallpapers.
pub fn wallpapers_dir() -> PathBuf {
    let data = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".local/share"));
    data.join("vide-de/wallpapers")
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
