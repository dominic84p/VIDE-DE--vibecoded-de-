//! System tray: volume (PipeWire, through `wpctl`), brightness and the network flyout.
//! Brightness dims the picture in the compositor, so it works on any monitor, not
//! just laptop panels.
//!
//! The network flyout is Windows 10's: available networks (click one to expand it,
//! Connect asks for the security key right in the flyout), "Network & Internet
//! settings", and the Wi-Fi / Airplane mode / Mobile hotspot tiles. NetworkManager,
//! rfkill and BlueZ are asked on a thread, so a slow `nmcli` never stalls a frame;
//! answers come back over a calloop channel.

use std::{process::Command, time::Duration};

use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    reexports::calloop::{
        channel::{self, Sender},
        timer::{TimeoutAction, Timer},
    },
    utils::{Logical, Point, Rectangle},
};

use crate::{
    canvas::{ellipsize, hex, measure, Canvas, Rgba},
    state::State,
    theme::{th, ACCENT, ACCENT_HOVER, ON_ACCENT},
    ui::TASKBAR_H,
};

pub const FLYOUT_W: i32 = 360;
pub const FLYOUT_H: i32 = 96;
/// The network flyout, when the screen is tall enough.
const NET_H: i32 = 560;
const SLIDER_X: i32 = 60;
const SLIDER_W: i32 = FLYOUT_W - SLIDER_X - 64;
/// Lowest brightness we allow, so the screen can't be dimmed to black by accident.
pub const MIN_BRIGHTNESS: f32 = 0.2;
/// Windows' wording for a wrong Wi-Fi password, in its two lines.
const WRONG_KEY: [&str; 2] = ["The network security key isn't correct.", "Please try again."];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Flyout {
    Volume,
    Brightness,
    Network,
}

/// What the taskbar's network icon shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NetIcon {
    Airplane,
    Wired,
    /// Connected to Wi-Fi, with 1-4 bars.
    Wifi(u8),
    /// Wi-Fi on but not connected.
    Searching,
    WifiOff,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NetRow {
    pub ssid: String,
    pub signal: u32,
    pub secure: bool,
    pub active: bool,
}

/// What a click on the network flyout means. Input routing applies it.
#[derive(Clone, Debug, PartialEq)]
pub enum FlyoutAction {
    /// Expand (or collapse) a network's row.
    Select(String),
    Connect(String),
    Disconnect,
    Properties,
    ToggleAutoConnect,
    PasswordField,
    PasswordNext,
    PasswordCancel,
    WifiToggle,
    AirplaneToggle,
    HotspotToggle,
    NetworkSettings,
}

/// One look at the radios and NetworkManager, taken on a thread.
#[derive(Clone, Debug, Default)]
pub struct NetSnapshot {
    pub wifi_on: bool,
    pub bt_on: bool,
    pub airplane: bool,
    pub hotspot: bool,
    /// Name of the wired connection, when one is up.
    pub wired: Option<String>,
    pub wifi_dev: Option<String>,
    pub networks: Vec<NetRow>,
    /// Wi-Fi connections NetworkManager already has a password for.
    pub saved: Vec<String>,
}

pub enum TrayUpdate {
    Net(NetSnapshot),
    Connected { ssid: String, ok: bool },
}

pub struct Tray {
    pub volume: f32,
    pub muted: bool,
    pub brightness: f32,
    pub flyout: Option<Flyout>,
    /// Left button held on the slider.
    pub dragging: bool,
    pub net: NetSnapshot,
    /// The network row that's open (Connect / Disconnect showing).
    pub expanded: Option<String>,
    /// Typing a security key for `expanded`.
    pub password: Option<String>,
    pub password_error: bool,
    pub auto_connect: bool,
    pub connecting: Option<String>,
    pub hover: Option<FlyoutAction>,
    /// Network rows scrolled past.
    pub scroll: usize,
    pub(crate) picture: Option<(String, MemoryRenderBuffer, Vec<(Rectangle<i32, Logical>, FlyoutAction)>)>,
    updates: Option<Sender<TrayUpdate>>,
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
            net: NetSnapshot::default(),
            expanded: None,
            password: None,
            password_error: false,
            auto_connect: true,
            connecting: None,
            hover: None,
            scroll: 0,
            picture: None,
            updates: None,
        }
    }

    pub fn wifi_on(&self) -> bool {
        self.net.wifi_on
    }

    pub fn icon(&self) -> NetIcon {
        let n = &self.net;
        if n.airplane {
            NetIcon::Airplane
        } else if n.wired.is_some() {
            NetIcon::Wired
        } else if let Some(a) = n.networks.iter().find(|r| r.active) {
            NetIcon::Wifi(((a.signal + 24) / 25).clamp(1, 4) as u8)
        } else if n.wifi_on {
            NetIcon::Searching
        } else {
            NetIcon::WifiOff
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

/// Run a command off the main loop, without a shell (SSIDs and keys are user text).
fn run_quiet(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd).args(args).output().is_ok_and(|o| o.status.success())
}

/// nmcli's terse output escapes ':' inside fields as '\:'.
fn split_terse(line: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.last_mut().unwrap().extend(chars.next()),
            ':' => out.push(String::new()),
            c => out.last_mut().unwrap().push(c),
        }
    }
    out
}

