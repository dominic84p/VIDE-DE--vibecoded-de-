//! VIDE-DE Settings: a Windows-Settings-style app for the desktop's options.
//! Writes ~/.config/vide-de/settings.conf; the compositor picks changes up live.
//!
//! System pages talk to the same services the tray uses: NetworkManager (`nmcli`),
//! BlueZ (`bluetoothctl`) and PipeWire/PulseAudio (`pactl`). Nothing here needs root;
//! operations that do (joining a protected network, pairing) are handled by the
//! desktop's polkit agent or the service's own agent.

use std::{
    path::PathBuf,
    process::Command,
    time::{Duration, Instant},
};

use eframe::egui::{self, Color32, FontId, RichText};
use vide_config::Config;

const BG: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
const SIDEBAR: Color32 = Color32::from_rgb(0x19, 0x19, 0x19);
const CARD: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
const ACCENT: Color32 = Color32::from_rgb(0x76, 0xb9, 0xed);
const DIM: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);
const TEXT: Color32 = Color32::WHITE;

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Personalization,
    Display,
    Sound,
    Network,
    Mouse,
    Gaming,
    Start,
    About,
}

impl Page {
    const ALL: [Page; 8] = [
        Page::Personalization,
        Page::Display,
        Page::Sound,
        Page::Network,
        Page::Mouse,
        Page::Gaming,
        Page::Start,
        Page::About,
    ];

    fn title(self) -> &'static str {
        match self {
            Page::Personalization => "Personalization",
            Page::Display => "Display",
            Page::Sound => "Sound",
            Page::Network => "Network & Internet",
            Page::Mouse => "Mouse",
            Page::Gaming => "Gaming",
            Page::Start => "Start",
            Page::About => "About",
        }
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Settings")
            .with_app_id("vide-settings")
            .with_inner_size([900.0, 680.0])
            .with_min_inner_size([560.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Settings",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            // `--page personalization` (from the desktop's Personalize item) opens straight there.
            let args: Vec<String> = std::env::args().collect();
            let page = match args.iter().position(|a| a == "--page").and_then(|i| args.get(i + 1)).map(String::as_str) {
                Some("personalization") => Page::Personalization,
                Some("display") => Page::Display,
                Some("sound") => Page::Sound,
                Some("network") => Page::Network,
                Some("mouse") => Page::Mouse,
                Some("start") => Page::Start,
                Some("gaming") => Page::Gaming,
                Some("about") => Page::About,
                _ => Page::Display,
            };
            let custom = String::new();
            Ok(Box::new(Settings {
                config: Config::load(),
                page,
                error: None,
                custom,
                system: System::default(),
            }))
        }),
    )
}

struct Settings {
    /// "Browse" path being typed on the Personalization page.
    custom: String,
    config: Config,
    page: Page,
    error: Option<String>,
    system: System,
}

impl Settings {
    fn save(&mut self) {
        self.error = self.config.save().err().map(|e| format!("Couldn't save settings: {e}"));
    }
}

/// A Windows-style settings row: label and description on the left, control on the right.
fn row(ui: &mut egui::Ui, title: &str, desc: &str, control: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let mut changed = false;
    egui::Frame::new().fill(CARD).corner_radius(6.0).inner_margin(14.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(title).color(Color32::WHITE).size(15.0));
                ui.label(RichText::new(desc).color(DIM).size(12.5));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                changed = control(ui);
            });
        });
    });
    ui.add_space(8.0);
    changed
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(text).color(Color32::WHITE).size(16.0));
    ui.add_space(4.0);
}

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

fn spawn(cmd: &str, args: &[&str]) {
    let _ = Command::new(cmd).args(args).spawn();
}

// ---------------------------------------------------------------- system state

#[derive(Default)]
struct System {
    checked: Option<Instant>,
    sinks: Vec<Sink>,
    sink_inputs: Vec<SinkInput>,
    default_sink: String,
    wifi_on: bool,
    wifi: Vec<Wifi>,
    wifi_password: String,
    wifi_target: Option<String>,
    bt_on: bool,
    bt_devices: Vec<BtDevice>,
}

