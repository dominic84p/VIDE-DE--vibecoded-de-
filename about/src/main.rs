//! About VIDE-DE: a standalone app (not part of the compositor), laid out like Windows 10's
//! Settings → About. Facts come from fastfetch, with uptime and memory read live, and it's
//! rendered by a real GUI toolkit so text stays sharp at any scale.

use std::{
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

use eframe::egui::{self, Color32, FontId, RichText, Sense, Stroke, vec2};

const BG: Color32 = Color32::WHITE;
const CARD: Color32 = Color32::from_rgb(0xf8, 0xf8, 0xf8);
const BORDER: Color32 = Color32::from_rgb(0xdd, 0xdd, 0xdd);
const DIVIDER: Color32 = Color32::from_rgb(0xea, 0xea, 0xea);
const ACCENT: Color32 = Color32::from_rgb(0x00, 0x78, 0xd7);
const ACCENT_HOVER: Color32 = Color32::from_rgb(0x10, 0x6e, 0xbe);
const TEXT: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);
const DIM: Color32 = Color32::from_rgb(0x60, 0x60, 0x60);

/// fastfetch modules we take values from; the rest is ours.
const MODULES: &str = "OS:Host:Kernel:Packages:Shell:Display:CPU:GPU:Disk";

/// Run fastfetch and parse its `Key§Value` lines, keyed by the bare module name.
fn fastfetch() -> Vec<(String, String)> {
    let Ok(out) = std::process::Command::new("fastfetch").args(["--logo", "none", "--pipe", "--separator", "§", "-s", MODULES]).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once('§'))
        .map(|(k, v)| (k.split(" (").next().unwrap_or(k).trim().to_string(), v.trim().to_string()))
        .collect()
}

fn uptime() -> Option<String> {
    let secs: f64 = std::fs::read_to_string("/proc/uptime").ok()?.split_whitespace().next()?.parse().ok()?;
    let secs = secs as u64;
    let (d, h, m) = (secs / 86_400, secs % 86_400 / 3_600, secs % 3_600 / 60);
    let plural = |n: u64, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
    let mut parts = Vec::new();
    if d > 0 {
        parts.push(plural(d, "day"));
    }
    if h > 0 {
        parts.push(plural(h, "hour"));
    }
    parts.push(plural(m, "minute"));
    Some(parts.join(", "))
}

fn memory() -> Option<String> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let kib = |key: &str| -> Option<f64> { text.lines().find(|l| l.starts_with(key))?.split_whitespace().nth(1)?.parse().ok() };
    let (total, avail) = (kib("MemTotal:")?, kib("MemAvailable:")?);
    let gib = |k: f64| k / 1024.0 / 1024.0;
    Some(format!("{:.1} GiB ({:.1} GiB in use)", gib(total), gib(total - avail)))
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname").ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "this PC".into())
}