fn snapshot() -> NetSnapshot {
    let mut s = NetSnapshot { wifi_on: command("nmcli", &["radio", "wifi"]).is_some_and(|s| s.trim() == "enabled"), ..Default::default() };
    let mut wifi_conn = None;
    for line in command("nmcli", &["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device"]).unwrap_or_default().lines() {
        let f = split_terse(line);
        if f.len() < 4 {
            continue;
        }
        match f[1].as_str() {
            "ethernet" if f[2] == "connected" && s.wired.is_none() => s.wired = Some(f[3].clone()),
            "wifi" if s.wifi_dev.is_none() => {
                s.wifi_dev = Some(f[0].clone());
                wifi_conn = (f[2] == "connected").then(|| f[3].clone());
            }
            _ => {}
        }
    }
    for line in command("nmcli", &["-t", "-f", "IN-USE,SIGNAL,SECURITY,SSID", "device", "wifi", "list", "--rescan", "no"]).unwrap_or_default().lines() {
        let f = split_terse(line);
        if f.len() < 4 || f[3].is_empty() {
            continue;
        }
        let active = f[0].contains('*');
        // One row per name, though a network often has several access points; the
        // one we're on may not be the first listed.
        if let Some(n) = s.networks.iter_mut().find(|n| n.ssid == f[3]) {
            n.active |= active;
            continue;
        }
        s.networks.push(NetRow { ssid: f[3].clone(), signal: f[1].parse().unwrap_or(0), secure: !f[2].is_empty() && f[2] != "--", active });
    }
    // The connection is usually named after the network, which covers lists that don't mark it.
    if !s.networks.iter().any(|n| n.active)
        && let Some(c) = &wifi_conn
        && let Some(n) = s.networks.iter_mut().find(|n| n.ssid == *c)
    {
        n.active = true;
    }
    // Windows lists the connected network first, then by signal.
    s.networks.sort_by(|a, b| b.active.cmp(&a.active).then(b.signal.cmp(&a.signal)));
    s.networks.truncate(20);
    for line in command("nmcli", &["-t", "-f", "NAME,TYPE", "connection", "show"]).unwrap_or_default().lines() {
        let f = split_terse(line);
        if f.len() >= 2 && f[1] == "802-11-wireless" {
            s.saved.push(f[0].clone());
        }
    }
    s.hotspot = command("nmcli", &["-t", "-f", "NAME", "connection", "show", "--active"]).is_some_and(|o| o.lines().any(|l| l == "Hotspot"));
    // Airplane mode is every radio soft-blocked, the way rfkill (and GNOME/KDE) see it.
    let rf = command("rfkill", &["list"]).unwrap_or_default();
    let soft: Vec<bool> = rf.lines().filter_map(|l| l.trim().strip_prefix("Soft blocked: ")).map(|v| v == "yes").collect();
    s.airplane = !soft.is_empty() && soft.iter().all(|b| *b);
    s.bt_on = command("bluetoothctl", &["show"]).is_some_and(|o| o.contains("Powered: yes"));
    s
}

impl State {
    pub fn flyout_rect(&self) -> Rectangle<i32, Logical> {
        let h = match self.tray.flyout {
            Some(Flyout::Network) => NET_H.min(self.output_size.h - TASKBAR_H - 16),
            _ => FLYOUT_H,
        };
        let x = self.output_size.w - FLYOUT_W - 8;
        Rectangle::new((x, self.output_size.h - TASKBAR_H - h - 8).into(), (FLYOUT_W, h).into())
    }

    fn tray_sender(&mut self) -> Sender<TrayUpdate> {
        if let Some(tx) = &self.tray.updates {
            return tx.clone();
        }
        let (tx, rx) = channel::channel();
        let res = self.loop_handle.insert_source(rx, |event, _, state| {
            if let channel::Event::Msg(u) = event {
                crate::supervisor::guard("tray update", || state.apply_tray_update(u));
            }
        });
        if let Err(e) = res {
            tracing::warn!("tray updates disabled: {e}");
        }
        self.tray.updates = Some(tx.clone());
        tx
    }

    /// Re-read the radios and networks on a thread.
    pub fn refresh_network(&mut self) {
        let tx = self.tray_sender();
        std::thread::spawn(move || {
            let _ = tx.send(TrayUpdate::Net(snapshot()));
        });
    }

    fn refresh_network_in(&mut self, d: Duration) {
        let _ = self.loop_handle.insert_source(Timer::from_duration(d), |_, _, state| {
            state.refresh_network();
            TimeoutAction::Drop
        });
    }

