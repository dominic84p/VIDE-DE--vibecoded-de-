mod backend;
mod icons;

use std::sync::mpsc::{self, Receiver, Sender};

use backend::{
    AppItem, BackendEvent, DetectedBackends, InstalledSets, PackageSource, SourceFilter,
    StoreAction, featured_catalog, launch_app, spawn_action, spawn_refresh_library, spawn_search,
};
use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, RichText, Stroke, Vec2};
use icons::{IconCache, IconLookup};

// Authentic Windows 10 Store Light Palette
const WIN10_HEADER_BG: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
const WIN10_CANVAS_BG: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
const WIN10_TILE_BG: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
const WIN10_TILE_STAGE: Color32 = Color32::from_rgb(0xf6, 0xf6, 0xf6);
const WIN10_TILE_HOVER: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
const WIN10_BORDER: Color32 = Color32::from_rgb(0xe1, 0xe1, 0xe1);
const WIN10_BLUE: Color32 = Color32::from_rgb(0x00, 0x78, 0xd7);
const WIN10_BLUE_DARK: Color32 = Color32::from_rgb(0x00, 0x5a, 0x9e);
const WIN10_GREEN: Color32 = Color32::from_rgb(0x10, 0x7c, 0x10);
const WIN10_RED: Color32 = Color32::from_rgb(0xe8, 0x11, 0x23);
const WIN10_GOLD: Color32 = Color32::from_rgb(0xff, 0xb9, 0x00);
const WIN10_TEXT: Color32 = Color32::from_rgb(0x11, 0x11, 0x11);
const WIN10_MUTED: Color32 = Color32::from_rgb(0x66, 0x66, 0x66);

#[derive(Clone, Copy, PartialEq, Eq)]
enum NavTab {
    Home,
    Apps,
    Gaming,
    Library,
    Updates,
}

struct StoreApp {
    backends: DetectedBackends,
    tx: Sender<BackendEvent>,
    rx: Receiver<BackendEvent>,
    icons: IconCache,

    tab: NavTab,
    search_query: String,
    source_filter: SourceFilter,
    searching: bool,
    search_results: Vec<AppItem>,
    showing_search: bool,

    loading_library: bool,
    installed: Vec<AppItem>,
    sets: InstalledSets,
    updates: Vec<AppItem>,
    library_filter: String,
    library_only_flatpak: bool,

    selected_app: Option<AppItem>,

    task_running: bool,
    task_title: String,
    task_status: Option<(bool, String)>,
    task_logs: Vec<String>,
    show_activity_log: bool,
    style_applied: bool,
}

