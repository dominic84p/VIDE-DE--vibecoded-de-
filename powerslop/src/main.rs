//! PowerSlop: VIDE-DE's shell window. Like PowerShell, but vibecoded.
//!
//! A copy of the Windows PowerShell console window: the deep blue screen, the
//! startup banner, a `PS path>` prompt and the console's light scrollbar, with no tabs
//! or toolbar. Right-click copies the selection, or pastes when nothing is selected,
//! like the console's QuickEdit mode. The terminal itself is alacritty's emulator
//! (through egui_term), so TUI apps, colours and scrollback work.
//! Ctrl+Shift+C / V copy and paste, Ctrl+Shift+N opens another window.
//!
//! `powerslop -e PROGRAM [ARGS...]` runs a program instead of a shell (File Exploder
//! uses it for scripts), and waits for Enter once it exits so the output can be read,
//! like a Windows console window.

use std::sync::mpsc::{self, Receiver, Sender};

use alacritty_terminal::grid::Dimensions;
use eframe::egui::{self, Color32, FontId, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Vec2, pos2, vec2};
use egui_term::{BackendCommand, BackendSettings, ColorPalette, FontSettings, PtyEvent, TerminalBackend, TerminalFont, TerminalTheme, TerminalView};

/// PowerShell's signature blue.
const PS_BLUE: Color32 = Color32::from_rgb(0x01, 0x24, 0x56);
/// The console's scrollbar: Windows' light theme, whatever the text colours.
const SB_W: f32 = 17.0;
const SB_TRACK: Color32 = Color32::from_rgb(0xf0, 0xf0, 0xf0);
const SB_THUMB: Color32 = Color32::from_rgb(0xcd, 0xcd, 0xcd);
const SB_THUMB_HOVER: Color32 = Color32::from_rgb(0xa6, 0xa6, 0xa6);
const SB_THUMB_DRAG: Color32 = Color32::from_rgb(0x60, 0x60, 0x60);
const SB_ARROW: Color32 = Color32::from_rgb(0x60, 0x60, 0x60);
const SB_ARROW_HOVER: Color32 = Color32::from_rgb(0xda, 0xda, 0xda);

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run = match args.first().map(String::as_str) {
        Some("-e") if args.len() > 1 => Some(args[1..].to_vec()),
        _ => None,
    };
    // The console names its window after what it runs.
    let title = run.as_ref().map(|r| r[0].clone()).unwrap_or_else(|| "PowerSlop".into());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_app_id("powerslop")
            .with_inner_size([960.0, 560.0])
            .with_min_inner_size([360.0, 200.0]),
        ..Default::default()
    };
    eframe::run_native("PowerSlop", options, Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, run, title)))))
}

struct App {
    backend: Option<TerminalBackend>,
    title: String,
    events_rx: Receiver<(u64, PtyEvent)>,
    error: Option<String>,
    /// Pointer offset into the thumb while dragging it.
    drag: Option<f32>,
}

/// Campbell colours on PowerShell's blue, like the Windows PowerShell console.
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
    // Windows PowerShell's banner, line for line.
    let banner = "PowerSlop\\nCopyright (C) Nobody. No humans were involved.\\n\\n\
                  Try the new cross-platform PowerSlop https://just.ask.an.ai\\n\\n";
    let rc = rc_file();
    if shell.ends_with("bash") && rc.is_some() {
        format!("printf '{banner}'; exec '{shell}' --rcfile '{}' -i", rc.unwrap())
    } else {
        format!("printf '{banner}'; exec '{shell}'")
    }
}

/// A bashrc that loads the user's own, then sets the PowerShell-style prompt: the
/// full path, as PowerShell prints it, not `~`.
fn rc_file() -> Option<String> {
    let dir = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"))
        .join("vide-de");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join("powerslop.bashrc");
    let rc = "[ -f ~/.bashrc ] && . ~/.bashrc\nPS1='PS ${PWD}> '\n";
    std::fs::write(&path, rc).ok()?;
    Some(path.to_string_lossy().into_owned())
}