#[derive(Clone)]
struct Sink {
    name: String,
    description: String,
    volume: u32,
    muted: bool,
}

#[derive(Clone)]
struct SinkInput {
    index: u32,
    app: String,
    title: String,
    volume: u32,
    muted: bool,
}

#[derive(Clone)]
struct Wifi {
    ssid: String,
    signal: u32,
    security: String,
    active: bool,
}

#[derive(Clone)]
struct BtDevice {
    mac: String,
    name: String,
    connected: bool,
}

impl System {
    fn refresh(&mut self, page: Page) {
        if self.checked.is_some_and(|t| t.elapsed() < Duration::from_secs(2)) {
            return;
        }
        self.checked = Some(Instant::now());
        match page {
            Page::Sound => self.refresh_sound(),
            Page::Network => {
                self.refresh_network();
                self.refresh_bluetooth();
            }
            _ => {}
        }
    }

    fn refresh_sound(&mut self) {
        self.sinks.clear();
        self.sink_inputs.clear();
        let Some(json) = run("pactl", &["-f", "json", "list", "sinks"]) else { return };
        if let Ok(serde_json::Value::Array(items)) = serde_json::from_str(&json) {
            for s in items {
                let name = s["name"].as_str().unwrap_or_default().to_string();
                let description = s["description"].as_str().unwrap_or(&name).to_string();
                let muted = s["mute"].as_bool().unwrap_or(false);
                let volume = volume_percent(&s["volume"]);
                self.sinks.push(Sink { name, description, volume, muted });
            }
        }
        self.default_sink = run("pactl", &["get-default-sink"]).map(|s| s.trim().to_string()).unwrap_or_default();

        if let Some(json) = run("pactl", &["-f", "json", "list", "sink-inputs"]) {
            if let Ok(serde_json::Value::Array(items)) = serde_json::from_str(&json) {
                for s in items {
                    let index = s["index"].as_u64().unwrap_or(0) as u32;
                    let props = &s["properties"];
                    let app = props["application.name"]
                        .as_str()
                        .or_else(|| props["application.process.binary"].as_str())
                        .unwrap_or("Application")
                        .to_string();
                    let title = s["description"].as_str().filter(|t| !t.is_empty()).unwrap_or("").to_string();
                    let muted = s["mute"].as_bool().unwrap_or(false);
                    let volume = volume_percent(&s["volume"]);
                    self.sink_inputs.push(SinkInput { index, app, title, volume, muted });
                }
            }
        }
    }

    fn refresh_network(&mut self) {
        self.wifi_on = run("nmcli", &["radio", "wifi"]).is_some_and(|s| s.trim() == "enabled");
        self.wifi.clear();
        let Some(out) = run("nmcli", &["-t", "-f", "IN-USE,SIGNAL,SECURITY,SSID", "device", "wifi", "list", "--rescan", "no"]) else { return };
        for line in out.lines() {
            let mut f = line.splitn(4, ':');
            let in_use = f.next().unwrap_or("").contains('*');
            let signal = f.next().unwrap_or("0").parse().unwrap_or(0);
            let security = f.next().unwrap_or("").replace("\\:", ":");
            let ssid = f.next().unwrap_or("").replace("\\:", ":");
            if ssid.is_empty() {
                continue;
            }
            if self.wifi.iter().any(|w| w.ssid == ssid) {
                continue;
            }
            self.wifi.push(Wifi { ssid, signal, security, active: in_use });
        }
        self.wifi.sort_by(|a, b| b.signal.cmp(&a.signal));
    }

    fn refresh_bluetooth(&mut self) {
        self.bt_on = run("bluetoothctl", &["show"]).is_some_and(|s| s.contains("Powered: yes"));
        self.bt_devices.clear();
        let Some(out) = run("bluetoothctl", &["devices", "Paired"]) else { return };
        for line in out.lines() {
            let Some(rest) = line.strip_prefix("Device ") else { continue };
            let Some((mac, name)) = rest.split_once(' ') else { continue };
            let connected = run("bluetoothctl", &["info", mac]).is_some_and(|s| s.contains("Connected: yes"));
            self.bt_devices.push(BtDevice { mac: mac.to_string(), name: name.to_string(), connected });
        }
    }
}

