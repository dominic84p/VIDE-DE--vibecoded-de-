//! Terminaitor: VIDE-DE's terminal. It'll be back.
//!
//! Windows Terminal look: tab strip on top, Campbell colours. The terminal itself is
//! alacritty's emulator (through egui_term), so TUI apps, colours and scrollback work.
//! Ctrl+Shift+T new tab, Ctrl+Shift+W close tab; a tab closes when its shell exits.

use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui::{self, Color32, FontId, Key, Modifiers, RichText};
use egui_term::{BackendSettings, ColorPalette, FontSettings, PtyEvent, TerminalBackend, TerminalFont, TerminalTheme, TerminalView};

const BAR: Color32 = Color32::from_rgb(0x20, 0x20, 0x20);
const TAB_ACTIVE: Color32 = Color32::from_rgb(0x0c, 0x0c, 0x0c);
const DIM: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Terminaitor")
            .with_app_id("terminaitor")
            .with_inner_size([900.0, 560.0])
            .with_min_inner_size([360.0, 200.0]),
        ..Default::default()
    };
    eframe::run_native("Terminaitor", options, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))))
}

struct Tab {
    id: u64,
    backend: TerminalBackend,
    title: String,
}

struct App {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    events_tx: Sender<(u64, PtyEvent)>,
    events_rx: Receiver<(u64, PtyEvent)>,
    error: Option<String>,
}

/// Windows Terminal's default "Campbell" scheme.
fn campbell() -> TerminalTheme {
    let c = |s: &str| s.to_string();
    TerminalTheme::new(Box::new(ColorPalette {
        foreground: c("#cccccc"),
        background: c("#0c0c0c"),
        black: c("#0c0c0c"),
        red: c("#c50f1f"),
        green: c("#13a10e"),
        yellow: c("#c19c00"),
        blue: c("#0037da"),
        magenta: c("#881798"),
        cyan: c("#3a96dd"),
        white: c("#cccccc"),
        bright_black: c("#767676"),
        bright_red: c("#e74856"),
        bright_green: c("#16c60c"),
        bright_yellow: c("#f9f1a5"),
        bright_blue: c("#3b78ff"),
        bright_magenta: c("#b4009e"),
        bright_cyan: c("#61d6d6"),
        bright_white: c("#f2f2f2"),
        ..ColorPalette::default()
    }))
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        let mut app = Self { tabs: Vec::new(), active: 0, next_id: 0, events_tx, events_rx, error: None };
        app.new_tab(ctx);
        app
    }

    fn new_tab(&mut self, ctx: &egui::Context) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let id = self.next_id;
        self.next_id += 1;
        let settings = BackendSettings { shell: shell.clone(), ..Default::default() };
        match TerminalBackend::new(id, ctx.clone(), self.events_tx.clone(), settings) {
            Ok(backend) => {
                let title = shell.rsplit('/').next().unwrap_or("shell").to_string();
                self.tabs.push(Tab { id, backend, title });
                self.active = self.tabs.len() - 1;
            }
            Err(e) => self.error = Some(format!("couldn't start {shell}: {e}")),
        }
    }

    fn close_tab(&mut self, i: usize, ctx: &egui::Context) {
        if i < self.tabs.len() {
            self.tabs.remove(i);
        }
        if self.tabs.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.active = self.active.min(self.tabs.len() - 1);
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Shell exits close their tab; title changes rename it.
        while let Ok((id, event)) = self.events_rx.try_recv() {
            let Some(i) = self.tabs.iter().position(|t| t.id == id) else { continue };
            match event {
                PtyEvent::Exit => self.close_tab(i, ctx),
                PtyEvent::Title(t) => self.tabs[i].title = t,
                _ => {}
            }
        }

        let shortcut = |k| ctx.input_mut(|i| i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, k));
        if shortcut(Key::T) {
            self.new_tab(ctx);
        }
        if shortcut(Key::W) {
            self.close_tab(self.active, ctx);
        }
        if self.tabs.is_empty() {
            return;
        }
        if let Some(t) = self.tabs.get(self.active) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{} - Terminaitor", t.title)));
        }

        egui::TopBottomPanel::top("tabs").frame(egui::Frame::new().fill(BAR).inner_margin(egui::Margin::symmetric(6, 4))).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let mut close = None;
                for (i, tab) in self.tabs.iter().enumerate() {
                    let active = i == self.active;
                    let fill = if active { TAB_ACTIVE } else { BAR };
                    egui::Frame::new().fill(fill).corner_radius(egui::CornerRadius { nw: 6, ne: 6, sw: 0, se: 0 }).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
                        let label = RichText::new(&tab.title).color(if active { Color32::WHITE } else { DIM });
                        if ui.add(egui::Label::new(label).sense(egui::Sense::click())).clicked() {
                            self.active = i;
                        }
                        if ui.add(egui::Button::new(RichText::new("×").color(DIM)).frame(false)).clicked() {
                            close = Some(i);
                        }
                    });
                }
                if ui.add(egui::Button::new(RichText::new("+").size(16.0).color(DIM)).frame(false)).on_hover_text("New tab (Ctrl+Shift+T)").clicked() {
                    self.new_tab(ctx);
                }
                if let Some(i) = close {
                    self.close_tab(i, ctx);
                }
            });
        });

        egui::CentralPanel::default().frame(egui::Frame::new().fill(TAB_ACTIVE)).show(ctx, |ui| {
            if let Some(e) = &self.error {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            let Some(tab) = self.tabs.get_mut(self.active) else { return };
            let font = TerminalFont::new(FontSettings { font_type: FontId::monospace(14.0) });
            let view = TerminalView::new(ui, &mut tab.backend)
                .set_focus(true)
                .set_theme(campbell())
                .set_font(font)
                .set_size(ui.available_size());
            ui.add(view);
        });
    }
}
