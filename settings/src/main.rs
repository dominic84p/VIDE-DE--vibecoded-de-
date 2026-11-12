mod system;
mod ui;

use std::path::PathBuf;

use eframe::egui::{self, Color32, FontId, Pos2, Rect, RichText, Stroke, StrokeKind, Vec2};
use vide_config::Config;

use system::{RefreshKind, System};
use ui::{
    ACCENT, BG, BORDER, CARD_HOVER, DIM, SIDEBAR, TEXT, apply_win10_style, draw_category_icon,
    draw_home_icon, draw_search_icon,
};

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Category {
    System,
    Devices,
    Network,
    Personalization,
    Apps,
    Accounts,
    TimeLanguage,
    Gaming,
    EaseOfAccess,
    UpdateSecurity,
}

impl Category {
    const ALL: [Self; 10] = [
        Self::System,
        Self::Devices,
        Self::Network,
        Self::Personalization,
        Self::Apps,
        Self::Accounts,
        Self::TimeLanguage,
        Self::Gaming,
        Self::EaseOfAccess,
        Self::UpdateSecurity,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::System => "System",
            Self::Devices => "Devices",
            Self::Network => "Network & Internet",
            Self::Personalization => "Personalization",
            Self::Apps => "Apps",
            Self::Accounts => "Accounts",
            Self::TimeLanguage => "Time & Language",
            Self::Gaming => "Gaming",
            Self::EaseOfAccess => "Ease of Access",
            Self::UpdateSecurity => "Update & Security",
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Self::System => "Display, sound, notifications, power",
            Self::Devices => "Bluetooth, mouse, keyboard",
            Self::Network => "Wi-Fi, status, Ethernet",
            Self::Personalization => "Background, Start, taskbar",
            Self::Apps => "Apps & features, default apps",
            Self::Accounts => "Your info, session",
            Self::TimeLanguage => "Date & time, region, language",
            Self::Gaming => "Game Mode, captures",
            Self::EaseOfAccess => "Display scale, night light, cursor",
            Self::UpdateSecurity => "Health status, services, logs",
        }
    }

    fn subpages(self) -> &'static [SubPage] {
        match self {
            Self::System => &[
                SubPage::Display,
                SubPage::Sound,
                SubPage::Notifications,
                SubPage::Power,
                SubPage::Storage,
                SubPage::About,
            ],
            Self::Devices => &[SubPage::Bluetooth, SubPage::Mouse, SubPage::Keyboard],
            Self::Network => &[SubPage::NetStatus, SubPage::Wifi, SubPage::Ethernet],
            Self::Personalization => &[SubPage::Background, SubPage::Start, SubPage::Taskbar],
            Self::Apps => &[SubPage::AppsFeatures, SubPage::DefaultApps],
            Self::Accounts => &[SubPage::YourInfo],
            Self::TimeLanguage => &[SubPage::DateTime, SubPage::Language],
            Self::Gaming => &[SubPage::GameMode, SubPage::Captures],
            Self::EaseOfAccess => &[SubPage::EaseDisplay, SubPage::EaseMouse],
            Self::UpdateSecurity => &[SubPage::UpdateStatus],
        }
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
pub enum SubPage {
    Display,
    Sound,
    Notifications,
    Power,
    Storage,
    About,
    Bluetooth,
    Mouse,
    Keyboard,
    NetStatus,
    Wifi,
    Ethernet,
    Background,
    Start,
    Taskbar,
    AppsFeatures,
    DefaultApps,
    YourInfo,
    DateTime,
    Language,
    GameMode,
    Captures,
    EaseDisplay,
    EaseMouse,
    UpdateStatus,
}

