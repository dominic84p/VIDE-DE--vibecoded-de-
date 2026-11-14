//! PowerSlop 2.0: VIDE-DE's shell window, styled after Windoze PowerSlop 2.0.
//!
//! Classic PowerSlop 2.0 deep blue console (`#012456`), 2009 startup banner,
//! `PS /path>` prompt, built-in PowerSlop 2.0 cmdlet/alias shims for bash/zsh,
//! Windoze 7 light scrollbar, QuickEdit copy/paste (`Ctrl+C` copies when text
//! is selected or sends SIGINT otherwise, right-click copies or pastes), the mouse wheel
//! sent to apps that track the mouse (Shift+wheel scrolls history), and
//! `Ctrl+=`/`-`/`0` or `Ctrl+Scroll` font zoom.
//!
//! `powerslop -e PROGRAM [ARGS...]` runs a program directly and waits for Enter
//! on exit so output can be read, like a Windoze console host.

use std::sync::mpsc::{self, Receiver, Sender};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::selection::SelectionType;
use alacritty_terminal::term::TermMode;
use eframe::egui::{self, Color32, FontId, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Vec2, pos2, vec2};
use egui_term::{
    BackendCommand, BackendSettings, ColorPalette, FontSettings, PtyEvent, TerminalBackend, TerminalFont, TerminalTheme,
    TerminalView,
};

/// Classic PowerSlop 2.0 deep blue background.
const PS_BLUE: Color32 = Color32::from_rgb(0x01, 0x24, 0x56);
/// Windoze 7 / Server 2008 R2 classic light scrollbar.
const SB_W: f32 = 17.0;
const SB_TRACK: Color32 = Color32::from_rgb(0xf0, 0xf0, 0xf0);
const SB_BORDER: Color32 = Color32::from_rgb(0xc8, 0xc8, 0xc8);
const SB_THUMB: Color32 = Color32::from_rgb(0xcd, 0xcd, 0xcd);
const SB_THUMB_HOVER: Color32 = Color32::from_rgb(0xa6, 0xa6, 0xa6);
const SB_THUMB_DRAG: Color32 = Color32::from_rgb(0x60, 0x60, 0x60);
const SB_ARROW: Color32 = Color32::from_rgb(0x50, 0x50, 0x50);
const SB_ARROW_HOVER: Color32 = Color32::from_rgb(0xda, 0xda, 0xda);
const BASE_FONT_SIZE: f32 = 15.0;
/// Wheel travel (egui points) per line of scrollback, and per wheel event sent to an app.
const LINE_PX: f32 = 12.0;
const NOTCH_PX: f32 = 36.0;

/// Tell programs what kind of terminal this is. Nothing above us sets TERM (the session
/// starts from a display manager), and without it zsh, fish and vim can't move the cursor:
/// backspace left junk and the arrow keys did nothing. egui_term runs alacritty's terminal,
/// so its terminfo is the exact match when installed; xterm-256color is close enough otherwise.
fn set_term_env() {
    let home = std::env::var("HOME").unwrap_or_default();
    let alacritty = ["/usr/share/terminfo/a/alacritty", "/usr/lib/terminfo/a/alacritty", "/lib/terminfo/a/alacritty"]
        .iter()
        .map(std::path::PathBuf::from)
        .chain([std::path::Path::new(&home).join(".terminfo/a/alacritty")])
        .any(|p| p.exists());
    // SAFETY: called first thing in main, before any thread exists.
    unsafe {
        std::env::set_var("TERM", if alacritty { "alacritty" } else { "xterm-256color" });
        std::env::set_var("COLORTERM", "truecolor");
    }
}