/// `pactl`'s JSON volume is a map of channel → {"value_percent": "75%"}; take the first.
fn volume_percent(v: &serde_json::Value) -> u32 {
    v.as_object()
        .and_then(|m| m.values().next())
        .and_then(|c| c["value_percent"].as_str())
        .and_then(|s| s.trim_end_matches('%').parse().ok())
        .unwrap_or(100)
}

// ---------------------------------------------------------------- display modes

/// Connected DRM connectors and their modes, from sysfs. Empty when nested (there's
/// no real connector to ask), in which case the page still lets you set scaling.
fn display_modes() -> Vec<(String, Vec<u32>)> {
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
            // sysfs gives resolution only; offer the rates monitors commonly advertise.
            out.push((m.to_string(), vec![60, 75, 100, 120, 144, 165, 240]));
        }
    }
    out.sort();
    out
}

fn fmt_res(r: &str) -> String {
    match r.split_once('x') {
        Some((w, h)) => format!("{w} × {h}"),
        None => r.to_string(),
    }
}

// ---------------------------------------------------------------- UI

impl eframe::App for Settings {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut visuals = egui::Visuals::dark();
        visuals.selection.bg_fill = ACCENT;
        ctx.set_visuals(visuals);
        self.system.refresh(self.page);

        egui::Panel::left("nav").exact_size(200.0).frame(egui::Frame::new().fill(SIDEBAR).inner_margin(12.0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Image::new(egui::include_image!("../../assets/logo-512.png")).max_height(28.0));
                ui.label(RichText::new("Settings").font(FontId::proportional(18.0)).color(Color32::WHITE));
            });
            ui.add_space(16.0);
            for p in Page::ALL {
                let selected = self.page == p;
                let text = RichText::new(p.title()).size(14.0).color(if selected { Color32::WHITE } else { DIM });
                if ui.add_sized([176.0, 34.0], egui::Button::selectable(selected, text)).on_hover_text(p.title()).clicked() {
                    self.page = p;
                    self.system.checked = None;
                }
            }
        });

        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(24.0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(self.page.title()).font(FontId::proportional(28.0)).color(Color32::WHITE));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |_ui| {});
            });
            ui.add_space(12.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                let mut changed = false;
                match self.page {
                    Page::Personalization => changed = self.personalization(ui),
                    Page::Display => changed = self.display(ui),
                    Page::Sound => self.sound(ui),
                    Page::Network => self.network(ui),
                    Page::Mouse => changed = self.mouse(ui),
                    Page::Gaming => {
                        changed |= row(
                            ui,
                            "Ultra Performance Mode",
                            "Fullscreen optimizations: fullscreen games go straight to the screen instead of being composited (direct scanout). Lower latency; may flicker on some NVIDIA setups.",
                            |ui| ui.checkbox(&mut self.config.direct_scanout, "").changed(),
                        );
                    }
                    Page::Start => {
                        changed |= row(
                            ui,
                            "Hide KDE apps in Start search",
                            "Keeps KDE's own apps out of the Start list and search. They stay installed and still open from anywhere else.",
                            |ui| ui.checkbox(&mut self.config.hide_kde_apps, "").changed(),
                        );
                    }
                    Page::About => {
                        row(ui, "VIDE-DE", "The vibecoded desktop environment. No human code was harmed.", |ui| {
                            if ui.button("Open About").clicked() {
                                let about = std::env::current_exe().ok().and_then(|p| Some(p.parent()?.join("vide-about")));
                                if let Some(p) = about {
                                    let _ = std::process::Command::new(p).spawn();
                                }
                            }
                            false
                        });
                        ui.label(RichText::new(format!("Settings file: {}", Config::path().display())).small().color(DIM));
                    }
                }
                if changed {
                    self.save();
                }
                if let Some(e) = &self.error {
                    ui.colored_label(Color32::LIGHT_RED, e);
                }
            });
        });
    }
}

