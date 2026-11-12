//! VIDE-DE Settings: a Windows-Settings-style app for the desktop's options.
//! Writes ~/.config/vide-de/settings.conf; the compositor picks changes up live.

use eframe::egui::{self, Color32, FontId, RichText};
use vide_config::Config;

const BG: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
const SIDEBAR: Color32 = Color32::from_rgb(0x19, 0x19, 0x19);
const CARD: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
const ACCENT: Color32 = Color32::from_rgb(0x76, 0xb9, 0xed);
const DIM: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Personalization,
    Mouse,
    Start,
    About,
}

impl Page {
    const ALL: [Page; 4] = [Page::Personalization, Page::Mouse, Page::Start, Page::About];

    fn title(self) -> &'static str {
        match self {
            Page::Personalization => "Personalization",
            Page::Mouse => "Mouse",
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
                Some("start") => Page::Start,
                Some("about") => Page::About,
                _ => Page::Mouse,
            };
            let custom = String::new();
            Ok(Box::new(Settings { config: Config::load(), page, error: None, custom }))
        }),
    )
}

struct Settings {
    /// "Browse" path being typed on the Personalization page.
    custom: String,
    config: Config,
    page: Page,
    error: Option<String>,
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

impl eframe::App for Settings {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut visuals = egui::Visuals::dark();
        visuals.selection.bg_fill = ACCENT;
        ctx.set_visuals(visuals);

        egui::Panel::left("nav").exact_size(200.0).frame(egui::Frame::new().fill(SIDEBAR).inner_margin(12.0)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Image::new(egui::include_image!("../../assets/logo-512.png")).max_height(28.0));
                ui.label(RichText::new("Settings").font(FontId::proportional(18.0)).color(Color32::WHITE));
            });
            ui.add_space(16.0);
            for p in Page::ALL {
                let selected = self.page == p;
                let text = RichText::new(p.title()).size(14.0).color(if selected { Color32::WHITE } else { DIM });
                if ui.add_sized([176.0, 34.0], egui::Button::selectable(selected, text)).clicked() {
                    self.page = p;
                }
            }
        });

        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(24.0)).show(ui, |ui| {
            ui.label(RichText::new(self.page.title()).font(FontId::proportional(28.0)).color(Color32::WHITE));
            ui.add_space(16.0);
            let mut changed = false;
            match self.page {
                Page::Personalization => {
                    ui.label(RichText::new("Background").color(Color32::WHITE).size(16.0));
                    ui.add_space(8.0);
                    // Current wallpaper, big, then the bundled ones to pick from.
                    let current = format!("file://{}", self.config.wallpaper.display());
                    ui.add(egui::Image::new(current).fit_to_exact_size(egui::vec2(384.0, 216.0)).corner_radius(4.0));
                    ui.add_space(12.0);
                    ui.label(RichText::new("Choose your picture").color(DIM).size(13.0));
                    ui.add_space(4.0);
                    let mut walls: Vec<_> = std::fs::read_dir(vide_config::wallpapers_dir())
                        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "png" || x == "jpg")).collect())
                        .unwrap_or_default();
                    walls.sort();
                    // As many 160px thumbnails per row as fit.
                    let per_row = ((ui.available_width() / 172.0) as usize).max(1);
                    for chunk in walls.chunks(per_row) {
                      ui.horizontal(|ui| {
                        for path in chunk.iter().cloned() {
                            let selected = path == self.config.wallpaper;
                            let img = egui::Image::new(format!("file://{}", path.display())).fit_to_exact_size(egui::vec2(160.0, 90.0)).corner_radius(3.0);
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
                            let p = std::path::PathBuf::from(self.custom.trim());
                            if p.is_file() {
                                self.config.wallpaper = p;
                                return true;
                            }
                        }
                        false
                    });
                }
                Page::Mouse => {
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
    }
}
