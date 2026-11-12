//! System tray: volume (PipeWire, through `wpctl`) and brightness. Brightness dims
//! the picture in the compositor, so it works on any monitor, not just laptop panels.

use smithay::{
    backend::renderer::element::memory::MemoryRenderBuffer,
    utils::{Logical, Point, Rectangle},
};

use crate::{
    canvas::{hex, Canvas, Rgba},
    state::State,
    ui::TASKBAR_H,
};

pub const FLYOUT_W: i32 = 360;
pub const FLYOUT_H: i32 = 96;
const SLIDER_X: i32 = 60;
const SLIDER_W: i32 = FLYOUT_W - SLIDER_X - 64;
const BG: Rgba = hex(0x1f1f1f);
const BORDER: Rgba = hex(0x3c3c3c);
const TRACK: Rgba = hex(0x5a5a5a);
const FILL: Rgba = hex(0x76b9ed);
const WHITE: Rgba = hex(0xffffff);
/// Lowest brightness we allow, so the screen can't be dimmed to black by accident.
pub const MIN_BRIGHTNESS: f32 = 0.2;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Flyout {
    Volume,
    Brightness,
}

pub struct Tray {
    pub volume: f32,
    pub muted: bool,
    pub brightness: f32,
    pub flyout: Option<Flyout>,
    /// Left button held on the slider.
    pub dragging: bool,
    picture: Option<((Flyout, u32, bool), MemoryRenderBuffer)>,
}

impl Tray {
    pub fn new() -> Self {
        let (volume, muted) = read_volume().unwrap_or((0.5, false));
        Self { volume, muted, brightness: 1.0, flyout: None, dragging: false, picture: None }
    }
}

/// `wpctl get-volume` prints e.g. "Volume: 0.45" or "Volume: 0.45 [MUTED]".
fn read_volume() -> Option<(f32, bool)> {
    let out = std::process::Command::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let v = text.split_whitespace().nth(1)?.parse().ok()?;
    Some((v, text.contains("MUTED")))
}

impl State {
    pub fn flyout_rect(&self) -> Rectangle<i32, Logical> {
        let x = self.output_size.w - FLYOUT_W - 8;
        Rectangle::new((x, self.output_size.h - TASKBAR_H - FLYOUT_H - 8).into(), (FLYOUT_W, FLYOUT_H).into())
    }

    pub fn toggle_flyout(&mut self, f: Flyout) {
        self.tray.flyout = if self.tray.flyout == Some(f) { None } else { Some(f) };
        if f == Flyout::Volume {
            if let Some((v, m)) = read_volume() {
                self.tray.volume = v;
                self.tray.muted = m;
            }
        }
        self.needs_redraw = true;
    }

    pub fn close_flyout(&mut self) {
        if self.tray.flyout.take().is_some() {
            self.tray.dragging = false;
            self.needs_redraw = true;
        }
    }

    pub fn set_volume(&mut self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.tray.volume = v;
        self.spawn(&format!("wpctl set-volume @DEFAULT_AUDIO_SINK@ {v:.2}"));
        if self.tray.muted && v > 0.0 {
            self.tray.muted = false;
            self.spawn("wpctl set-mute @DEFAULT_AUDIO_SINK@ 0");
        }
        self.needs_redraw = true;
    }

    pub fn toggle_mute(&mut self) {
        self.tray.muted = !self.tray.muted;
        self.spawn("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle");
        self.needs_redraw = true;
    }

    pub fn set_brightness(&mut self, b: f32) {
        self.tray.brightness = b.clamp(MIN_BRIGHTNESS, 1.0);
        self.needs_redraw = true;
    }

    /// Pointer at `p` on the open flyout's slider (or the mute icon): apply it.
    pub fn flyout_press(&mut self, p: Point<f64, Logical>) {
        let Some(f) = self.tray.flyout else { return };
        let local = p - self.flyout_rect().loc.to_f64();
        if f == Flyout::Volume && local.x < SLIDER_X as f64 - 8.0 {
            self.toggle_mute();
            return;
        }
        self.tray.dragging = true;
        self.flyout_drag(p);
    }

    pub fn flyout_drag(&mut self, p: Point<f64, Logical>) {
        let Some(f) = self.tray.flyout else { return };
        let local = p.x - self.flyout_rect().loc.x as f64;
        let v = ((local - SLIDER_X as f64) / SLIDER_W as f64).clamp(0.0, 1.0) as f32;
        match f {
            Flyout::Volume => self.set_volume(v),
            Flyout::Brightness => self.set_brightness(MIN_BRIGHTNESS + v * (1.0 - MIN_BRIGHTNESS)),
        }
    }