impl StoreApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let backends = DetectedBackends::detect();
        let (tx, rx) = mpsc::channel();
        spawn_refresh_library(backends.clone(), tx.clone(), cc.egui_ctx.clone());

        Self {
            backends,
            tx,
            rx,
            icons: IconCache::new(),
            tab: NavTab::Home,
            search_query: String::new(),
            source_filter: SourceFilter::All,
            searching: false,
            search_results: Vec::new(),
            showing_search: false,
            loading_library: true,
            installed: Vec::new(),
            sets: InstalledSets::default(),
            updates: Vec::new(),
            library_filter: String::new(),
            library_only_flatpak: false,
            selected_app: None,
            task_running: false,
            task_title: String::new(),
            task_status: None,
            task_logs: Vec::new(),
            show_activity_log: false,
            style_applied: false,
        }
    }

    fn apply_win10_style(&mut self, ctx: &egui::Context) {
        if self.style_applied {
            return;
        }
        self.style_applied = true;
        let mut style = (*ctx.global_style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.panel_fill = WIN10_CANVAS_BG;
        style.visuals.window_fill = WIN10_CANVAS_BG;
        style.visuals.extreme_bg_color = WIN10_CANVAS_BG;
        style.visuals.faint_bg_color = WIN10_TILE_STAGE;
        style.visuals.selection.bg_fill = WIN10_BLUE;
        style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(0xe6, 0xe6, 0xe6);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xee, 0xee, 0xee);
        style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, WIN10_TEXT);
        style.visuals.widgets.inactive.corner_radius = CornerRadius::same(2);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(0xd8, 0xd8, 0xd8);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0xe2, 0xe2, 0xe2);
        style.visuals.widgets.hovered.corner_radius = CornerRadius::same(2);
        style.visuals.widgets.active.bg_fill = WIN10_BLUE;
        style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        style.visuals.widgets.active.corner_radius = CornerRadius::same(2);
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(14.0, 6.0);
        ctx.set_global_style(style);
    }

    fn poll_events(&mut self, ctx: &egui::Context) {
        if self.icons.poll(ctx) || self.loading_library || self.searching || self.task_running {
            ctx.request_repaint();
        }
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                BackendEvent::LibraryLoaded {
                    installed,
                    updates,
                    sets,
                } => {
                    self.loading_library = false;
                    self.installed = installed;
                    self.updates = updates;
                    self.sets = sets;
                    if let Some(sel) = &mut self.selected_app {
                        sel.installed = self.sets.contains_any(&sel.id);
                        sel.new_version = self
                            .updates
                            .iter()
                            .find(|u| u.id == sel.id)
                            .and_then(|u| u.new_version.clone());
                    }
                }
                BackendEvent::SearchResults { query, results } => {
                    if query == self.search_query.trim() {
                        self.searching = false;
                        self.search_results = results;
                    }
                }
                BackendEvent::TaskStarted { title } => {
                    self.task_running = true;
                    self.task_title = title;
                    self.task_status = None;
                    self.task_logs.clear();
                    self.show_activity_log = true;
                }
                BackendEvent::TaskLog { line } => {
                    self.task_logs.push(line);
                    if self.task_logs.len() > 400 {
                        let drain = self.task_logs.len() - 400;
                        self.task_logs.drain(0..drain);
                    }
                }
                BackendEvent::TaskFinished { ok, message } => {
                    self.task_running = false;
                    self.task_status = Some((ok, message));
                    self.loading_library = true;
                    spawn_refresh_library(self.backends.clone(), self.tx.clone(), ctx.clone());
                }
            }
        }
    }

    fn trigger_search(&mut self, ctx: &egui::Context) {
        let q = self.search_query.trim().to_string();
        if q.is_empty() {
            self.showing_search = false;
            self.searching = false;
            self.search_results.clear();
            return;
        }
        self.showing_search = true;
        self.searching = true;
        self.selected_app = None;
        spawn_search(
            self.backends.clone(),
            q,
            self.source_filter,
            self.sets.clone(),
            self.tx.clone(),
            ctx.clone(),
        );
    }

    fn trigger_action(&mut self, action: StoreAction, ctx: &egui::Context) {
        if self.task_running {
            return;
        }
        spawn_action(self.backends.clone(), action, self.tx.clone(), ctx.clone());
    }

    fn trigger_refresh(&mut self, ctx: &egui::Context) {
        self.loading_library = true;
        spawn_refresh_library(self.backends.clone(), self.tx.clone(), ctx.clone());
    }

    /// Authentic Windows 10 Store horizontal top navigation bar (tabs + sharp search box).
    fn render_win10_top_bar(&mut self, ui: &mut egui::Ui) {
        let win_w = ui.available_width();
        egui::Panel::top("win10_store_header")
            .frame(
                egui::Frame::new()
                    .fill(WIN10_HEADER_BG)
                    .inner_margin(Margin::symmetric(14, 0))
                    .stroke(Stroke::new(1.0, WIN10_BORDER)),
            )
            .show(ui, |ui| {
                ui.set_min_height(48.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;

                    if self.selected_app.is_some() || self.showing_search {
                        let (back_r, back_resp) =
                            ui.allocate_exact_size(Vec2::new(34.0, 46.0), egui::Sense::click());
                        if back_resp.hovered() {
                            ui.painter().rect_filled(
                                back_r,
                                0.0,
                                Color32::from_rgb(0xe2, 0xe2, 0xe2),
                            );
                        }
                        ui.painter().text(
                            back_r.center(),
                            egui::Align2::CENTER_CENTER,
                            "←",
                            FontId::proportional(17.0),
                            WIN10_TEXT,
                        );
                        if back_resp.clicked() {
                            if self.selected_app.is_some() {
                                self.selected_app = None;
                            } else {
                                self.showing_search = false;
                                self.search_query.clear();
                            }
                        }
                    }

                    // Shopping bag brand
                    let (bag_rect, _) =
                        ui.allocate_exact_size(Vec2::new(22.0, 22.0), egui::Sense::hover());
                    draw_store_bag(ui.painter(), bag_rect);
                    if win_w > 780.0 {
                        ui.label(
                            RichText::new("Window Shopping")
                                .font(FontId::proportional(13.5))
                                .color(WIN10_TEXT),
                        );
                        ui.add_space(10.0);
                    }

                    // Win10 horizontal underline tabs
                    let tabs = [
                        (NavTab::Home, "Home", 0usize),
                        (NavTab::Apps, "Apps", 0usize),
                        (NavTab::Gaming, "Gaming", 0usize),
                        (NavTab::Library, "Library", 0usize),
                        (NavTab::Updates, "Updates", self.updates.len()),
                    ];
                    for (tab, label, badge) in tabs {
                        let selected = !self.showing_search
                            && self.selected_app.is_none()
                            && self.tab == tab;
                        let text_w = if badge > 0 { 82.0 } else { 58.0 };
                        let (r, resp) = ui.allocate_exact_size(
                            Vec2::new(text_w, 46.0),
                            egui::Sense::click(),
                        );
                        if resp.hovered() && !selected {
                            ui.painter().rect_filled(
                                r,
                                0.0,
                                Color32::from_rgb(0xe7, 0xe7, 0xe7),
                            );
                        }
                        let fg = if selected { WIN10_TEXT } else { WIN10_MUTED };
                        let font = if selected {
                            FontId::proportional(14.0)
                        } else {
                            FontId::proportional(13.5)
                        };
                        let label_str = if badge > 0 {
                            format!("{label} ({badge})")
                        } else {
                            label.to_string()
                        };
                        ui.painter().text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            label_str,
                            font,
                            fg,
                        );
                        if selected {
                            let underline = egui::Rect::from_min_max(
                                egui::pos2(r.left() + 6.0, r.bottom() - 3.0),
                                egui::pos2(r.right() - 6.0, r.bottom()),
                            );
                            ui.painter().rect_filled(underline, 0.0, WIN10_BLUE);
                        }
                        if resp.clicked() {
                            self.tab = tab;
                            self.showing_search = false;
                            self.selected_app = None;
                        }
                    }

                    // Right side: Sharp Win10 Search box + refresh + activity
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(egui::Button::new("⋯").frame(false))
                            .on_hover_text("Activity log")
                            .clicked()
                        {
                            self.show_activity_log = !self.show_activity_log;
                        }
                        if self.loading_library {
                            let (r, _) =
                                ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
                            draw_fluent_spinner(ui, r.center(), 7.0, 1.8);
                        } else if ui
                            .add(egui::Button::new("↻").frame(false))
                            .on_hover_text("Check for updates & refresh")
                            .clicked()
                        {
                            self.trigger_refresh(ui.ctx());
                        }

                        // Sharp rectangular Win10 Search Box
                        let search_w = (win_w * 0.28).clamp(130.0, 260.0);
                        let mut enter_pressed = false;
                        egui::Frame::new()
                            .fill(Color32::WHITE)
                            .stroke(Stroke::new(1.0, Color32::from_rgb(0x99, 0x99, 0x99)))
                            .corner_radius(CornerRadius::ZERO)
                            .inner_margin(Margin::symmetric(8, 4))
                            .show(ui, |ui| {
                                ui.set_width(search_w);
                                ui.horizontal(|ui| {
                                    let resp = ui.add(
                                        egui::TextEdit::singleline(&mut self.search_query)
                                            .desired_width((search_w - 42.0).max(60.0))
                                            .frame(egui::Frame::NONE)
                                            .hint_text("Search"),
                                    );
                                    if resp.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                    {
                                        enter_pressed = true;
                                    }
                                    if !self.search_query.is_empty()
                                        && ui.add(egui::Button::new("✕").frame(false)).clicked()
                                    {
                                        self.search_query.clear();
                                        self.showing_search = false;
                                        self.search_results.clear();
                                    }
                                    if ui.add(egui::Button::new("🔍").frame(false)).clicked() {
                                        enter_pressed = true;
                                    }
                                });
                            });
                        if enter_pressed {
                            self.trigger_search(ui.ctx());
                        }
                    });
                });
            });
    }

    fn render_activity_drawer(&mut self, ui: &mut egui::Ui) {
        if !self.show_activity_log && !self.task_running && self.task_status.is_none() {
            return;
        }
        egui::Panel::bottom("win10_activity_bar")
            .frame(
                egui::Frame::new()
                    .fill(WIN10_HEADER_BG)
                    .inner_margin(Margin::symmetric(14, 8))
                    .stroke(Stroke::new(1.0, WIN10_BORDER)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.task_running {
                        let (r, _) =
                            ui.allocate_exact_size(Vec2::splat(18.0), egui::Sense::hover());
                        draw_fluent_spinner(ui, r.center(), 6.5, 1.8);
                        ui.label(
                            RichText::new(&self.task_title)
                                .font(FontId::proportional(12.5))
                                .strong()
                                .color(WIN10_BLUE),
                        );
                    } else if let Some((ok, msg)) = &self.task_status {
                        let color = if *ok { WIN10_GREEN } else { WIN10_RED };
                        ui.label(
                            RichText::new(msg)
                                .font(FontId::proportional(12.5))
                                .strong()
                                .color(color),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !self.task_running && ui.small_button("Dismiss").clicked() {
                            self.task_status = None;
                            self.show_activity_log = false;
                        }
                        if ui
                            .small_button(if self.show_activity_log {
                                "Hide log"
                            } else {
                                "Show log"
                            })
                            .clicked()
                        {
                            self.show_activity_log = !self.show_activity_log;
                        }
                    });
                });

                if self.show_activity_log && !self.task_logs.is_empty() {
                    ui.add_space(6.0);
                    egui::Frame::new()
                        .fill(Color32::from_rgb(0x14, 0x14, 0x14))
                        .corner_radius(CornerRadius::ZERO)
                        .inner_margin(Margin::same(8))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(120.0)
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    for line in &self.task_logs {
                                        ui.label(
                                            RichText::new(line)
                                                .font(FontId::monospace(11.0))
                                                .color(Color32::from_rgb(0xe0, 0xe0, 0xe0)),
                                        );
                                    }
                                });
                        });
                }
            });
    }

    fn render_main(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(WIN10_CANVAS_BG)
                    .inner_margin(Margin::symmetric(24, 18)),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if let Some(app) = self.selected_app.clone() {
                            self.render_detail_view(ui, &app);
                        } else if self.showing_search {
                            self.render_search_view(ui);
                        } else {
                            match self.tab {
                                NavTab::Home => self.render_home_view(ui),
                                NavTab::Apps => {
                                    self.render_catalog_view(ui, "Top free apps", Some("Apps"))
                                }
                                NavTab::Gaming => {
                                    self.render_catalog_view(ui, "PC gaming", Some("Gaming"))
                                }
                                NavTab::Library => self.render_library_view(ui),
                                NavTab::Updates => self.render_updates_view(ui),
                            }
                        }
                    });
            });
    }

    /// Windows 10 Store Home: Mosaic Hero Spotlight + Top Free Apps vertical product tiles.
    fn render_home_view(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_width().max(200.0);
        let catalog = featured_catalog(&self.backends, &self.sets);

        // Windows 10 Store Spotlight Mosaic
        if avail > 620.0 {
            let gap = 8.0;
            let left_w = ((avail - gap) * 0.62).floor();
            let right_w = (avail - gap - left_w).max(200.0);
            let mosaic_h = 220.0;
            let tile_h = (mosaic_h - gap) * 0.5;

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;

                // Big featured hero tile
                let hero_item = catalog.first().cloned();
                ui.allocate_ui_with_layout(
                    egui::vec2(left_w, mosaic_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::Frame::new()
                            .fill(WIN10_BLUE_DARK)
                            .corner_radius(CornerRadius::same(2))
                            .inner_margin(Margin::same(22))
                            .show(ui, |ui| {
                                ui.set_width((left_w - 44.0).max(200.0));
                                ui.set_height((mosaic_h - 44.0).max(120.0));
                                ui.horizontal(|ui| {
                                    let text_w = (left_w - 156.0).max(160.0);
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(text_w, mosaic_h - 44.0),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.label(
                                                RichText::new("FEATURED ON WINDOW SHOPPING")
                                                    .font(FontId::proportional(10.5))
                                                    .strong()
                                                    .color(Color32::from_rgb(0xb4, 0xd9, 0xff)),
                                            );
                                            ui.add_space(6.0);
                                            ui.label(
                                                RichText::new("Get essential apps & games")
                                                    .font(FontId::proportional(22.0))
                                                    .strong()
                                                    .color(Color32::WHITE),
                                            );
                                            ui.add_space(6.0);
                                            ui.label(
                                                RichText::new(format!(
                                                    "Powered by {} — search, install, and update in one place.",
                                                    self.backends.summary()
                                                ))
                                                .font(FontId::proportional(12.5))
                                                .color(Color32::from_rgb(0xe3, 0xf2, 0xfd)),
                                            );
                                            ui.add_space(14.0);
                                            ui.horizontal(|ui| {
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new(if self.updates.is_empty() {
                                                                "Explore Apps"
                                                            } else {
                                                                "Get Updates"
                                                            })
                                                            .color(Color32::WHITE)
                                                            .strong(),
                                                        )
                                                        .fill(WIN10_BLUE)
                                                        .corner_radius(CornerRadius::same(2)),
                                                    )
                                                    .clicked()
                                                {
                                                    self.tab = if self.updates.is_empty() {
                                                        NavTab::Apps
                                                    } else {
                                                        NavTab::Updates
                                                    };
                                                }
                                                if !self.updates.is_empty() {
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "{} updates ready",
                                                            self.updates.len()
                                                        ))
                                                        .font(FontId::proportional(12.0))
                                                        .color(Color32::from_rgb(0xc8, 0xe4, 0xff)),
                                                    );
                                                }
                                            });
                                        },
                                    );
                                    if let Some(h) = &hero_item {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                self.draw_app_icon(ui, h, 80.0);
                                            },
                                        );
                                    }
                                });
                            });
                    },
                );

                // Right column: 2 stacked Win10 promo tiles (Gaming + Library)
                ui.allocate_ui_with_layout(
                    egui::vec2(right_w, mosaic_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.spacing_mut().item_spacing.y = gap;
                        self.render_promo_tile(
                            ui,
                            right_w,
                            tile_h,
                            Color32::from_rgb(0x1b, 0x28, 0x38),
                            "PC Gaming",
                            "Steam, Prism Launcher & Lutris",
                            catalog.iter().find(|a| a.category == "Gaming"),
                            NavTab::Gaming,
                        );
                        self.render_promo_tile(
                            ui,
                            right_w,
                            tile_h,
                            Color32::from_rgb(0x5c, 0x2d, 0x91),
                            "Your Library",
                            &format!("{} installed apps & packages", self.installed.len()),
                            catalog.get(1),
                            NavTab::Library,
                        );
                    },
                );
            });
        } else {
            // Compact hero for narrow windows
            egui::Frame::new()
                .fill(WIN10_BLUE_DARK)
                .corner_radius(CornerRadius::same(2))
                .inner_margin(Margin::same(16))
                .show(ui, |ui| {
                    ui.set_width((avail - 32.0).max(160.0));
                    ui.label(
                        RichText::new("Window Shopping")
                            .font(FontId::proportional(20.0))
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.label(
                        RichText::new(self.backends.summary())
                            .font(FontId::proportional(12.0))
                            .color(Color32::from_rgb(0xd8, 0xe8, 0xf8)),
                    );
                });
        }

        ui.add_space(22.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Top free apps")
                    .font(FontId::proportional(18.0))
                    .strong()
                    .color(WIN10_TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(RichText::new("Show all").color(WIN10_BLUE))
                            .frame(false),
                    )
                    .clicked()
                {
                    self.tab = NavTab::Apps;
                }
            });
        });
        ui.add_space(8.0);

        self.render_win10_tile_grid(ui, &catalog);
    }

    #[allow(clippy::too_many_arguments)]
    fn render_promo_tile(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        height: f32,
        bg: Color32,
        title: &str,
        subtitle: &str,
        icon_item: Option<&AppItem>,
        target_tab: NavTab,
    ) {
        let resp = ui
            .allocate_ui_with_layout(
                egui::vec2(width, height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::Frame::new()
                        .fill(bg)
                        .corner_radius(CornerRadius::same(2))
                        .inner_margin(Margin::same(14))
                        .show(ui, |ui| {
                            ui.set_width((width - 28.0).max(120.0));
                            ui.set_height((height - 28.0).max(60.0));
                            ui.horizontal(|ui| {
                                let text_w = (width - 96.0).max(100.0);
                                ui.allocate_ui_with_layout(
                                    egui::vec2(text_w, height - 28.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.label(
                                            RichText::new(title)
                                                .font(FontId::proportional(15.5))
                                                .strong()
                                                .color(Color32::WHITE),
                                        );
                                        ui.add_space(3.0);
                                        ui.label(
                                            RichText::new(subtitle)
                                                .font(FontId::proportional(12.0))
                                                .color(Color32::from_rgb(0xe0, 0xe0, 0xe0)),
                                        );
                                    },
                                );
                                if let Some(item) = icon_item {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            self.draw_app_icon(ui, item, 46.0);
                                        },
                                    );
                                }
                            });
                        });
                },
            )
            .response
            .interact(egui::Sense::click());
        if resp.clicked() {
            self.tab = target_tab;
        }
    }

    fn render_catalog_view(&mut self, ui: &mut egui::Ui, title: &str, category: Option<&str>) {
        ui.label(
            RichText::new(title)
                .font(FontId::proportional(24.0))
                .strong()
                .color(WIN10_TEXT),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Select an app to install, launch, or view details.")
                .font(FontId::proportional(12.5))
                .color(WIN10_MUTED),
        );
        ui.add_space(14.0);

        let catalog: Vec<AppItem> = featured_catalog(&self.backends, &self.sets)
            .into_iter()
            .filter(|a| category.is_none_or(|c| a.category == c))
            .collect();
        self.render_win10_tile_grid(ui, &catalog);
    }

    fn render_search_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("Search results for \"{}\"", self.search_query.trim()))
                    .font(FontId::proportional(22.0))
                    .strong()
                    .color(WIN10_TEXT),
            );
            ui.add_space(14.0);
            let mut changed = false;
            for (filter, label) in [
                (SourceFilter::All, "All"),
                (SourceFilter::Flatpak, "Flatpak"),
                (SourceFilter::System, self.backends.native_label()),
            ] {
                if ui
                    .selectable_label(self.source_filter == filter, label)
                    .clicked()
                {
                    self.source_filter = filter;
                    changed = true;
                }
            }
            if changed {
                self.trigger_search(ui.ctx());
            }
        });
        ui.add_space(14.0);

        if self.searching {
            draw_centered_loading(ui, "Searching Flathub and system repositories...");
            return;
        }

        if self.search_results.is_empty() {
            ui.label(
                RichText::new("No matching apps or packages found.")
                    .font(FontId::proportional(14.0))
                    .color(WIN10_MUTED),
            );
            return;
        }

        let results = self.search_results.clone();
        self.render_win10_tile_grid(ui, &results);
    }

    fn render_library_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("My Library")
                    .font(FontId::proportional(24.0))
                    .strong()
                    .color(WIN10_TEXT),
            );
            ui.add_space(12.0);
            if ui
                .selectable_label(!self.library_only_flatpak, "All installed")
                .clicked()
            {
                self.library_only_flatpak = false;
            }
            if ui
                .selectable_label(self.library_only_flatpak, "Flatpak")
                .clicked()
            {
                self.library_only_flatpak = true;
            }
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.library_filter)
                    .desired_width(180.0)
                    .hint_text("Filter installed..."),
            );
            if ui.button("Refresh").clicked() {
                self.trigger_refresh(ui.ctx());
            }
        });
        ui.add_space(10.0);

        if self.loading_library && self.installed.is_empty() {
            draw_centered_loading(ui, "Loading installed apps and packages...");
            return;
        }

        let needle = self.library_filter.trim().to_lowercase();
        let filtered: Vec<AppItem> = self
            .installed
            .iter()
            .filter(|a| !self.library_only_flatpak || a.source == PackageSource::Flatpak)
            .filter(|a| {
                needle.is_empty()
                    || a.name.to_lowercase().contains(&needle)
                    || a.id.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();

        ui.label(
            RichText::new(format!("{} items", filtered.len()))
                .font(FontId::proportional(12.0))
                .color(WIN10_MUTED),
        );
        ui.add_space(8.0);

        for item in filtered {
            self.render_list_row(ui, &item);
            ui.add_space(4.0);
        }
    }

    fn render_updates_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("Downloads and updates")
                    .font(FontId::proportional(24.0))
                    .strong()
                    .color(WIN10_TEXT),
            );
            ui.add_space(16.0);
            if ui
                .add_enabled(!self.loading_library, egui::Button::new("Get updates"))
                .clicked()
            {
                self.trigger_refresh(ui.ctx());
            }
            if !self.updates.is_empty() {
                let has_flatpak = self.updates.iter().any(|u| u.source == PackageSource::Flatpak);
                let has_pacman = self.updates.iter().any(|u| u.source == PackageSource::Pacman);
                let has_aur = self.updates.iter().any(|u| u.source == PackageSource::Aur);
                let has_apt = self.updates.iter().any(|u| u.source == PackageSource::Apt);
                let has_dnf = self.updates.iter().any(|u| u.source == PackageSource::Dnf);
                if ui
                    .add_enabled(
                        !self.task_running,
                        egui::Button::new(
                            RichText::new("Update all").color(Color32::WHITE).strong(),
                        )
                        .fill(WIN10_BLUE),
                    )
                    .clicked()
                {
                    self.trigger_action(
                        StoreAction::UpdateAll {
                            has_flatpak_updates: has_flatpak,
                            has_pacman_updates: has_pacman,
                            has_aur_updates: has_aur,
                            has_apt_updates: has_apt,
                            has_dnf_updates: has_dnf,
                        },
                        ui.ctx(),
                    );
                }
            }
        });
        ui.add_space(14.0);

        if self.loading_library && self.updates.is_empty() {
            draw_centered_loading(ui, "Checking Flatpak and system repositories for updates...");
            return;
        }
        if self.loading_library {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), egui::Sense::hover());
                draw_fluent_spinner(ui, r.center(), 6.5, 1.8);
                ui.label("Checking for updates...");
            });
            ui.add_space(8.0);
        }

        if self.updates.is_empty() && !self.loading_library {
            egui::Frame::new()
                .fill(WIN10_TILE_STAGE)
                .stroke(Stroke::new(1.0, WIN10_BORDER))
                .corner_radius(CornerRadius::same(2))
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.set_width((ui.available_width() - 36.0).max(160.0));
                    ui.label(
                        RichText::new("You're good to go")
                            .font(FontId::proportional(16.0))
                            .strong()
                            .color(WIN10_GREEN),
                    );
                    ui.label(
                        RichText::new("All Flatpak apps and system packages are up to date.")
                            .font(FontId::proportional(12.5))
                            .color(WIN10_MUTED),
                    );
                });
            return;
        }

        let updates = self.updates.clone();
        for item in updates {
            self.render_list_row(ui, &item);
            ui.add_space(4.0);
        }
    }

    /// Windows 10 Store rectangular vertical product tiles (icon stage top, title/stars/Free-or-Installed bottom).
    fn render_win10_tile_grid(&mut self, ui: &mut egui::Ui, items: &[AppItem]) {
        let avail = ui.available_width().max(170.0);
        let gap = 10.0;
        let cols = (((avail + gap) / (168.0 + gap)).floor() as usize).clamp(1, 6);
        let tile_outer_w = ((avail - (cols as f32 - 1.0) * gap) / cols as f32).floor();
        let tile_h = 224.0;

        for row in items.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for item in row {
                    self.render_win10_product_tile(ui, item, tile_outer_w, tile_h);
                }
            });
            ui.add_space(gap);
        }
    }

    fn render_win10_product_tile(
        &mut self,
        ui: &mut egui::Ui,
        item: &AppItem,
        outer_width: f32,
        outer_height: f32,
    ) {
        let card_id = ui.make_persistent_id(("win10_tile", item.source, &item.id));
        let hovered = ui
            .ctx()
            .memory(|m| m.data.get_temp::<bool>(card_id))
            .unwrap_or(false);
        let fill = if hovered {
            WIN10_TILE_HOVER
        } else {
            WIN10_TILE_BG
        };
        let stroke = if hovered {
            Stroke::new(1.0, WIN10_BLUE)
        } else {
            Stroke::new(1.0, WIN10_BORDER)
        };

        let inner_w = (outer_width - 2.0).max(120.0);
        let stage_h = 116.0;
        let meta_h = (outer_height - stage_h - 2.0).max(96.0);
        let meta_inner_w = (inner_w - 20.0).max(96.0);

        let resp = ui
            .allocate_ui_with_layout(
                egui::vec2(outer_width, outer_height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    egui::Frame::new()
                        .fill(fill)
                        .stroke(stroke)
                        .corner_radius(CornerRadius::same(2))
                        .inner_margin(Margin::ZERO)
                        .show(ui, |ui| {
                            ui.set_width(inner_w);
                            ui.set_height(outer_height - 2.0);
                            ui.spacing_mut().item_spacing.y = 0.0;

                            // Top icon stage (fixed-height Windows 10 Store art box)
                            egui::Frame::new()
                                .fill(WIN10_TILE_STAGE)
                                .inner_margin(Margin::ZERO)
                                .show(ui, |ui| {
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(inner_w, stage_h),
                                        egui::Layout::centered_and_justified(
                                            egui::Direction::TopDown,
                                        ),
                                        |ui| {
                                            self.draw_app_icon(ui, item, 60.0);
                                        },
                                    );
                                });

                            // Bottom metadata block (vertical, fixed height)
                            egui::Frame::new()
                                .inner_margin(Margin::same(10))
                                .show(ui, |ui| {
                                    ui.allocate_ui_with_layout(
                                        egui::vec2(meta_inner_w, (meta_h - 20.0).max(76.0)),
                                        egui::Layout::top_down(egui::Align::Min),
                                        |ui| {
                                            ui.spacing_mut().item_spacing.y = 3.0;
                                            ui.label(
                                                RichText::new(truncate(&item.name, 18))
                                                    .font(FontId::proportional(13.5))
                                                    .strong()
                                                    .color(WIN10_TEXT),
                                            );
                                            ui.label(
                                                RichText::new(format!("★★★★☆  {}", item.rating))
                                                    .font(FontId::proportional(11.0))
                                                    .color(WIN10_GOLD),
                                            );
                                            ui.add_space(16.0);
                                            ui.allocate_ui_with_layout(
                                                egui::vec2(meta_inner_w, 18.0),
                                                egui::Layout::left_to_right(egui::Align::Center),
                                                |ui| {
                                                    if item.has_update() {
                                                        ui.label(
                                                            RichText::new("Update")
                                                                .font(FontId::proportional(12.0))
                                                                .strong()
                                                                .color(WIN10_BLUE),
                                                        );
                                                    } else if item.installed {
                                                        ui.label(
                                                            RichText::new("Installed")
                                                                .font(FontId::proportional(12.0))
                                                                .strong()
                                                                .color(WIN10_GREEN),
                                                        );
                                                    } else {
                                                        ui.label(
                                                            RichText::new("Free")
                                                                .font(FontId::proportional(12.0))
                                                                .color(WIN10_TEXT),
                                                        );
                                                    }
                                                    ui.with_layout(
                                                        egui::Layout::right_to_left(
                                                            egui::Align::Center,
                                                        ),
                                                        |ui| {
                                                            ui.label(
                                                                RichText::new(item.source.label())
                                                                    .font(FontId::proportional(10.5))
                                                                    .color(WIN10_MUTED),
                                                            );
                                                        },
                                                    );
                                                },
                                            );
                                        },
                                    );
                                });
                        });
                },
            )
            .response
            .interact(egui::Sense::click());

        ui.ctx()
            .memory_mut(|m| m.data.insert_temp(card_id, resp.hovered()));
        if resp.clicked() {
            self.selected_app = Some(item.clone());
        }
    }

    fn render_list_row(&mut self, ui: &mut egui::Ui, item: &AppItem) {
        let mut action_clicked = false;
        let resp = egui::Frame::new()
            .fill(WIN10_TILE_BG)
            .stroke(Stroke::new(1.0, WIN10_BORDER))
            .corner_radius(CornerRadius::same(2))
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width((ui.available_width() - 28.0).max(180.0));
                ui.horizontal(|ui| {
                    self.draw_app_icon(ui, item, 40.0);
                    ui.add_space(10.0);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&item.name)
                                    .font(FontId::proportional(14.0))
                                    .strong()
                                    .color(WIN10_TEXT),
                            );
                            draw_source_pill(ui, item.source);
                        });
                        let ver_text = if let Some(new_v) = &item.new_version {
                            if item.version.is_empty() {
                                format!("{} • -> {new_v}", item.id)
                            } else {
                                format!("{} • {} -> {new_v}", item.id, item.version)
                            }
                        } else {
                            format!("{} • {}", item.id, item.version)
                        };
                        ui.label(
                            RichText::new(truncate(&ver_text, 52))
                                .font(FontId::proportional(11.5))
                                .color(WIN10_MUTED),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if item.has_update() {
                            if ui
                                .add_enabled(
                                    !self.task_running,
                                    egui::Button::new(
                                        RichText::new("Update").color(Color32::WHITE),
                                    )
                                    .fill(WIN10_BLUE),
                                )
                                .clicked()
                            {
                                action_clicked = true;
                                self.trigger_action(StoreAction::UpdateOne(item.clone()), ui.ctx());
                            }
                        } else if item.installed {
                            if ui
                                .add_enabled(!self.task_running, egui::Button::new("Uninstall"))
                                .clicked()
                            {
                                action_clicked = true;
                                self.trigger_action(StoreAction::Uninstall(item.clone()), ui.ctx());
                            }
                            if (item.source == PackageSource::Flatpak
                                || self.sets.exec_by_id.contains_key(&item.id))
                                && ui.button("Launch").clicked()
                            {
                                action_clicked = true;
                                launch_app(item, &self.sets);
                            }
                        }
                    });
                });
            })
            .response
            .interact(egui::Sense::click());
        if resp.clicked() && !action_clicked {
            self.selected_app = Some(item.clone());
        }
    }

    fn render_detail_view(&mut self, ui: &mut egui::Ui, item: &AppItem) {
        egui::Frame::new()
            .fill(WIN10_TILE_STAGE)
            .stroke(Stroke::new(1.0, WIN10_BORDER))
            .corner_radius(CornerRadius::same(2))
            .inner_margin(Margin::same(24))
            .show(ui, |ui| {
                ui.set_width((ui.available_width() - 48.0).max(200.0));
                ui.horizontal_wrapped(|ui| {
                    egui::Frame::new()
                        .fill(Color32::WHITE)
                        .stroke(Stroke::new(1.0, WIN10_BORDER))
                        .inner_margin(Margin::same(10))
                        .show(ui, |ui| {
                            self.draw_app_icon(ui, item, 88.0);
                        });
                    ui.add_space(18.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&item.name)
                                .font(FontId::proportional(26.0))
                                .strong()
                                .color(WIN10_TEXT),
                        );
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(&item.publisher)
                                    .font(FontId::proportional(13.0))
                                    .color(WIN10_BLUE),
                            );
                            ui.label(RichText::new("•").color(WIN10_MUTED));
                            draw_source_pill(ui, item.source);
                            ui.label(
                                RichText::new(format!("• ★★★★☆ {}", item.rating))
                                    .font(FontId::proportional(12.5))
                                    .color(WIN10_GOLD),
                            );
                        });
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if item.installed {
                                if item.has_update() {
                                    if ui
                                        .add_enabled(
                                            !self.task_running,
                                            egui::Button::new(
                                                RichText::new("Update")
                                                    .color(Color32::WHITE)
                                                    .strong(),
                                            )
                                            .min_size(Vec2::new(120.0, 34.0))
                                            .fill(WIN10_BLUE),
                                        )
                                        .clicked()
                                    {
                                        self.trigger_action(
                                            StoreAction::UpdateOne(item.clone()),
                                            ui.ctx(),
                                        );
                                    }
                                } else if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("Launch")
                                                .color(Color32::WHITE)
                                                .strong(),
                                        )
                                        .min_size(Vec2::new(120.0, 34.0))
                                        .fill(WIN10_BLUE),
                                    )
                                    .clicked()
                                {
                                    launch_app(item, &self.sets);
                                }
                                if ui
                                    .add_enabled(
                                        !self.task_running,
                                        egui::Button::new("Uninstall")
                                            .min_size(Vec2::new(100.0, 34.0)),
                                    )
                                    .clicked()
                                {
                                    self.trigger_action(
                                        StoreAction::Uninstall(item.clone()),
                                        ui.ctx(),
                                    );
                                }
                            } else if ui
                                .add_enabled(
                                    !self.task_running,
                                    egui::Button::new(
                                        RichText::new("Get")
                                            .color(Color32::WHITE)
                                            .strong(),
                                    )
                                    .min_size(Vec2::new(130.0, 34.0))
                                    .fill(WIN10_BLUE),
                                )
                                .clicked()
                            {
                                self.trigger_action(StoreAction::Install(item.clone()), ui.ctx());
                            }
                        });
                    });
                });
            });

        ui.add_space(20.0);
        ui.label(
            RichText::new("Overview")
                .font(FontId::proportional(18.0))
                .strong()
                .color(WIN10_TEXT),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(if item.summary.is_empty() {
                "No description provided."
            } else {
                item.summary.as_str()
            })
            .font(FontId::proportional(13.5))
            .color(WIN10_TEXT),
        );

        ui.add_space(20.0);
        ui.label(
            RichText::new("Additional information")
                .font(FontId::proportional(18.0))
                .strong()
                .color(WIN10_TEXT),
        );
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(WIN10_TILE_STAGE)
            .stroke(Stroke::new(1.0, WIN10_BORDER))
            .corner_radius(CornerRadius::same(2))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width((ui.available_width() - 28.0).max(180.0));
                egui::Grid::new("win10_meta_grid")
                    .num_columns(2)
                    .spacing([28.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Package ID").color(WIN10_MUTED));
                        ui.label(RichText::new(&item.id).color(WIN10_TEXT));
                        ui.end_row();
                        ui.label(RichText::new("Source").color(WIN10_MUTED));
                        ui.label(
                            RichText::new(format!("{} ({})", item.source.label(), item.origin))
                                .color(WIN10_TEXT),
                        );
                        ui.end_row();
                        ui.label(RichText::new("Status").color(WIN10_MUTED));
                        ui.label(
                            RichText::new(if item.installed {
                                "Installed"
                            } else {
                                "Not installed"
                            })
                            .color(if item.installed {
                                WIN10_GREEN
                            } else {
                                WIN10_TEXT
                            }),
                        );
                        ui.end_row();
                        ui.label(RichText::new("Version").color(WIN10_MUTED));
                        ui.label(
                            RichText::new(if item.version.is_empty() {
                                "Latest"
                            } else {
                                &item.version
                            })
                            .color(WIN10_TEXT),
                        );
                        ui.end_row();
                        if let Some(new_v) = &item.new_version {
                            ui.label(RichText::new("Available update").color(WIN10_MUTED));
                            ui.label(RichText::new(new_v).color(WIN10_BLUE));
                            ui.end_row();
                        }
                    });
            });
    }

    fn draw_app_icon(&mut self, ui: &mut egui::Ui, item: &AppItem, size: f32) {
        match self.icons.get(ui.ctx(), &item.icon_name, &item.id) {
            IconLookup::Ready(tex) => {
                ui.add(egui::Image::new(tex).fit_to_exact_size(Vec2::splat(size)));
                return;
            }
            IconLookup::Loading => {
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(2),
                    Color32::from_rgb(0xea, 0xee, 0xf3),
                );
                draw_fluent_spinner(ui, rect.center(), size * 0.24, (size * 0.055).max(1.8));
                return;
            }
            IconLookup::Missing => {}
        }
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
        let bg = Color32::from_rgb(item.accent_rgb[0], item.accent_rgb[1], item.accent_rgb[2]);
        ui.painter().rect_filled(rect, CornerRadius::same(2), bg);
        let initials: String = item
            .name
            .split_whitespace()
            .take(2)
            .filter_map(|w| w.chars().next())
            .collect::<String>()
            .to_uppercase();
        let label = if initials.is_empty() {
            "A".to_string()
        } else {
            initials
        };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            FontId::proportional(size * 0.36),
            Color32::WHITE,
        );
    }
}