/// The two sections, in order. The live values (uptime, memory) are read fresh each call.
fn sections(base: &[(String, String)]) -> [(&'static str, Vec<(&'static str, String)>); 2] {
    let get = |k: &str| base.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let keep = |rows: Vec<(&'static str, Option<String>)>| rows.into_iter().filter_map(|(k, v)| Some((k, v?))).collect::<Vec<_>>();
    [
        (
            "Device specifications",
            keep(vec![
                ("Device name", Some(hostname())),
                ("Processor", get("CPU")),
                ("Installed RAM", memory()),
                ("Graphics", get("GPU")),
                ("Storage", get("Disk")),
                ("Display", get("Display")),
                ("System type", Some("64-bit operating system, x64-based processor".into()).filter(|_| cfg!(target_arch = "x86_64"))),
            ]),
        ),
        (
            "VIDE-DE specifications",
            keep(vec![
                ("Edition", Some("VIDE-DE".into())),
                ("Version", Some(env!("CARGO_PKG_VERSION").into())),
                ("Compositor", Some("vibe-comp".into())),
                ("Operating system", get("OS")),
                ("Kernel", get("Kernel")),
                ("Packages", get("Packages")),
                ("Shell", std::env::var("SHELL").ok().and_then(|s| s.rsplit('/').next().map(String::from)).or_else(|| get("Shell"))),
                ("Uptime", uptime()),
            ]),
        ),
    ]
}

struct About {
    base: Vec<(String, String)>,
    rx: Receiver<Vec<(String, String)>>,
    loaded: bool,
    copied: Option<Instant>,
}

impl About {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(fastfetch());
        });
        Self { base: Vec::new(), rx, loaded: false, copied: None }
    }

    fn plain_text(&self) -> String {
        sections(&self.base)
            .iter()
            .map(|(title, rows)| format!("{title}\n{}\n", rows.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("\n")))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn section(ui: &mut egui::Ui, title: &str, rows: &[(&str, String)], copy: Option<&mut bool>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(FontId::proportional(18.0)).color(TEXT));
        if let Some(copy) = copy {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, resp) = ui.allocate_exact_size(vec2(72.0, 30.0), Sense::click());
                ui.painter().rect_filled(rect, 2.0, if resp.hovered() { ACCENT_HOVER } else { ACCENT });
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Copy", FontId::proportional(14.0), Color32::WHITE);
                *copy = resp.clicked();
            });
        }
    });
    ui.add_space(8.0);
    egui::Frame::new().fill(CARD).stroke(Stroke::new(1.0, BORDER)).inner_margin(egui::Margin::symmetric(18, 4)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for (i, (k, v)) in rows.iter().enumerate() {
            if i > 0 {
                ui.painter().hline(ui.min_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, DIVIDER));
            }
            ui.horizontal_top(|ui| {
                ui.add_space(0.0);
                ui.allocate_ui_with_layout(vec2(150.0, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_min_width(150.0);
                    ui.add_space(9.0);
                    ui.label(RichText::new(*k).font(FontId::proportional(14.0)).color(DIM));
                });
                ui.vertical(|ui| {
                    ui.add_space(9.0);
                    ui.add(egui::Label::new(RichText::new(v).font(FontId::proportional(14.0)).color(TEXT)).wrap());
                    ui.add_space(9.0);
                });
            });
        }
    });
}

impl eframe::App for About {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = BG;
        ctx.set_visuals(visuals);
        if let Ok(rows) = self.rx.try_recv() {
            self.base = rows;
            self.loaded = true;
        }
        // Uptime and memory move; nothing else does.
        ctx.request_repaint_after(Duration::from_secs(if self.loaded { 5 } else { 1 }));

        egui::CentralPanel::default().frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(32, 24))).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(ui.available_width().min(760.0));
                ui.label(RichText::new("About").font(FontId::proportional(28.0)).color(TEXT));
                ui.add_space(18.0);

                ui.horizontal(|ui| {
                    ui.add(egui::Image::new(egui::include_image!("../../assets/logo-512.png")).fit_to_exact_size(vec2(76.0, 76.0)));
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.add_space(8.0);
                        ui.label(RichText::new("VIDE-DE").font(FontId::proportional(26.0)).color(TEXT));
                        ui.label(RichText::new("The vibecoded desktop environment").font(FontId::proportional(14.0)).color(DIM));
                    });
                });
                ui.add_space(26.0);

                let mut copy = false;
                let [device, vide] = sections(&self.base);
                if !self.loaded {
                    ui.label(RichText::new("Reading your system…").color(DIM));
                    ui.add_space(8.0);
                }
                section(ui, device.0, &device.1, Some(&mut copy));
                ui.add_space(26.0);
                section(ui, vide.0, &vide.1, None);
                ui.add_space(20.0);

                if copy {
                    ctx.copy_text(self.plain_text());
                    self.copied = Some(Instant::now());
                }
                if self.copied.is_some_and(|c| c.elapsed() < Duration::from_millis(1800)) {
                    ui.label(RichText::new("Copied to the clipboard.").color(ACCENT));
                    ctx.request_repaint_after(Duration::from_millis(1900));
                }
            });
        });
    }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("About VIDE-DE").with_app_id("vide-about").with_inner_size([700.0, 720.0]).with_min_inner_size([460.0, 480.0]),
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
