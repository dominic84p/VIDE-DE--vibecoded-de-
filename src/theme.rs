//! Windoze 10's two "Windoze modes" for the shell: dark (its default) and light. Every colour
//! the taskbar, Start, Action Center, flyouts and menus paint comes from here, so switching
//! the mode (Settings > Personalization > Colors) flips all of them at once. Titlebars stay
//! white in both, like Windoze 10's. Things on the accent colour stay white text either way.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::canvas::{hex, Rgba};

const fn rgba(c: u32, a: u8) -> Rgba {
    let h = hex(c);
    Rgba(h.0, h.1, h.2, a)
}

pub const ACCENT: Rgba = hex(0x0078d7);
pub const ACCENT_HOVER: Rgba = hex(0x1a86dd);
/// Text and icons on the accent colour.
pub const ON_ACCENT: Rgba = hex(0xffffff);

pub struct Theme {
    pub taskbar: Rgba,
    /// Overlays for hovered and pressed/active taskbar buttons.
    pub hover: Rgba,
    pub active: Rgba,
    /// Running-app underline and slider fill.
    pub accent_light: Rgba,
    pub search_bg: Rgba,
    pub search_hover: Rgba,
    pub divider: Rgba,
    pub text: Rgba,
    pub text_dim: Rgba,
    pub body: Rgba,
    pub link: Rgba,
    pub error: Rgba,
    /// Start menu and search: list and rail.
    pub start: Rgba,
    pub start_rail: Rgba,
    /// Flyouts, Action Center, context menus, Task View.
    pub panel: Rgba,
    pub border: Rgba,
    pub card: Rgba,
    pub card_hover: Rgba,
    pub tile: Rgba,
    pub tile_hover: Rgba,
    pub button: Rgba,
    pub button_hover: Rgba,
    pub row_hover: Rgba,
    pub row_open: Rgba,
    pub track: Rgba,
    pub field: Rgba,
    /// Context menus: background, the row under the pointer, and greyed-out entries.
    pub menu: Rgba,
    pub menu_hover: Rgba,
    pub disabled: Rgba,
    /// Behind a hovered thumbnail in Task View.
    pub highlight: Rgba,
}

const DARK: Theme = Theme {
    taskbar: hex(0x101010),
    hover: rgba(0xffffff, 0x1a),
    active: rgba(0xffffff, 0x26),
    accent_light: hex(0x76b9ed),
    search_bg: hex(0x3a3a3a),
    search_hover: hex(0x474747),
    divider: hex(0x3a3a3a),
    text: hex(0xffffff),
    text_dim: hex(0xa0a0a0),
    body: hex(0xc8c8c8),
    link: hex(0x8ec5f5),
    error: hex(0xff99a4),
    start: rgba(0x2b2b2b, 0xf5),
    start_rail: rgba(0x1f1f1f, 0xf5),
    panel: hex(0x1f1f1f),
    border: hex(0x484848),
    card: hex(0x2b2b2b),
    card_hover: hex(0x353535),
    tile: hex(0x3b3b3b),
    tile_hover: hex(0x4a4a4a),
    button: hex(0x4c4c4c),
    button_hover: hex(0x5c5c5c),
    row_hover: hex(0x333333),
    row_open: hex(0x3a3a3a),
    track: hex(0x5a5a5a),
    field: hex(0x101010),
    menu: hex(0x2b2b2b),
    menu_hover: rgba(0xffffff, 0x1f),
    disabled: hex(0x7a7a7a),
    highlight: rgba(0xffffff, 0x30),
};

const LIGHT: Theme = Theme {
    taskbar: hex(0xe6e6e6),
    hover: rgba(0xffffff, 0x99),
    active: rgba(0xffffff, 0xcc),
    accent_light: ACCENT,
    search_bg: hex(0xffffff),
    search_hover: hex(0xffffff),
    divider: hex(0xc4c4c4),
    text: hex(0x000000),
    text_dim: hex(0x5d5d5d),
    body: hex(0x333333),
    link: hex(0x0063b1),
    error: hex(0xc50f1f),
    start: rgba(0xe6e6e6, 0xf5),
    start_rail: rgba(0xdadada, 0xf5),
    panel: hex(0xf2f2f2),
    border: hex(0xcccccc),
    card: hex(0xfafafa),
    card_hover: hex(0xffffff),
    tile: hex(0xd4d4d4),
    tile_hover: hex(0xc4c4c4),
    button: hex(0xcccccc),
    button_hover: hex(0xbdbdbd),
    row_hover: hex(0xe0e0e0),
    row_open: hex(0xd6d6d6),
    track: hex(0x999999),
    field: hex(0xffffff),
    menu: hex(0xf2f2f2),
    menu_hover: rgba(0x000000, 0x14),
    disabled: hex(0xa0a0a0),
    highlight: rgba(0x000000, 0x1c),
};

static LIGHT_MODE: AtomicBool = AtomicBool::new(false);

/// The shell's palette for the current mode.
pub fn th() -> &'static Theme {
    if light() { &LIGHT } else { &DARK }
}

pub fn light() -> bool {
    LIGHT_MODE.load(Ordering::Relaxed)
}

/// Switch modes; true when that changed anything (cached pictures then need repainting).
pub fn set_light(on: bool) -> bool {
    LIGHT_MODE.swap(on, Ordering::Relaxed) != on
}
