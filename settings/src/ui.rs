use std::path::PathBuf;

use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Stroke, StrokeKind, Vec2};
use vide_config::Config;

use crate::system::{display_modes, fmt_bytes, fmt_res, launch_desktop_exec, run, spawn};
use crate::{Category, Settings, SubPage};

/// Display scales offered, in percent (Windoze's list).
const SCALES: [i32; 7] = [100, 125, 150, 175, 200, 250, 300];

pub const ACCENT: Color32 = Color32::from_rgb(0x00, 0x78, 0xd7);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x10, 0x6e, 0xbe);
pub const BG: Color32 = Color32::WHITE;
pub const SIDEBAR: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
pub const CARD: Color32 = Color32::from_rgb(0xf8, 0xf8, 0xf8);
pub const CARD_HOVER: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
pub const BORDER: Color32 = Color32::from_rgb(0xd4, 0xd4, 0xd4);
pub const TEXT: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);
pub const DIM: Color32 = Color32::from_rgb(0x60, 0x60, 0x60);

pub fn apply_doze_style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::light();
    visuals.window_fill = BG;
    visuals.panel_fill = BG;
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.noninteractive.corner_radius = 0.0.into();
    visuals.widgets.inactive.corner_radius = 0.0.into();
    visuals.widgets.hovered.corner_radius = 0.0.into();
    visuals.widgets.active.corner_radius = 0.0.into();
    visuals.widgets.open.corner_radius = 0.0.into();
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(0xe6, 0xe6, 0xe6);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0xd6, 0xd6, 0xd6);
    visuals.widgets.active.bg_fill = Color32::from_rgb(0xc6, 0xc6, 0xc6);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0xc8, 0xc8, 0xc8));
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x88, 0x88, 0x88));
    visuals.window_corner_radius = 0.0.into();
    ctx.set_visuals(visuals);
}

/// Windoze 10 pill toggle switch with "On" / "Off" label.
/// "io.github.astralvixen.geforce-infinity" -> "Geforce Infinity": the last part, spaced and capitalised.
fn humanize_app_id(id: &str) -> String {
    let last = id.rsplit('.').next().unwrap_or(id);
    last.split(['-', '_'])
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Turn one xkb option on or off in the comma-separated list, keeping the rest.
fn set_xkb_option(options: &mut String, opt: &str, on: bool) {
    let mut list: Vec<&str> = options.split(',').filter(|o| !o.is_empty() && *o != opt).collect();
    if on {
        list.push(opt);
    }
    *options = list.join(",");
}

pub fn doze_toggle(ui: &mut egui::Ui, value: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let size = Vec2::new(44.0, 20.0);
        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
        if resp.clicked() {
            *value = !*value;
            changed = true;
        }
        let painter = ui.painter();
        let r = rect.height() / 2.0;
        if *value {
            let fill = if resp.hovered() { ACCENT_HOVER } else { ACCENT };
            painter.rect_filled(rect, r, fill);
            let knob = Pos2::new(rect.right() - r, rect.center().y);
            painter.circle_filled(knob, 5.0, Color32::WHITE);
        } else {
            let stroke_color = if resp.hovered() { TEXT } else { Color32::from_rgb(0x55, 0x55, 0x55) };
            painter.rect_stroke(rect, r, Stroke::new(1.6, stroke_color), StrokeKind::Inside);
            let knob = Pos2::new(rect.left() + r, rect.center().y);
            painter.circle_filled(knob, 5.0, stroke_color);
        }
        ui.add_space(6.0);
        ui.label(RichText::new(if *value { "On" } else { "Off" }).color(TEXT).size(13.5));
    });
    changed
}

pub fn section_title(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).font(FontId::proportional(17.0)).color(TEXT));
    ui.add_space(6.0);
}

pub fn setting_block(ui: &mut egui::Ui, title: &str, desc: &str, control: impl FnOnce(&mut egui::Ui) -> bool) -> bool {
    let mut changed = false;
    ui.vertical(|ui| {
        ui.label(RichText::new(title).color(TEXT).size(14.5));
        if !desc.is_empty() {
            ui.label(RichText::new(desc).color(DIM).size(12.5));
        }
        ui.add_space(6.0);
        changed = control(ui);
    });
    ui.add_space(14.0);
    changed
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .inner_margin(12.0)
}

pub fn kv_row(ui: &mut egui::Ui, key: &str, val: &str) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(170.0, 18.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(RichText::new(key).color(DIM).size(13.0));
            },
        );
        ui.label(RichText::new(val).color(TEXT).size(13.0));
    });
    ui.add_space(2.0);
}