impl eframe::App for StoreApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.apply_win10_style(ui.ctx());
        self.poll_events(ui.ctx());
        self.render_win10_top_bar(ui);
        self.render_activity_drawer(ui);
        self.render_main(ui);
    }
}

fn draw_source_pill(ui: &mut egui::Ui, source: PackageSource) {
    let rgb = source.badge_rgb();
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], 24))
        .corner_radius(CornerRadius::same(2))
        .inner_margin(Margin::symmetric(5, 1))
        .show(ui, |ui| {
            ui.label(
                RichText::new(source.label())
                    .font(FontId::proportional(10.5))
                    .color(Color32::from_rgb(rgb[0], rgb[1], rgb[2])),
            );
        });
}

fn draw_store_bag(painter: &egui::Painter, rect: egui::Rect) {
    let handle = egui::Rect::from_min_max(
        egui::pos2(rect.center().x - 4.0, rect.top() + 1.0),
        egui::pos2(rect.center().x + 4.0, rect.top() + 7.0),
    );
    painter.rect_stroke(
        handle,
        CornerRadius::same(2),
        Stroke::new(1.4, Color32::from_rgb(0x66, 0x66, 0x66)),
        egui::StrokeKind::Inside,
    );
    let body = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 2.0, rect.top() + 5.0),
        egui::pos2(rect.right() - 2.0, rect.bottom() - 1.0),
    );
    painter.rect_filled(body, CornerRadius::same(1), Color32::WHITE);
    painter.rect_stroke(
        body,
        CornerRadius::same(1),
        Stroke::new(1.2, Color32::from_rgb(0x88, 0x88, 0x88)),
        egui::StrokeKind::Inside,
    );
    let cx = body.center().x;
    let cy = body.center().y;
    let s = 3.5;
    let gap = 1.1;
    let tiles = [
        (
            egui::pos2(cx - s - gap * 0.5, cy - s - gap * 0.5),
            Color32::from_rgb(0xf2, 0x50, 0x22),
        ),
        (
            egui::pos2(cx + gap * 0.5, cy - s - gap * 0.5),
            Color32::from_rgb(0x7f, 0xba, 0x00),
        ),
        (
            egui::pos2(cx - s - gap * 0.5, cy + gap * 0.5),
            Color32::from_rgb(0x00, 0xa4, 0xef),
        ),
        (
            egui::pos2(cx + gap * 0.5, cy + gap * 0.5),
            Color32::from_rgb(0xff, 0xb9, 0x00),
        ),
    ];
    for (pos, col) in tiles {
        painter.rect_filled(egui::Rect::from_min_size(pos, Vec2::splat(s)), 0.0, col);
    }
}