fn main() -> eframe::Result {
    set_term_env();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let run = match args.first().map(String::as_str) {
        Some("-e") if args.len() > 1 => Some(args[1..].to_vec()),
        _ => None,
    };
    let title = run
        .as_ref()
        .map(|r| format!("Windoze PowerSlop 2.0 - {}", r[0]))
        .unwrap_or_else(|| "Windoze PowerSlop 2.0".into());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_app_id("powerslop")
            .with_icon(ps_icon())
            .with_inner_size([960.0, 560.0])
            .with_min_inner_size([360.0, 200.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Windoze PowerSlop 2.0",
        options,
        Box::new(move |cc| {
            vide_fonts::install_egui!(eframe::egui, cc.egui_ctx);
            Ok(Box::new(App::new(&cc.egui_ctx, run, title)))
        }),
    )
}

/// 32x32 PowerSlop 2.0 blue icon with a crisp white `>_` glyph.
fn ps_icon() -> egui::IconData {
    let (w, h) = (32usize, 32usize);
    let mut rgba = vec![0u8; w * h * 4];
    let put = |buf: &mut [u8], x: usize, y: usize, r: u8, g: u8, b: u8| {
        if x < w && y < h {
            let i = (y * w + x) * 4;
            buf[i] = r;
            buf[i + 1] = g;
            buf[i + 2] = b;
            buf[i + 3] = 255;
        }
    };
    for y in 0..h {
        for x in 0..w {
            let border = x == 0 || y == 0 || x == w - 1 || y == h - 1;
            if border {
                put(&mut rgba, x, y, 0x3b, 0x78, 0xff);
            } else {
                put(&mut rgba, x, y, 0x01, 0x24, 0x56);
            }
        }
    }
    // `>` chevron
    for i in 0..6 {
        for t in 0..2 {
            put(&mut rgba, 7 + i + t, 10 + i, 242, 242, 242);
            put(&mut rgba, 7 + i + t, 21 - i, 242, 242, 242);
        }
    }
    // `_` underscore
    for x in 16..25 {
        put(&mut rgba, x, 20, 242, 242, 242);
        put(&mut rgba, x, 21, 242, 242, 242);
    }
    egui::IconData { rgba, width: w as u32, height: h as u32 }
}

struct App {
    backend: Option<TerminalBackend>,
    events_rx: Receiver<(u64, PtyEvent)>,
    error: Option<String>,
    font_size: f32,
    /// Pointer offset into the thumb while dragging it.
    drag: Option<f32>,
    /// Wheel travel not yet turned into whole lines.
    scroll_acc: f32,
}

/// Classic Windoze PowerSlop 2.0 console palette on `#012456`.
fn powershell2_palette() -> TerminalTheme {
    let c = |s: &str| s.to_string();
    TerminalTheme::new(Box::new(ColorPalette {
        foreground: c("#eeedf0"),
        background: c("#012456"),
        black: c("#000000"),
        red: c("#800000"),
        green: c("#008000"),
        yellow: c("#eeedf0"),
        blue: c("#000080"),
        magenta: c("#800080"),
        cyan: c("#008080"),
        white: c("#c0c0c0"),
        bright_black: c("#808080"),
        bright_red: c("#ff0000"),
        bright_green: c("#00ff00"),
        bright_yellow: c("#ffff00"),
        bright_blue: c("#3b78ff"),
        bright_magenta: c("#ff00ff"),
        bright_cyan: c("#00ffff"),
        bright_white: c("#ffffff"),
        ..ColorPalette::default()
    }))
}

/// PowerSlop 2.0 cmdlet & alias shims shared by bash and zsh.
const PS2_SHIM: &str = r#"
export PSVersionTable="Name: PSVersion = 2.0 (PowerSlop)"
Get-Host() { printf 'Name             : ConsoleHost\nVersion          : 2.0.0.0 (PowerSlop)\nCurrentCulture   : en-US\n'; }
Get-Help() {
    printf 'TOPIC\n    Windoze PowerSlop 2.0 Help\n\nBUILT-IN CMDLETS\n'
    printf '    Get-Location (gl)      Print current directory\n'
    printf '    Set-Location (sl)      Change directory\n'
    printf '    Get-ChildItem (gci)    List directory contents\n'
    printf '    Get-Content (gc)       Print file contents\n'
    printf '    Select-String (sls)    Search text in files\n'
    printf '    Get-Process (gps)      List running processes\n'
    printf '    Stop-Process           Kill a process\n'
    printf '    Get-Command (gcm)      Find a command\n'
    printf '    Get-Date               Show current date/time\n'
    printf '    Get-History (h)        Show command history\n'
    printf '    Get-Variable (gv)      List environment variables\n'
    printf '    Write-Host             Write colored output\n'
    printf '    Clear-Host (cls)       Clear the screen\n'
    printf '    Start-Process (saps)   Launch a program or file\n'
}
Get-Location() { pwd "$@"; }
Set-Location() { cd "${1:-$HOME}"; }
Get-ChildItem() { ls -la --color=auto "$@"; }
Clear-Host() { clear; }
Write-Output() { printf '%s\n' "$*"; }
Write-Host() {
    local col=""
    if [ "$1" = "-ForegroundColor" ] && [ -n "$2" ]; then
        case "$2" in
            Red|red) col='\033[91m' ;;
            Green|green) col='\033[92m' ;;
            Yellow|yellow) col='\033[93m' ;;
            Blue|blue) col='\033[94m' ;;
            Magenta|magenta) col='\033[95m' ;;
            Cyan|cyan) col='\033[96m' ;;
            White|white) col='\033[97m' ;;
        esac
        shift 2
    fi
    if [ -n "$col" ]; then printf "${col}%s\033[0m\n" "$*"; else printf '%s\n' "$*"; fi
}
Get-Date() { date "$@"; }
Get-Process() { ps aux "$@"; }
Stop-Process() { kill "$@"; }
Get-Content() { cat "$@"; }
Select-String() { grep "$@"; }
Get-Command() { which "$@"; }
Get-History() { history "$@"; }
Get-Variable() { env | sort; }
Start-Process() { xdg-open "$@" >/dev/null 2>&1 & }
alias gl=Get-Location sl=Set-Location gci=Get-ChildItem cls=Clear-Host
alias gps=Get-Process gc=Get-Content sls=Select-String gcm=Get-Command
alias h=Get-History gv=Get-Variable saps=Start-Process
"#;