pub fn draw_category_icon(painter: &egui::Painter, rect: Rect, cat: Category, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.42;
    let stroke = Stroke::new(1.8, color);
    match cat {
        Category::System => {
            // Monitor
            let scr = Rect::from_center_size(c - Vec2::new(0.0, s * 0.12), Vec2::new(s * 1.7, s * 1.15));
            painter.rect_stroke(scr, 0.0, stroke, StrokeKind::Inside);
            painter.line_segment([Pos2::new(c.x, scr.bottom()), Pos2::new(c.x, scr.bottom() + s * 0.3)], stroke);
            painter.line_segment(
                [Pos2::new(c.x - s * 0.45, scr.bottom() + s * 0.3), Pos2::new(c.x + s * 0.45, scr.bottom() + s * 0.3)],
                stroke,
            );
        }
        Category::Devices => {
            // Keyboard / speaker / mouse glyph
            let body = Rect::from_center_size(c, Vec2::new(s * 1.65, s * 1.1));
            painter.rect_stroke(body, 0.0, stroke, StrokeKind::Inside);
            for i in -2..=2 {
                let x = c.x + i as f32 * s * 0.25;
                painter.line_segment([Pos2::new(x, c.y - s * 0.22), Pos2::new(x + s * 0.1, c.y - s * 0.22)], stroke);
            }
            painter.line_segment([Pos2::new(c.x - s * 0.4, c.y + s * 0.22), Pos2::new(c.x + s * 0.4, c.y + s * 0.22)], stroke);
        }
        Category::Network => {
            // Globe / network
            painter.circle_stroke(c, s * 0.78, stroke);
            painter.line_segment([Pos2::new(c.x - s * 0.78, c.y), Pos2::new(c.x + s * 0.78, c.y)], stroke);
            painter.line_segment([Pos2::new(c.x, c.y - s * 0.78), Pos2::new(c.x, c.y + s * 0.78)], stroke);
            painter.rect_stroke(Rect::from_center_size(c, Vec2::new(s * 0.72, s * 1.48)), s * 0.36, stroke, StrokeKind::Inside);
        }
        Category::Personalization => {
            // Brush / canvas
            let frame = Rect::from_center_size(c, Vec2::new(s * 1.5, s * 1.25));
            painter.rect_stroke(frame, 0.0, stroke, StrokeKind::Inside);
            painter.line_segment(
                [Pos2::new(frame.left() + 2.0, frame.bottom() - 3.0), Pos2::new(c.x - s * 0.1, c.y + s * 0.1)],
                stroke,
            );
            painter.line_segment(
                [Pos2::new(c.x - s * 0.1, c.y + s * 0.1), Pos2::new(frame.right() - 3.0, frame.top() + 3.0)],
                stroke,
            );
        }
        Category::Apps => {
            // List / tiles
            for row_i in -1..=1 {
                let y = c.y + row_i as f32 * s * 0.48;
                painter.rect_stroke(Rect::from_center_size(Pos2::new(c.x - s * 0.55, y), Vec2::splat(s * 0.28)), 0.0, stroke, StrokeKind::Inside);
                painter.line_segment([Pos2::new(c.x - s * 0.2, y), Pos2::new(c.x + s * 0.7, y)], stroke);
            }
        }
        Category::Accounts => {
            // Person
            painter.circle_stroke(Pos2::new(c.x, c.y - s * 0.32), s * 0.34, stroke);
            let arc = Rect::from_center_size(Pos2::new(c.x, c.y + s * 0.45), Vec2::new(s * 1.25, s * 0.75));
            painter.rect_stroke(arc, s * 0.35, stroke, StrokeKind::Inside);
        }
        Category::TimeLanguage => {
            // Clock
            painter.circle_stroke(c, s * 0.78, stroke);
            painter.line_segment([c, Pos2::new(c.x, c.y - s * 0.48)], stroke);
            painter.line_segment([c, Pos2::new(c.x + s * 0.38, c.y + s * 0.18)], stroke);
        }
        Category::Gaming => {
            // Gamepad D-pad + buttons
            let body = Rect::from_center_size(c, Vec2::new(s * 1.65, s * 1.0));
            painter.rect_stroke(body, s * 0.35, stroke, StrokeKind::Inside);
            let d = Pos2::new(c.x - s * 0.38, c.y);
            painter.line_segment([Pos2::new(d.x - s * 0.2, d.y), Pos2::new(d.x + s * 0.2, d.y)], stroke);
            painter.line_segment([Pos2::new(d.x, d.y - s * 0.2), Pos2::new(d.x, d.y + s * 0.2)], stroke);
            painter.circle_filled(Pos2::new(c.x + s * 0.35, c.y - s * 0.12), 2.0, color);
            painter.circle_filled(Pos2::new(c.x + s * 0.5, c.y + s * 0.12), 2.0, color);
        }
        Category::EaseOfAccess => {
            // Accessibility circle + person
            painter.circle_stroke(c, s * 0.8, stroke);
            painter.circle_filled(Pos2::new(c.x, c.y - s * 0.3), 2.2, color);
            painter.line_segment([Pos2::new(c.x - s * 0.4, c.y - s * 0.05), Pos2::new(c.x + s * 0.4, c.y - s * 0.05)], stroke);
            painter.line_segment([Pos2::new(c.x, c.y - s * 0.05), Pos2::new(c.x - s * 0.22, c.y + s * 0.45)], stroke);
            painter.line_segment([Pos2::new(c.x, c.y - s * 0.05), Pos2::new(c.x + s * 0.22, c.y + s * 0.45)], stroke);
        }
        Category::Privacy => {
            // Padlock
            let body = Rect::from_center_size(Pos2::new(c.x, c.y + s * 0.3), Vec2::new(s * 1.35, s * 1.0));
            painter.rect_stroke(body, 2.0, stroke, StrokeKind::Inside);
            let shackle = Rect::from_center_size(Pos2::new(c.x, c.y - s * 0.38), Vec2::new(s * 0.85, s * 0.9));
            painter.rect_stroke(shackle, s * 0.42, stroke, StrokeKind::Inside);
            painter.circle_filled(Pos2::new(c.x, c.y + s * 0.3), 2.2, color);
        }
        Category::UpdateSecurity => {
            // Sync / circular arrows
            painter.circle_stroke(c, s * 0.72, stroke);
            painter.line_segment([Pos2::new(c.x + s * 0.35, c.y - s * 0.65), Pos2::new(c.x + s * 0.72, c.y - s * 0.3)], stroke);
            painter.line_segment([Pos2::new(c.x - s * 0.35, c.y + s * 0.65), Pos2::new(c.x - s * 0.72, c.y + s * 0.3)], stroke);
        }
    }
}

pub fn draw_home_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.38;
    let stroke = Stroke::new(1.5, color);
    let top = Pos2::new(c.x, c.y - s);
    let left = Pos2::new(c.x - s, c.y - s * 0.05);
    let right = Pos2::new(c.x + s, c.y - s * 0.05);
    painter.line_segment([top, left], stroke);
    painter.line_segment([top, right], stroke);
    let bl = Pos2::new(c.x - s * 0.72, c.y + s * 0.85);
    let br = Pos2::new(c.x + s * 0.72, c.y + s * 0.85);
    painter.line_segment([Pos2::new(c.x - s * 0.72, c.y - s * 0.25), bl], stroke);
    painter.line_segment([Pos2::new(c.x + s * 0.72, c.y - s * 0.25), br], stroke);
    painter.line_segment([bl, br], stroke);
}

pub fn draw_search_icon(painter: &egui::Painter, rect: Rect, color: Color32) {
    let c = rect.center() - Vec2::splat(1.5);
    let r = rect.width().min(rect.height()) * 0.26;
    let stroke = Stroke::new(1.5, color);
    painter.circle_stroke(c, r, stroke);
    painter.line_segment([c + Vec2::splat(r * 0.7), c + Vec2::splat(r * 1.65)], stroke);
}

pub fn signal_bars_ui(ui: &mut egui::Ui, signal: u8) {
    let bars = if signal >= 75 { 4 } else if signal >= 50 { 3 } else if signal >= 25 { 2 } else { 1 };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(20.0, 16.0), egui::Sense::hover());
    let painter = ui.painter();
    for i in 0..4 {
        let h = 4.0 + i as f32 * 3.0;
        let x = rect.left() + i as f32 * 5.0;
        let y = rect.bottom() - h;
        let bar = Rect::from_min_size(Pos2::new(x, y), Vec2::new(3.0, h));
        let col = if i < bars { ACCENT } else { Color32::from_rgb(0xcc, 0xcc, 0xcc) };
        painter.rect_filled(bar, 0.0, col);
    }
}

impl Settings {
    pub fn render_subpage(&mut self, ui: &mut egui::Ui, page: SubPage) -> bool {
        match page {
            SubPage::Display => self.page_display(ui),
            SubPage::Sound => {
                self.page_sound(ui);
                false
            }
            SubPage::Notifications => self.page_notifications(ui),
            SubPage::Power => self.page_power(ui),
            SubPage::Storage => {
                self.page_storage(ui);
                false
            }
            SubPage::About => {
                self.page_about(ui);
                false
            }
            SubPage::Bluetooth => {
                self.page_bluetooth(ui);
                false
            }
            SubPage::Mouse => self.page_mouse(ui),
            SubPage::Touchpad => self.page_touchpad(ui),
            SubPage::Keyboard => self.page_keyboard(ui),
            SubPage::NetStatus => {
                self.page_net_status(ui);
                false
            }
            SubPage::Wifi => {
                self.page_wifi(ui);
                false
            }
            SubPage::Ethernet => {
                self.page_ethernet(ui);
                false
            }
            SubPage::Background => self.page_background(ui),
            SubPage::Colors => self.page_colors(ui),
            SubPage::Start => self.page_start(ui),
            SubPage::Taskbar => {
                self.page_taskbar(ui);
                false
            }
            SubPage::AppsFeatures => {
                self.page_apps_features(ui);
                false
            }
            SubPage::DefaultApps => {
                self.page_default_apps(ui);
                false
            }
            SubPage::YourInfo => {
                self.page_your_info(ui);
                false
            }
            SubPage::DateTime => self.page_date_time(ui),
            SubPage::Language => {
                self.page_language(ui);
                false
            }
            SubPage::GameMode => self.page_game_mode(ui),
            SubPage::Captures => {
                self.page_captures(ui);
                false
            }
            SubPage::EaseDisplay => self.page_ease_display(ui),
            SubPage::EaseMouse => self.page_mouse(ui),
            SubPage::AppPermissions => {
                self.page_privacy(ui);
                false
            }
            SubPage::UpdateStatus => {
                self.page_update_status(ui);
                false
            }
        }
    }