    pub fn flyout_picture(&mut self) -> Option<&MemoryRenderBuffer> {
        let f = self.tray.flyout?;
        let (value, muted) = match f {
            Flyout::Volume => (self.tray.volume, self.tray.muted),
            Flyout::Brightness => ((self.tray.brightness - MIN_BRIGHTNESS) / (1.0 - MIN_BRIGHTNESS), false),
        };
        let key = (f, (value * 100.0).round() as u32, muted);
        if self.tray.picture.as_ref().is_none_or(|(k, _)| *k != key) {
            let mut c = Canvas::new(FLYOUT_W, FLYOUT_H);
            c.fill(BG);
            c.outline(0, 0, FLYOUT_W, FLYOUT_H, BORDER);
            let cy = FLYOUT_H / 2 + 6;
            match f {
                Flyout::Volume => speaker(&mut c, 22.0, cy as f32, muted, value, WHITE),
                Flyout::Brightness => sun(&mut c, 30.0, cy as f32, WHITE),
            }
            if let Some(font) = self.font.as_ref() {
                let title = if f == Flyout::Volume { "Volume" } else { "Brightness" };
                c.text(font, 12.0, 16, 8, 20, hex(0xa0a0a0), title);
                let pct = format!("{}", key.1);
                c.text(font, 16.0, FLYOUT_W - 48, cy - 14, 28, WHITE, &pct);
            }
            let fill_w = (SLIDER_W as f32 * value) as i32;
            c.round_rect(SLIDER_X, cy - 1, SLIDER_W, 3, 1, TRACK);
            c.round_rect(SLIDER_X, cy - 1, fill_w, 3, 1, if muted { TRACK } else { FILL });
            c.round_rect(SLIDER_X + fill_w - 5, cy - 11, 10, 22, 4, if muted { TRACK } else { FILL });
            self.tray.picture = Some((key, c.into_buffer()));
        }
        self.tray.picture.as_ref().map(|(_, b)| b)
    }
}

// Windows-style outline glyphs (20x20 grid, white, round joins), rendered with resvg.
const SPEAKER: &str = r##"<path d="M2.5 7.5h3.2L10.5 3.8v12.4L5.7 12.5H2.5z" fill="none" stroke="#fff" stroke-width="1.4" stroke-linejoin="round"/>"##;
const WAVES: [&str; 3] = [
    r##"<path d="M13 8a2.8 2.8 0 0 1 0 4" fill="none" stroke="#fff" stroke-width="1.4" stroke-linecap="round"/>"##,
    r##"<path d="M15 6a5.6 5.6 0 0 1 0 8" fill="none" stroke="#fff" stroke-width="1.4" stroke-linecap="round"/>"##,
    r##"<path d="M17 4a8.5 8.5 0 0 1 0 12" fill="none" stroke="#fff" stroke-width="1.4" stroke-linecap="round"/>"##,
];
const MUTE: &str = r##"<path d="M13.5 7.8l4.4 4.4M17.9 7.8l-4.4 4.4" fill="none" stroke="#fff" stroke-width="1.4" stroke-linecap="round"/>"##;

fn sun_svg() -> String {
    let mut rays = String::new();
    for i in 0..8 {
        let a = i as f32 * std::f32::consts::FRAC_PI_4;
        let (s, c) = a.sin_cos();
        rays += &format!(
            r##"<line x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}"/>"##,
            10.0 + c * 6.0, 10.0 + s * 6.0, 10.0 + c * 8.2, 10.0 + s * 8.2
        );
    }
    format!(
        r##"<circle cx="10" cy="10" r="3.6" fill="none" stroke="#fff" stroke-width="1.4"/><g stroke="#fff" stroke-width="1.4" stroke-linecap="round">{rays}</g>"##
    )
}

thread_local! {
    static GLYPHS: std::cell::RefCell<std::collections::HashMap<String, std::rc::Rc<crate::icons::Icon>>> = Default::default();
}

/// Rasterize (and cache) one glyph, centred at (x, y).
fn glyph(c: &mut Canvas, body: String, x: f32, y: f32, size: u32) {
    let key = format!("{size}:{body}");
    let icon = GLYPHS.with(|g| {
        g.borrow_mut()
            .entry(key)
            .or_insert_with(|| {
                let svg = format!(r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20">{body}</svg>"##);
                std::rc::Rc::new(crate::icons::render_svg(svg.as_bytes(), size).unwrap_or(crate::icons::Icon {
                    w: 1,
                    h: 1,
                    rgba: vec![0; 4],
                }))
            })
            .clone()
    });
    c.icon(&icon, (x - size as f32 / 2.0).round() as i32, (y - size as f32 / 2.0).round() as i32);
}

/// Speaker with 0-3 sound waves for the level, or an X when muted.
pub fn speaker(c: &mut Canvas, x: f32, y: f32, muted: bool, level: f32, _color: Rgba) {
    let mut body = SPEAKER.to_string();
    if muted {
        body += MUTE;
    } else {
        let waves = if level <= 0.01 { 0 } else if level < 0.34 { 1 } else if level < 0.67 { 2 } else { 3 };
        body.extend(WAVES.iter().take(waves).copied());
    }
    glyph(c, body, x + 5.0, y, 20);
}

/// Sun: brightness.
pub fn sun(c: &mut Canvas, x: f32, y: f32, _color: Rgba) {
    glyph(c, sun_svg(), x, y, 20);
}