fn config_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::var("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config"))
        .join("vide-de");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Write bashrc / zshrc init files and return the shell launch command.
fn launcher(shell: &str) -> String {
    let banner = "Windoze PowerSlop 2.0\\nCopyright (C) 2009 Microslop Corporation (vibecoded). All rights reserved.\\n\\n";
    let Some(dir) = config_dir() else {
        return format!("printf '{banner}'; exec '{shell}'");
    };
    if shell.ends_with("bash") {
        let path = dir.join("powerslop.bashrc");
        let rc = format!("[ -f ~/.bashrc ] && . ~/.bashrc\n{PS2_SHIM}\nPS1='PS ${{PWD}}> '\n");
        if std::fs::write(&path, rc).is_ok() {
            return format!("printf '{banner}'; exec '{shell}' --rcfile '{}' -i", path.display());
        }
    } else if shell.ends_with("zsh") {
        let zdir = dir.join("powerslop-zsh");
        if std::fs::create_dir_all(&zdir).is_ok() {
            let rc = format!("[ -f ~/.zshrc ] && . ~/.zshrc\n{PS2_SHIM}\nsetopt PROMPT_SUBST\nPROMPT='PS %~> '\n");
            if std::fs::write(zdir.join(".zshrc"), rc).is_ok() {
                return format!("printf '{banner}'; ZDOTDIR='{}' exec '{shell}' -i", zdir.display());
            }
        }
    }
    format!("printf '{banner}'; exec '{shell}'")
}

impl App {
    fn new(ctx: &egui::Context, run: Option<Vec<String>>, _title: String) -> Self {
        let (events_tx, events_rx) = mpsc::channel();
        let mut app = Self { backend: None, events_rx, error: None, font_size: BASE_FONT_SIZE, drag: None, scroll_acc: 0.0 };
        app.start(ctx, run, events_tx);
        app
    }

