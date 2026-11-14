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
    /// Touchpad tap-to-click.
    pub tap_to_click: bool,
    /// Natural (reverse) scrolling.
    pub natural_scroll: bool,
    /// Disable touchpad while typing.
    pub disable_while_typing: bool,
    /// XKB keyboard layout (e.g. "us", "de"); empty = system default.
    pub xkb_layout: String,
    /// XKB keyboard variant; empty = default.
    pub xkb_variant: String,
    /// XKB options (e.g. "caps:escape,grp:alt_shift_toggle"); empty = none.
    pub xkb_options: String,
    /// Key repeat delay in ms.
    pub key_repeat_delay: i32,
    /// Key repeat rate in Hz.
    pub key_repeat_rate: i32,
    /// Show a popup for each new notification (they always land in the Action Center).
    pub toasts: bool,
    /// How long a notification popup stays, in seconds, unless the app asks otherwise.
    pub toast_seconds: u32,
    /// Taskbar clock in 24-hour time.
    pub clock_24h: bool,
    /// Windoze 10's "default Windoze mode": light taskbar, Start, Action Center and menus
    /// instead of dark. Apps stay light either way.
    pub light_mode: bool,
    /// The sound a new notification plays: one of `SOUNDS` by id, or "none".
    pub notification_sound: String,
    /// Windoze's "Show animations": windows zoom in and out, Start and panels slide in.
    pub animations: bool,
    /// Per-app notification sound, as `app=always;other=never`: see `app_sound`.
    pub notification_sound_apps: String,
}

/// Whether an app's notifications make a sound.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppSound {
    /// Play our sound unless the app asks for silence (`suppress-sound`).
    AppsChoice,
    /// Play it even when the app asks for silence. The Claude app always does.
    Always,
    Never,
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
            tap_to_click: true,
            natural_scroll: false,
            disable_while_typing: true,
            xkb_layout: String::new(),
            xkb_variant: String::new(),
            xkb_options: String::new(),
            key_repeat_delay: 300,
            key_repeat_rate: 30,
            toasts: true,
            toast_seconds: 5,
            clock_24h: false,
            light_mode: false,
            notification_sound: "ding".into(),
            animations: true,
            notification_sound_apps: String::new(),
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
                "tap_to_click" => c.tap_to_click = v == "true",
                "natural_scroll" => c.natural_scroll = v == "true",
                "disable_while_typing" => c.disable_while_typing = v == "true",
                "xkb_layout" => c.xkb_layout = v.to_string(),
                "xkb_variant" => c.xkb_variant = v.to_string(),
                "xkb_options" => c.xkb_options = v.to_string(),
                "key_repeat_delay" => c.key_repeat_delay = v.parse().unwrap_or(300).clamp(100, 2000),
                "key_repeat_rate" => c.key_repeat_rate = v.parse().unwrap_or(30).clamp(5, 100),
                "toasts" => c.toasts = v == "true",
                "toast_seconds" => c.toast_seconds = v.parse().unwrap_or(5).clamp(2, 60),
                "clock_24h" => c.clock_24h = v == "true",
                "light_mode" => c.light_mode = v == "true",
                "notification_sound" if !v.is_empty() => c.notification_sound = v.to_string(),
                "animations" => c.animations = v == "true",
                "notification_sound_apps" => c.notification_sound_apps = v.to_string(),
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
             display_mode={}\ndisplay_scale={:.2}\nnight_light={}\nnight_light_strength={:.2}\nidle_lock_minutes={}\n\
             tap_to_click={}\nnatural_scroll={}\ndisable_while_typing={}\n\
             xkb_layout={}\nxkb_variant={}\nxkb_options={}\nkey_repeat_delay={}\nkey_repeat_rate={}\n\
             toasts={}\ntoast_seconds={}\nclock_24h={}\nlight_mode={}\nnotification_sound={}\nanimations={}\nnotification_sound_apps={}\n",
            self.hide_kde_apps,
            self.mouse_flat,
            self.mouse_speed,
            self.wallpaper.display(),
            self.direct_scanout,
            self.display_mode,
            self.display_scale,
            self.night_light,
            self.night_light_strength,
            self.idle_lock_minutes,
            self.tap_to_click,
            self.natural_scroll,
            self.disable_while_typing,
            self.xkb_layout,
            self.xkb_variant,
            self.xkb_options,
            self.key_repeat_delay,
            self.key_repeat_rate,
            self.toasts,
            self.toast_seconds,
            self.clock_24h,
            self.light_mode,
            self.notification_sound,
            self.animations,
            self.notification_sound_apps
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

/// Our notification sounds, (id, name in Settings). Installed to `sounds_dir()` as `<id>.wav`.
pub const SOUNDS: &[(&str, &str)] = &[
    ("ding", "Ding"),
    ("chime", "Chime"),
    ("pop", "Pop"),
    ("bell", "Bell"),
    ("glass", "Glass"),
    ("aurora", "Aurora"),
    ("marimba", "Marimba"),
    ("crystal", "Crystal"),
    ("kalimba", "Kalimba"),
    ("harp", "Harp"),
    ("zen", "Zen"),
    ("droplet", "Droplet"),
    ("orbit", "Orbit"),
    ("misery", "Misery"),
];

/// Where VIDE-DE installs its notification sounds.
pub fn sounds_dir() -> PathBuf {
    wallpapers_dir().with_file_name("sounds")
}

impl Config {
    /// An app's notification sound setting, by the id `notifying_app_id` gives it.
    pub fn app_sound(&self, app: &str) -> AppSound {
        let entry = self.notification_sound_apps.split(';').filter_map(|e| e.split_once('=')).find(|(a, _)| *a == app);
        match entry.map(|(_, s)| s) {
            Some("always") => AppSound::Always,
            Some("never") => AppSound::Never,
            _ => AppSound::AppsChoice,
        }
    }

