//! System tray: volume (PipeWire, through `wpctl`), brightness, Wi-Fi and Bluetooth.
//! Brightness dims the picture in the compositor, so it works on any monitor, not
//! just laptop panels. NetworkManager and BlueZ are queried only when a flyout opens,
//! so an idle desktop doesn't keep spawning helpers.

use std::process::Command;

use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    utils::{Logical, Point, Rectangle},
};

use crate::{
    canvas::{hex, Canvas, Rgba},
    state::State,
    ui::TASKBAR_H,
};

pub const FLYOUT_W: i32 = 360;
pub const FLYOUT_H: i32 = 96;
pub const MENU_H: i32 = 224;
const SLIDER_X: i32 = 60;
const SLIDER_W: i32 = FLYOUT_W - SLIDER_X - 64;
const BG: Rgba = hex(0x1f1f1f);
const BORDER: Rgba = hex(0x3c3c3c);
const TRACK: Rgba = hex(0x5a5a5a);
const FILL: Rgba = hex(0x76b9ed);
const WHITE: Rgba = hex(0xffffff);
const DIM: Rgba = hex(0xa0a0a0);
/// Lowest brightness we allow, so the screen can't be dimmed to black by accident.
pub const MIN_BRIGHTNESS: f32 = 0.2;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Flyout {
    Volume,
    Brightness,
    Network,
    Bluetooth,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetRow {
    pub ssid: String,
    pub signal: u32,
    pub secure: bool,
    pub active: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BtRow {
    pub mac: String,
    pub name: String,
    pub connected: bool,
}

/// What a click on a flyout row means. Input routing applies it.
#[derive(Clone, Debug, PartialEq)]
pub enum FlyoutAction {
    WifiToggle,
    WifiConnect(String, bool),
    BluetoothToggle,
    BluetoothConnect(String, bool),
    NetworkSettings,
    BluetoothSettings,
    SoundSettings,
}

pub struct Tray {
    pub volume: f32,
    pub muted: bool,
    pub brightness: f32,
    pub flyout: Option<Flyout>,
    /// Left button held on the slider.
    pub dragging: bool,
    pub wifi_on: bool,
    pub bt_on: bool,
    pub ssid: String,
    pub networks: Vec<NetRow>,
    pub devices: Vec<BtRow>,
    /// Last refresh/action message, shown under the flyout title.
    pub notice: Option<String>,
    picture: Option<(String, MemoryRenderBuffer)>,
}

impl Tray {
    pub fn new() -> Self {
        let (volume, muted) = read_volume().unwrap_or((0.5, false));
        Self {
            volume,
            muted,
            brightness: 1.0,
            flyout: None,
            dragging: false,
            wifi_on: false,
            bt_on: false,
            ssid: String::new(),
            networks: Vec::new(),
            devices: Vec::new(),
            notice: None,
            picture: None,
        }
    }
}

/// `wpctl get-volume` prints e.g. "Volume: 0.45" or "Volume: 0.45 [MUTED]".
fn read_volume() -> Option<(f32, bool)> {
    let out = Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let v = text.split_whitespace().nth(1)?.parse().ok()?;
    Some((v, text.contains("MUTED")))
}

fn command(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

impl State {
    pub fn flyout_rect(&self) -> Rectangle<i32, Logical> {
        let h = match self.tray.flyout {
            Some(Flyout::Network | Flyout::Bluetooth) => MENU_H,
            _ => FLYOUT_H,
        };
        let x = self.output_size.w - FLYOUT_W - 8;
        Rectangle::new((x, self.output_size.h - TASKBAR_H - h - 8).into(), (FLYOUT_W, h).into())
    }

    pub fn toggle_flyout(&mut self, f: Flyout) {
        self.tray.flyout = if self.tray.flyout == Some(f) { None } else { Some(f) };
        self.tray.notice = None;
        match f {
            Flyout::Volume => {
                if let Some((v, m)) = read_volume() {
                    self.tray.volume = v;
                    self.tray.muted = m;
                }
            }
            Flyout::Network => self.refresh_network(),
            Flyout::Bluetooth => self.refresh_bluetooth(),
            Flyout::Brightness => {}
        }
        self.needs_redraw = true;
    }

    pub fn close_flyout(&mut self) {
        if self.tray.flyout.take().is_some() {
            self.tray.dragging = false;
            self.needs_redraw = true;
        }
    }

    pub fn set_volume(&mut self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.tray.volume = v;
        self.spawn(&format!("wpctl set-volume @DEFAULT_AUDIO_SINK@ {v:.2}"));
        if self.tray.muted && v > 0.0 {
            self.tray.muted = false;
            self.spawn("wpctl set-mute @DEFAULT_AUDIO_SINK@ 0");
        }
        self.needs_redraw = true;
    }

    pub fn toggle_mute(&mut self) {
        self.tray.muted = !self.tray.muted;
        self.spawn("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle");
        self.needs_redraw = true;
    }

    pub fn set_brightness(&mut self, b: f32) {
        self.tray.brightness = b.clamp(MIN_BRIGHTNESS, 1.0);
        self.needs_redraw = true;
    }

    fn refresh_network(&mut self) {
        self.tray.wifi_on = command("nmcli", &["radio", "wifi"]).is_some_and(|s| s.trim() == "enabled");
        self.tray.ssid.clear();
        self.tray.networks.clear();
        let Some(out) = command("nmcli", &["-t", "-f", "IN-USE,SIGNAL,SECURITY,SSID", "device", "wifi", "list", "--rescan", "no"]) else { return };
        for line in out.lines() {
            let mut f = line.splitn(4, ':');
            let active = f.next().unwrap_or("").contains('*');
            let signal = f.next().unwrap_or("0").parse().unwrap_or(0);
            let security = f.next().unwrap_or("").replace("\\:", ":");
            let ssid = f.next().unwrap_or("").replace("\\:", ":");
            if ssid.is_empty() || self.tray.networks.iter().any(|n| n.ssid == ssid) {
                continue;
            }
            if active {
                self.tray.ssid = ssid.clone();
            }
            self.tray.networks.push(NetRow { ssid, signal, secure: !security.is_empty() && security != "--", active });
        }
        self.tray.networks.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
        self.tray.networks.truncate(5);
    }

    fn refresh_bluetooth(&mut self) {
        self.tray.bt_on = command("bluetoothctl", &["show"]).is_some_and(|s| s.contains("Powered: yes"));
        self.tray.devices.clear();
        let Some(out) = command("bluetoothctl", &["devices", "Paired"]) else { return };
        let connected = command("bluetoothctl", &["devices", "Connected"]).unwrap_or_default();
        for line in out.lines() {
            let Some(rest) = line.strip_prefix("Device ") else { continue };
            let Some((mac, name)) = rest.split_once(' ') else { continue };
            self.tray.devices.push(BtRow {
                mac: mac.to_string(),
                name: name.to_string(),
                connected: connected.contains(mac),
            });
        }
        self.tray.devices.truncate(5);
    }

    /// The row under `p` in an open Network/Bluetooth flyout.
    pub fn flyout_action(&self, p: Point<f64, Logical>) -> Option<FlyoutAction> {
        let f = self.tray.flyout?;
        let local = p - self.flyout_rect().loc.to_f64();
        if local.x < 0.0 || local.x >= FLYOUT_W as f64 {
            return None;
        }
        if local.y >= (MENU_H - 32) as f64 {
            return match f {
                Flyout::Network => Some(FlyoutAction::NetworkSettings),
                Flyout::Bluetooth => Some(FlyoutAction::BluetoothSettings),
                _ => None,
            };
        }
        // The first 36px are the heading; rows are 28px high after that.
        if local.y < 36.0 {
            return match f {
                Flyout::Network => Some(FlyoutAction::WifiToggle),
                Flyout::Bluetooth => Some(FlyoutAction::BluetoothToggle),
                _ => None,
            };
        }
        let row = ((local.y - 36.0) / 28.0) as usize;
        match f {
            Flyout::Network => self.tray.networks.get(row).map(|n| FlyoutAction::WifiConnect(n.ssid.clone(), n.secure)),
            Flyout::Bluetooth => self.tray.devices.get(row).map(|d| FlyoutAction::BluetoothConnect(d.mac.clone(), d.connected)),
            _ => None,
        }
    }

    /// Run a flyout click: NetworkManager/BlueZ calls that must not block the
    /// compositor are spawned, then the flyout is refreshed.
    pub fn run_flyout_action(&mut self, a: FlyoutAction) {
        match a {
            FlyoutAction::WifiToggle => {
                let on = !self.tray.wifi_on;
                self.spawn(&format!("nmcli radio wifi {}", if on { "on" } else { "off" }));
                self.tray.wifi_on = on;
                self.tray.notice = Some(if on { "Wi-Fi on".into() } else { "Wi-Fi off".into() });
            }
            FlyoutAction::WifiConnect(ssid, secure) => {
                if secure {
                    // We can't type a Wi-Fi password in a tiny canvas flyout; Settings has a field.
                    self.tray.notice = Some("Opening Settings for the password…".into());
                    if let Some(settings) = crate::input::own_app("vide-settings") {
                        self.spawn(&format!("{settings} --page network"));
                    }
                } else {
                    self.spawn(&format!("nmcli device wifi connect '{}'", ssid.replace('\'', "'\\''")));
                    self.tray.notice = Some(format!("Connecting to {ssid}…"));
                }
            }
            FlyoutAction::BluetoothToggle => {
                let on = !self.tray.bt_on;
                self.spawn(&format!("bluetoothctl power {}", if on { "on" } else { "off" }));
                self.tray.bt_on = on;
                self.tray.notice = Some(if on { "Bluetooth on".into() } else { "Bluetooth off".into() });
            }
            FlyoutAction::BluetoothConnect(mac, connected) => {
                self.spawn(&format!("bluetoothctl {} {mac}", if connected { "disconnect" } else { "connect" }));
                self.tray.notice = Some(if connected { "Disconnecting…".into() } else { "Connecting…".into() });
            }
            FlyoutAction::NetworkSettings => {
                if let Some(settings) = crate::input::own_app("vide-settings") {
                    self.spawn(&format!("{settings} --page network"));
                }
            }
            FlyoutAction::BluetoothSettings => {
                if let Some(settings) = crate::input::own_app("vide-settings") {
                    self.spawn(&format!("{settings} --page bluetooth"));
                }
            }
            FlyoutAction::SoundSettings => {
                if let Some(settings) = crate::input::own_app("vide-settings") {
                    self.spawn(&format!("{settings} --page sound"));
                }
            }
        }
        self.close_flyout();
        self.needs_redraw = true;
    }

    /// Pointer at `p` on the open flyout's slider (or the mute icon): apply it.
    pub fn flyout_press(&mut self, p: Point<f64, Logical>) {
        let Some(f) = self.tray.flyout else { return };
        if matches!(f, Flyout::Network | Flyout::Bluetooth) {
            if let Some(a) = self.flyout_action(p) {
                self.run_flyout_action(a);
            }
            return;
        }
        let local = p - self.flyout_rect().loc.to_f64();
        if f == Flyout::Volume && local.x < SLIDER_X as f64 - 8.0 {
            self.toggle_mute();
            return;
        }
        self.tray.dragging = true;
        self.flyout_drag(p);
    }

    pub fn flyout_drag(&mut self, p: Point<f64, Logical>) {
        let Some(f) = self.tray.flyout else { return };
        if matches!(f, Flyout::Network | Flyout::Bluetooth) {
            return;
        }
        let local = p.x - self.flyout_rect().loc.x as f64;
        let v = ((local - SLIDER_X as f64) / SLIDER_W as f64).clamp(0.0, 1.0) as f32;
        match f {
            Flyout::Volume => self.set_volume(v),
            Flyout::Brightness => self.set_brightness(MIN_BRIGHTNESS + v * (1.0 - MIN_BRIGHTNESS)),
            _ => {}
        }
    }

    pub fn flyout_picture(&mut self) -> Option<&MemoryRenderBuffer> {
        let f = self.tray.flyout?;
        let key = match f {
            Flyout::Volume => format!("volume:{}:{}", (self.tray.volume * 100.0).round(), self.tray.muted),
            Flyout::Brightness => format!("brightness:{}", (self.tray.brightness * 100.0).round()),
            Flyout::Network => format!(
                "network:{}:{}:{}:{}",
                self.tray.wifi_on,
                self.tray.ssid,
                self.tray.notice.clone().unwrap_or_default(),
                self.tray.networks.iter().map(|n| format!("{}:{}:{}:{},", n.ssid, n.signal, n.secure, n.active)).collect::<String>()
            ),
            Flyout::Bluetooth => format!(
                "bt:{}:{}:{}:{}",
                self.tray.bt_on,
                self.tray.notice.clone().unwrap_or_default(),
                self.tray.devices.iter().map(|d| format!("{}:{}:{},", d.mac, d.name, d.connected)).collect::<String>(),
                self.tray.devices.len()
            ),
        };
        if self.tray.picture.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut c = Canvas::new(FLYOUT_W, if matches!(f, Flyout::Network | Flyout::Bluetooth) { MENU_H } else { FLYOUT_H });
            c.fill(BG);
            c.outline(0, 0, c.w, c.h, BORDER);
            match f {
                Flyout::Volume | Flyout::Brightness => draw_slider(&mut c, self, f),
                Flyout::Network => draw_network(&mut c, self),
                Flyout::Bluetooth => draw_bluetooth(&mut c, self),
            }
            self.tray.picture = Some((key, c.into_buffer()));
        }
        self.tray.picture.as_ref().map(|(_, b)| b)
    }
}

fn draw_slider(c: &mut Canvas, state: &State, f: Flyout) {
    let value = match f {
        Flyout::Volume => state.tray.volume,
        _ => (state.tray.brightness - MIN_BRIGHTNESS) / (1.0 - MIN_BRIGHTNESS),
    };
    let muted = f == Flyout::Volume && state.tray.muted;
    let cy = FLYOUT_H / 2 + 6;
    match f {
        Flyout::Volume => speaker(c, 22.0, cy as f32, muted, value, WHITE),
        _ => sun(c, 30.0, cy as f32, WHITE),
    }
    if let Some(font) = state.font.as_ref() {
        let title = if f == Flyout::Volume { "Volume" } else { "Brightness" };
        c.text(font, 12.0, 12, 16, 8, DIM, title);
        let pct = format!("{}", (value * 100.0).round() as i32);
        c.text(font, 16.0, FLYOUT_W - 48, cy - 14, 28, WHITE, &pct);
    }
    let fill_w = (SLIDER_W as f32 * value) as i32;
    c.round_rect(SLIDER_X, cy - 1, SLIDER_W, 3, 1, TRACK);
    c.round_rect(SLIDER_X, cy - 1, fill_w, 3, 1, if muted { TRACK } else { FILL });
    c.round_rect(SLIDER_X + fill_w - 5, cy - 11, 10, 22, 4, if muted { TRACK } else { FILL });
}

fn draw_network(c: &mut Canvas, state: &State) {
    let Some(font) = state.font.as_ref() else { return };
    c.text(font, 13.0, 12, 8, 22, WHITE, "Wi-Fi");
    let status = if state.tray.wifi_on {
        if state.tray.ssid.is_empty() { "On".to_string() } else { format!("On · {}", state.tray.ssid) }
    } else {
        "Off".into()
    };
    let tw = crate::canvas::measure(font, 11.0, &status);
    c.text(font, 11.0, FLYOUT_W - 12 - tw, 10, 20, if state.tray.wifi_on { FILL } else { DIM }, &status);
    c.rect(12, 32, FLYOUT_W - 24, 1, BORDER);

    if !state.tray.wifi_on {
        c.text(font, 12.5, 18, 48, 24, DIM, "Wi-Fi is off. Click the title to turn it on.");
    } else if state.tray.networks.is_empty() {
        c.text(font, 12.5, 18, 48, 24, DIM, "No networks found.");
    }
    for (i, net) in state.tray.networks.iter().take(5).enumerate() {
        let y = 36 + i as i32 * 28;
        if net.active {
            c.rect(8, y, FLYOUT_W - 16, 26, hex(0x2f2f2f));
        }
        wifi_bars(c, 18, y + 13, net.signal);
        let name = if net.ssid.len() > 26 { format!("{}…", &net.ssid[..26]) } else { net.ssid.clone() };
        c.text(font, 12.5, 36, y + 4, 20, if net.active { FILL } else { WHITE }, &name);
        if net.secure {
            lock(c, FLYOUT_W - 58, y + 7, DIM);
        }
        if net.active {
            c.text(font, 11.0, FLYOUT_W - 44, y + 5, 20, FILL, "Connected");
        }
    }
    if let Some(n) = &state.tray.notice {
        c.text(font, 10.5, 18, MENU_H - 56, 16, DIM, n);
    }
    c.rect(12, MENU_H - 36, FLYOUT_W - 24, 1, BORDER);
    c.text(font, 12.5, 18, MENU_H - 30, 24, WHITE, "Network settings");
}

fn draw_bluetooth(c: &mut Canvas, state: &State) {
    let Some(font) = state.font.as_ref() else { return };
    c.text(font, 13.0, 12, 8, 22, WHITE, "Bluetooth");
    let status = if state.tray.bt_on { "On" } else { "Off" };
    let tw = crate::canvas::measure(font, 11.0, status);
    c.text(font, 11.0, FLYOUT_W - 12 - tw, 10, 20, if state.tray.bt_on { FILL } else { DIM }, status);
    c.rect(12, 32, FLYOUT_W - 24, 1, BORDER);
    if !state.tray.bt_on {
        c.text(font, 12.5, 18, 48, 24, DIM, "Bluetooth is off. Click the title to turn it on.");
    } else if state.tray.devices.is_empty() {
        c.text(font, 12.5, 18, 48, 24, DIM, "No paired devices.");
    }
    for (i, dev) in state.tray.devices.iter().take(5).enumerate() {
        let y = 36 + i as i32 * 28;
        if dev.connected {
            c.rect(8, y, FLYOUT_W - 16, 26, hex(0x2f2f2f));
        }
        bluetooth_glyph(c, 18, y + 4, if dev.connected { FILL } else { WHITE });
        let name = if dev.name.len() > 28 { format!("{}…", &dev.name[..28]) } else { dev.name.clone() };
        c.text(font, 12.5, 34, y + 4, 20, if dev.connected { FILL } else { WHITE }, &name);
        let action = if dev.connected { "Disconnect" } else { "Connect" };
        let aw = crate::canvas::measure(font, 10.5, action);
        c.text(font, 10.5, FLYOUT_W - 14 - aw, y + 5, 20, DIM, action);
    }
    if let Some(n) = &state.tray.notice {
        c.text(font, 10.5, 18, MENU_H - 56, 16, DIM, n);
    }
    c.rect(12, MENU_H - 36, FLYOUT_W - 24, 1, BORDER);
    c.text(font, 12.5, 18, MENU_H - 30, 24, WHITE, "Bluetooth settings");
}

fn wifi_bars(c: &mut Canvas, x: i32, y: i32, signal: u32) {
    for i in 0..4 {
        let h = 4 + i * 3;
        let on = signal as f32 / 100.0 > i as f32 * 0.25;
        c.rect(x + i * 5, y + 9 - h, 3, h, if on { WHITE } else { TRACK });
    }
}

fn lock(c: &mut Canvas, x: i32, y: i32, color: Rgba) {
    c.outline(x, y + 5, 9, 8, color);
    c.outline(x + 2, y, 5, 6, color);
}

fn bluetooth_glyph(c: &mut Canvas, x: i32, y: i32, color: Rgba) {
    let (x, y) = (x as f32, y as f32);
    c.line(x + 5.0, y + 1.0, x + 10.0, y + 9.0, 1.4, color);
    c.line(x + 10.0, y + 9.0, x + 4.0, y + 14.0, 1.4, color);
    c.line(x + 4.0, y + 14.0, x + 4.0, y + 0.0, 1.4, color);
    c.line(x + 4.0, y + 0.0, x + 10.0, y + 5.0, 1.4, color);
    c.line(x + 10.0, y + 5.0, x + 5.0, y + 13.0, 1.4, color);
}

// Windows-style outline glyphs (20x20 grid, white, round joins), rendered with resvg.
const SPEAKER: &str = r##"<path d="M2.5 7.5h3.2L10.5 3.8v12.4L5.7 12.5H2.5z" fill="none" stroke="#fff" stroke-width="1.4" stroke-linejoin="round"/>"##;
const WAVES: [&str; 3] = [
    r##"<path d="M13 8a2.8 2.8 0 0 1 0 4" fill="none" stroke="#fff" stroke-width="1.4" stroke-linecap="round"/>"##,
    r##"<path d="M15 6a5.6 5.6 0 0 1 0 8" fill="none" stroke="#fff" stroke-width="1.4" stroke-linecap="round"/>"##,
    r##"<path d="M17 4a8.5 8.5 0 0 1 0 12" fill="none" stroke="#fff" stroke-width="1.4" stroke-linecap="round"/>"##,
];
const MUTE: &str = r##"<path d="M13.5 7.8l4.4 4.4M17.9 7.8l-4.4 4.4" fill="none" stroke="#fff" stroke-width="1.4" stroke-linecap="round"/>"##;

fn sun_svg() -> String {
    let mut rays = String::new();
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        let (s, c) = a.sin_cos();
        rays += &format!(
            r##"<line x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}"/>"##,
            10.0 + c * 6.0, 10.0 + s * 6.0, 10.0 + c * 8.2, 10.0 + s * 8.2
        );
    }
    format!(
        r##"<circle cx="10" cy="10" r="3.6" fill="none" stroke="#fff" stroke-width="1.4"/><g stroke="#fff" stroke-width="1.4" stroke-linecap="round">{rays}</g>"##
    )
}

fn wifi_svg() -> String {
    r##"<path d="M2.5 7.5a11 11 0 0 1 15 0M5.5 10.8a7 7 0 0 1 9 0M8.5 14a3 3 0 0 1 3 0" fill="none" stroke="#fff" stroke-width="1.6" stroke-linecap="round"/><circle cx="10" cy="17" r="1.1" fill="#fff"/>"##.into()
}

fn bt_svg() -> String {
    r##"<path d="M8 2.5v15l6-4.2-10-6.6M14 9.3l-6 4.2" fill="none" stroke="#fff" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>"##.into()
}

thread_local! {
    static GLYPHS: std::cell::RefCell<std::collections::HashMap<String, std::rc::Rc<crate::icons::Icon>>> = Default::default();
}

/// Rasterize (and cache) one glyph, centred at (x, y).
fn glyph(c: &mut Canvas, body: String, x: f32, y: f32, size: u32) {
    let key = format!("{size}:{body}");
    let icon = GLYPHS.with(|g| {
        g.borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                let svg = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20">{body}</svg>"##);
                std::rc::Rc::new(crate::icons::render_svg(svg.as_bytes(), size).unwrap_or(crate::icons::Icon {
                    w: 1,
                    h: 1,
                    rgba: vec![0; 4],
                }))
            })
            .clone()
    });
    c.icon(&icon, (x - size as f32 / 2.0).round() as i32, (y - size as f32 / 2.0).round() as i32);
}