    fn page_display(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        section_title(ui, "Color");
        changed |= setting_block(ui, "Night light", "Use warmer colors to help block blue light.", |ui| {
            doze_toggle(ui, &mut self.config.night_light)
        });
        changed |= setting_block(ui, "Night light strength", "Adjust color temperature warmth when Night light is on.", |ui| {
            let mut pct = (self.config.night_light_strength * 100.0).round() as i32;
            let r = ui.add_enabled(self.config.night_light, egui::Slider::new(&mut pct, 0..=100).suffix("%"));
            self.config.night_light_strength = pct as f64 / 100.0;
            r.changed()
        });

        section_title(ui, "Scale and layout");
        changed |= setting_block(ui, "Change the size of text, apps, and other items", "100% is native size. Apps that support fractional scaling stay sharp at 125% and 150%; older ones are stretched.", |ui| {
            let mut c = false;
            ui.horizontal(|ui| {
                let cur = (self.config.display_scale * 100.0).round() as i32;
                for pct in SCALES {
                    if ui.selectable_label(cur == pct, format!("{pct}%")).clicked() {
                        self.config.display_scale = pct as f64 / 100.0;
                        c = true;
                    }
                }
            });
            c
        });

        let modes = display_modes();
        let (cur_res, cur_hz) = match self.config.display_mode.split_once('@') {
            Some((r, hz)) => (r.to_string(), hz.to_string()),
            None => (self.config.display_mode.clone(), "60".into()),
        };
        setting_block(ui, "Display resolution & refresh rate", "Applies immediately to connected monitors in TTY mode.", |ui| {
            let mut c = false;
            ui.horizontal(|ui| {
                let label = if cur_res.is_empty() { "Preferred".to_string() } else { fmt_res(&cur_res) };
                egui::ComboBox::from_id_salt("resolution").selected_text(label).width(190.0).show_ui(ui, |ui| {
                    if ui.selectable_label(cur_res.is_empty(), "Preferred").clicked() {
                        self.config.display_mode.clear();
                        c = true;
                    }
                    for (res, _) in &modes {
                        if ui.selectable_label(&cur_res == res, fmt_res(res)).clicked() {
                            self.config.display_mode = format!("{res}@{cur_hz}");
                            c = true;
                        }
                    }
                    if modes.is_empty() {
                        ui.label(RichText::new("Nested session (no DRM connector)").color(DIM).small());
                    }
                });
                ui.add_space(12.0);
                let rates = modes
                    .iter()
                    .find(|(r, _)| *r == cur_res)
                    .map(|(_, rates)| rates.clone())
                    .unwrap_or_else(|| vec![60, 75, 120, 144, 165, 240]);
                egui::ComboBox::from_id_salt("refresh").selected_text(format!("{cur_hz} Hz")).width(100.0).show_ui(ui, |ui| {
                    for hz in rates {
                        if ui.selectable_label(cur_hz == hz.to_string(), format!("{hz} Hz")).clicked() {
                            let res = if cur_res.is_empty() {
                                modes.first().map(|(r, _)| r.clone()).unwrap_or_else(|| "1920x1080".into())
                            } else {
                                cur_res.clone()
                            };
                            self.config.display_mode = format!("{res}@{hz}");
                            c = true;
                        }
                    }
                });
            });
            c
        }) | changed
    }

    fn page_sound(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Output");
        if self.system.sinks.is_empty() {
            ui.label(RichText::new("No PipeWire/PulseAudio output devices found.").color(DIM));
        }
        let sinks = self.system.sinks.clone();
        let default_sink = self.system.default_sink.clone();
        for sink in &sinks {
            let selected = sink.name == default_sink;
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if ui.radio(selected, "").clicked() && !selected {
                        spawn("pactl", &["set-default-sink", &sink.name]);
                        self.system.invalidate();
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&sink.description).color(TEXT).size(14.0));
                        ui.label(RichText::new(&sink.name).color(DIM).size(11.0));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut vol = sink.volume as f32;
                        if ui.add(egui::Slider::new(&mut vol, 0.0..=100.0).suffix("%")).changed() {
                            spawn("pactl", &["set-sink-volume", &sink.name, &format!("{}%", vol as u32)]);
                        }
                        if ui.button(if sink.muted { "Unmute" } else { "Mute" }).clicked() {
                            spawn("pactl", &["set-sink-mute", &sink.name, "toggle"]);
                            self.system.invalidate();
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }

        section_title(ui, "Input");
        if self.system.sources.is_empty() {
            ui.label(RichText::new("No microphones / input devices found.").color(DIM));
        }
        let sources = self.system.sources.clone();
        let default_src = self.system.default_source.clone();
        for src in &sources {
            let selected = src.name == default_src;
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if ui.radio(selected, "").clicked() && !selected {
                        spawn("pactl", &["set-default-source", &src.name]);
                        self.system.invalidate();
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&src.description).color(TEXT).size(14.0));
                        ui.label(RichText::new(&src.name).color(DIM).size(11.0));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut vol = src.volume as f32;
                        if ui.add(egui::Slider::new(&mut vol, 0.0..=100.0).suffix("%")).changed() {
                            spawn("pactl", &["set-source-volume", &src.name, &format!("{}%", vol as u32)]);
                        }
                        if ui.button(if src.muted { "Unmute" } else { "Mute" }).clicked() {
                            spawn("pactl", &["set-source-mute", &src.name, "toggle"]);
                            self.system.invalidate();
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }

        section_title(ui, "App volume and device preferences");
        let inputs = self.system.sink_inputs.clone();
        if inputs.is_empty() {
            ui.label(RichText::new("No applications are currently playing audio.").color(DIM));
        }
        for input in &inputs {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&input.app).color(TEXT).size(14.0));
                        if !input.title.is_empty() {
                            ui.label(RichText::new(&input.title).color(DIM).size(11.0));
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let mut vol = input.volume as f32;
                        if ui.add(egui::Slider::new(&mut vol, 0.0..=100.0).suffix("%")).changed() {
                            spawn("pactl", &["set-sink-input-volume", &input.index.to_string(), &format!("{}%", vol as u32)]);
                        }
                        if ui.button(if input.muted { "Unmute" } else { "Mute" }).clicked() {
                            spawn("pactl", &["set-sink-input-mute", &input.index.to_string(), "toggle"]);
                            self.system.invalidate();
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }
    }