impl App {
    fn new(ctx: &egui::Context, run: Option<Vec<String>>, title: String) -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        let mut app = Self { backend: None, title, events_rx, error: None, drag: None };
        app.start(ctx, run, events_tx);
        app
    }

    fn start(&mut self, ctx: &egui::Context, run: Option<Vec<String>>, events_tx: Sender<(u64, PtyEvent)>) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
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
        match TerminalBackend::new(0, ctx.clone(), events_tx, settings) {
            Ok(b) => self.backend = Some(b),
            Err(e) => self.error = Some(format!("couldn't start {shell}: {e}")),
        }
    }

    /// The selection as text, one line per screen row (egui_term's own copy runs
    /// rows together), trailing blanks trimmed like the console does.
    fn selection(&self) -> String {
        let Some(b) = &self.backend else { return String::new() };
        let content = b.last_content();
        let Some(range) = content.selectable_range else { return String::new() };
        let mut out = String::new();
        let mut line = None;
        for cell in content.grid.display_iter() {
            if !range.contains(cell.point) {
                continue;
            }
            if line.is_some_and(|l| l != cell.point.line) {
                let trimmed = out.trim_end_matches(' ').len();
                out.truncate(trimmed);
                out.push('\n');
            }
            line = Some(cell.point.line);
            out.push(cell.c);
        }
        out.trim_end_matches(' ').to_string()
    }

    fn copy(&mut self, ctx: &egui::Context) -> bool {
        let text = self.selection();
        if text.is_empty() {
            return false;
        }
        ctx.copy_text(text);
        true
    }

    /// Clipboard text typed into the shell. egui can't read the clipboard on demand,
    /// so ask wl-paste.
    fn paste(&mut self) {
        let Some(b) = self.backend.as_mut() else { return };
        match std::process::Command::new("wl-paste").args(["--no-newline", "--type", "text"]).output() {
            Ok(out) if out.status.success() && !out.stdout.is_empty() => {
                b.process_command(BackendCommand::Write(out.stdout));
            }
            Ok(_) => {}
            Err(e) => self.error = Some(format!("Paste needs wl-paste (wl-clipboard): {e}")),
        }
    }

    /// The console's vertical scrollbar: arrows at both ends, a thumb sized to the
    /// visible share of the buffer, paging when the track is clicked.
    fn scrollbar(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let Some(b) = self.backend.as_mut() else { return };
        let grid = &b.last_content().grid;
        let (history, offset, screen) = (grid.history_size(), grid.display_offset(), grid.screen_lines().max(1));
        let total = (history + screen) as f32;
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, SB_TRACK);

        let up = Rect::from_min_size(rect.min, vec2(SB_W, SB_W));
        let down = Rect::from_min_size(pos2(rect.left(), rect.bottom() - SB_W), vec2(SB_W, SB_W));
        let track = Rect::from_min_max(pos2(rect.left(), up.bottom()), pos2(rect.right(), down.top()));
        let mut scroll = 0i32;
        for (r, dir) in [(up, -1.0), (down, 1.0)] {
            let resp = ui.interact(r, ui.id().with(("arrow", dir as i32)), Sense::click());
            if resp.hovered() {
                p.rect_filled(r, 0.0, SB_ARROW_HOVER);
            }
            let c = r.center();
            p.add(egui::Shape::convex_polygon(
                vec![c + vec2(-4.0, -2.0 * dir), c + vec2(4.0, -2.0 * dir), c + vec2(0.0, 2.0 * dir)],
                SB_ARROW,
                egui::Stroke::NONE,
            ));
            if resp.clicked() || resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.press_start_time()).is_some_and(|t| ui.input(|i| i.time) - t > 0.4) {
                scroll += if dir < 0.0 { 1 } else { -1 };
            }
        }

        let thumb_h = (track.height() * screen as f32 / total).clamp(17.0, track.height());
        let room = (track.height() - thumb_h).max(1.0);
        let at = if history == 0 { 1.0 } else { (history - offset.min(history)) as f32 / history as f32 };
        let thumb = Rect::from_min_size(pos2(track.left() + 1.0, track.top() + room * at), vec2(SB_W - 2.0, thumb_h));
        let t_resp = ui.interact(thumb, ui.id().with("thumb"), Sense::drag());
        let tr_resp = ui.interact(track, ui.id().with("track"), Sense::click());
        if t_resp.drag_started() {
            self.drag = t_resp.interact_pointer_pos().map(|pos| pos.y - thumb.top());
        }
        if t_resp.drag_stopped() {
            self.drag = None;
        }
        if let (Some(grab), Some(pos)) = (self.drag, t_resp.interact_pointer_pos()) {
            let frac = ((pos.y - grab - track.top()) / room).clamp(0.0, 1.0);
            let want = history as i32 - (frac * history as f32).round() as i32;
            scroll += want - offset as i32;
        } else if tr_resp.clicked()
            && let Some(pos) = tr_resp.interact_pointer_pos()
        {
            // Clicking the track pages, like Windows.
            scroll += if pos.y < thumb.top() { screen as i32 } else if pos.y > thumb.bottom() { -(screen as i32) } else { 0 };
        }
        let color = if self.drag.is_some() {
            SB_THUMB_DRAG
        } else if t_resp.hovered() {
            SB_THUMB_HOVER
        } else {
            SB_THUMB
        };
        p.rect_filled(thumb, 0.0, color);
        if scroll != 0 {
            b.process_command(BackendCommand::Scroll(scroll));
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // The shell exiting closes the window; title changes rename it.
        while let Ok((_, event)) = self.events_rx.try_recv() {
            match event {
                PtyEvent::Exit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                PtyEvent::Title(t) => {
                    self.title = t;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.title.clone()));
                }
                _ => {}
            }
        }

        let shortcut = |k| ctx.input_mut(|i| i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, k));
        if shortcut(Key::C) {
            self.copy(ctx);
        }
        if shortcut(Key::V) {
            self.paste();
        }
        if shortcut(Key::N) || shortcut(Key::T) {
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).spawn();
            }
        }

        egui::CentralPanel::default().frame(egui::Frame::new().fill(PS_BLUE)).show(ctx, |ui| {
            if let Some(e) = &self.error {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            let full = ui.available_rect_before_wrap();
            let term_rect = Rect::from_min_max(full.min, pos2(full.right() - SB_W, full.bottom()));
            let sb_rect = Rect::from_min_max(pos2(full.right() - SB_W, full.top()), full.max);

            // Right-click: copy the selection, or paste when there's none (QuickEdit).
            let right_click = ui.input(|i| i.pointer.button_clicked(PointerButton::Secondary) && i.pointer.interact_pos().is_some_and(|p: Pos2| term_rect.contains(p)));
            if right_click && !self.copy(ctx) {
                self.paste();
            }

            let Some(b) = self.backend.as_mut() else { return };
            // Wheel over the text scrolls the scrollback.
            if ui.input(|i| i.pointer.hover_pos().is_some_and(|p| term_rect.contains(p))) {
                let scroll_delta = ui.input(|i| i.raw_scroll_delta.y);
                if scroll_delta != 0.0 {
                    let lines = (scroll_delta / 12.0).round() as i32;
                    b.process_command(BackendCommand::Scroll(lines));
                }
            }
            let font = TerminalFont::new(FontSettings { font_type: FontId::monospace(15.0) });
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(term_rect));
            let view = TerminalView::new(&mut child, b).set_focus(true).set_theme(campbell()).set_font(font).set_size(Vec2::new(term_rect.width(), term_rect.height()));
            child.add(view);
            self.scrollbar(ui, sb_rect);
            ui.allocate_rect(full, Sense::hover());
        });
    }
}