fn draw_fluent_spinner(ui: &egui::Ui, center: egui::Pos2, radius: f32, stroke_w: f32) {
    let t = ui.ctx().input(|i| i.time) as f32;
    let start = t * 4.4;
    let sweep = 1.6 + 0.9 * (t * 2.3).sin();
    let segments = 20;
    let mut pts = Vec::with_capacity(segments + 1);
    for i in 0..=segments {
        let a = start + sweep * (i as f32 / segments as f32);
        pts.push(egui::pos2(
            center.x + radius * a.cos(),
            center.y + radius * a.sin(),
        ));
    }
    ui.painter().circle_stroke(
        center,
        radius,
        Stroke::new(stroke_w, Color32::from_rgba_unmultiplied(0, 120, 215, 36)),
    );
    ui.painter()
        .add(egui::Shape::line(pts, Stroke::new(stroke_w, WIN10_BLUE)));
    ui.ctx().request_repaint();
}

fn draw_centered_loading(ui: &mut egui::Ui, label: &str) {
    ui.add_space(48.0);
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), egui::Sense::hover());
        draw_fluent_spinner(ui, rect.center(), 16.0, 3.0);
        ui.add_space(12.0);
        ui.label(
            RichText::new(label)
                .font(FontId::proportional(13.5))
                .color(WIN10_MUTED),
        );
    });
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Window Shopping")
            .with_app_id("window-shopping")
            .with_inner_size([1020.0, 680.0])
            .with_min_inner_size([480.0, 380.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Window Shopping",
        options,
        Box::new(|cc| Ok(Box::new(StoreApp::new(cc)))),
    )
}
