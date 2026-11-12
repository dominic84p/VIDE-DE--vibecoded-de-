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
    Mouse,
    Start,
    About,
}

impl Page {
    const ALL: [Page; 3] = [Page::Mouse, Page::Start, Page::About];

    fn title(self) -> &'static str {
        match self {
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
            .with_inner_size([760.0, 520.0])
            .with_min_inner_size([560.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Settings",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(Settings { config: Config::load(), page: Page::Mouse, error: None }))
        }),
    )
}

struct Settings {
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