/// Speaker with 0-3 sound waves for the level, or an X when muted.
pub fn speaker(c: &mut Canvas, x: f32, y: f32, muted: bool, level: f32, _color: Rgba) {
    let mut body = SPEAKER.to_string();
    if muted {
        body += MUTE;
    } else {
        let waves = if level <= 0.01 { 0 } else if level < 0.34 { 1 } else if level < 0.67 { 2 } else { 3 };
        body.extend(WAVES.iter().take(waves).copied());
    }
    glyph(c, body, x + 5.0, y, 20);
}

/// Sun: brightness.
pub fn sun(c: &mut Canvas, x: f32, y: f32, _color: Rgba) {
    glyph(c, sun_svg(), x, y, 20);
}

/// Wi-Fi: radio waves.
pub fn wifi(c: &mut Canvas, x: f32, y: f32, on: bool) {
    if !on {
        // Off: the waves with a slash through them.
        glyph(c, format!(r##"{}{}"##, wifi_svg(), r##"<path d="M3 3l14 14" stroke="#fff" stroke-width="1.5" stroke-linecap="round"/>"##), x, y, 20);
    } else {
        glyph(c, wifi_svg(), x, y, 20);
    }
}

/// Bluetooth rune.
pub fn bluetooth(c: &mut Canvas, x: f32, y: f32, on: bool) {
    let body = if on { bt_svg() } else { format!(r##"{}{}"##, bt_svg(), r##"<path d="M3 3l14 14" stroke="#fff" stroke-width="1.4" stroke-linecap="round"/>"##) };
    glyph(c, body, x, y, 20);
}