    pub fn set_app_sound(&mut self, app: &str, sound: AppSound) {
        let mut entries: Vec<String> =
            self.notification_sound_apps.split(';').filter(|e| !e.is_empty() && e.split_once('=').is_none_or(|(a, _)| a != app)).map(str::to_string).collect();
        match sound {
            AppSound::Always => entries.push(format!("{app}=always")),
            AppSound::Never => entries.push(format!("{app}=never")),
            AppSound::AppsChoice => {}
        }
        self.notification_sound_apps = entries.join(";");
    }
}

/// How notifications from one app are told apart: its desktop entry when it says (the Claude
/// app sends `com.anthropic.Claude`), else its app name. `;` and `=` are separators in the
/// settings file, so they're dropped.
pub fn notifying_app_id(desktop_entry: Option<&str>, app_name: &str) -> String {
    desktop_entry.filter(|d| !d.is_empty()).unwrap_or(app_name).replace([';', '=', '\n'], "")
}

/// The apps that have sent notifications, (id, name), for Settings to list. The compositor
/// adds to it as notifications arrive.
pub fn notifying_apps_file() -> PathBuf {
    wallpapers_dir().with_file_name("notification-apps")
}

pub fn notifying_apps() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(notifying_apps_file()).unwrap_or_default();
    let mut apps: Vec<(String, String)> =
        text.lines().filter_map(|l| l.split_once('\t')).map(|(id, name)| (id.to_string(), name.to_string())).collect();
    apps.sort_by_key(|(_, n)| n.to_lowercase());
    apps
}

/// Note an app that sent a notification; a no-op when it's already listed.
pub fn remember_notifying_app(id: &str, name: &str) {
    if id.is_empty() || notifying_apps().iter().any(|(i, _)| i == id) {
        return;
    }
    let path = notifying_apps_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let name = name.replace(['\t', '\n'], " ");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = writeln!(f, "{id}\t{name}");
    }
}

/// Where your own notification sounds go: anything playable dropped here shows up in Settings.
pub fn custom_sounds_dir() -> PathBuf {
    sounds_dir().join("custom")
}

/// File types offered from `custom_sounds_dir()`; PipeWire's player reads all of them.
pub const CUSTOM_SOUND_TYPES: [&str; 6] = ["wav", "ogg", "oga", "flac", "mp3", "opus"];

/// Your own sounds, as (id, name in Settings): `custom:<file name>`, named after the file.
pub fn custom_sounds() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(custom_sounds_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()).is_some_and(|e| CUSTOM_SOUND_TYPES.contains(&e.to_ascii_lowercase().as_str())))
        .filter_map(|p| {
            let file = p.file_name()?.to_str()?.to_string();
            let stem = p.file_stem()?.to_string_lossy().replace(['_', '-'], " ");
            Some((format!("custom:{file}"), stem))
        })
        .collect();
    out.sort_by_key(|(_, n)| n.to_lowercase());
    out
}

/// The file for a sound id: one of ours, or one of yours (`custom:<file name>`).
/// "none" and unknown ids have none.
pub fn sound_file(id: &str) -> Option<PathBuf> {
    if let Some(file) = id.strip_prefix("custom:") {
        // A bare file name only: the id comes from a settings file anyone can edit.
        let p = custom_sounds_dir().join(file);
        return (!file.contains('/') && p.is_file()).then_some(p);
    }
    SOUNDS.iter().any(|(s, _)| *s == id).then(|| sounds_dir().join(format!("{id}.wav")))
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

#[cfg(test)]
mod sound_tests {
    use super::*;

    #[test]
    fn custom_sounds_are_listed_and_found() {
        let data = std::env::temp_dir().join(format!("vide-sounds-test-{}", std::process::id()));
        // SAFETY: this test is the only one in this crate that touches XDG_DATA_HOME.
        unsafe { std::env::set_var("XDG_DATA_HOME", &data) };
        let dir = custom_sounds_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("my_cool-sound.ogg"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();

        let list = custom_sounds();
        assert_eq!(list, vec![("custom:my_cool-sound.ogg".to_string(), "my cool sound".to_string())]);
        assert_eq!(sound_file("custom:my_cool-sound.ogg"), Some(dir.join("my_cool-sound.ogg")));
        assert_eq!(sound_file("custom:gone.wav"), None);
        // An edited settings file can't point outside the folder.
        assert_eq!(sound_file("custom:../../../etc/passwd"), None);
        assert_eq!(sound_file("custom:.."), None);
        assert_eq!(sound_file("zen"), Some(sounds_dir().join("zen.wav")));
        std::fs::remove_dir_all(&data).unwrap();
    }

    #[test]
    fn per_app_sounds() {
        let mut c = Config::default();
        assert_eq!(c.app_sound("com.anthropic.Claude"), AppSound::AppsChoice);
        c.set_app_sound("com.anthropic.Claude", AppSound::Always);
        c.set_app_sound("dev.vencord.Vesktop", AppSound::Never);
        assert_eq!(c.app_sound("com.anthropic.Claude"), AppSound::Always);
        assert_eq!(c.app_sound("dev.vencord.Vesktop"), AppSound::Never);
        c.set_app_sound("com.anthropic.Claude", AppSound::AppsChoice);
        assert_eq!(c.notification_sound_apps, "dev.vencord.Vesktop=never");
        assert_eq!(notifying_app_id(Some("com.anthropic.Claude"), "Claude"), "com.anthropic.Claude");
        assert_eq!(notifying_app_id(None, "notify-send"), "notify-send");
        assert_eq!(notifying_app_id(Some(""), "a=b;c"), "abc");
    }
}