impl Settings {
    fn personalization(&mut self, ui: &mut egui::Ui) -> bool {
        heading(ui, "Background");
        let current = format!("file://{}", self.config.wallpaper.display());
        ui.add(egui::Image::new(current).fit_to_exact_size(egui::vec2(384.0, 216.0)).corner_radius(4.0));
        ui.add_space(12.0);
        ui.label(RichText::new("Choose your picture").color(DIM).size(13.0));
        ui.add_space(4.0);
        let mut changed = false;
        let mut walls: Vec<_> = std::fs::read_dir(vide_config::wallpapers_dir())
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "png" || x == "jpg")).collect())
            .unwrap_or_default();
        walls.sort();
        let per_row = ((ui.available_width() / 172.0) as usize).max(1);
        for chunk in walls.chunks(per_row) {
            ui.horizontal(|ui| {
                for path in chunk.iter().cloned() {
                    let selected = path == self.config.wallpaper;
                    let img = egui::Image::new(format!("file://{}", path.display()))
                        .fit_to_exact_size(egui::vec2(160.0, 90.0))
                        .corner_radius(3.0);
                    let r = egui::Frame::new()
                        .stroke((if selected { 3.0 } else { 0.0 }, ACCENT))
                        .inner_margin(2.0)
                        .show(ui, |ui| ui.add(img.sense(egui::Sense::click())))
                        .inner;
                    if r.clicked() {
                        self.config.wallpaper = path.clone();
                        changed = true;
                    }
                }
            });
        }
        ui.add_space(12.0);
        changed |= row(ui, "Browse", "Use your own picture (PNG): type its full path and press Enter.", |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.custom).hint_text("/home/you/Pictures/wall.png").desired_width(280.0));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let p = PathBuf::from(self.custom.trim());
                if p.is_file() {
                    self.config.wallpaper = p;
                    return true;
                }
            }
            false
        });
        changed
    }

    fn display(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        heading(ui, "Display");
        ui.label(
            RichText::new("Resolution and refresh rate. The compositor applies these to every connected monitor that supports them; changes take effect immediately.")
                .color(DIM)
                .size(12.5),
        );
        ui.add_space(8.0);

        let modes = display_modes();
        let (cur_res, cur_hz) = match self.config.display_mode.split_once('@') {
            Some((r, hz)) => (r.to_string(), hz.to_string()),
            None => (self.config.display_mode.clone(), "60".into()),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new("Resolution").color(Color32::WHITE));
            let label = if cur_res.is_empty() { "Preferred".to_string() } else { fmt_res(&cur_res) };
            egui::ComboBox::from_id_salt("resolution").selected_text(label).width(180.0).show_ui(ui, |ui| {
                if ui.selectable_label(cur_res.is_empty(), "Preferred").clicked() {
                    self.config.display_mode.clear();
                    changed = true;
                }
                for (res, _) in &modes {
                    if ui.selectable_label(&cur_res == res, fmt_res(res)).clicked() {
                        self.config.display_mode = format!("{res}@{cur_hz}");
                        changed = true;
                    }
                }
                if modes.is_empty() {
                    ui.label(RichText::new("No DRM connector here (nested session)").color(DIM).small());
                }
            });
            ui.add_space(16.0);
            ui.label(RichText::new("Refresh rate").color(Color32::WHITE));
            let rates = modes
                .iter()
                .find(|(r, _)| *r == cur_res)
                .map(|(_, rates)| rates.clone())
                .unwrap_or_else(|| vec![60, 75, 120, 144, 165, 240]);
            egui::ComboBox::from_id_salt("refresh").selected_text(format!("{cur_hz} Hz")).width(90.0).show_ui(ui, |ui| {
                for hz in rates {
                    if ui.selectable_label(cur_hz == hz.to_string(), format!("{hz} Hz")).clicked() {
                        let res = if cur_res.is_empty() {
                            modes.first().map(|(r, _)| r.clone()).unwrap_or_else(|| "1920x1080".into())
                        } else {
                            cur_res.clone()
                        };
                        self.config.display_mode = format!("{res}@{hz}");
                        changed = true;
                    }
                }
            });
        });
        ui.add_space(8.0);

        changed |= row(ui, "Scale", "Make everything bigger or smaller. 100% is the monitor's native size.", |ui| {
            let mut pct = (self.config.display_scale * 100.0).round() as i32;
            let r = ui.add(egui::Slider::new(&mut pct, 50..=300).suffix("%").step_by(5.0).show_value(true));
            self.config.display_scale = pct as f64 / 100.0;
            r.changed()
        });
        changed |= row(ui, "Night light", "Warm the screen in the evening to cut blue light.", |ui| {
            ui.checkbox(&mut self.config.night_light, "").changed()
        });
        changed |= row(ui, "Night light strength", "How warm the screen gets when Night light is on.", |ui| {
            let mut pct = (self.config.night_light_strength * 100.0).round() as i32;
            let r = ui.add_enabled(self.config.night_light, egui::Slider::new(&mut pct, 0..=100).suffix("%").show_value(true));
            self.config.night_light_strength = pct as f64 / 100.0;
            r.changed()
        });
        changed
    }

    fn mouse(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        // Windows calls acceleration "Enhance pointer precision".
        let mut accel = !self.config.mouse_flat;
        changed |= row(ui, "Enhance pointer precision", "Mouse acceleration. Turn off for games and consistent aim.", |ui| {
            ui.checkbox(&mut accel, "").changed()
        });
        self.config.mouse_flat = !accel;
        changed |= row(ui, "Pointer speed", "How far the cursor moves for a given hand movement.", |ui| {
            ui.spacing_mut().slider_width = 220.0;
            ui.add(egui::Slider::new(&mut self.config.mouse_speed, -1.0..=1.0).show_value(false)).changed()
        });
        changed
    }

    fn sound(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Output");
        if self.system.sinks.is_empty() {
            ui.label(RichText::new("No PipeWire/PulseAudio sinks found. Is the sound server running?").color(DIM));
            ui.add_space(8.0);
        }
        let sinks = self.system.sinks.clone();
        let default = self.system.default_sink.clone();
        for sink in &sinks {
            let selected = sink.name == default;
            egui::Frame::new().fill(CARD).corner_radius(6.0).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if ui.radio(selected, "").clicked() && !selected {
                        spawn("pactl", &["set-default-sink", &sink.name]);
                        self.system.checked = None;
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&sink.description).color(Color32::WHITE).size(14.0));
                        ui.label(RichText::new(&sink.name).color(DIM).size(11.0));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut vol = sink.volume as f32;
                        if ui.add(egui::Slider::new(&mut vol, 0.0..=100.0).show_value(true).suffix("%")).changed() {
                            spawn("pactl", &["set-sink-volume", &sink.name, &format!("{}%", vol as u32)]);
                        }
                        let mute = if sink.muted { "Unmute" } else { "Mute" };
                        if ui.button(mute).clicked() {
                            spawn("pactl", &["set-sink-mute", &sink.name, "toggle"]);
                            self.system.checked = None;
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }

        heading(ui, "Applications");
        ui.label(RichText::new("Per-app volume, the same list PipeWire exposes. Drag a slider to change just that app.").color(DIM).size(12.5));
        ui.add_space(6.0);
        let inputs = self.system.sink_inputs.clone();
        if inputs.is_empty() {
            ui.label(RichText::new("Nothing is playing right now.").color(DIM));
        }
        for input in &inputs {
            egui::Frame::new().fill(CARD).corner_radius(6.0).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&input.app).color(Color32::WHITE).size(14.0));
                        if !input.title.is_empty() {
                            ui.label(RichText::new(&input.title).color(DIM).size(11.0));
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut vol = input.volume as f32;
                        if ui.add(egui::Slider::new(&mut vol, 0.0..=100.0).show_value(true).suffix("%")).changed() {
                            spawn("pactl", &["set-sink-input-volume", &input.index.to_string(), &format!("{}%", vol as u32)]);
                        }
                        let mute = if input.muted { "Unmute" } else { "Mute" };
                        if ui.button(mute).clicked() {
                            spawn("pactl", &["set-sink-input-mute", &input.index.to_string(), "toggle"]);
                            self.system.checked = None;
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }
    }

    fn network(&mut self, ui: &mut egui::Ui) {
        // Wi-Fi section
        self.wifi_section(ui);
        ui.add_space(24.0);
        // Bluetooth section
        self.bluetooth_section(ui);
    }

    fn wifi_section(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Wi-Fi");
        let mut on = self.system.wifi_on;
        if row(ui, "Wi-Fi", "Connect to wireless networks.", |ui| {
            let r = ui.checkbox(&mut on, "");
            r.changed()
        }) {
            spawn("nmcli", &["radio", "wifi", if on { "on" } else { "off" }]);
            self.system.checked = None;
        }
        ui.add_space(8.0);

        if !self.system.wifi_on {
            ui.label(RichText::new("Wi-Fi is turned off. Turn it on to see available networks.").color(DIM).size(13.0));
            ui.add_space(8.0);
            return;
        }

        // Show current connection
        if let Some(connected) = self.system.wifi.iter().find(|n| n.active) {
            egui::Frame::new().fill(Color32::from_rgb(0x00, 0x5a, 0x9e)).corner_radius(6.0).inner_margin(12.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&connected.ssid).color(Color32::WHITE).size(16.0));
                        let sec = if connected.security.is_empty() { "Open".to_string() } else { "Secured".to_string() };
                        ui.label(RichText::new(format!("{} · {}", sec, signal_bars(connected.signal))).color(Color32::from_rgb(0xb3, 0xd4, 0xed)).size(12.0));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Disconnect").clicked() {
                            spawn("nmcli", &["connection", "down", "id", &connected.ssid]);
                            self.system.checked = None;
                        }
                        if ui.button("Properties").clicked() {
                            // Could show IP, DNS, etc.
                        }
                    });
                });
            });
            ui.add_space(8.0);
        }

        // Available networks
        ui.label(RichText::new("Available networks").color(DIM).size(13.0));
        ui.add_space(4.0);
        
        ui.horizontal(|ui| {
            if ui.button("Show available networks").clicked() {
                spawn("nmcli", &["device", "wifi", "list", "--rescan", "yes"]);
                self.system.checked = None;
            }
            if ui.button("Refresh").clicked() {
                self.system.checked = None;
            }
            if ui.button("Hardware properties").clicked() {
                // Show adapter info
            }
        });
        ui.add_space(8.0);

        let networks: Vec<_> = self.system.wifi.iter()
            .filter(|n| !n.active)
            .cloned()
            .collect();
        
        if networks.is_empty() {
            ui.label(RichText::new("No networks found.").color(DIM).size(13.0));
        } else {
            for net in &networks {
                egui::Frame::new().fill(CARD).corner_radius(4.0).inner_margin(10.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        // Signal bars
                        signal_bars_ui(ui, net.signal);
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&net.ssid).color(Color32::WHITE).size(14.0));
                            let sec = if net.security.is_empty() { "Open" } else { "Secured" };
                            ui.label(RichText::new(format!("{} · {}%", sec, net.signal)).color(DIM).size(11.0));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Connect").clicked() {
                                if net.security.is_empty() {
                                    spawn("nmcli", &["device", "wifi", "connect", &net.ssid]);
                                    self.system.checked = None;
                                } else {
                                    self.system.wifi_target = Some(net.ssid.clone());
                                    self.system.wifi_password.clear();
                                }
                            }
                        });
                    });
                });
                ui.add_space(4.0);
            }
        }

        // Password dialog
        if let Some(target) = self.system.wifi_target.clone() {
            let mut open = true;
            egui::Window::new("Enter network security key")
                .collapsible(false).resizable(false).open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ui.ctx(), |ui| {
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("Enter the security key for \"{target}\"")).color(TEXT).size(13.0));
                    ui.add_space(8.0);
                    let r = ui.add(egui::TextEdit::singleline(&mut self.system.wifi_password)
                        .password(true)
                        .desired_width(300.0)
                        .hint_text("Security key"));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        spawn("nmcli", &["device", "wifi", "connect", &target, "password", &self.system.wifi_password]);
                        self.system.wifi_target = None;
                        self.system.checked = None;
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Connect").clicked() {
                                spawn("nmcli", &["device", "wifi", "connect", &target, "password", &self.system.wifi_password]);
                                self.system.wifi_target = None;
                                self.system.checked = None;
                            }
                            if ui.button("Cancel").clicked() {
                                self.system.wifi_target = None;
                            }
                        });
                    });
                    ui.add_space(8.0);
                });
            if !open {
                self.system.wifi_target = None;
            }
        }
    }

    fn bluetooth_section(&mut self, ui: &mut egui::Ui) {
        heading(ui, "Bluetooth");
        let mut on = self.system.bt_on;
        if row(ui, "Bluetooth", "Connect to Bluetooth devices.", |ui| {
            let r = ui.checkbox(&mut on, "");
            r.changed()
        }) {
            spawn("bluetoothctl", &["power", if on { "on" } else { "off" }]);
            self.system.checked = None;
        }
        ui.add_space(8.0);

        if !self.system.bt_on {
            ui.label(RichText::new("Bluetooth is turned off. Turn it on to connect devices.").color(DIM).size(13.0));
            ui.add_space(8.0);
            return;
        }

        ui.horizontal(|ui| {
            if ui.button("Add device").clicked() {
                let _ = Command::new("bluetoothctl").args(["--timeout", "30", "scan", "on"]).spawn();
            }
            if ui.button("Refresh").clicked() {
                self.system.checked = None;
            }
            if ui.button("More Bluetooth options").clicked() {
                // Could open a dialog
            }
        });
        ui.add_space(8.0);

        let devices = self.system.bt_devices.clone();
        if devices.is_empty() {
            ui.label(RichText::new("No paired devices. Click \"Add device\" to pair a new one.").color(DIM).size(13.0));
        } else {
            ui.label(RichText::new("Paired devices").color(DIM).size(13.0));
            ui.add_space(4.0);
            for dev in &devices {
                egui::Frame::new().fill(CARD).corner_radius(4.0).inner_margin(10.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        // Bluetooth icon placeholder
                        ui.add(egui::Label::new(RichText::new("🔷").size(24.0)));
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&dev.name).color(Color32::WHITE).size(14.0));
                            ui.label(RichText::new(&dev.mac).color(DIM).size(11.0));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if dev.connected {
                                ui.label(RichText::new("Connected").color(Color32::from_rgb(0x00, 0xaa, 0x44)).size(12.0));
                                ui.add_space(12.0);
                                if ui.button("Disconnect").clicked() {
                                    spawn("bluetoothctl", &["disconnect", &dev.mac]);
                                    self.system.checked = None;
                                }
                            } else {
                                if ui.button("Connect").clicked() {
                                    spawn("bluetoothctl", &["connect", &dev.mac]);
                                    self.system.checked = None;
                                }
                            }
                            ui.add_space(8.0);
                            if ui.button("Remove device").clicked() {
                                spawn("bluetoothctl", &["remove", &dev.mac]);
                                self.system.checked = None;
                            }
                        });
                    });
                });
                ui.add_space(4.0);
            }
        }
    }
}

fn signal_bars(signal: u32) -> String {
    let bars = ((signal as f32 / 100.0) * 4.0).round() as usize;
    "▂▃▄▅".chars().take(bars.min(4)).collect::<String>()
}

fn signal_bars_ui(ui: &mut egui::Ui, signal: u32) {
    let bars = ((signal as f32 / 100.0) * 4.0).round() as usize;
    ui.horizontal(|ui| {
        for i in 0..4 {
            let active = i < bars;
            let color = if active { Color32::from_rgb(0x00, 0x78, 0xd7) } else { DIM };
            let h = 4.0 + i as f32 * 4.0;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(3.0, h), egui::Sense::hover());
            ui.painter().rect_filled(rect, 0.0, color);
        }
    });
}

fn bars(ui: &mut egui::Ui, signal: u32) {
    signal_bars_ui(ui, signal);
}