impl SubPage {
    fn title(self) -> &'static str {
        match self {
            Self::Display => "Display",
            Self::Sound => "Sound",
            Self::Notifications => "Notifications & actions",
            Self::Power => "Power & sleep",
            Self::Storage => "Storage",
            Self::About => "About",
            Self::Bluetooth => "Bluetooth & other devices",
            Self::Mouse => "Mouse",
            Self::Keyboard => "Keyboard",
            Self::NetStatus => "Status",
            Self::Wifi => "Wi-Fi",
            Self::Ethernet => "Ethernet",
            Self::Background => "Background",
            Self::Start => "Start",
            Self::Taskbar => "Taskbar",
            Self::AppsFeatures => "Apps & features",
            Self::DefaultApps => "Default apps",
            Self::YourInfo => "Your info",
            Self::DateTime => "Date & time",
            Self::Language => "Language & region",
            Self::GameMode => "Game Mode",
            Self::Captures => "Captures",
            Self::EaseDisplay => "Display",
            Self::EaseMouse => "Mouse pointer",
            Self::UpdateStatus => "VIDE-DE Status & Logs",
        }
    }

    fn refresh_kind(self) -> RefreshKind {
        match self {
            Self::Sound => RefreshKind::Sound,
            Self::NetStatus | Self::Wifi | Self::Ethernet => RefreshKind::Network,
            Self::Bluetooth => RefreshKind::Bluetooth,
            Self::Storage => RefreshKind::Storage,
            Self::Power => RefreshKind::Power,
            Self::AppsFeatures => RefreshKind::Apps,
            Self::DefaultApps => RefreshKind::Defaults,
            Self::About | Self::YourInfo => RefreshKind::About,
            Self::DateTime | Self::Language => RefreshKind::Time,
            Self::UpdateStatus => RefreshKind::Update,
            _ => RefreshKind::None,
        }
    }
}

struct SearchItem {
    label: &'static str,
    keywords: &'static str,
    cat: Category,
    page: SubPage,
}

const SEARCH_INDEX: &[SearchItem] = &[
    SearchItem { label: "Display resolution, scale & night light", keywords: "display resolution refresh rate scale dpi night light", cat: Category::System, page: SubPage::Display },
    SearchItem { label: "Sound output, input & app volume", keywords: "sound audio volume speaker mic microphone pipewire", cat: Category::System, page: SubPage::Sound },
    SearchItem { label: "Notifications & tray settings", keywords: "notifications tray sni kde apps", cat: Category::System, page: SubPage::Notifications },
    SearchItem { label: "Power, sleep & idle screen lock", keywords: "power sleep idle lock suspend reboot shutdown", cat: Category::System, page: SubPage::Power },
    SearchItem { label: "Storage & disk usage", keywords: "storage disk space drives df", cat: Category::System, page: SubPage::Storage },
    SearchItem { label: "About your PC & specs", keywords: "about specs cpu ram gpu kernel winver", cat: Category::System, page: SubPage::About },
    SearchItem { label: "Bluetooth & other devices", keywords: "bluetooth pair connect audio", cat: Category::Devices, page: SubPage::Bluetooth },
    SearchItem { label: "Mouse speed & pointer precision", keywords: "mouse pointer precision acceleration speed", cat: Category::Devices, page: SubPage::Mouse },
    SearchItem { label: "Keyboard test & shortcuts", keywords: "keyboard typing shortcuts keybinds", cat: Category::Devices, page: SubPage::Keyboard },
    SearchItem { label: "Network status", keywords: "network status internet ip", cat: Category::Network, page: SubPage::NetStatus },
    SearchItem { label: "Wi-Fi networks", keywords: "wifi wireless ssid connect", cat: Category::Network, page: SubPage::Wifi },
    SearchItem { label: "Ethernet adapters", keywords: "ethernet wired lan", cat: Category::Network, page: SubPage::Ethernet },
    SearchItem { label: "Background wallpaper", keywords: "background wallpaper picture desktop", cat: Category::Personalization, page: SubPage::Background },
    SearchItem { label: "Start menu settings", keywords: "start menu hide kde apps", cat: Category::Personalization, page: SubPage::Start },
    SearchItem { label: "Taskbar info", keywords: "taskbar pins tray", cat: Category::Personalization, page: SubPage::Taskbar },
    SearchItem { label: "Apps & features", keywords: "apps programs installed launch", cat: Category::Apps, page: SubPage::AppsFeatures },
    SearchItem { label: "Default apps", keywords: "default apps browser file manager", cat: Category::Apps, page: SubPage::DefaultApps },
    SearchItem { label: "Your account info", keywords: "account user profile session", cat: Category::Accounts, page: SubPage::YourInfo },
    SearchItem { label: "Date & time", keywords: "date time clock timezone ntp", cat: Category::TimeLanguage, page: SubPage::DateTime },
    SearchItem { label: "Language & region", keywords: "language region locale", cat: Category::TimeLanguage, page: SubPage::Language },
    SearchItem { label: "Game Mode & direct scanout", keywords: "gaming game mode direct scanout ultra performance fps", cat: Category::Gaming, page: SubPage::GameMode },
    SearchItem { label: "Captures & screenshots", keywords: "captures screenshot snip", cat: Category::Gaming, page: SubPage::Captures },
    SearchItem { label: "Ease of Access display scale", keywords: "ease of access accessibility scale night light", cat: Category::EaseOfAccess, page: SubPage::EaseDisplay },
    SearchItem { label: "VIDE-DE status, services & logs", keywords: "update security logs services health", cat: Category::UpdateSecurity, page: SubPage::UpdateStatus },
];