    fn page_notifications(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        section_title(ui, "Notifications");
        changed |= setting_block(ui, "Show notification popups", "Notifications always go to the Action Center. This is the popup in the corner.", |ui| {
            doze_toggle(ui, &mut self.config.toasts)
        });
        changed |= setting_block(ui, "How long popups stay", "Apps can ask for longer, and urgent notifications stay until you close them.", |ui| {
            let mut c = false;
            ui.add_enabled_ui(self.config.toasts, |ui| {
                egui::ComboBox::from_id_salt("toast_secs").selected_text(format!("{} seconds", self.config.toast_seconds)).width(150.0).show_ui(ui, |ui| {
                    for secs in [3u32, 5, 8, 10, 15, 30] {
                        if ui.selectable_label(self.config.toast_seconds == secs, format!("{secs} seconds")).clicked() {
                            self.config.toast_seconds = secs;
                            c = true;
                        }
                    }
                });
            });
            c
        });
        changed |= setting_block(ui, "Notification sound", "Plays when a notification arrives, except in Focus assist.", |ui| {
            let mut c = false;
            let name = |id: &str| vide_config::SOUNDS.iter().find(|(s, _)| *s == id).map_or("None", |(_, n)| *n);
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("notif_sound").selected_text(name(&self.config.notification_sound)).width(150.0).show_ui(ui, |ui| {
                    for (id, label) in vide_config::SOUNDS.iter().copied().chain([("none", "None")]) {
                        if ui.selectable_label(self.config.notification_sound == id, label).clicked() {
                            self.config.notification_sound = id.to_string();
                            preview_sound(id);
                            c = true;
                        }
                    }
                });
                let can_play = vide_config::sound_file(&self.config.notification_sound).is_some();
                if ui.add_enabled(can_play, egui::Button::new("▶  Play")).clicked() {
                    preview_sound(&self.config.notification_sound);
                }
            });
            c
        });
        ui.add_space(12.0);
        section_title(ui, "Tray");
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Apps like Discord, Steam and nm-applet put their icons on the taskbar by themselves.").color(DIM).size(12.5));
        });
        ui.add_space(12.0);
        changed |= setting_block(ui, "Hide KDE apps in Start search", "Keeps KDE utility entries out of Start menu search.", |ui| {
            doze_toggle(ui, &mut self.config.hide_kde_apps)
        });
        changed
    }

    fn page_touchpad(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        section_title(ui, "Touchpad");
        ui.label(RichText::new("These apply to touchpads on real hardware, as soon as you change them.").color(DIM).size(12.5));
        ui.add_space(6.0);
        changed |= setting_block(ui, "Tap to click", "Tap the touchpad to click; two fingers to right-click.", |ui| doze_toggle(ui, &mut self.config.tap_to_click));
        changed |= setting_block(ui, "Natural scrolling", "Content follows your fingers, like a phone: swipe up and the page moves up.", |ui| {
            doze_toggle(ui, &mut self.config.natural_scroll)
        });
        changed |= setting_block(ui, "Ignore the touchpad while typing", "Stops your palm from moving the cursor mid-sentence.", |ui| {
            doze_toggle(ui, &mut self.config.disable_while_typing)
        });
        changed
    }

    fn page_power(&mut self, ui: &mut egui::Ui) -> bool {
        let changed = false;
        if let Some(profile) = self.system.power_profile.clone() {
            section_title(ui, "Power mode");
            ui.horizontal(|ui| {
                for (id, label) in [("power-saver", "Best power efficiency"), ("balanced", "Balanced"), ("performance", "Best performance")] {
                    if ui.selectable_label(profile == id, label).clicked() {
                        spawn("powerprofilesctl", &["set", id]);
                        self.system.invalidate();
                    }
                }
            });
            ui.add_space(12.0);
        }

        section_title(ui, "Session actions");
        ui.horizontal(|ui| {
            if ui.button("Suspend").clicked() {
                spawn("systemctl", &["suspend"]);
            }
            if ui.button("Restart").clicked() {
                spawn("systemctl", &["reboot"]);
            }
            if ui.button("Shut down").clicked() {
                spawn("systemctl", &["poweroff"]);
            }
        });
        changed
    }

    fn page_storage(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Local storage");
        if self.system.disks.is_empty() {
            ui.label(RichText::new("Reading mounted filesystems...").color(DIM));
        }
        for disk in &self.system.disks {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let name = if disk.mount == "/" { "System drive (/)".to_string() } else { disk.mount.clone() };
                    ui.label(RichText::new(name).color(TEXT).size(15.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} / {} ({:.0}%)", fmt_bytes(disk.used_bytes), fmt_bytes(disk.total_bytes), disk.pct))
                                .color(DIM)
                                .size(12.5),
                        );
                    });
                });
                ui.add_space(6.0);
                let (bar_rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), egui::Sense::hover());
                let painter = ui.painter();
                painter.rect_filled(bar_rect, 0.0, Color32::from_rgb(0xe0, 0xe0, 0xe0));
                let fill_w = (bar_rect.width() * (disk.pct / 100.0).clamp(0.0, 1.0)).max(2.0);
                let fill_col = if disk.pct > 90.0 { Color32::from_rgb(0xd1, 0x34, 0x38) } else { ACCENT };
                painter.rect_filled(Rect::from_min_size(bar_rect.min, Vec2::new(fill_w, bar_rect.height())), 0.0, fill_col);
                ui.add_space(4.0);
                ui.label(RichText::new(&disk.fs).color(DIM).size(11.0));
            });
            ui.add_space(8.0);
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button("Open File Exploder").clicked() {
                spawn("file-exploder", &[]);
            }
            if ui.button("Refresh drives").clicked() {
                self.system.invalidate();
            }
        });
    }

    fn page_about(&mut self, ui: &mut egui::Ui) {
        let s = &self.system.specs;
        section_title(ui, "Device specifications");
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            kv_row(ui, "Device name", &s.hostname);
            kv_row(ui, "Processor", &format!("{} ({} threads)", s.cpu, s.cores));
            kv_row(ui, "Installed RAM", &format!("{:.1} GB", s.ram_gb));
            kv_row(ui, "Graphics", &s.gpu);
            kv_row(ui, "System type", "64-bit operating system, x86_64");
        });

        section_title(ui, "VIDE-DE specifications");
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            kv_row(ui, "Edition", "VIDE-DE (Vibecoded Desktop Environment)");
            kv_row(ui, "Compositor", "vibe-comp (Smithay 0.7)");
            kv_row(ui, "Linux Kernel", &s.kernel);
            kv_row(ui, "Session", &format!("{} ({})", s.session, s.wayland_display));
            kv_row(ui, "Settings file", &Config::path().display().to_string());
        });

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Open About (winver)").clicked() {
                let about = std::env::current_exe().ok().and_then(|p| Some(p.parent()?.join("vide-about")));
                if let Some(p) = about {
                    let _ = std::process::Command::new(p).spawn();
                } else {
                    spawn("vide-about", &[]);
                }
            }
            if ui.button("Copy specs").clicked() {
                let txt = format!(
                    "Device: {}\nCPU: {} ({} threads)\nRAM: {:.1} GB\nGPU: {}\nOS: VIDE-DE / Linux {}",
                    s.hostname, s.cpu, s.cores, s.ram_gb, s.gpu, s.kernel
                );
                ui.ctx().copy_text(txt);
            }
        });
    }

    fn page_bluetooth(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Bluetooth");
        let mut on = self.system.bt_on;
        if setting_block(ui, "Bluetooth", "Discoverable as your PC and connect to audio, mice, and keyboards.", |ui| doze_toggle(ui, &mut on)) {
            spawn("bluetoothctl", &["power", if on { "on" } else { "off" }]);
            self.system.invalidate();
        }
        ui.horizontal(|ui| {
            if ui.button("Scan for devices (6s)").clicked() {
                spawn("bluetoothctl", &["--timeout", "6", "scan", "on"]);
                self.system.invalidate();
            }
            if ui.button("Refresh").clicked() {
                self.system.invalidate();
            }
        });
        ui.add_space(10.0);
        if self.system.bt_devices.is_empty() {
            ui.label(RichText::new("No paired Bluetooth devices found.").color(DIM));
            return;
        }
        let devices = self.system.bt_devices.clone();
        for dev in &devices {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&dev.name).color(TEXT).size(14.0));
                        let st = if dev.connected { "Connected" } else { "Paired" };
                        ui.label(RichText::new(format!("{st} · {}", dev.mac)).color(DIM).size(11.5));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let cmd = if dev.connected { "disconnect" } else { "connect" };
                        let label = if dev.connected { "Disconnect" } else { "Connect" };
                        if ui.button(label).clicked() {
                            spawn("bluetoothctl", &[cmd, &dev.mac]);
                            self.system.invalidate();
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }
    }

    fn page_mouse(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        section_title(ui, "Cursor & pointer");
        let mut accel = !self.config.mouse_flat;
        changed |= setting_block(ui, "Enhance pointer precision", "Mouse acceleration. Turn off for games and 1:1 raw aim.", |ui| {
            doze_toggle(ui, &mut accel)
        });
        self.config.mouse_flat = !accel;

        changed |= setting_block(ui, "Cursor speed", "Adjust libinput pointer speed (-1.0 slow to +1.0 fast).", |ui| {
            ui.spacing_mut().slider_width = 260.0;
            ui.horizontal(|ui| {
                let r = ui.add(egui::Slider::new(&mut self.config.mouse_speed, -1.0..=1.0).fixed_decimals(2));
                if ui.button("Reset").clicked() {
                    self.config.mouse_speed = 0.0;
                    return true;
                }
                r.changed()
            })
            .inner
        });

        section_title(ui, "Test pad");
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width().min(420.0), 72.0), egui::Sense::click_and_drag());
            ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(0xf2, 0xf2, 0xf2));
            ui.painter().rect_stroke(rect, 0.0, Stroke::new(1.0, if resp.hovered() { ACCENT } else { BORDER }), StrokeKind::Inside);
            let msg = if resp.dragged() {
                "Dragging..."
            } else if resp.hovered() {
                "Move cursor here to feel current pointer speed"
            } else {
                "Hover or click here to test"
            };
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, msg, FontId::proportional(13.0), DIM);
        });
        changed
    }

    fn page_keyboard(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        if self.system.xkb.is_none() {
            self.system.xkb = Some(crate::system::load_xkb());
        }
        let xkb = self.system.xkb.take().unwrap_or_default();

        // The config keeps layouts and variants as two comma lists that line up.
        let mut layouts: Vec<(String, String)> = {
            let l: Vec<&str> = self.config.xkb_layout.split(',').collect();
            let v: Vec<&str> = self.config.xkb_variant.split(',').collect();
            l.iter().enumerate().map(|(i, l)| (l.trim().to_string(), v.get(i).map(|v| v.trim().to_string()).unwrap_or_default())).collect()
        };
        let describe = |code: &str| {
            if code.is_empty() {
                "System default".to_string()
            } else {
                xkb.layouts.iter().find(|(c, _)| c == code).map(|(c, d)| format!("{d} ({c})")).unwrap_or_else(|| code.to_string())
            }
        };

        section_title(ui, "Layouts");
        ui.label(RichText::new("The first one is used when you log in.").color(DIM).size(12.5));
        ui.add_space(6.0);
        let mut layouts_changed = false;
        let mut remove = None;
        let count = layouts.len();
        for (i, (layout, variant)) in layouts.iter_mut().enumerate() {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt(("kb_layout", i)).selected_text(describe(layout)).width(230.0).height(320.0).show_ui(ui, |ui| {
                        if ui.selectable_label(layout.is_empty(), "System default").clicked() {
                            layout.clear();
                            variant.clear();
                            layouts_changed = true;
                        }
                        for (code, desc) in &xkb.layouts {
                            if ui.selectable_label(layout == code, format!("{desc} ({code})")).clicked() {
                                *layout = code.clone();
                                variant.clear();
                                layouts_changed = true;
                            }
                        }
                    });
                    let options: Vec<&(String, String, String)> = xkb.variants.iter().filter(|(l, _, _)| l == layout).collect();
                    if !options.is_empty() {
                        let current = options.iter().find(|(_, v, _)| v == variant).map(|(_, _, d)| d.clone()).unwrap_or_else(|| "Standard".into());
                        egui::ComboBox::from_id_salt(("kb_variant", i)).selected_text(current).width(230.0).height(260.0).show_ui(ui, |ui| {
                            if ui.selectable_label(variant.is_empty(), "Standard").clicked() {
                                variant.clear();
                                layouts_changed = true;
                            }
                            for (_, code, desc) in &options {
                                if ui.selectable_label(variant == code, desc).clicked() {
                                    *variant = code.clone();
                                    layouts_changed = true;
                                }
                            }
                        });
                    }
                    if count > 1 && ui.button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            });
            ui.add_space(4.0);
        }
        if let Some(i) = remove {
            layouts.remove(i);
            layouts_changed = true;
        }
        if layouts.len() < 4 && ui.button("Add a layout").clicked() {
            layouts.push(("us".to_string(), String::new()));
            layouts_changed = true;
        }
        if layouts_changed {
            let only_default = layouts.len() == 1 && layouts[0].0.is_empty();
            self.config.xkb_layout = if only_default { String::new() } else { layouts.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>().join(",") };
            self.config.xkb_variant = if layouts.iter().all(|(_, v)| v.is_empty()) { String::new() } else { layouts.iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>().join(",") };
            // Switching between layouts needs a shortcut: Alt+Shift unless one is already set.
            if layouts.len() > 1 && !self.config.xkb_options.split(',').any(|o| o.starts_with("grp:")) {
                set_xkb_option(&mut self.config.xkb_options, "grp:alt_shift_toggle", true);
            }
            changed = true;
        }
        ui.add_space(10.0);

        section_title(ui, "Keys");
        let on = |ui: &mut egui::Ui, opts: &mut String, title: &str, desc: &str, opt: &str| -> bool {
            let mut v = opts.split(',').any(|o| o == opt);
            let was = v;
            let c = setting_block(ui, title, desc, |ui| doze_toggle(ui, &mut v));
            if c && v != was {
                set_xkb_option(opts, opt, v);
            }
            c
        };
        if layouts.len() > 1 {
            changed |= on(ui, &mut self.config.xkb_options, "Switch layouts with Alt + Shift", "Press both to move to the next layout.", "grp:alt_shift_toggle");
        }
        changed |= on(ui, &mut self.config.xkb_options, "Caps Lock acts as Escape", "Handy for editors: a bigger, easier Escape key.", "caps:escape");
        changed |= on(ui, &mut self.config.xkb_options, "Caps Lock acts as Ctrl", "Moves Ctrl to the home row.", "ctrl:nocaps");

        changed |= setting_block(ui, "Delay before a held key repeats", "Shorter repeats sooner.", |ui| {
            let r = ui.add(egui::Slider::new(&mut self.config.key_repeat_delay, 150..=1000).suffix(" ms"));
            r.changed()
        });
        changed |= setting_block(ui, "Repeat speed", "Characters per second while a key is held.", |ui| {
            let r = ui.add(egui::Slider::new(&mut self.config.key_repeat_rate, 5..=60).suffix(" /s"));
            r.changed()
        });

        ui.add_space(10.0);
        section_title(ui, "Keyboard test");
        ui.label(RichText::new("Type below to try your layout and key repeat.").color(DIM).size(12.5));
        ui.add_space(6.0);
        ui.add(egui::TextEdit::multiline(&mut self.kbd_test).desired_rows(3).desired_width(460.0).hint_text("Type here to test keys..."));
        ui.add_space(12.0);
        section_title(ui, "Shortcuts");
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            kv_row(ui, "Super", "Open / close Start menu");
            kv_row(ui, "Super + Tab", "Task View (virtual desktops)");
            kv_row(ui, "Alt + Tab", "Switch open windows");
            kv_row(ui, "Win + Shift + S", "Snip & Sketchy screen capture");
            kv_row(ui, "Print Screen", "Fullscreen screenshot");
        });
        self.system.xkb = Some(xkb);
        changed
    }

    fn page_net_status(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Network status");
        let active: Vec<_> = self.system.net_ifaces.iter().filter(|i| i.state == "connected").cloned().collect();
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            if active.is_empty() {
                ui.label(RichText::new("Not connected").color(Color32::from_rgb(0xff, 0x8c, 0x00)).size(16.0));
                ui.label(RichText::new("No active network connection detected.").color(DIM).size(12.5));
            } else {
                ui.label(RichText::new("You're connected to the Internet").color(ACCENT).size(16.0));
                ui.add_space(6.0);
                for iface in &active {
                    kv_row(ui, &format!("{} ({})", iface.device, iface.kind), &format!("{} — {}", iface.connection, iface.ip));
                }
            }
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Refresh status").clicked() {
                self.system.invalidate();
            }
            if ui.button("Go to Wi-Fi").clicked() {
                self.page = Some((Category::Network, SubPage::Wifi));
                self.system.invalidate();
            }
        });
    }

    fn page_wifi(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Wi-Fi");
        let mut on = self.system.wifi_on;
        if setting_block(ui, "Wi-Fi", "Connect to wireless networks.", |ui| doze_toggle(ui, &mut on)) {
            spawn("nmcli", &["radio", "wifi", if on { "on" } else { "off" }]);
            self.system.invalidate();
        }
        if !self.system.wifi_on {
            ui.label(RichText::new("Turn Wi-Fi on to view nearby networks.").color(DIM));
            return;
        }
        if let Some(connected) = self.system.wifi.iter().find(|n| n.active).cloned() {
            egui::Frame::new().fill(ACCENT).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&connected.ssid).color(Color32::WHITE).size(16.0));
                        let sec = if connected.security.is_empty() { "Open" } else { "Secured" };
                        ui.label(RichText::new(format!("Connected · {sec} · {}%", connected.signal)).color(Color32::from_rgb(0xe6, 0xf2, 0xfc)).size(12.0));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Disconnect").clicked() {
                            spawn("nmcli", &["connection", "down", "id", &connected.ssid]);
                            self.system.invalidate();
                        }
                    });
                });
            });
            ui.add_space(10.0);
        }

        ui.horizontal(|ui| {
            if ui.button("Rescan networks").clicked() {
                spawn("nmcli", &["device", "wifi", "list", "--rescan", "yes"]);
                self.system.invalidate();
            }
            if ui.button("Refresh").clicked() {
                self.system.invalidate();
            }
        });
        ui.add_space(8.0);

        let networks: Vec<_> = self.system.wifi.iter().filter(|n| !n.active).cloned().collect();
        if networks.is_empty() {
            ui.label(RichText::new("No other networks found.").color(DIM));
        } else {
            for net in &networks {
                card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        signal_bars_ui(ui, net.signal);
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&net.ssid).color(TEXT).size(14.0));
                            let sec = if net.security.is_empty() { "Open" } else { "Secured" };
                            ui.label(RichText::new(format!("{sec} · {}%", net.signal)).color(DIM).size(11.0));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Connect").clicked() {
                                if net.security.is_empty() {
                                    spawn("nmcli", &["device", "wifi", "connect", &net.ssid]);
                                    self.system.invalidate();
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

        if let Some(target) = self.system.wifi_target.clone() {
            let mut open = true;
            egui::Window::new("Enter network security key")
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ui.ctx(), |ui| {
                    ui.label(RichText::new(format!("Enter the security key for \"{target}\"")).color(TEXT));
                    ui.add_space(6.0);
                    let r = ui.add(egui::TextEdit::singleline(&mut self.system.wifi_password).password(true).desired_width(280.0));
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        spawn("nmcli", &["device", "wifi", "connect", &target, "password", &self.system.wifi_password]);
                        self.system.wifi_target = None;
                        self.system.invalidate();
                    }
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("Connect").clicked() {
                            spawn("nmcli", &["device", "wifi", "connect", &target, "password", &self.system.wifi_password]);
                            self.system.wifi_target = None;
                            self.system.invalidate();
                        }
                        if ui.button("Cancel").clicked() {
                            self.system.wifi_target = None;
                        }
                    });
                });
            if !open {
                self.system.wifi_target = None;
            }
        }
    }

    fn page_ethernet(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Ethernet adapters");
        let wired: Vec<_> = self.system.net_ifaces.iter().filter(|i| i.kind == "ethernet").cloned().collect();
        if wired.is_empty() {
            ui.label(RichText::new("No Ethernet adapters detected.").color(DIM));
            return;
        }
        for iface in &wired {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                kv_row(ui, "Adapter", &iface.device);
                kv_row(ui, "State", &iface.state);
                kv_row(ui, "Profile", &iface.connection);
                kv_row(ui, "IPv4 address", if iface.ip.is_empty() { "—" } else { &iface.ip });
            });
            ui.add_space(6.0);
        }
    }

    fn page_background(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        section_title(ui, "Background");
        let current = format!("file://{}", self.config.wallpaper.display());
        egui::Frame::new().stroke(Stroke::new(2.0, BORDER)).show(ui, |ui| {
            ui.add(egui::Image::new(current).fit_to_exact_size(Vec2::new(384.0, 216.0)));
        });
        ui.add_space(12.0);
        ui.label(RichText::new("Choose your picture").color(TEXT).size(14.0));
        ui.add_space(6.0);

        let mut walls: Vec<_> = std::fs::read_dir(vide_config::wallpapers_dir())
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "png" || x == "jpg"))
                    .collect()
            })
            .unwrap_or_default();
        walls.sort();
        let per_row = ((ui.available_width() / 172.0) as usize).max(1);
        for chunk in walls.chunks(per_row) {
            ui.horizontal(|ui| {
                for path in chunk.iter().cloned() {
                    let selected = path == self.config.wallpaper;
                    let img = egui::Image::new(format!("file://{}", path.display())).fit_to_exact_size(Vec2::new(156.0, 88.0));
                    let r = egui::Frame::new()
                        .stroke(Stroke::new(if selected { 3.0 } else { 1.0 }, if selected { ACCENT } else { BORDER }))
                        .inner_margin(2.0)
                        .show(ui, |ui| ui.add(img.sense(egui::Sense::click())))
                        .inner;
                    if r.clicked() {
                        self.config.wallpaper = path;
                        changed = true;
                    }
                }
            });
            ui.add_space(4.0);
        }
        ui.add_space(10.0);
        changed |= setting_block(ui, "Browse custom image", "Enter a path to a PNG/JPG and press Enter.", |ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.custom_wall).hint_text("/home/you/Pictures/wall.png").desired_width(320.0));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let p = PathBuf::from(self.custom_wall.trim());
                if p.is_file() {
                    self.config.wallpaper = p;
                    return true;
                }
            }
            false
        });
        changed
    }

    fn page_colors(&mut self, ui: &mut egui::Ui) -> bool {
        section_title(ui, "Colors");
        setting_block(ui, "Choose your default Windoze mode", "The taskbar, Start, Action Center and menus.", |ui| {
            let mut changed = false;
            for (light, label) in [(true, "Light"), (false, "Dark")] {
                if ui.radio(self.config.light_mode == light, label).clicked() && self.config.light_mode != light {
                    self.config.light_mode = light;
                    changed = true;
                }
            }
            changed
        })
    }

    fn page_start(&mut self, ui: &mut egui::Ui) -> bool {
        section_title(ui, "Start");
        setting_block(ui, "Hide KDE apps in Start search", "Keeps KDE's own apps out of the Start list and search.", |ui| {
            doze_toggle(ui, &mut self.config.hide_kde_apps)
        })
    }

    fn page_taskbar(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Taskbar");
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("VIDE-DE Taskbar").color(TEXT).size(14.5));
            ui.add_space(4.0);
            ui.label(RichText::new("• Right-click any app in Start or on the taskbar to Pin / Unpin it.").color(DIM).size(12.5));
            ui.label(RichText::new("• Task View button and Win+Tab open virtual desktops.").color(DIM).size(12.5));
            ui.label(RichText::new("• System tray shows Volume, Brightness, Wi-Fi, Bluetooth, and SNI apps.").color(DIM).size(12.5));
        });
    }

    fn page_apps_features(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Apps & features");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.app_filter).hint_text("Search this list").desired_width(280.0));
            ui.label(RichText::new(format!("{} apps installed", self.system.apps.len())).color(DIM).size(12.5));
        });
        ui.add_space(10.0);
        let q = self.app_filter.to_lowercase();
        let filtered: Vec<_> = self
            .system
            .apps
            .iter()
            .filter(|a| q.is_empty() || a.name.to_lowercase().contains(&q) || a.comment.to_lowercase().contains(&q))
            .take(60)
            .cloned()
            .collect();
        for app in &filtered {
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&app.name).color(TEXT).size(14.0));
                        if !app.comment.is_empty() {
                            ui.label(RichText::new(&app.comment).color(DIM).size(11.5));
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Launch").clicked() {
                            launch_desktop_exec(&app.exec);
                        }
                    });
                });
            });
            ui.add_space(4.0);
        }
    }

    fn page_default_apps(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Choose default apps");
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            kv_row(ui, "Web browser", &self.system.default_browser);
            kv_row(ui, "File manager", &self.system.default_files);
            kv_row(ui, "Text editor", &self.system.default_text);
            kv_row(ui, "Photo viewer", &self.system.default_image);
        });
        ui.add_space(8.0);
        if ui.button("Refresh defaults").clicked() {
            self.system.invalidate();
        }
    }

    fn page_your_info(&mut self, ui: &mut egui::Ui) {
        let s = &self.system.specs;
        section_title(ui, "Your info");
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(72.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 34.0, ACCENT);
            let initial = s.username.chars().next().unwrap_or('U').to_uppercase().to_string();
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initial, FontId::proportional(28.0), Color32::WHITE);
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(&s.username).color(TEXT).size(20.0));
                ui.label(RichText::new(format!("Local Account · {}", s.hostname)).color(DIM).size(13.0));
                ui.label(RichText::new(format!("Session: {} ({})", s.session, s.wayland_display)).color(DIM).size(12.0));
            });
        });
    }

    fn page_date_time(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        if self.system.timezones.is_empty() {
            self.system.timezones = crate::system::load_timezones();
        }
        let current = self.system.specs.timezone.clone();
        let ntp = self.system.specs.ntp_enabled;

        section_title(ui, "Date & time");
        changed |= setting_block(ui, "24-hour clock", "Show the taskbar clock as 14:30 instead of 2:30 PM.", |ui| doze_toggle(ui, &mut self.config.clock_24h));

        let mut ntp_on = ntp;
        if setting_block(ui, "Set time automatically", "Sync the clock with internet time servers (may ask for your password).", |ui| doze_toggle(ui, &mut ntp_on)) {
            spawn("timedatectl", &["set-ntp", if ntp_on { "true" } else { "false" }]);
            self.system.invalidate();
        }

        // Region, then city: the full list is over four hundred zones.
        let (region, city) = current.split_once('/').map(|(r, c)| (r.to_string(), c.to_string())).unwrap_or_else(|| (current.clone(), String::new()));
        let mut regions: Vec<String> = self.system.timezones.iter().filter_map(|z| z.split_once('/').map(|(r, _)| r.to_string())).collect();
        regions.dedup();
        let mut pick: Option<String> = None;
        setting_block(ui, "Time zone", "Changing it may ask for your password.", |ui| {
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("tz_region").selected_text(if region.is_empty() { "Region".into() } else { region.clone() }).width(150.0).show_ui(ui, |ui| {
                    for r in &regions {
                        if ui.selectable_label(*r == region, r).clicked() {
                            // First zone in that region until a city is chosen.
                            pick = self.system.timezones.iter().find(|z| z.starts_with(&format!("{r}/"))).cloned();
                        }
                    }
                });
                egui::ComboBox::from_id_salt("tz_city").selected_text(if city.is_empty() { "City".into() } else { city.replace('_', " ") }).width(220.0).height(320.0).show_ui(ui, |ui| {
                    for z in self.system.timezones.iter().filter(|z| z.starts_with(&format!("{region}/"))) {
                        let name = z[region.len() + 1..].to_string();
                        if ui.selectable_label(*z == current, name.replace('_', " ")).clicked() {
                            pick = Some(z.clone());
                        }
                    }
                });
            });
            false
        });
        if let Some(tz) = pick.filter(|z| *z != current) {
            spawn("timedatectl", &["set-timezone", &tz]);
            self.system.invalidate();
        }
        changed
    }

    fn page_language(&mut self, ui: &mut egui::Ui) {
        let s = &self.system.specs;
        section_title(ui, "Language & region");
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            kv_row(ui, "System locale", &s.locale);
            kv_row(ui, "Time zone", &s.timezone);
        });
    }

    fn page_game_mode(&mut self, ui: &mut egui::Ui) -> bool {
        section_title(ui, "Game Mode");
        let changed = setting_block(
            ui,
            "Ultra Performance Mode (Direct Scanout)",
            "Fullscreen games bypass compositing for lower latency. May flicker on some NVIDIA setups.",
            |ui| doze_toggle(ui, &mut self.config.direct_scanout),
        );
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new("Gaming features in vibe-comp").color(TEXT).size(14.0));
            ui.add_space(4.0);
            ui.label(RichText::new("• Relative pointer & pointer lock enabled for Xwayland/Wayland games.").color(DIM).size(12.5));
            ui.label(RichText::new("• Press Super to release a locked game cursor.").color(DIM).size(12.5));
        });
        changed
    }

    fn page_captures(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Captures & Screenshots");
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            kv_row(ui, "Fullscreen capture", "Print Screen (PrtSc)");
            kv_row(ui, "Region snip", "Win + Shift + S (Snip & Sketchy)");
        });
        ui.add_space(10.0);
        if ui.button("Open Snip & Sketchy").clicked() {
            spawn("snip-and-sketchy", &[]);
        }
    }

    fn page_ease_display(&mut self, ui: &mut egui::Ui) -> bool {
        let mut changed = false;
        section_title(ui, "Make everything bigger");
        changed |= setting_block(ui, "Display scaling", "100% is native size; 150% makes everything half as big again.", |ui| {
            let mut c = false;
            ui.horizontal(|ui| {
                let cur = (self.config.display_scale * 100.0).round() as i32;
                for pct in SCALES {
                    if ui.selectable_label(cur == pct, format!("{pct}%")).clicked() {
                        self.config.display_scale = pct as f64 / 100.0;
                        c = true;
                    }
                }
            });
            c
        });
        changed |= setting_block(ui, "Night light", "Reduce blue light strain.", |ui| doze_toggle(ui, &mut self.config.night_light));
        changed
    }

    /// Who asked for what: camera, microphone, location, notifications and screen sharing, from the
    /// portal permission store, with the controls to change your mind. Websites aren't here: a
    /// browser keeps its own site permissions.
    fn page_privacy(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Apps ask the first time they need these. Change your mind here.").color(DIM).size(12.5));
        ui.label(RichText::new("Websites are managed in the browser itself, under its site settings.").color(DIM).size(12.5));
        ui.add_space(8.0);

        section_title(ui, "Camera");
        self.permission_rows(ui, "devices", "camera", &[("yes", "Allow"), ("no", "Block"), ("ask", "Ask")]);
        ui.add_space(8.0);
        section_title(ui, "Microphone");
        self.permission_rows(ui, "devices", "microphone", &[("yes", "Allow"), ("no", "Block"), ("ask", "Ask")]);
        ui.add_space(8.0);
        section_title(ui, "Notifications");
        self.permission_rows(ui, "notifications", "notification", &[("yes", "Allow"), ("no", "Block")]);
        ui.add_space(8.0);

        section_title(ui, "Location");
        let places: Vec<crate::system::PermEntry> = self.system.privacy.iter().filter(|e| e.table == "location").cloned().collect();
        if places.is_empty() {
            ui.label(RichText::new("No app has asked yet.").color(DIM).size(12.5));
        }
        for e in places {
            let name = self.app_label(&e.app);
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).color(TEXT).size(14.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // The store can only clear everything for an app at once.
                        if !e.app.is_empty() && ui.button("Reset all for this app").clicked() {
                            run("flatpak", &["permission-reset", &e.app]);
                            self.system.invalidate();
                        }
                    });
                });
            });
            ui.add_space(4.0);
        }
        ui.add_space(8.0);

        section_title(ui, "Screen sharing & recording");
        let casts: Vec<crate::system::PermEntry> = self.system.privacy.iter().filter(|e| e.table == "screencast").cloned().collect();
        if casts.is_empty() {
            ui.label(RichText::new("No app has been allowed to share the screen.").color(DIM).size(12.5));
        }
        for e in casts {
            let name = self.app_label(&e.app);
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).color(TEXT).size(14.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Revoke").clicked() {
                            run("flatpak", &["permission-remove", "screencast", &e.object]);
                            self.system.invalidate();
                        }
                    });
                });
            });
            ui.add_space(4.0);
        }
        ui.add_space(4.0);
        self.vide_screenshare_permissions(ui);
    }

    /// The name people know an app by: flatpak's, else the id, else a note for apps outside a sandbox.
    fn app_label(&self, app: &str) -> String {
        if app.is_empty() {
            return "Desktop app (not sandboxed)".into();
        }
        self.system.app_names.get(app).cloned().unwrap_or_else(|| humanize_app_id(app))
    }

    /// One row per app that has asked for `object` in `table`, with a button per choice.
    fn permission_rows(&mut self, ui: &mut egui::Ui, table: &str, object: &str, choices: &[(&str, &str)]) {
        let rows: Vec<crate::system::PermEntry> = self.system.privacy.iter().filter(|e| e.table == table && e.object == object).cloned().collect();
        if rows.is_empty() {
            ui.label(RichText::new("No app has asked yet.").color(DIM).size(12.5));
            return;
        }
        for e in rows {
            let name = self.app_label(&e.app);
            card_frame().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(name).color(TEXT).size(14.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if e.app.is_empty() {
                            // The store has no app id to change for these.
                            ui.label(RichText::new(if e.value == "yes" { "Allowed" } else { "Blocked" }).color(DIM));
                            return;
                        }
                        for (value, label) in choices.iter().rev() {
                            if ui.selectable_label(e.value == *value, *label).clicked() && e.value != *value {
                                run("flatpak", &["permission-set", table, object, &e.app, value]);
                                self.system.invalidate();
                            }
                        }
                    });
                });
            });
            ui.add_space(4.0);
        }
    }

    /// Apps VIDE's own screen-sharing portal has remembered (vide-portal keeps this file).
    fn vide_screenshare_permissions(&mut self, ui: &mut egui::Ui) {
        let perms_path = {
            let base = std::env::var("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|_| std::env::var("HOME").map(PathBuf::from).unwrap_or_default().join(".config"));
            base.join("vide-de/screencast-perms.conf")
        };
        let mut entries: Vec<(String, String)> = std::fs::read_to_string(&perms_path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| l.trim().split_once('='))
            .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
            .collect();
        if entries.is_empty() {
            ui.label(RichText::new("No saved screen-sharing permissions yet. Apps will prompt when they request screen capture.").color(DIM).size(12.5));
        } else {
            let mut changed = false;
            let mut remove_idx = None;
            for (idx, (app, perm)) in entries.iter_mut().enumerate() {
                card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&*app).color(TEXT).size(13.5));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Revoke").clicked() {
                                remove_idx = Some(idx);
                                changed = true;
                            }
                            let next = if perm == "allow" { "deny" } else { "allow" };
                            let label = if perm == "allow" { "Allowed" } else { "Denied" };
                            if ui.button(label).clicked() {
                                *perm = next.to_string();
                                changed = true;
                            }
                        });
                    });
                });
                ui.add_space(4.0);
            }
            if let Some(i) = remove_idx {
                entries.remove(i);
            }
            if changed {
                let out: String = entries.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
                let _ = std::fs::write(&perms_path, out);
            }
        }
        ui.add_space(8.0);
    }

    fn page_update_status(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Session & services health");
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            for svc in &self.system.services {
                let status = match svc.state {
                    crate::system::ServiceState::Running => "Running",
                    crate::system::ServiceState::Stopped => "Not running",
                    crate::system::ServiceState::Unknown => "Couldn't check",
                };
                kv_row(ui, svc.name, status);
            }
        });

        section_title(ui, "Recent compositor activity");
        ui.horizontal(|ui| {
            if ui.button("Refresh logs").clicked() {
                self.system.invalidate();
            }
        });
        ui.add_space(6.0);
        card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(&self.system.log_tail).font(FontId::monospace(11.5)).color(DIM));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::humanize_app_id;

    #[test]
    fn app_ids_become_names() {
        assert_eq!(humanize_app_id("io.github.astralvixen.geforce-infinity"), "Geforce Infinity");
        assert_eq!(humanize_app_id("org.kde.konsole"), "Konsole");
        assert_eq!(humanize_app_id("plain"), "Plain");
    }
}

#[cfg(test)]
mod xkb_option_tests {
    use super::set_xkb_option;

    #[test]
    fn options_toggle_without_touching_others() {
        let mut o = "caps:escape".to_string();
        set_xkb_option(&mut o, "grp:alt_shift_toggle", true);
        assert_eq!(o, "caps:escape,grp:alt_shift_toggle");
        set_xkb_option(&mut o, "caps:escape", false);
        assert_eq!(o, "grp:alt_shift_toggle");
        set_xkb_option(&mut o, "grp:alt_shift_toggle", false);
        assert_eq!(o, "");
    }
}

/// Play a notification sound once, for choosing one.
fn preview_sound(id: &str) {
    if let Some(file) = vide_config::sound_file(id) {
        std::thread::spawn(move || {
            let _ = std::process::Command::new("pw-play").arg(file).status();
        });
    }
}