    /// Keep the taskbar icon honest: a quick look every 20 s.
    pub fn start_network_polling(&mut self) {
        self.refresh_network();
        let _ = self.loop_handle.insert_source(Timer::from_duration(Duration::from_secs(20)), |_, _, state| {
            state.refresh_network();
            TimeoutAction::ToDuration(Duration::from_secs(20))
        });
    }

    fn apply_tray_update(&mut self, u: TrayUpdate) {
        match u {
            TrayUpdate::Net(s) => {
                self.tray.net = s;
                self.taskbar = None;
            }
            TrayUpdate::Connected { ssid, ok } => {
                self.tray.connecting = None;
                if ok {
                    self.tray.expanded = None;
                    self.tray.password = None;
                } else if self.tray.net.networks.iter().any(|n| n.ssid == ssid && n.secure) {
                    // Ask again, with Windows' wrong-key message.
                    self.tray.expanded = Some(ssid);
                    self.tray.password = Some(String::new());
                    self.tray.password_error = true;
                }
                self.refresh_network();
            }
        }
        self.needs_redraw = true;
    }

    pub fn toggle_flyout(&mut self, f: Flyout) {
        self.tray.flyout = if self.tray.flyout == Some(f) { None } else { Some(f) };
        if self.tray.flyout.is_some() {
            self.release_pointer();
        }
        match f {
            Flyout::Volume => {
                if let Some((v, m)) = read_volume() {
                    self.tray.volume = v;
                    self.tray.muted = m;
                }
            }
            Flyout::Network => {
                self.tray.expanded = None;
                self.tray.password = None;
                self.tray.scroll = 0;
                self.refresh_network();
                // Windows rescans when the flyout opens; results land a few seconds later.
                std::thread::spawn(|| run_quiet("nmcli", &["device", "wifi", "rescan"]));
                self.refresh_network_in(Duration::from_secs(4));
            }
            Flyout::Brightness => {}
        }
        self.needs_redraw = true;
    }