    fn start(&mut self, ctx: &egui::Context, run: Option<Vec<String>>, events_tx: Sender<(u64, PtyEvent)>) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let args = match run {
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

    fn write(&mut self, bytes: Vec<u8>) {
        if let Some(b) = self.backend.as_mut() {
            b.process_command(BackendCommand::Write(bytes));
        }
    }

    /// Selection text, one line per screen row, trailing blanks trimmed.
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

    /// Windoze QuickEdit copy: copy the selection and drop it, so the next right-click pastes
    /// and the next Ctrl+C interrupts instead of copying the same text again.
    fn quick_copy(&mut self, ctx: &egui::Context) -> bool {
        if !self.copy(ctx) {
            return false;
        }
        if let Some(b) = self.backend.as_mut() {
            // A selection that starts and ends on the same point is empty.
            b.process_command(BackendCommand::SelectStart(SelectionType::Simple, 0.0, 0.0));
        }
        true
    }

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

    /// Classic Windoze 7 / PowerSlop 2.0 light-themed vertical scrollbar.
    fn scrollbar(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let Some(b) = self.backend.as_mut() else { return };
        let grid = &b.last_content().grid;
        let (history, offset, screen) = (grid.history_size(), grid.display_offset(), grid.screen_lines().max(1));
        let total = (history + screen) as f32;
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, SB_TRACK);
        p.line_segment([rect.left_top(), rect.left_bottom()], egui::Stroke::new(1.0_f32, SB_BORDER));

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
            if resp.clicked()
                || resp.is_pointer_button_down_on()
                    && ui.input(|i| i.pointer.press_start_time()).is_some_and(|t| ui.input(|i| i.time) - t > 0.4)
            {
                scroll += if dir < 0.0 { 1 } else { -1 };
            }
        }

        let thumb_h = (track.height() * screen as f32 / total).clamp(17.0, track.height());
        let room = (track.height() - thumb_h).max(1.0);
        let at = if history == 0 { 1.0 } else { (history - offset.min(history)) as f32 / history as f32 };
        let thumb = Rect::from_min_size(pos2(track.left() + 2.0, track.top() + room * at), vec2(SB_W - 3.0, thumb_h));
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
        p.rect_stroke(thumb, 0.0, egui::Stroke::new(1.0_f32, SB_BORDER), egui::StrokeKind::Inside);
        if scroll != 0 {
            b.process_command(BackendCommand::Scroll(scroll));
        }
    }
}

/// Forward keyboard events to the PTY when PowerSlop has window focus but the
/// mouse pointer is outside `term_rect` (working around egui_term's `contains_pointer` gate).
/// An xterm mouse report for a wheel `button` (64 up, 65 down) at a 0-based cell, in the
/// encoding the app asked for.
fn wheel_report(mode: TermMode, button: u8, col: usize, line: usize) -> Vec<u8> {
    if mode.contains(TermMode::SGR_MOUSE) {
        return format!("\x1b[<{button};{};{}M", col + 1, line + 1).into_bytes();
    }
    let mut out = vec![0x1b, b'[', b'M', 32 + button];
    for v in [col, line] {
        let v = 32 + 1 + v as u32;
        if mode.contains(TermMode::UTF8_MOUSE) {
            let mut buf = [0; 4];
            out.extend(char::from_u32(v).unwrap_or(' ').encode_utf8(&mut buf).as_bytes());
        } else {
            out.push(v.min(255) as u8);
        }
    }
    out
}

