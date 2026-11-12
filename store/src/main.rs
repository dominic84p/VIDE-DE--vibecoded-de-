mod backend;
mod icons;

use std::sync::mpsc::{self, Receiver, Sender};

use backend::{
    AppItem, BackendEvent, DetectedBackends, InstalledSets, PackageSource, SourceFilter,
    StoreAction, featured_catalog, launch_app, spawn_action, spawn_refresh_library, spawn_search,
};
use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, RichText, Stroke, Vec2};
use icons::{IconCache, IconLookup};

const BG_MICA: Color32 = Color32::from_rgb(0xf3, 0xf3, 0xf3);
const BG_SURFACE: Color32 = Color32::from_rgb(0xfb, 0xfb, 0xfb);
const BG_CARD: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
const BG_CARD_HOVER: Color32 = Color32::from_rgb(0xf5, 0xf8, 0xfc);
const BORDER: Color32 = Color32::from_rgb(0xe5, 0xe5, 0xe5);
const BORDER_HOVER: Color32 = Color32::from_rgb(0xb8, 0xce, 0xe6);
const ACCENT: Color32 = Color32::from_rgb(0x00, 0x67, 0xc0);
const ACCENT_GREEN: Color32 = Color32::from_rgb(0x0f, 0x7b, 0x0f);
const ACCENT_RED: Color32 = Color32::from_rgb(0xc4, 0x2b, 0x1c);
const TEXT: Color32 = Color32::from_rgb(0x1b, 0x1b, 0x1b);
const TEXT_MUTED: Color32 = Color32::from_rgb(0x61, 0x61, 0x61);

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

    fn apply_store_style(&mut self, ctx: &egui::Context) {
        if self.style_applied {
            return;
        }
        self.style_applied = true;
        let mut style = (*ctx.global_style()).clone();
        style.visuals = egui::Visuals::light();
        style.visuals.panel_fill = BG_MICA;
        style.visuals.window_fill = BG_SURFACE;
        style.visuals.extreme_bg_color = BG_CARD;
        style.visuals.faint_bg_color = BG_CARD;
        style.visuals.selection.bg_fill = ACCENT;
        style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(0xec, 0xec, 0xec);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xf2, 0xf2, 0xf2);
        style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(0xe1, 0xe8, 0xf0);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0xe8, 0xee, 0xf5);
        style.visuals.widgets.active.bg_fill = ACCENT;
        style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 5.0);
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

    fn render_top_bar(&mut self, ui: &mut egui::Ui) {
        let win_w = ui.available_width();
        egui::Panel::top("store_top_bar")
            .frame(
                egui::Frame::new()
                    .fill(BG_MICA)
                    .inner_margin(Margin::symmetric(14, 8)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.selected_app.is_some() || self.showing_search {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("←").size(15.0))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            if self.selected_app.is_some() {
                                self.selected_app = None;
                            } else {
                                self.showing_search = false;
                                self.search_query.clear();
                            }
                        }
                    }

                    let (bag_rect, _) =
                        ui.allocate_exact_size(Vec2::new(22.0, 22.0), egui::Sense::hover());
                    draw_store_bag(ui.painter(), bag_rect);

                    if win_w > 560.0 {
                        ui.label(
                            RichText::new("Window Shopping")
                                .font(FontId::proportional(13.5))
                                .strong()
                                .color(TEXT),
                        );
                    }

                    ui.add_space(12.0);

                    // Responsive centered-feeling pill search box
                    let search_w = (win_w * 0.44).clamp(160.0, 440.0);
                    let mut enter_pressed = false;
                    egui::Frame::new()
                        .fill(BG_CARD)
                        .stroke(Stroke::new(1.0, Color32::from_rgb(0xd4, 0xd4, 0xd4)))
                        .corner_radius(CornerRadius::same(16))
                        .inner_margin(Margin::symmetric(12, 5))
                        .show(ui, |ui| {
                            ui.set_width(search_w);
                            ui.horizontal(|ui| {
                                let resp = ui.add(
                                    egui::TextEdit::singleline(&mut self.search_query)
                                        .desired_width((search_w - 52.0).max(80.0))
                                        .frame(egui::Frame::NONE)
                                        .hint_text("Search apps, games, and packages"),
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

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                !self.loading_library,
                                egui::Button::new("↻").frame(false),
                            )
                            .on_hover_text("Refresh installed apps & updates")
                            .clicked()
                        {
                            self.trigger_refresh(ui.ctx());
                        }
                        if win_w > 760.0 {
                            ui.label(
                                RichText::new(self.backends.summary())
                                    .font(FontId::proportional(11.0))
                                    .color(TEXT_MUTED),
                            );
                        }
                    });
                });
            });
    }

    fn render_nav_rail(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("store_nav_rail")
            .exact_size(74.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_MICA)
                    .inner_margin(Margin::symmetric(6, 8)),
            )
            .show(ui, |ui| {
                let tabs = [
                    (NavTab::Home, "🏠", "Home", 0usize),
                    (NavTab::Apps, "⊞", "Apps", 0usize),
                    (NavTab::Gaming, "🎮", "Gaming", 0usize),
                    (NavTab::Library, "▤", "Library", 0usize),
                    (NavTab::Updates, "⬇", "Updates", self.updates.len()),
                ];

                for (tab, icon, label, badge) in tabs {
                    let selected =
                        !self.showing_search && self.selected_app.is_none() && self.tab == tab;
                    let (rect, resp) =
                        ui.allocate_exact_size(Vec2::new(62.0, 54.0), egui::Sense::click());
                    if selected {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            Color32::from_rgb(0xff, 0xff, 0xff),
                        );
                        // Win11 left blue indicator pill
                        let bar = egui::Rect::from_min_size(
                            egui::pos2(rect.left() + 2.0, rect.center().y - 10.0),
                            Vec2::new(3.0, 20.0),
                        );
                        ui.painter()
                            .rect_filled(bar, CornerRadius::same(2), ACCENT);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            Color32::from_rgb(0xe8, 0xe8, 0xe8),
                        );
                    }

                    let fg = if selected { ACCENT } else { TEXT };
                    ui.painter().text(
                        egui::pos2(rect.center().x, rect.top() + 18.0),
                        egui::Align2::CENTER_CENTER,
                        icon,
                        FontId::proportional(17.0),
                        fg,
                    );
                    ui.painter().text(
                        egui::pos2(rect.center().x, rect.bottom() - 12.0),
                        egui::Align2::CENTER_CENTER,
                        label,
                        FontId::proportional(10.5),
                        if selected { ACCENT } else { TEXT_MUTED },
                    );

                    if badge > 0 {
                        let b_center = egui::pos2(rect.right() - 13.0, rect.top() + 12.0);
                        ui.painter().circle_filled(b_center, 8.0, ACCENT);
                        ui.painter().text(
                            b_center,
                            egui::Align2::CENTER_CENTER,
                            badge.min(99).to_string(),
                            FontId::proportional(9.5),
                            Color32::WHITE,
                        );
                    }

                    if resp.clicked() {
                        self.tab = tab;
                        self.showing_search = false;
                        self.selected_app = None;
                    }
                    ui.add_space(2.0);
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(RichText::new("📋").size(14.0))
                                .frame(false),
                        )
                        .on_hover_text("Toggle Activity Log")
                        .clicked()
                    {
                        self.show_activity_log = !self.show_activity_log;
                    }
                    if self.loading_library {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
                        draw_fluent_spinner(ui, r.center(), 7.5, 2.0);
                    }
                });
            });
    }

    fn render_activity_drawer(&mut self, ui: &mut egui::Ui) {
        if !self.show_activity_log && !self.task_running && self.task_status.is_none() {
            return;
        }
        egui::Panel::bottom("store_activity_bar")
            .frame(
                egui::Frame::new()
                    .fill(BG_CARD)
                    .inner_margin(Margin::symmetric(14, 8))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.task_running {
                        ui.spinner();
                        ui.label(
                            RichText::new(&self.task_title)
                                .font(FontId::proportional(12.5))
                                .strong()
                                .color(ACCENT),
                        );
                    } else if let Some((ok, msg)) = &self.task_status {
                        let color = if *ok { ACCENT_GREEN } else { ACCENT_RED };
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
                                "Hide Log"
                            } else {
                                "Show Log"
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
                        .fill(Color32::from_rgb(0x18, 0x18, 0x18))
                        .corner_radius(CornerRadius::same(6))
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
                                                .color(Color32::from_rgb(0xde, 0xde, 0xde)),
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
                    .fill(BG_SURFACE)
                    .stroke(Stroke::new(1.0, BORDER))
                    .corner_radius(CornerRadius {
                        nw: 8,
                        ne: 0,
                        sw: 0,
                        se: 0,
                    })
                    .inner_margin(Margin::symmetric(20, 18)),
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
                                    self.render_catalog_view(ui, "Top apps", Some("Apps"))
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

    fn render_home_view(&mut self, ui: &mut egui::Ui) {
        let avail_w = ui.available_width();
        let catalog = featured_catalog(&self.backends, &self.sets);

        // Win11 Store Hero Spotlight Banner
        let hero_rect = egui::Frame::new()
            .fill(Color32::from_rgb(0x0f, 0x2d, 0x5c))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(22))
            .show(ui, |ui| {
                ui.set_width((avail_w - 44.0).max(160.0));
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_max_width(if avail_w > 640.0 {
                            avail_w * 0.56
                        } else {
                            avail_w - 48.0
                        });
                        egui::Frame::new()
                            .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 28))
                            .corner_radius(CornerRadius::same(4))
                            .inner_margin(Margin::symmetric(8, 2))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("FEATURED COLLECTION")
                                        .font(FontId::proportional(10.5))
                                        .strong()
                                        .color(Color32::from_rgb(0xd0, 0xe8, 0xff)),
                                );
                            });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Essential apps & games")
                                .font(FontId::proportional(24.0))
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Install from Flathub and your system repos, manage installed apps, and keep everything up to date.",
                            )
                            .font(FontId::proportional(13.0))
                            .color(Color32::from_rgb(0xd8, 0xe8, 0xf8)),
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(if self.updates.is_empty() {
                                            "Browse Library"
                                        } else {
                                            "View Updates"
                                        })
                                        .color(Color32::from_rgb(0x0f, 0x2d, 0x5c))
                                        .strong(),
                                    )
                                    .fill(Color32::WHITE)
                                    .corner_radius(CornerRadius::same(6)),
                                )
                                .clicked()
                            {
                                self.tab = if self.updates.is_empty() {
                                    NavTab::Library
                                } else {
                                    NavTab::Updates
                                };
                            }
                            if !self.updates.is_empty() {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(format!(
                                        "{} updates available",
                                        self.updates.len()
                                    ))
                                    .font(FontId::proportional(12.5))
                                    .color(Color32::from_rgb(0x9d, 0xd4, 0xff)),
                                );
                            }
                        });
                    });

                    // Right-side mini spotlight tiles when window is wide enough
                    if avail_w > 660.0 {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            for item in catalog.iter().take(3).rev() {
                                let resp = egui::Frame::new()
                                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 20))
                                    .stroke(Stroke::new(
                                        1.0,
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 38),
                                    ))
                                    .corner_radius(CornerRadius::same(8))
                                    .inner_margin(Margin::same(10))
                                    .show(ui, |ui| {
                                        ui.set_width(72.0);
                                        ui.vertical_centered(|ui| {
                                            self.draw_app_icon(ui, item, 42.0);
                                            ui.add_space(4.0);
                                            ui.label(
                                                RichText::new(truncate(&item.name, 10))
                                                    .font(FontId::proportional(11.0))
                                                    .color(Color32::WHITE),
                                            );
                                        });
                                    })
                                    .response
                                    .interact(egui::Sense::click());
                                if resp.clicked() {
                                    self.selected_app = Some(item.clone());
                                }
                            }
                        });
                    }
                });
            })
            .response;
        let _ = hero_rect;

        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Top free apps")
                    .font(FontId::proportional(17.5))
                    .strong()
                    .color(TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(RichText::new("See all >").color(ACCENT)).frame(false))
                    .clicked()
                {
                    self.tab = NavTab::Apps;
                }
            });
        });
        ui.add_space(8.0);

        self.render_app_grid(ui, &catalog);
    }

    fn render_catalog_view(&mut self, ui: &mut egui::Ui, title: &str, category: Option<&str>) {
        ui.label(
            RichText::new(title)
                .font(FontId::proportional(22.0))
                .strong()
                .color(TEXT),
        );
        ui.add_space(2.0);
        ui.label(
            RichText::new("Click any app for details, or search above to browse all Flathub & system packages.")
                .font(FontId::proportional(12.5))
                .color(TEXT_MUTED),
        );
        ui.add_space(14.0);

        let catalog: Vec<AppItem> = featured_catalog(&self.backends, &self.sets)
            .into_iter()
            .filter(|a| category.is_none_or(|c| a.category == c))
            .collect();
        self.render_app_grid(ui, &catalog);
    }

    fn render_search_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("Results for \"{}\"", self.search_query.trim()))
                    .font(FontId::proportional(20.0))
                    .strong()
                    .color(TEXT),
            );
            ui.add_space(12.0);
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
        ui.add_space(12.0);

        if self.searching {
            draw_centered_loading(ui, "Searching Flathub and system repositories...");
            return;
        }

        if self.search_results.is_empty() {
            ui.label(
                RichText::new("No matching apps or packages found.")
                    .font(FontId::proportional(14.0))
                    .color(TEXT_MUTED),
            );
            return;
        }

        let results = self.search_results.clone();
        self.render_app_grid(ui, &results);
    }

    fn render_library_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("Library")
                    .font(FontId::proportional(22.0))
                    .strong()
                    .color(TEXT),
            );
            ui.add_space(8.0);
            if ui
                .selectable_label(!self.library_only_flatpak, "All installed")
                .clicked()
            {
                self.library_only_flatpak = false;
            }
            if ui
                .selectable_label(self.library_only_flatpak, "Flatpak apps")
                .clicked()
            {
                self.library_only_flatpak = true;
            }
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.library_filter)
                    .desired_width(180.0)
                    .hint_text("Filter..."),
            );
            if ui.button("↻ Refresh").clicked() {
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
            RichText::new(format!("{} installed items", filtered.len()))
                .font(FontId::proportional(12.0))
                .color(TEXT_MUTED),
        );
        ui.add_space(8.0);

        for item in filtered {
            self.render_list_row(ui, &item);
            ui.add_space(6.0);
        }
    }

    fn render_updates_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("Downloads and updates")
                    .font(FontId::proportional(22.0))
                    .strong()
                    .color(TEXT),
            );
            ui.add_space(12.0);
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
                        .fill(ACCENT),
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
        ui.add_space(12.0);

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
                .fill(BG_CARD)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.set_width((ui.available_width() - 36.0).max(160.0));
                    ui.label(
                        RichText::new("✓ You're good to go")
                            .font(FontId::proportional(15.5))
                            .strong()
                            .color(ACCENT_GREEN),
                    );
                    ui.label(
                        RichText::new("All Flatpak apps and system packages are up to date.")
                            .font(FontId::proportional(12.5))
                            .color(TEXT_MUTED),
                    );
                });
            return;
        }

        let updates = self.updates.clone();
        for item in updates {
            self.render_list_row(ui, &item);
            ui.add_space(6.0);
        }
    }

    fn render_app_grid(&mut self, ui: &mut egui::Ui, items: &[AppItem]) {
        // Subtract scrollbar margin so cards never clip horizontally
        let avail = (ui.available_width() - 14.0).max(200.0);
        let cols = ((avail / 235.0).floor() as usize).clamp(1, 4);
        let gap = 10.0;
        let card_outer_w = (avail - (cols as f32 - 1.0) * gap) / cols as f32;

        for row in items.chunks(cols) {
            ui.horizontal(|ui| {
                for (idx, item) in row.iter().enumerate() {
                    if idx > 0 {
                        ui.add_space(gap - 8.0);
                    }
                    self.render_app_card(ui, item, card_outer_w);
                }
            });
            ui.add_space(6.0);
        }
    }

    fn render_app_card(&mut self, ui: &mut egui::Ui, item: &AppItem, outer_width: f32) {
        let card_id = ui.make_persistent_id(("card", item.source, &item.id));
        let hovered = ui
            .ctx()
            .memory(|m| m.data.get_temp::<bool>(card_id))
            .unwrap_or(false);
        let fill = if hovered { BG_CARD_HOVER } else { BG_CARD };
        let stroke_col = if hovered { BORDER_HOVER } else { BORDER };

        let mut action_clicked = false;
        let resp = egui::Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0, stroke_col))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                let inner_w = (outer_width - 26.0).max(140.0);
                ui.set_width(inner_w);
                ui.set_min_height(110.0);
                ui.horizontal(|ui| {
                    self.draw_app_icon(ui, item, 48.0);
                    ui.add_space(6.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(truncate(&item.name, 20))
                                .font(FontId::proportional(14.0))
                                .strong()
                                .color(TEXT),
                        );
                        ui.label(
                            RichText::new(truncate(&item.publisher, 22))
                                .font(FontId::proportional(11.5))
                                .color(TEXT_MUTED),
                        );
                        ui.add_space(2.0);
                        draw_source_pill(ui, item.source);
                    });
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{} ★", item.rating))
                            .font(FontId::proportional(11.5))
                            .color(TEXT_MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if item.installed {
                            if item.has_update() {
                                if ui
                                    .add_enabled(
                                        !self.task_running,
                                        egui::Button::new(
                                            RichText::new("Update").size(11.5).color(Color32::WHITE),
                                        )
                                        .fill(ACCENT)
                                        .corner_radius(CornerRadius::same(4)),
                                    )
                                    .clicked()
                                {
                                    action_clicked = true;
                                    self.trigger_action(
                                        StoreAction::UpdateOne(item.clone()),
                                        ui.ctx(),
                                    );
                                }
                            } else if ui
                                .add(
                                    egui::Button::new(RichText::new("Open").size(11.5))
                                        .corner_radius(CornerRadius::same(4)),
                                )
                                .clicked()
                            {
                                action_clicked = true;
                                launch_app(item, &self.sets);
                            }
                            ui.label(
                                RichText::new("Installed")
                                    .font(FontId::proportional(11.0))
                                    .color(ACCENT_GREEN),
                            );
                        } else if ui
                            .add_enabled(
                                !self.task_running,
                                egui::Button::new(
                                    RichText::new("Get").size(11.5).color(Color32::WHITE),
                                )
                                .fill(ACCENT)
                                .corner_radius(CornerRadius::same(4)),
                            )
                            .clicked()
                        {
                            action_clicked = true;
                            self.trigger_action(StoreAction::Install(item.clone()), ui.ctx());
                        }
                    });
                });
            })
            .response
            .interact(egui::Sense::click());

        ui.ctx()
            .memory_mut(|m| m.data.insert_temp(card_id, resp.hovered()));
        if resp.clicked() && !action_clicked {
            self.selected_app = Some(item.clone());
        }
    }

    fn render_list_row(&mut self, ui: &mut egui::Ui, item: &AppItem) {
        let mut action_clicked = false;
        let resp = egui::Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width((ui.available_width() - 28.0).max(180.0));
                ui.horizontal(|ui| {
                    self.draw_app_icon(ui, item, 38.0);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&item.name)
                                    .font(FontId::proportional(14.0))
                                    .strong()
                                    .color(TEXT),
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
                            RichText::new(truncate(&ver_text, 48))
                                .font(FontId::proportional(11.5))
                                .color(TEXT_MUTED),
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
                                    .fill(ACCENT),
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
                                && ui.button("Open").clicked()
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
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::same(20))
            .show(ui, |ui| {
                ui.set_width((ui.available_width() - 40.0).max(200.0));
                ui.horizontal_wrapped(|ui| {
                    self.draw_app_icon(ui, item, 76.0);
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&item.name)
                                .font(FontId::proportional(23.0))
                                .strong()
                                .color(TEXT),
                        );
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(&item.publisher)
                                    .font(FontId::proportional(13.0))
                                    .color(ACCENT),
                            );
                            ui.label(RichText::new("•").color(TEXT_MUTED));
                            draw_source_pill(ui, item.source);
                            ui.label(
                                RichText::new(format!("• ★ {}", item.rating))
                                    .font(FontId::proportional(12.5))
                                    .color(TEXT_MUTED),
                            );
                        });
                        ui.add_space(10.0);
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
                                            .fill(ACCENT),
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
                                            RichText::new("Open")
                                                .color(Color32::WHITE)
                                                .strong(),
                                        )
                                        .fill(ACCENT),
                                    )
                                    .clicked()
                                {
                                    launch_app(item, &self.sets);
                                }
                                if ui
                                    .add_enabled(!self.task_running, egui::Button::new("Uninstall"))
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
                                        RichText::new("Install")
                                            .color(Color32::WHITE)
                                            .strong(),
                                    )
                                    .fill(ACCENT),
                                )
                                .clicked()
                            {
                                self.trigger_action(StoreAction::Install(item.clone()), ui.ctx());
                            }
                        });
                    });
                });
            });

        ui.add_space(16.0);
        ui.label(
            RichText::new("Description")
                .font(FontId::proportional(16.5))
                .strong()
                .color(TEXT),
        );
        ui.add_space(6.0);
        ui.label(
            RichText::new(if item.summary.is_empty() {
                "No description provided."
            } else {
                item.summary.as_str()
            })
            .font(FontId::proportional(13.5))
            .color(TEXT),
        );

        ui.add_space(16.0);
        ui.label(
            RichText::new("Additional information")
                .font(FontId::proportional(16.5))
                .strong()
                .color(TEXT),
        );
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width((ui.available_width() - 28.0).max(180.0));
                egui::Grid::new("app_meta_grid")
                    .num_columns(2)
                    .spacing([24.0, 6.0])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Package ID").color(TEXT_MUTED));
                        ui.label(RichText::new(&item.id).color(TEXT));
                        ui.end_row();
                        ui.label(RichText::new("Source").color(TEXT_MUTED));
                        ui.label(
                            RichText::new(format!("{} ({})", item.source.label(), item.origin))
                                .color(TEXT),
                        );
                        ui.end_row();
                        ui.label(RichText::new("Status").color(TEXT_MUTED));
                        ui.label(
                            RichText::new(if item.installed {
                                "Installed"
                            } else {
                                "Not installed"
                            })
                            .color(if item.installed { ACCENT_GREEN } else { TEXT }),
                        );
                        ui.end_row();
                        ui.label(RichText::new("Version").color(TEXT_MUTED));
                        ui.label(
                            RichText::new(if item.version.is_empty() {
                                "Latest"
                            } else {
                                &item.version
                            })
                            .color(TEXT),
                        );
                        ui.end_row();
                        if let Some(new_v) = &item.new_version {
                            ui.label(RichText::new("Available update").color(TEXT_MUTED));
                            ui.label(RichText::new(new_v).color(ACCENT));
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
                    CornerRadius::same((size * 0.2) as u8),
                    Color32::from_rgb(0xee, 0xf2, 0xf6),
                );
                draw_fluent_spinner(ui, rect.center(), size * 0.24, (size * 0.055).max(1.8));
                return;
            }
            IconLookup::Missing => {}
        }
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
        let bg = Color32::from_rgb(item.accent_rgb[0], item.accent_rgb[1], item.accent_rgb[2]);
        ui.painter()
            .rect_filled(rect, CornerRadius::same((size * 0.2) as u8), bg);
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
        self.apply_store_style(ui.ctx());
        self.poll_events(ui.ctx());
        self.render_top_bar(ui);
        self.render_nav_rail(ui);
        self.render_activity_drawer(ui);
        self.render_main(ui);
    }
}

