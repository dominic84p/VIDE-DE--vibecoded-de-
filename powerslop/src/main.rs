//! PowerSlop: VIDE-DE's shell window. Like PowerShell, but vibecoded.
//!
//! Classic PowerShell look: the deep blue background, a startup banner and a
//! `PS path>` prompt. The terminal itself is alacritty's emulator (through egui_term),
//! so TUI apps, colours and scrollback work. Tabs: Ctrl+Shift+T new, Ctrl+Shift+W close.
//!
//! `powerslop -e PROGRAM [ARGS...]` runs a program in the first tab instead of a shell
//! (File Exploder uses it for scripts), and waits for Enter once it exits so the output
//! can be read, like a Windows console window.

use std::sync::mpsc::{self, Receiver, Sender};

use eframe::egui::{self, Color32, FontId, Key, Modifiers, RichText};
use egui_term::{BackendCommand, BackendSettings, ColorPalette, FontSettings, PtyEvent, TerminalBackend, TerminalFont, TerminalTheme, TerminalView};

/// PowerShell's signature blue.
const PS_BLUE: Color32 = Color32::from_rgb(0x01, 0x24, 0x56);
const BAR: Color32 = Color32::from_rgb(0x00, 0x1a, 0x40);
const TAB_ACTIVE: Color32 = PS_BLUE;
const DIM: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("PowerSlop")
            .with_app_id("powerslop")
            .with_inner_size([900.0, 560.0])
            .with_min_inner_size([360.0, 200.0]),
        ..Default::default()
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run = match args.first().map(String::as_str) {
        Some("-e") if args.len() > 1 => Some(args[1..].to_vec()),
        _ => None,
    };
    eframe::run_native("PowerSlop", options, Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, run)))))
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

/// Campbell colours on PowerShell's blue, like the classic PowerShell console.
fn campbell() -> TerminalTheme {
    let c = |s: &str| s.to_string();
    TerminalTheme::new(Box::new(ColorPalette {
        foreground: c("#eeedf0"),
        background: c("#012456"),
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

/// Print the banner, then start the user's shell; bash also gets a `PS path>` prompt
/// on top of the user's own bashrc.
fn launcher(shell: &str) -> String {
    let banner = "PowerSlop\\nCopyright (C) Nobody. No humans were involved.\\n\\n\
                  Install the latest PowerSlop for new features and improvements! Just ask an AI.\\n\\n";
    let rc = rc_file();
    if shell.ends_with("bash") && rc.is_some() {
        format!("printf '{banner}'; exec '{shell}' --rcfile '{}' -i", rc.unwrap())
    } else {
        format!("printf '{banner}'; exec '{shell}'")
    }
}

/// A bashrc that loads the user's own, then sets the PowerShell-style prompt.
fn rc_file() -> Option<String> {
    let dir = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"))
        .join("vide-de");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("powerslop.bashrc");
    let rc = "[ -f ~/.bashrc ] && . ~/.bashrc\nPS1='PS \\w> '\n";
    std::fs::write(&path, rc).ok()?;
    Some(path.to_string_lossy().into_owned())
}

impl App {
    fn new(ctx: &egui::Context, run: Option<Vec<String>>) -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        let mut app = Self { tabs: Vec::new(), active: 0, next_id: 0, events_tx, events_rx, error: None };
        app.new_tab(ctx, run);
        app
    }

    fn new_tab(&mut self, ctx: &egui::Context, run: Option<Vec<String>>) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let id = self.next_id;
        self.next_id += 1;
        let args = match run {
            // The program and its arguments go in as "$@", so no quoting is needed.
            Some(cmd) => {
                let script = "\"$@\"; s=$?; printf '\\n[Exited with code %s. Press Enter to close.]' \"$s\"; read _";
                let mut a = vec!["-c".to_string(), script.to_string(), "powerslop".to_string()];
                a.extend(cmd);
                a
            }
            None => vec!["-c".into(), launcher(&shell)],
        };
        let settings = BackendSettings { shell: "/bin/sh".into(), args, ..Default::default() };
        match TerminalBackend::new(id, ctx.clone(), self.events_tx.clone(), settings) {
            Ok(backend) => {
                let title = "PowerSlop".to_string();
                self.tabs.push(Tab { id, backend, title });
                self.active = self.tabs.len() - 1;
            }
            Err(e) => self.error = Some(format!("couldn't start {shell}: {e}")),
        }
    }

    /// Selected text to the clipboard.
    fn copy(&mut self, ctx: &egui::Context) {
        if let Some(tab) = self.tabs.get(self.active) {
            let text = tab.backend.selectable_content();
            if !text.is_empty() {
                ctx.copy_text(text);
            }
        }
    }

    /// Clipboard text typed into the shell. egui can't read the clipboard on demand,
    /// so ask wl-paste.
    fn paste(&mut self) {
        let Some(tab) = self.tabs.get_mut(self.active) else { return };
        match std::process::Command::new("wl-paste").args(["--no-newline", "--type", "text"]).output() {
            Ok(out) if out.status.success() && !out.stdout.is_empty() => {
                tab.backend.process_command(BackendCommand::Write(out.stdout));
            }
            Ok(_) => {}
            Err(e) => self.error = Some(format!("Paste needs wl-paste (wl-clipboard): {e}")),
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
            self.new_tab(ctx, None);
        }
        if shortcut(Key::W) {
            self.close_tab(self.active, ctx);
        }
        if shortcut(Key::C) {
            self.copy(ctx);
        }
        if shortcut(Key::V) {
            self.paste();
        }
        if self.tabs.is_empty() {
            return;
        }
        if let Some(t) = self.tabs.get(self.active) {
            let title = if t.title == "PowerSlop" { t.title.clone() } else { format!("{} - PowerSlop", t.title) };
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
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
                    self.new_tab(ctx, None);
                }
                if let Some(i) = close {
                    self.close_tab(i, ctx);
                }
                // Copy / Paste, on the right like a toolbar.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let btn = |ui: &mut egui::Ui, label: &str, tip: &str| {
                        ui.add(egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(PS_BLUE).corner_radius(4.0)).on_hover_text(tip).clicked()
                    };
                    if btn(ui, "Paste", "Paste (Ctrl+Shift+V)") {
                        self.paste();
                    }
                    if btn(ui, "Copy", "Copy selection (Ctrl+Shift+C)") {
                        self.copy(ctx);
                    }
                });
            });
        });

        egui::CentralPanel::default().frame(egui::Frame::new().fill(TAB_ACTIVE)).show(ctx, |ui| {
            if let Some(e) = &self.error {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            let Some(tab) = self.tabs.get_mut(self.active) else { return };
            
            // Handle mouse wheel scrolling for scrollback BEFORE creating the view
            let rect = ui.available_rect_before_wrap();
            if ui.input(|i| i.pointer.hover_pos().is_some_and(|p| rect.contains(p))) {
                let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
                if scroll_delta != 0.0 {
                    let lines = (scroll_delta / 12.0).round() as i32;
                    tab.backend.process_command(BackendCommand::Scroll(lines));
                }
            }
            
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