fn forward_unhovered_keys(ctx: &egui::Context, b: &mut TerminalBackend) {
    let (focused, events) = ctx.input(|i| (i.focused, i.events.clone()));
    if !focused {
        return;
    }
    for ev in events {
        match ev {
            egui::Event::Text(t) => {
                b.process_command(BackendCommand::Write(t.into_bytes()));
            }
            egui::Event::Key { key, pressed: true, modifiers, .. } => {
                if modifiers.ctrl && !modifiers.shift && !modifiers.alt {
                    let name = key.name();
                    if name.len() == 1 {
                        let ch = name.as_bytes()[0].to_ascii_lowercase();
                        if ch.is_ascii_lowercase() {
                            b.process_command(BackendCommand::Write(vec![ch - b'a' + 1]));
                            continue;
                        }
                    }
                }
                let seq: Option<&[u8]> = match key {
                    Key::Enter => Some(b"\r"),
                    Key::Backspace => Some(b"\x7f"),
                    Key::Tab => Some(b"\t"),
                    Key::Escape => Some(b"\x1b"),
                    Key::ArrowUp => Some(b"\x1b[A"),
                    Key::ArrowDown => Some(b"\x1b[B"),
                    Key::ArrowRight => Some(b"\x1b[C"),
                    Key::ArrowLeft => Some(b"\x1b[D"),
                    Key::Home => Some(b"\x1b[H"),
                    Key::End => Some(b"\x1b[F"),
                    Key::Delete => Some(b"\x1b[3~"),
                    Key::PageUp => Some(b"\x1b[5~"),
                    Key::PageDown => Some(b"\x1b[6~"),
                    _ => None,
                };
                if let Some(bytes) = seq {
                    b.process_command(BackendCommand::Write(bytes.to_vec()));
                }
            }
            _ => {}
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok((_, event)) = self.events_rx.try_recv() {
            match event {
                PtyEvent::Exit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                PtyEvent::Title(t) => {
                    let title = if t.is_empty() { "Windoze PowerSlop 2.0".into() } else { format!("Windoze PowerSlop 2.0 - {t}") };
                    ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
                }
                // The terminal answering a program's question (what are you, where's the
                // cursor). These used to be dropped, so anything that asked (fish asks on
                // every start) waited for a reply that never came.
                PtyEvent::PtyWrite(text) => self.write(text.into_bytes()),
                // "What colour is the background?" (vim, fish and others pick light or dark
                // themes from it); the default text and background colours are the ones asked.
                PtyEvent::ColorRequest(index, reply) => {
                    let rgb = match index {
                        256 => Some((0xee, 0xed, 0xf0)),
                        257 => Some((0x01, 0x24, 0x56)),
                        _ => None,
                    };
                    if let Some((r, g, b)) = rgb {
                        self.write(reply(alacritty_terminal::vte::ansi::Rgb { r, g, b }).into_bytes());
                    }
                }
                _ => {}
            }
        }

        // egui-winit turns Ctrl+C / Ctrl+X / Ctrl+V (with or without Shift) into Copy / Cut /
        // Paste events and drops the key itself, so those keys are rebuilt from the events here.
        // Taking them out also keeps egui_term from acting on them a second time.
        let (clipboard_keys, shift) = ctx.input_mut(|i| {
            let mut keys = Vec::new();
            i.events.retain(|ev| match ev {
                egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) => {
                    keys.push(ev.clone());
                    false
                }
                _ => true,
            });
            (keys, i.modifiers.shift)
        });
        for ev in clipboard_keys {
            match ev {
                // Ctrl+Shift+C: copy, like Linux terminals.
                egui::Event::Copy if shift => {
                    self.copy(ctx);
                }
                // Windoze QuickEdit: Ctrl+C copies when a selection exists, otherwise sends ^C (SIGINT).
                egui::Event::Copy => {
                    if !self.quick_copy(ctx) && let Some(b) = self.backend.as_mut() {
                        b.process_command(BackendCommand::Write(vec![0x03]));
                    }
                }
                // Ctrl+X is the app's (nano's Exit, for one).
                egui::Event::Cut if !shift => {
                    if let Some(b) = self.backend.as_mut() {
                        b.process_command(BackendCommand::Write(vec![0x18]));
                    }
                }
                egui::Event::Paste(_) => self.paste(),
                _ => {}
            }
        }
        let ctrl_shift = |k| ctx.input_mut(|i| i.consume_key(Modifiers::CTRL | Modifiers::SHIFT, k));
        let ctrl = |k| ctx.input_mut(|i| i.consume_key(Modifiers::CTRL, k));
        if ctrl(Key::Insert) {
            self.copy(ctx);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Insert)) {
            self.paste();
        }
        if ctrl_shift(Key::N) || ctrl_shift(Key::T) {
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).spawn();
            }
        }
        if ctrl(Key::Equals) || ctrl(Key::Plus) {
            self.font_size = (self.font_size + 1.0).min(28.0);
        }
        if ctrl(Key::Minus) {
            self.font_size = (self.font_size - 1.0).max(10.0);
        }
        if ctrl(Key::Num0) {
            self.font_size = BASE_FONT_SIZE;
        }

        egui::CentralPanel::default().frame(egui::Frame::new().fill(PS_BLUE)).show(ctx, |ui| {
            if let Some(e) = &self.error {
                ui.colored_label(Color32::LIGHT_RED, e);
            }
            let full = ui.available_rect_before_wrap();
            let term_rect = Rect::from_min_max(full.min + vec2(4.0, 4.0), pos2(full.right() - SB_W - 2.0, full.bottom() - 2.0));
            let sb_rect = Rect::from_min_max(pos2(full.right() - SB_W, full.top()), full.max);

            let right_click = ui.input(|i| {
                i.pointer.button_clicked(PointerButton::Secondary) && i.pointer.interact_pos().is_some_and(|p: Pos2| term_rect.contains(p))
            });
            if right_click && !self.quick_copy(ctx) {
                self.paste();
            }

            let Some(b) = self.backend.as_mut() else { return };
            let pointer_in_term = ui.input(|i| i.pointer.hover_pos().is_some_and(|p| term_rect.contains(p)));
            if pointer_in_term {
                // The wheel is ours alone: egui_term's view would scroll a second time, and it
                // turns the wheel into arrow keys even for apps that asked for mouse reports.
                let (scroll_delta, mods, pos) = ui.input_mut(|i| {
                    i.events.retain(|e| !matches!(e, egui::Event::MouseWheel { .. }));
                    (i.raw_scroll_delta.y, i.modifiers, i.pointer.hover_pos())
                });
                if scroll_delta != 0.0 && mods.ctrl {
                    self.font_size = (self.font_size + scroll_delta.signum()).clamp(10.0, 28.0);
                } else if scroll_delta != 0.0 {
                    // Touchpads send many small deltas: keep the remainder instead of rounding
                    // each one away.
                    self.scroll_acc += scroll_delta;
                    let content = b.last_content();
                    let mode = content.terminal_mode;
                    // Like Konsole: apps that track the mouse (vim, htop, mc) get real wheel
                    // events, one per notch; Shift+wheel still scrolls our own history.
                    if mode.intersects(TermMode::MOUSE_MODE) && !mods.shift {
                        let notches = (self.scroll_acc / NOTCH_PX).trunc();
                        self.scroll_acc -= notches * NOTCH_PX;
                        if notches != 0.0 {
                            let size = &content.terminal_size;
                            let local = pos.unwrap_or(term_rect.min) - term_rect.min;
                            let col = (local.x / size.cell_width.max(1) as f32).max(0.0) as usize;
                            let line = (local.y / size.cell_height.max(1) as f32).max(0.0) as usize;
                            let col = col.min(content.grid.columns().saturating_sub(1));
                            let line = line.min(content.grid.screen_lines().saturating_sub(1));
                            let button = if notches > 0.0 { 64 } else { 65 };
                            let report = wheel_report(mode, button, col, line);
                            b.process_command(BackendCommand::Write(report.repeat(notches.abs() as usize)));
                        }
                    } else {
                        let lines = (self.scroll_acc / LINE_PX).trunc();
                        self.scroll_acc -= lines * LINE_PX;
                        if lines != 0.0 {
                            b.process_command(BackendCommand::Scroll(lines as i32));
                        }
                    }
                }
            } else {
                forward_unhovered_keys(ctx, b);
            }
            let font = TerminalFont::new(FontSettings { font_type: FontId::monospace(self.font_size) });
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(term_rect));
            let view = TerminalView::new(&mut child, b)
                .set_focus(true)
                .set_theme(powershell2_palette())
                .set_font(font)
                .set_size(Vec2::new(term_rect.width(), term_rect.height()));
            child.add(view);
            self.scrollbar(ui, sb_rect);
            ui.allocate_rect(full, Sense::hover());
        });
    }
}