fn draw_source_pill(ui: &mut egui::Ui, source: PackageSource) {
    let rgb = source.badge_rgb();
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], 24))
        .corner_radius(CornerRadius::same(4))
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
        CornerRadius::same(3),
        Stroke::new(1.4, Color32::from_rgb(0x70, 0x70, 0x70)),
        egui::StrokeKind::Inside,
    );
    let body = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 2.0, rect.top() + 5.0),
        egui::pos2(rect.right() - 2.0, rect.bottom() - 1.0),
    );
    painter.rect_filled(body, CornerRadius::same(3), Color32::from_rgb(0xfc, 0xfc, 0xfc));
    painter.rect_stroke(
        body,
        CornerRadius::same(3),
        Stroke::new(1.2, Color32::from_rgb(0x90, 0x90, 0x90)),
        egui::StrokeKind::Inside,
    );
    let cx = body.center().x;
    let cy = body.center().y;
    let s = 3.6;
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
        Stroke::new(stroke_w, Color32::from_rgba_unmultiplied(0, 103, 192, 32)),
    );
    ui.painter()
        .add(egui::Shape::line(pts, Stroke::new(stroke_w, ACCENT)));
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
                .color(TEXT_MUTED),
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
            .with_inner_size([960.0, 640.0])
            .with_min_inner_size([480.0, 380.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Window Shopping",
        options,
        Box::new(|cc| Ok(Box::new(StoreApp::new(cc)))),
    )
}