    pub fn close_flyout(&mut self) {
        if self.tray.flyout.take().is_some() {
            self.tray.dragging = false;
            self.tray.password = None;
            self.tray.hover = None;
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

    pub fn set_bluetooth(&mut self, on: bool) {
        self.tray.net.bt_on = on;
        std::thread::spawn(move || run_quiet("bluetoothctl", &["power", if on { "on" } else { "off" }]));
        self.refresh_network_in(Duration::from_millis(1500));
        self.needs_redraw = true;
    }

    pub fn set_airplane(&mut self, on: bool) {
        self.tray.net.airplane = on;
        if on {
            self.tray.net.wifi_on = false;
            self.tray.net.bt_on = false;
        }
        std::thread::spawn(move || run_quiet("rfkill", &[if on { "block" } else { "unblock" }, "all"]));
        self.refresh_network_in(Duration::from_millis(1500));
        self.taskbar = None;
        self.needs_redraw = true;
    }

    pub fn set_hotspot(&mut self, on: bool) {
        self.tray.net.hotspot = on;
        std::thread::spawn(move || {
            if on {
                run_quiet("nmcli", &["device", "wifi", "hotspot"])
            } else {
                run_quiet("nmcli", &["connection", "down", "Hotspot"])
            }
        });
        self.refresh_network_in(Duration::from_secs(2));
        self.needs_redraw = true;
    }

    fn connect(&mut self, ssid: String, key: Option<String>) {
        let saved = self.tray.net.saved.contains(&ssid);
        let auto = self.tray.auto_connect;
        let tx = self.tray_sender();
        self.tray.connecting = Some(ssid.clone());
        self.tray.password = None;
        self.tray.password_error = false;
        std::thread::spawn(move || {
            let ok = match &key {
                Some(k) => run_quiet("nmcli", &["device", "wifi", "connect", &ssid, "password", k]),
                None if saved => run_quiet("nmcli", &["connection", "up", "id", &ssid]),
                None => run_quiet("nmcli", &["device", "wifi", "connect", &ssid]),
            };
            if ok && !auto {
                run_quiet("nmcli", &["connection", "modify", "id", &ssid, "connection.autoconnect", "no"]);
            }
            let _ = tx.send(TrayUpdate::Connected { ssid, ok });
        });
    }

    /// The network flyout's hit under `p`.
    pub fn flyout_action(&self, p: Point<f64, Logical>) -> Option<FlyoutAction> {
        if self.tray.flyout != Some(Flyout::Network) {
            return None;
        }
        let local = (p - self.flyout_rect().loc.to_f64()).to_i32_round();
        let (_, _, hits) = self.tray.picture.as_ref()?;
        hits.iter().rev().find(|(r, _)| r.contains(local)).map(|(_, a)| a.clone())
    }

    /// Run a network flyout click. The flyout stays open, as in Windows, except for
    /// the links that open Settings.
    pub fn run_flyout_action(&mut self, a: FlyoutAction) {
        let settings = |state: &mut State| {
            if let Some(settings) = crate::input::own_app("vide-settings") {
                state.spawn(&format!("{settings} --page network"));
            }
            state.close_flyout();
        };
        match a {
            FlyoutAction::Select(ssid) => {
                let open = self.tray.expanded.as_ref() != Some(&ssid);
                self.tray.expanded = open.then_some(ssid);
                self.tray.password = None;
                self.tray.password_error = false;
                self.tray.auto_connect = true;
            }
            FlyoutAction::Connect(ssid) => {
                let secure = self.tray.net.networks.iter().any(|n| n.ssid == ssid && n.secure);
                if secure && !self.tray.net.saved.contains(&ssid) {
                    self.tray.password = Some(String::new());
                } else {
                    self.connect(ssid, None);
                }
            }
            FlyoutAction::PasswordNext => {
                if let (Some(ssid), Some(key)) = (self.tray.expanded.clone(), self.tray.password.clone())
                    && !key.is_empty()
                {
                    self.connect(ssid, Some(key));
                }
            }
            FlyoutAction::PasswordCancel => {
                self.tray.password = None;
                self.tray.password_error = false;
            }
            FlyoutAction::PasswordField => {}
            FlyoutAction::Disconnect => {
                if let Some(dev) = self.tray.net.wifi_dev.clone() {
                    std::thread::spawn(move || run_quiet("nmcli", &["device", "disconnect", &dev]));
                    self.refresh_network_in(Duration::from_millis(1500));
                }
                self.tray.expanded = None;
            }
            FlyoutAction::ToggleAutoConnect => self.tray.auto_connect = !self.tray.auto_connect,
            FlyoutAction::WifiToggle => {
                let on = !self.tray.net.wifi_on;
                self.tray.net.wifi_on = on;
                if on {
                    self.tray.net.airplane = false;
                }
                std::thread::spawn(move || run_quiet("nmcli", &["radio", "wifi", if on { "on" } else { "off" }]));
                self.refresh_network_in(Duration::from_secs(2));
                self.taskbar = None;
            }
            FlyoutAction::AirplaneToggle => {
                let on = !self.tray.net.airplane;
                self.set_airplane(on);
            }
            FlyoutAction::HotspotToggle => {
                let on = !self.tray.net.hotspot;
                self.set_hotspot(on);
            }
            FlyoutAction::Properties | FlyoutAction::NetworkSettings => settings(self),
        }
        self.needs_redraw = true;
    }

    /// Typing into the security key box.
    pub fn flyout_type(&mut self, c: Option<char>, backspace: bool) {
        let Some(pw) = self.tray.password.as_mut() else { return };
        if backspace {
            pw.pop();
        } else if let Some(c) = c {
            pw.push(c);
        }
        self.tray.password_error = false;
        self.needs_redraw = true;
    }

    pub fn flyout_scroll(&mut self, rows: i32) {
        let max = self.tray.net.networks.len().saturating_sub(1);
        self.tray.scroll = (self.tray.scroll as i32 + rows).clamp(0, max as i32) as usize;
        self.needs_redraw = true;
    }

    /// Pointer at `p` on the open flyout: a network-flyout click, or the slider / mute icon.
    pub fn flyout_press(&mut self, p: Point<f64, Logical>) {
        let Some(f) = self.tray.flyout else { return };
        if f == Flyout::Network {
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
        if f == Flyout::Network {
            return;
        }
        let local = p.x - self.flyout_rect().loc.x as f64;
        let v = ((local - SLIDER_X as f64) / SLIDER_W as f64).clamp(0.0, 1.0) as f32;
        match f {
            Flyout::Volume => self.set_volume(v),
            Flyout::Brightness => self.set_brightness(MIN_BRIGHTNESS + v * (1.0 - MIN_BRIGHTNESS)),
            Flyout::Network => {}
        }
    }

    pub fn flyout_picture(&mut self) -> Option<&MemoryRenderBuffer> {
        let f = self.tray.flyout?;
        let h = self.flyout_rect().size.h;
        let t = &self.tray;
        let key = match f {
            Flyout::Volume => format!("volume:{}:{}", (t.volume * 100.0).round(), t.muted),
            Flyout::Brightness => format!("brightness:{}", (t.brightness * 100.0).round()),
            Flyout::Network => format!(
                "network:{h}:{:?}:{:?}:{:?}:{}:{}:{:?}:{:?}:{}",
                t.net, t.expanded, t.password.as_ref().map(|p| p.chars().count()), t.password_error, t.auto_connect, t.connecting, t.hover, t.scroll
            ),
        };
        if self.tray.picture.as_ref().is_none_or(|(k, _, _)| *k != key) {
            let mut c = Canvas::new(FLYOUT_W, h);
            c.fill(th().panel);
            c.outline(0, 0, c.w, c.h, th().border);
            let mut hits = Vec::new();
            match f {
                Flyout::Volume | Flyout::Brightness => draw_slider(&mut c, self, f),
                Flyout::Network => hits = draw_network(&mut c, self),
            }
            self.tray.picture = Some((key, c.into_buffer(), hits));
        }
        self.tray.picture.as_ref().map(|(_, b, _)| b)
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
        Flyout::Volume => speaker(c, 22.0, cy as f32, muted, value, th().text),
        _ => sun(c, 30.0, cy as f32, th().text),
    }
    if let Some(font) = state.font.as_ref() {
        let title = if f == Flyout::Volume { "Volume" } else { "Brightness" };
        c.text(font, 12.0, 12, 16, 8, th().text_dim, title);
        let pct = format!("{}", (value * 100.0).round() as i32);
        c.text(font, 16.0, FLYOUT_W - 48, cy - 14, 28, th().text, &pct);
    }
    let fill_w = (SLIDER_W as f32 * value) as i32;
    c.round_rect(SLIDER_X, cy - 1, SLIDER_W, 3, 1, th().track);
    c.round_rect(SLIDER_X, cy - 1, fill_w, 3, 1, if muted { th().track } else { th().accent_light });
    c.round_rect(SLIDER_X + fill_w - 5, cy - 11, 10, 22, 4, if muted { th().track } else { th().accent_light });
}

fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Logical> {
    Rectangle::new((x, y).into(), (w, h).into())
}

/// A flat Windows flyout button; returns its rect.
fn button(c: &mut Canvas, font: &fontdue::Font, x: i32, y: i32, w: i32, label: &str, hover: bool) -> Rectangle<i32, Logical> {
    c.rect(x, y, w, 32, if hover { th().button_hover } else { th().button });
    if hover {
        c.outline(x, y, w, 32, hex(0x858585));
    }
    let tw = measure(font, 13.0, label);
    c.text(font, 13.0, x + (w - tw) / 2, y, 32, th().text, label);
    rect(x, y, w, 32)
}

/// Windows 10's network flyout. Returns the click targets.
fn draw_network(c: &mut Canvas, state: &State) -> Vec<(Rectangle<i32, Logical>, FlyoutAction)> {
    let mut hits = vec![];
    let Some(font) = state.font.as_ref() else { return hits };
    let t = &state.tray;
    let n = &t.net;
    let h = c.h;
    let hover = |a: &FlyoutAction| t.hover.as_ref() == Some(a);
    let tiles_y = h - 82;
    let settings_y = tiles_y - 70;
    let list_bottom = settings_y - 4;
    let mut y = 0;

    // Wired connection first, like Windows.
    if let Some(name) = &n.wired {
        ethernet_glyph(c, 28.0, (y + 32) as f32);
        c.text(font, 14.0, 56, y + 12, 20, th().text, &ellipsize(font, 14.0, name, FLYOUT_W - 72));
        c.text(font, 12.0, 56, y + 32, 18, th().text_dim, "Connected");
        y += 64;
    }

    if n.airplane {
        c.text(font, 13.0, 16, y + 16, 20, th().text_dim, "Airplane mode is on.");
    } else if !n.wifi_on {
        c.text(font, 13.0, 16, y + 16, 20, th().text_dim, "Wi-Fi is turned off.");
    } else if n.networks.is_empty() {
        c.text(font, 13.0, 16, y + 16, 20, th().text_dim, "No Wi-Fi networks found.");
    }
    let rows: &[NetRow] = if n.wifi_on && !n.airplane { &n.networks } else { &[] };
    for net in rows.iter().skip(t.scroll) {
        let open = t.expanded.as_ref() == Some(&net.ssid);
        let typing = open && t.password.is_some();
        let rh = match (open, net.active, typing) {
            (false, _, _) => 64,
            (true, true, _) => 132,
            (true, false, false) => 140,
            (true, false, true) => if t.password_error { 210 } else { 176 },
        };
        if y + 64 > list_bottom {
            break;
        }
        let select = FlyoutAction::Select(net.ssid.clone());
        if open {
            c.rect(0, y, FLYOUT_W, rh.min(list_bottom - y), th().row_open);
        } else if hover(&select) {
            c.rect(0, y, FLYOUT_W, rh, th().row_hover);
        }
        hits.push((rect(0, y, FLYOUT_W, 64), select));
        wifi_signal(c, 28.0, (y + 30) as f32, net.signal, net.secure);
        c.text(font, 14.0, 56, y + 12, 20, th().text, &ellipsize(font, 14.0, &net.ssid, FLYOUT_W - 72));
        let status = if t.connecting.as_ref() == Some(&net.ssid) {
            "Connecting…".to_string()
        } else if net.active {
            format!("Connected, {}", if net.secure { "secured" } else { "open" })
        } else if net.secure {
            "Secured".into()
        } else {
            "Open".into()
        };
        c.text(font, 12.0, 56, y + 32, 18, th().text_dim, &status);

        if open && net.active {
            let props = "Properties";
            let pw = measure(font, 13.0, props);
            c.text(font, 13.0, 56, y + 62, 20, if hover(&FlyoutAction::Properties) { th().text } else { th().link }, props);
            hits.push((rect(52, y + 60, pw + 8, 24), FlyoutAction::Properties));
            let r = button(c, font, FLYOUT_W - 16 - 120, y + 88, 120, "Disconnect", hover(&FlyoutAction::Disconnect));
            hits.push((r, FlyoutAction::Disconnect));
        } else if open && !typing {
            // "Connect automatically" checkbox, then Connect.
            let (bx, by) = (56, y + 64);
            if t.auto_connect {
                c.rect(bx, by, 18, 18, ACCENT);
                c.line(bx as f32 + 4.0, by as f32 + 9.5, bx as f32 + 7.5, by as f32 + 13.0, 1.6, ON_ACCENT);
                c.line(bx as f32 + 7.5, by as f32 + 13.0, bx as f32 + 14.0, by as f32 + 5.5, 1.6, ON_ACCENT);
            } else {
                c.outline(bx, by, 18, 18, th().text);
                c.outline(bx + 1, by + 1, 16, 16, th().text);
            }
            c.text(font, 13.0, bx + 28, by - 1, 20, th().text, "Connect automatically");
            hits.push((rect(bx - 4, by - 4, 220, 26), FlyoutAction::ToggleAutoConnect));
            let busy = t.connecting.is_some();
            let r = button(c, font, FLYOUT_W - 16 - 120, y + 96, 120, "Connect", hover(&FlyoutAction::Connect(net.ssid.clone())) && !busy);
            if !busy {
                hits.push((r, FlyoutAction::Connect(net.ssid.clone())));
            }
        } else if typing {
            c.text(font, 13.0, 56, y + 62, 20, th().text, "Enter the network security key");
            let (fx, fy, fw) = (56, y + 88, FLYOUT_W - 56 - 16);
            c.rect(fx, fy, fw, 32, th().field);
            c.outline(fx, fy, fw, 32, ACCENT);
            c.outline(fx + 1, fy + 1, fw - 2, 30, ACCENT);
            // One dot per character, drawn: the UI font has no bullet glyph.
            let len = t.password.as_ref().map(|p| p.chars().count()).unwrap_or(0).min(((fw - 24) / 9) as usize);
            for i in 0..len {
                c.circle((fx + 14 + i as i32 * 9) as f32, (fy + 16) as f32, 3.0, th().text);
            }
            c.rect(fx + 12 + len as i32 * 9, fy + 8, 1, 16, th().text);
            hits.push((rect(fx, fy, fw, 32), FlyoutAction::PasswordField));
            let mut by = fy + 44;
            if t.password_error {
                for (i, line) in WRONG_KEY.iter().enumerate() {
                    c.text(font, 12.0, 56, fy + 36 + i as i32 * 16, 18, th().error, line);
                }
                by += 34;
            }
            let r = button(c, font, FLYOUT_W - 16 - 100, by, 100, "Cancel", hover(&FlyoutAction::PasswordCancel));
            hits.push((r, FlyoutAction::PasswordCancel));
            let r = button(c, font, FLYOUT_W - 16 - 208, by, 100, "Next", hover(&FlyoutAction::PasswordNext));
            hits.push((r, FlyoutAction::PasswordNext));
        }
        y += rh;
    }
    // Rows below the list area are covered by the settings link and tiles.
    c.rect(1, settings_y - 4, FLYOUT_W - 2, h - settings_y + 3, th().panel);

    // "Network & Internet settings".
    let s = FlyoutAction::NetworkSettings;
    if hover(&s) {
        c.rect(1, settings_y, FLYOUT_W - 2, 62, th().row_hover);
    }
    c.text(font, 14.0, 16, settings_y + 8, 22, th().text, "Network & Internet settings");
    c.text(font, 12.0, 16, settings_y + 32, 18, th().text_dim, "Change settings, such as making a connection metered.");
    hits.push((rect(0, settings_y, FLYOUT_W, 62), s));

    // Quick action tiles.
    let tw = (FLYOUT_W - 24 - 8) / 3;
    for (i, (label, on, a)) in [
        ("Wi-Fi", n.wifi_on && !n.airplane, FlyoutAction::WifiToggle),
        ("Airplane mode", n.airplane, FlyoutAction::AirplaneToggle),
        ("Mobile hotspot", n.hotspot, FlyoutAction::HotspotToggle),
    ]
    .into_iter()
    .enumerate()
    {
        let x = 12 + i as i32 * (tw + 4);
        let hov = hover(&a);
        c.rect(x, tiles_y, tw, 70, match (on, hov) {
            (true, true) => ACCENT_HOVER,
            (true, false) => ACCENT,
            (false, true) => th().tile_hover,
            (false, false) => th().tile,
        });
        let (gx, gy) = (x as f32 + 18.0, tiles_y as f32 + 20.0);
        let ink = if on { ON_ACCENT } else { th().text };
        with_ink(ink, || match i {
            0 => wifi(c, gx, gy, true),
            1 => airplane(c, gx, gy),
            _ => hotspot(c, gx, gy),
        });
        c.text(font, 12.0, x + 10, tiles_y + 44, 18, ink, &ellipsize(font, 12.0, label, tw - 14));
        hits.push((rect(x, tiles_y, tw, 70), a));
    }
    hits
}

fn lock(c: &mut Canvas, x: i32, y: i32, color: Rgba) {
    c.rect(x, y + 4, 8, 6, color);
    c.outline(x + 1, y, 6, 6, color);
}

// Windows 10 Segoe MDL2-style glyphs (20x20 grid, white, sharp miter/butt strokes), rendered with resvg.
const SPEAKER: &str = r##"<path d="M2.8 7.8h3.1L10.3 3.8v12.4L5.9 12.2H2.8z" fill="none" stroke="#fff" stroke-width="1.2" stroke-linejoin="miter"/>"##;
const WAVES: [&str; 3] = [
    r##"<path d="M12.5 7.9a3 3 0 0 1 0 4.2" fill="none" stroke="#fff" stroke-width="1.2" stroke-linecap="butt"/>"##,
    r##"<path d="M14.4 6a5.6 5.6 0 0 1 0 8" fill="none" stroke="#fff" stroke-width="1.2" stroke-linecap="butt"/>"##,
    r##"<path d="M16.3 4.1a8.3 8.3 0 0 1 0 11.8" fill="none" stroke="#fff" stroke-width="1.2" stroke-linecap="butt"/>"##,
];
const MUTE: &str = r##"<path d="M13 7.6l4.8 4.8M17.8 7.6l-4.8 4.8" fill="none" stroke="#fff" stroke-width="1.25" stroke-linecap="butt"/>"##;
/// Windows 10's 90-degree wedge Wi-Fi arcs, outermost first.
const ARCS: [&str; 3] = [
    "M1.9 7.7a11.5 11.5 0 0 1 16.2 0",
    "M4.3 10.1a8 8 0 0 1 11.4 0",
    "M6.8 12.6a4.5 4.5 0 0 1 6.4 0",
];

fn sun_svg() -> String {
    let mut rays = String::new();
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        let (s, c) = a.sin_cos();
        rays += &format!(
            r##"<line x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}"/>"##,
            10.0 + c * 5.8, 10.0 + s * 5.8, 10.0 + c * 8.2, 10.0 + s * 8.2
        );
    }
    format!(
        r##"<circle cx="10" cy="10" r="3.6" fill="none" stroke="#fff" stroke-width="1.2"/><g stroke="#fff" stroke-width="1.25" stroke-linecap="butt">{rays}</g>"##
    )
}

/// The Wi-Fi fan with `bars` (0-4) lit and the rest greyed, like Windows 10's signal icon.
fn wifi_svg(bars: u8) -> String {
    let mut s = String::new();
    for (i, d) in ARCS.iter().enumerate() {
        let lit = bars as usize >= 4 - i;
        let col = if lit { "#fff" } else { "#6a6a6a" };
        s += &format!(r##"<path d="{d}" fill="none" stroke="{col}" stroke-width="1.3" stroke-linecap="butt"/>"##);
    }
    let dot = if bars >= 1 { "#fff" } else { "#6a6a6a" };
    s + &format!(r##"<circle cx="10" cy="15.8" r="1.15" fill="{dot}"/>"##)
}

thread_local! {
    static GLYPHS: std::cell::RefCell<std::collections::HashMap<String, std::rc::Rc<crate::icons::Icon>>> = Default::default();
}

/// Rasterize (and cache) one glyph, centred at (x, y), in the theme's text colour.
pub fn glyph(c: &mut Canvas, body: String, x: f32, y: f32, size: u32) {
    glyph_ink(c, body, x, y, size, INK.get().unwrap_or(th().text));
}

thread_local! {
    static INK: std::cell::Cell<Option<Rgba>> = const { std::cell::Cell::new(None) };
}

/// Draw glyphs in `ink` instead of the theme's text colour, e.g. white on a lit accent tile.
pub fn with_ink<R>(ink: Rgba, f: impl FnOnce() -> R) -> R {
    let old = INK.replace(Some(ink));
    let r = f();
    INK.set(old);
    r
}

/// A glyph in a given colour: our SVG bodies are drawn in white (`#fff`) with unlit parts
/// in `#6a6a6a`; both are recoloured, so the same artwork works on dark, light and accent.
pub fn glyph_ink(c: &mut Canvas, body: String, x: f32, y: f32, size: u32, ink: Rgba) {
    let dim = if ink.0 as u32 + ink.1 as u32 + ink.2 as u32 > 384 { "#6a6a6a" } else { "#a8a8a8" };
    let ink = format!("\"#{:02x}{:02x}{:02x}\"", ink.0, ink.1, ink.2);
    let body = body.replace("\"#6a6a6a\"", &format!("\"{dim}\"")).replace("\"#fff\"", &ink);
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
pub fn speaker(c: &mut Canvas, x: f32, y: f32, muted: bool, level: f32, color: Rgba) {
    let mut body = SPEAKER.to_string();
    if muted {
        body += MUTE;
    } else {
        let waves = if level <= 0.01 { 0 } else if level < 0.34 { 1 } else if level < 0.67 { 2 } else { 3 };
        body.extend(WAVES.iter().take(waves).copied());
    }
    glyph_ink(c, body, x + 5.0, y, 20, color);
}

/// Sun: brightness.
pub fn sun(c: &mut Canvas, x: f32, y: f32, color: Rgba) {
    glyph_ink(c, sun_svg(), x, y, 20, color);
}

/// Wi-Fi: radio waves, all lit when `on`, struck through when off.
pub fn wifi(c: &mut Canvas, x: f32, y: f32, on: bool) {
    if on {
        glyph(c, wifi_svg(4), x, y, 20);
    } else {
        glyph(c, format!(r##"{}{}"##, wifi_svg(4), r##"<path d="M3 3l14 14" stroke="#fff" stroke-width="1.5" stroke-linecap="round"/>"##), x, y, 20);
    }
}

/// One network's signal strength, with a lock when it's secured.
fn wifi_signal(c: &mut Canvas, x: f32, y: f32, signal: u32, secure: bool) {
    glyph(c, wifi_svg(((signal + 24) / 25).min(4) as u8), x, y, 24);
    if secure {
        lock(c, x as i32 + 6, y as i32 + 2, th().text);
    }
}

fn ethernet_glyph(c: &mut Canvas, x: f32, y: f32) {
    glyph(
        c,
        r##"<rect x="2" y="3.5" width="11.5" height="8.5" rx="0.6" fill="none" stroke="#fff" stroke-width="1.2"/><path d="M5 15h5.5M7.75 12v3" stroke="#fff" stroke-width="1.2" stroke-linecap="round"/><path d="M15.5 3v10.5M14 5.5h3v3.5h-3z" fill="none" stroke="#fff" stroke-width="1.15" stroke-linejoin="round"/><path d="M14.5 15.5h2" stroke="#fff" stroke-width="1.2" stroke-linecap="round"/>"##.into(),
        x,
        y,
        20,
    );
}

pub fn airplane(c: &mut Canvas, x: f32, y: f32) {
    glyph(
        c,
        r##"<path d="M10 2.2c.7 0 1.1.6 1.1 1.5v4.5l6.4 3.7v1.6l-6.4-2v3.8l1.9 1.4v1.3L10 17.2l-3 .8v-1.3l1.9-1.4v-3.8l-6.4 2v-1.6l6.4-3.7V3.7c0-.9.4-1.5 1.1-1.5z" fill="none" stroke="#fff" stroke-width="1.1" stroke-linejoin="round"/>"##.into(),
        x,
        y,
        20,
    );
}

pub fn hotspot(c: &mut Canvas, x: f32, y: f32) {
    glyph(
        c,
        r##"<circle cx="10" cy="10" r="1.7" fill="#fff"/><path d="M6.6 6.6a4.8 4.8 0 0 0 0 6.8M13.4 6.6a4.8 4.8 0 0 1 0 6.8M4 4a8.5 8.5 0 0 0 0 12M16 4a8.5 8.5 0 0 1 0 12" fill="none" stroke="#fff" stroke-width="1.3" stroke-linecap="round"/>"##.into(),
        x,
        y,
        20,
    );
}

pub fn bluetooth(c: &mut Canvas, x: f32, y: f32) {
    glyph(
        c,
        r##"<path d="M10 2.5v15l4.6-3.7-9.2-7.6M10 17.5l4.6-3.7-9.2-7.6M10 2.5l4.6 3.7-4.6 3.8" fill="none" stroke="#fff" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>"##.into(),
        x,
        y,
        20,
    );
}

/// The taskbar's network icon.
pub fn net_icon(c: &mut Canvas, x: f32, y: f32, icon: NetIcon) {
    match icon {
        NetIcon::Airplane => airplane(c, x, y),
        NetIcon::Wired => ethernet_glyph(c, x, y),
        NetIcon::Wifi(bars) => glyph(c, wifi_svg(bars), x, y, 20),
        // Windows shows the fan greyed with an asterisk: networks available, none joined.
        NetIcon::Searching => glyph(c, wifi_svg(0) + r##"<path d="M15.5 13v4M13.5 14l4 2M17.5 14l-4 2" stroke="#fff" stroke-width="1.1" stroke-linecap="round"/>"##, x, y, 20),
        NetIcon::WifiOff => wifi(c, x, y, false),
    }
}