pub struct Settings {
    pub page: Option<(Category, SubPage)>,
    pub config: Config,
    pub system: System,
    pub search: String,
    pub custom_wall: String,
    pub app_filter: String,
    pub kbd_test: String,
    pub error: Option<String>,
}

impl Settings {
    fn save(&mut self) {
        self.error = self.config.save().err().map(|e| format!("Could not save settings: {e}"));
    }

    fn navigate(&mut self, cat: Category, sub: SubPage) {
        self.page = Some((cat, sub));
        self.search.clear();
        self.system.invalidate();
    }

    fn render_search_box(&mut self, ui: &mut egui::Ui, width: f32) {
        egui::Frame::new()
            .fill(Color32::from_rgb(0x24, 0x24, 0x24))
            .stroke(Stroke::new(1.0, Color32::from_rgb(0x55, 0x55, 0x55)))
            .inner_margin(egui::Margin::symmetric(8, 5))
            .show(ui, |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .frame(egui::Frame::NONE)
                            .hint_text("Find a setting")
                            .desired_width(width - 34.0),
                    );
                    let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
                    draw_search_icon(ui.painter(), icon_rect, DIM);
                });
            });
    }

    fn render_search_results(&mut self, ui: &mut egui::Ui, width: f32) {
        let q = self.search.trim().to_lowercase();
        if q.is_empty() {
            return;
        }
        let matches: Vec<&SearchItem> = SEARCH_INDEX
            .iter()
            .filter(|item| item.label.to_lowercase().contains(&q) || item.keywords.contains(&q))
            .take(8)
            .collect();
        egui::Frame::new()
            .fill(Color32::from_rgb(0x26, 0x26, 0x26))
            .stroke(Stroke::new(1.0, ACCENT))
            .inner_margin(4.0)
            .show(ui, |ui| {
                ui.set_width(width);
                if matches.is_empty() {
                    ui.label(RichText::new("No matching settings found.").color(DIM).size(12.5));
                } else {
                    for item in matches {
                        let text = format!("{}  —  {}", item.label, item.cat.title());
                        if ui.add_sized([width - 8.0, 28.0], egui::Button::selectable(false, RichText::new(text).color(TEXT).size(13.0))).clicked() {
                            self.navigate(item.cat, item.page);
                        }
                    }
                }
            });
    }

    fn render_home(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(28.0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.add_space(16.0);
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new("Windows Settings").font(FontId::proportional(26.0)).color(TEXT));
                        ui.add_space(16.0);
                        let sw = ui.available_width().min(420.0);
                        ui.allocate_ui(Vec2::new(sw, 32.0), |ui| {
                            self.render_search_box(ui, sw);
                        });
                        if !self.search.trim().is_empty() {
                            ui.add_space(4.0);
                            ui.allocate_ui(Vec2::new(sw, 10.0), |ui| {
                                self.render_search_results(ui, sw);
                            });
                        }
                    });

                    ui.add_space(28.0);

                    let avail = ui.available_width();
                    let tile_w = 215.0_f32;
                    let tile_h = 96.0_f32;
                    let spacing = 12.0_f32;
                    let cols = ((avail + spacing) / (tile_w + spacing)).floor().max(1.0) as usize;
                    let grid_w = cols as f32 * tile_w + (cols.saturating_sub(1)) as f32 * spacing;
                    let left_pad = ((avail - grid_w) / 2.0).max(0.0);

                    for chunk in Category::ALL.chunks(cols) {
                        ui.horizontal(|ui| {
                            ui.add_space(left_pad);
                            for &cat in chunk {
                                let (rect, resp) = ui.allocate_exact_size(Vec2::new(tile_w, tile_h), egui::Sense::click());
                                let painter = ui.painter();
                                if resp.hovered() {
                                    painter.rect_filled(rect, 0.0, CARD_HOVER);
                                    painter.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::from_rgb(0x55, 0x55, 0x55)), StrokeKind::Inside);
                                }
                                let icon_rect = Rect::from_min_size(rect.min + Vec2::new(14.0, 22.0), Vec2::splat(32.0));
                                draw_category_icon(painter, icon_rect, cat, ACCENT);
                                let text_x = icon_rect.right() + 14.0;
                                painter.text(
                                    Pos2::new(text_x, rect.top() + 24.0),
                                    egui::Align2::LEFT_TOP,
                                    cat.title(),
                                    FontId::proportional(15.0),
                                    TEXT,
                                );
                                painter.text(
                                    Pos2::new(text_x, rect.top() + 46.0),
                                    egui::Align2::LEFT_TOP,
                                    cat.subtitle(),
                                    FontId::proportional(11.5),
                                    DIM,
                                );
                                if resp.clicked() {
                                    self.navigate(cat, cat.subpages()[0]);
                                }
                                ui.add_space(spacing);
                            }
                        });
                        ui.add_space(spacing);
                    }
                });
            });
    }

    fn render_category(&mut self, ui: &mut egui::Ui, cat: Category, sub: SubPage) {
        self.system.refresh(sub.refresh_kind());

        egui::Panel::left("win10_sidebar")
            .exact_size(240.0)
            .frame(egui::Frame::new().fill(SIDEBAR).inner_margin(egui::Margin::symmetric(10, 14)))
            .show(ui, |ui| {
                // Home button
                let (h_rect, h_resp) = ui.allocate_exact_size(Vec2::new(220.0, 34.0), egui::Sense::click());
                if h_resp.hovered() {
                    ui.painter().rect_filled(h_rect, 0.0, CARD_HOVER);
                }
                let icon_r = Rect::from_min_size(h_rect.min + Vec2::new(10.0, 9.0), Vec2::splat(16.0));
                draw_home_icon(ui.painter(), icon_r, TEXT);
                ui.painter().text(
                    Pos2::new(icon_r.right() + 10.0, h_rect.center().y),
                    egui::Align2::LEFT_CENTER,
                    "Home",
                    FontId::proportional(14.0),
                    TEXT,
                );
                if h_resp.clicked() {
                    self.page = None;
                    self.search.clear();
                    return;
                }

                ui.add_space(10.0);
                self.render_search_box(ui, 218.0);
                if !self.search.trim().is_empty() {
                    ui.add_space(4.0);
                    self.render_search_results(ui, 218.0);
                }

                ui.add_space(14.0);
                ui.label(RichText::new(cat.title()).font(FontId::proportional(14.5)).color(TEXT));
                ui.add_space(8.0);

                for &p in cat.subpages() {
                    let selected = p == sub;
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(220.0, 36.0), egui::Sense::click());
                    let painter = ui.painter();
                    if selected {
                        painter.rect_filled(rect, 0.0, Color32::from_rgb(0x2d, 0x2d, 0x2d));
                        painter.rect_filled(
                            Rect::from_min_size(Pos2::new(rect.left(), rect.top() + 6.0), Vec2::new(4.0, rect.height() - 12.0)),
                            0.0,
                            ACCENT,
                        );
                    } else if resp.hovered() {
                        painter.rect_filled(rect, 0.0, CARD_HOVER);
                    }
                    painter.text(
                        Pos2::new(rect.left() + 16.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        p.title(),
                        FontId::proportional(13.5),
                        if selected { TEXT } else { Color32::from_rgb(0xd4, 0xd4, 0xd4) },
                    );
                    if resp.clicked() {
                        self.page = Some((cat, p));
                        self.system.invalidate();
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(RichText::new("VIDE-DE Settings").color(BORDER).size(11.0));
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(28.0))
            .show(ui, |ui| {
                ui.label(RichText::new(sub.title()).font(FontId::proportional(26.0)).color(TEXT));
                ui.add_space(10.0);
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    let changed = self.render_subpage(ui, sub);
                    if changed {
                        self.save();
                    }
                    if let Some(e) = &self.error {
                        ui.add_space(8.0);
                        ui.colored_label(Color32::LIGHT_RED, e);
                    }
                });
            });
    }
}

