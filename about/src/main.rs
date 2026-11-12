//! About VIDE-DE: a standalone app (not part of the compositor) that runs fastfetch
//! and lays the results out like a settings page, rendered by a real GUI toolkit so
//! text stays sharp at any scale.

use eframe::egui::{self, Color32, FontId, RichText, Vec2};

/// fastfetch modules to show, in order.
const MODULES: &str = "OS:Host:Kernel:Uptime:Packages:Shell:Display:DE:WM:Terminal:CPU:GPU:Memory:Disk";

const BG: Color32 = Color32::from_rgb(0x1f, 0x1f, 0x1f);
const CARD: Color32 = Color32::from_rgb(0x2b, 0x2b, 0x2b);
const ACCENT: Color32 = Color32::from_rgb(0x76, 0xb9, 0xed);
const DIM: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("About VIDE-DE")
            .with_app_id("vide-about")
            .with_inner_size([560.0, 640.0])
            .with_min_inner_size([420.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "About VIDE-DE",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(About::new()))
        }),
    )
}

struct About {
    rows: Vec<(String, String)>,
    error: Option<String>,
}

impl About {
    fn new() -> Self {
        match fastfetch() {
            Ok(rows) => Self { rows, error: None },
            Err(e) => Self { rows: Vec::new(), error: Some(e) },
        }
    }
}

/// Run fastfetch and parse its `Key§Value` lines.
fn fastfetch() -> Result<Vec<(String, String)>, String> {
    let out = std::process::Command::new("fastfetch")
        .args(["--logo", "none", "--pipe", "--separator", "§", "-s", MODULES])
        .output()
        .map_err(|e| format!("couldn't run fastfetch: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let rows = text
        .lines()
        .filter_map(|l| l.split_once('§'))
        .map(|(k, v)| {
            let k = k.trim().to_string();
            // fastfetch only knows the DEs it has heard of; tell it who we are.
            let v = if k == "Desktop" { "VIDE-DE".to_string() } else { v.trim().to_string() };
            (k, v)
        })
        .collect();
    Ok(rows)
}

impl eframe::App for About {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        ctx.set_visuals(egui::Visuals::dark());
        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(24.0)).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add(egui::Image::new(egui::include_image!("../../assets/logo-512.png")).max_height(120.0));
                    ui.add_space(8.0);
                    ui.label(RichText::new("VIDE-DE").font(FontId::proportional(32.0)).strong().color(Color32::WHITE));
                    ui.label(RichText::new("the vibecoded desktop environment").color(DIM));
                    ui.label(RichText::new(concat!("version ", env!("CARGO_PKG_VERSION"))).small().color(DIM));
                });
                ui.add_space(20.0);

                ui.label(RichText::new("Device specifications").font(FontId::proportional(18.0)).color(Color32::WHITE));
                ui.add_space(6.0);
                egui::Frame::new().fill(CARD).corner_radius(6.0).inner_margin(16.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if let Some(e) = &self.error {
                        ui.colored_label(Color32::LIGHT_RED, e);
                        return;
                    }
                    egui::Grid::new("specs").num_columns(2).spacing(Vec2::new(24.0, 10.0)).show(ui, |ui| {
                        for (k, v) in &self.rows {
                            ui.label(RichText::new(k).color(DIM));
                            ui.label(RichText::new(v).color(Color32::WHITE));
                            ui.end_row();
                        }
                    });
                });

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    if ui.button("Copy").clicked() {
                        let text: String = self.rows.iter().map(|(k, v)| format!("{k}: {v}\n")).collect();
                        ctx.copy_text(text);
                    }
                    if ui.button("Refresh").clicked() {
                        *self = About::new();
                    }
                    ui.label(RichText::new("No human code was harmed in the making of this desktop.").small().color(ACCENT));
                });
            });
        });
    }
}