impl eframe::App for Settings {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        apply_win10_style(ui.ctx());
        match self.page {
            None => self.render_home(ui),
            Some((cat, sub)) => self.render_category(ui, cat, sub),
        }
    }
}

fn parse_cli_page() -> Option<(Category, SubPage)> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--page" {
            return match args.next().as_deref() {
                Some("personalization" | "background") => Some((Category::Personalization, SubPage::Background)),
                Some("display") => Some((Category::System, SubPage::Display)),
                Some("sound") => Some((Category::System, SubPage::Sound)),
                Some("network" | "wifi") => Some((Category::Network, SubPage::Wifi)),
                Some("bluetooth") => Some((Category::Devices, SubPage::Bluetooth)),
                Some("mouse") => Some((Category::Devices, SubPage::Mouse)),
                Some("gaming") => Some((Category::Gaming, SubPage::GameMode)),
                Some("start") => Some((Category::Personalization, SubPage::Start)),
                Some("about") => Some((Category::System, SubPage::About)),
                Some("storage") => Some((Category::System, SubPage::Storage)),
                Some("power") => Some((Category::System, SubPage::Power)),
                Some("apps") => Some((Category::Apps, SubPage::AppsFeatures)),
                Some("update") => Some((Category::UpdateSecurity, SubPage::UpdateStatus)),
                _ => None,
            };
        }
    }
    None
}

fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(include_bytes!("../../assets/logo-512.png")).ok()?.into_rgba8();
    let (w, h) = img.dimensions();
    Some(egui::IconData { rgba: img.into_raw(), width: w, height: h })
}

fn main() -> eframe::Result<()> {
    let page = parse_cli_page();
    let config = Config::load();
    let custom_wall = if config.wallpaper == PathBuf::default() {
        String::new()
    } else {
        config.wallpaper.display().to_string()
    };

    let mut viewport = egui::ViewportBuilder::default().with_inner_size([1020.0, 700.0]).with_min_inner_size([720.0, 520.0]);
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let opts = eframe::NativeOptions { viewport, ..Default::default() };

    eframe::run_native(
        "Settings",
        opts,
        Box::new(move |cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(Settings {
                page,
                config,
                system: System::default(),
                search: String::new(),
                custom_wall,
                app_filter: String::new(),
                kbd_test: String::new(),
                error: None,
            }))
        }),
    )
}
