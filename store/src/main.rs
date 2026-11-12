mod backend;

use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender};

use backend::{
    AppItem, BackendEvent, DetectedBackends, PackageSource, SourceFilter, StoreAction,
    featured_catalog, launch_app, spawn_action, spawn_refresh_library, spawn_search,
};
use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, RichText, Stroke, Vec2};

const BG_WINDOW: Color32 = Color32::from_rgb(0xf3, 0xf3, 0xf3);
const BG_CANVAS: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
const BG_CARD: Color32 = Color32::from_rgb(0xf9, 0xf9, 0xf9);
const BG_CARD_HOVER: Color32 = Color32::from_rgb(0xf0, 0xf4, 0xf9);
const BORDER: Color32 = Color32::from_rgb(0xe2, 0xe2, 0xe2);
const ACCENT: Color32 = Color32::from_rgb(0x00, 0x67, 0xc0);
const ACCENT_GREEN: Color32 = Color32::from_rgb(0x10, 0x7c, 0x10);
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

    tab: NavTab,
    search_query: String,
    source_filter: SourceFilter,
    searching: bool,
    search_results: Vec<AppItem>,
    showing_search: bool,

    loading_library: bool,
    installed: Vec<AppItem>,
    installed_ids: HashSet<String>,
    updates: Vec<AppItem>,
    library_filter: String,

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
            tab: NavTab::Home,
            search_query: String::new(),
            source_filter: SourceFilter::All,
            searching: false,
            search_results: Vec::new(),
            showing_search: false,
            loading_library: true,
            installed: Vec::new(),
            installed_ids: HashSet::new(),
            updates: Vec::new(),
            library_filter: String::new(),
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
        style.visuals.panel_fill = BG_WINDOW;
        style.visuals.window_fill = BG_CANVAS;
        style.visuals.extreme_bg_color = BG_CANVAS;
        style.visuals.faint_bg_color = BG_CARD;
        style.visuals.selection.bg_fill = ACCENT;
        style.visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
        style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(0xea, 0xea, 0xea);
        style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0xf4, 0xf4, 0xf4);
        style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(0xdf, 0xe6, 0xee);
        style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0xe8, 0xee, 0xf5);
        style.visuals.widgets.active.bg_fill = ACCENT;
        style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        style.spacing.item_spacing = Vec2::new(8.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 6.0);
        ctx.set_global_style(style);
    }

    fn poll_events(&mut self, ctx: &egui::Context) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                BackendEvent::LibraryLoaded { installed, updates } => {
                    self.loading_library = false;
                    self.installed_ids = installed.iter().map(|i| i.id.clone()).collect();
                    self.installed = installed;
                    self.updates = updates;
                    if let Some(sel) = &mut self.selected_app {
                        sel.installed = self.installed_ids.contains(&sel.id);
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
                    spawn_refresh_library(
                        self.backends.clone(),
                        self.tx.clone(),
                        ctx.clone(),
                    );
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
            self.installed_ids.clone(),
            self.tx.clone(),
            ctx.clone(),
        );
    }

    fn trigger_action(&mut self, action: StoreAction, ctx: &egui::Context) {
        if self.task_running {
            return;
        }
        spawn_action(
            self.backends.clone(),
            action,
            self.tx.clone(),
            ctx.clone(),
        );
    }

    fn trigger_refresh(&mut self, ctx: &egui::Context) {
        self.loading_library = true;
        spawn_refresh_library(self.backends.clone(), self.tx.clone(), ctx.clone());
    }

    fn render_top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("store_top_bar")
            .frame(
                egui::Frame::new()
                    .fill(BG_WINDOW)
                    .inner_margin(Margin::symmetric(16, 10))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Microsoft Store branding
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(24.0, 24.0), egui::Sense::hover());
                    draw_ms_store_bag(ui.painter(), rect);
                    ui.label(
                        RichText::new("Microsoft Store")
                            .font(FontId::proportional(16.0))
                            .strong()
                            .color(TEXT),
                    );
                    ui.add_space(16.0);

                    if self.selected_app.is_some() && ui.button("← Back").clicked() {
                        self.selected_app = None;
                    }

                    // Search box
                    let search_w = (ui.available_width() - 250.0).max(220.0);
                    let search_frame = egui::Frame::new()
                        .fill(BG_CANVAS)
                        .stroke(Stroke::new(1.0, Color32::from_rgb(0xcc, 0xcc, 0xcc)))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(Margin::symmetric(10, 5));
                    let mut enter_pressed = false;
                    search_frame.show(ui, |ui| {
                        ui.set_width(search_w);
                        ui.horizontal(|ui| {
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.search_query)
                                    .desired_width(search_w - 60.0)
                                    .frame(egui::Frame::NONE)
                                    .hint_text("Search apps, games, Flatpaks, and packages"),
                            );
                            if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                enter_pressed = true;
                            }
                            if !self.search_query.is_empty() && ui.small_button("✕").clicked() {
                                self.search_query.clear();
                                self.showing_search = false;
                                self.search_results.clear();
                            }
                            if ui.small_button("🔍").clicked() {
                                enter_pressed = true;
                            }
                        });
                    });
                    if enter_pressed {
                        self.trigger_search(ui.ctx());
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(self.backends.summary())
                                .font(FontId::proportional(11.5))
                                .color(TEXT_MUTED),
                        );
                    });
                });
            });
    }

    fn render_nav_rail(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("store_nav_rail")
            .exact_size(148.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_WINDOW)
                    .inner_margin(Margin::symmetric(8, 12))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ui, |ui| {
                let updates_count = self.updates.len();
                let tabs = [
                    (NavTab::Home, "🏠  Home", 0usize),
                    (NavTab::Apps, "📦  Apps", 0usize),
                    (NavTab::Gaming, "🎮  Gaming", 0usize),
                    (NavTab::Library, "📚  Library", self.installed.len()),
                    (NavTab::Updates, "⬇  Updates", updates_count),
                ];

                for (tab, label, badge) in tabs {
                    let selected = !self.showing_search && self.selected_app.is_none() && self.tab == tab;
                    let fill = if selected {
                        Color32::from_rgb(0xe3, 0xec, 0xf6)
                    } else {
                        Color32::TRANSPARENT
                    };
                    let text_color = if selected { ACCENT } else { TEXT };
                    let frame = egui::Frame::new()
                        .fill(fill)
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(10, 9));
                    let resp = frame
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(label)
                                        .font(FontId::proportional(13.5))
                                        .color(text_color),
                                );
                                if badge > 0 {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let badge_bg = if tab == NavTab::Updates {
                                                ACCENT
                                            } else {
                                                Color32::from_rgb(0xdd, 0xdd, 0xdd)
                                            };
                                            let badge_fg = if tab == NavTab::Updates {
                                                Color32::WHITE
                                            } else {
                                                TEXT_MUTED
                                            };
                                            egui::Frame::new()
                                                .fill(badge_bg)
                                                .corner_radius(CornerRadius::same(8))
                                                .inner_margin(Margin::symmetric(6, 1))
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        RichText::new(badge.to_string())
                                                            .font(FontId::proportional(11.0))
                                                            .color(badge_fg),
                                                    );
                                                });
                                        },
                                    );
                                }
                            });
                        })
                        .response
                        .interact(egui::Sense::click());
                    if resp.clicked() {
                        self.tab = tab;
                        self.showing_search = false;
                        self.selected_app = None;
                    }
                    ui.add_space(2.0);
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    if ui
                        .button(if self.show_activity_log {
                            "Hide Activity"
                        } else {
                            "Activity Log"
                        })
                        .clicked()
                    {
                        self.show_activity_log = !self.show_activity_log;
                    }
                    ui.add_space(4.0);
                    if self.loading_library {
                        ui.label(
                            RichText::new("Refreshing...")
                                .font(FontId::proportional(11.5))
                                .color(TEXT_MUTED),
                        );
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
                    .fill(BG_WINDOW)
                    .inner_margin(Margin::symmetric(14, 8))
                    .stroke(Stroke::new(1.0, BORDER)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if self.task_running {
                        ui.spinner();
                        ui.label(
                            RichText::new(&self.task_title)
                                .font(FontId::proportional(13.0))
                                .strong()
                                .color(ACCENT),
                        );
                    } else if let Some((ok, msg)) = &self.task_status {
                        let color = if *ok { ACCENT_GREEN } else { ACCENT_RED };
                        ui.label(
                            RichText::new(msg)
                                .font(FontId::proportional(13.0))
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
                                "Collapse Log"
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
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(Margin::same(8))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(130.0)
                                .stick_to_bottom(true)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    for line in &self.task_logs {
                                        ui.label(
                                            RichText::new(line)
                                                .font(FontId::monospace(11.5))
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
            .frame(egui::Frame::new().fill(BG_CANVAS).inner_margin(Margin::same(22)))
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
                                NavTab::Apps => self.render_catalog_view(ui, "Apps", Some("Apps")),
                                NavTab::Gaming => self.render_catalog_view(ui, "Gaming", Some("Gaming")),
                                NavTab::Library => self.render_library_view(ui),
                                NavTab::Updates => self.render_updates_view(ui),
                            }
                        }
                    });
            });
    }

    fn render_home_view(&mut self, ui: &mut egui::Ui) {
        // Hero Banner
        egui::Frame::new()
            .fill(Color32::from_rgb(0x00, 0x5a, 0x9e))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(24))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Welcome to Microsoft Store")
                                .font(FontId::proportional(26.0))
                                .strong()
                                .color(Color32::WHITE),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(format!(
                                "Discover, install, and update Flatpaks and native system packages ({}).",
                                self.backends.summary()
                            ))
                            .font(FontId::proportional(13.5))
                            .color(Color32::from_rgb(0xe3, 0xf2, 0xfd)),
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Check for updates")
                                            .color(ACCENT)
                                            .strong(),
                                    )
                                    .fill(Color32::WHITE),
                                )
                                .clicked()
                            {
                                self.tab = NavTab::Updates;
                                self.trigger_refresh(ui.ctx());
                            }
                            if !self.updates.is_empty() {
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(format!(
                                        "{} updates ready",
                                        self.updates.len()
                                    ))
                                    .font(FontId::proportional(13.0))
                                    .color(Color32::WHITE),
                                );
                            }
                        });
                    });
                });
            });

        ui.add_space(20.0);
        ui.label(
            RichText::new("Essential apps")
                .font(FontId::proportional(19.0))
                .strong()
                .color(TEXT),
        );
        ui.add_space(10.0);

        let catalog = featured_catalog(&self.backends, &self.installed_ids);
        self.render_app_grid(ui, &catalog);
    }

    fn render_catalog_view(&mut self, ui: &mut egui::Ui, title: &str, category: Option<&str>) {
        ui.label(
            RichText::new(title)
                .font(FontId::proportional(24.0))
                .strong()
                .color(TEXT),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Popular picks ready to install from Flathub and your system repositories. Use the search bar above to find any package.")
                .font(FontId::proportional(13.0))
                .color(TEXT_MUTED),
        );
        ui.add_space(14.0);

        let catalog: Vec<AppItem> = featured_catalog(&self.backends, &self.installed_ids)
            .into_iter()
            .filter(|a| category.is_none_or(|c| a.category == c))
            .collect();
        self.render_app_grid(ui, &catalog);
    }

    fn render_search_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Results for \"{}\"", self.search_query.trim()))
                    .font(FontId::proportional(22.0))
                    .strong()
                    .color(TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut changed = false;
                for (filter, label) in [
                    (SourceFilter::System, self.backends.native_label()),
                    (SourceFilter::Flatpak, "Flatpak"),
                    (SourceFilter::All, "All sources"),
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
        });
        ui.add_space(12.0);

        if self.searching {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Searching Flathub and system repositories...");
            });
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
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Library")
                    .font(FontId::proportional(24.0))
                    .strong()
                    .color(TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("↻ Refresh").clicked() {
                    self.trigger_refresh(ui.ctx());
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.library_filter)
                        .desired_width(200.0)
                        .hint_text("Filter installed..."),
                );
            });
        });
        ui.add_space(10.0);

        if self.loading_library && self.installed.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading installed apps and packages...");
            });
            return;
        }

        let needle = self.library_filter.trim().to_lowercase();
        let filtered: Vec<AppItem> = self
            .installed
            .iter()
            .filter(|a| {
                needle.is_empty()
                    || a.name.to_lowercase().contains(&needle)
                    || a.id.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect();

        ui.label(
            RichText::new(format!("{} installed items", filtered.len()))
                .font(FontId::proportional(12.5))
                .color(TEXT_MUTED),
        );
        ui.add_space(8.0);

        for item in filtered {
            self.render_list_row(ui, &item);
            ui.add_space(6.0);
        }
    }

    fn render_updates_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Updates & Downloads")
                    .font(FontId::proportional(24.0))
                    .strong()
                    .color(TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                if ui
                    .add_enabled(!self.loading_library, egui::Button::new("Get updates"))
                    .clicked()
                {
                    self.trigger_refresh(ui.ctx());
                }
            });
        });
        ui.add_space(12.0);

        if self.loading_library {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking Flatpak and system repositories for updates...");
            });
            ui.add_space(8.0);
        }

        if self.updates.is_empty() && !self.loading_library {
            egui::Frame::new()
                .fill(BG_CARD)
                .stroke(Stroke::new(1.0, BORDER))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(
                        RichText::new("✓ You're up to date")
                            .font(FontId::proportional(16.0))
                            .strong()
                            .color(ACCENT_GREEN),
                    );
                    ui.label(
                        RichText::new("All Flatpak apps and system packages are on their latest versions.")
                            .font(FontId::proportional(13.0))
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
        let avail = ui.available_width();
        let cols = ((avail / 270.0).floor() as usize).clamp(1, 4);
        let card_w = (avail - (cols as f32 - 1.0) * 12.0) / cols as f32;

        for row in items.chunks(cols) {
            ui.horizontal(|ui| {
                for item in row {
                    self.render_app_card(ui, item, card_w);
                }
            });
            ui.add_space(10.0);
        }
    }

    fn render_app_card(&mut self, ui: &mut egui::Ui, item: &AppItem, width: f32) {
        let card_id = ui.make_persistent_id(("card", item.source, &item.id));
        let hovered = ui.ctx().memory(|m| m.data.get_temp::<bool>(card_id)).unwrap_or(false);
        let fill = if hovered { BG_CARD_HOVER } else { BG_CARD };

        let resp = egui::Frame::new()
            .fill(fill)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(width - 28.0);
                ui.set_min_height(132.0);
                ui.horizontal(|ui| {
                    draw_app_tile(ui, &item.name, item.accent_rgb, 48.0);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&item.name)
                                .font(FontId::proportional(15.0))
                                .strong()
                                .color(TEXT),
                        );
                        ui.horizontal(|ui| {
                            draw_source_pill(ui, item.source);
                            ui.label(
                                RichText::new(format!("★ {}", item.rating))
                                    .font(FontId::proportional(11.5))
                                    .color(TEXT_MUTED),
                            );
                        });
                    });
                });
                ui.add_space(8.0);
                ui.label(
                    RichText::new(truncate(&item.summary, 78))
                        .font(FontId::proportional(12.5))
                        .color(TEXT_MUTED),
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if item.installed {
                        if item.has_update() {
                            if ui
                                .add_enabled(
                                    !self.task_running,
                                    egui::Button::new(RichText::new("Update").color(Color32::WHITE))
                                        .fill(ACCENT),
                                )
                                .clicked()
                            {
                                self.trigger_action(StoreAction::UpdateOne(item.clone()), ui.ctx());
                            }
                        } else if ui.button("Open").clicked() {
                            launch_app(item);
                        }
                        ui.label(
                            RichText::new("Installed")
                                .font(FontId::proportional(11.5))
                                .color(ACCENT_GREEN),
                        );
                    } else if ui
                        .add_enabled(
                            !self.task_running,
                            egui::Button::new(RichText::new("Get").color(Color32::WHITE))
                                .fill(ACCENT),
                        )
                        .clicked()
                    {
                        self.trigger_action(StoreAction::Install(item.clone()), ui.ctx());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Details").clicked() {
                            self.selected_app = Some(item.clone());
                        }
                    });
                });
            })
            .response;

        ui.ctx().memory_mut(|m| m.data.insert_temp(card_id, resp.hovered()));
    }

    fn render_list_row(&mut self, ui: &mut egui::Ui, item: &AppItem) {
        egui::Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    draw_app_tile(ui, &item.name, item.accent_rgb, 36.0);
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&item.name)
                                    .font(FontId::proportional(14.5))
                                    .strong()
                                    .color(TEXT),
                            );
                            draw_source_pill(ui, item.source);
                        });
                        let ver_text = if let Some(new_v) = &item.new_version {
                            if item.version.is_empty() {
                                format!("{} • Update -> {new_v}", item.id)
                            } else {
                                format!("{} • {} -> {new_v}", item.id, item.version)
                            }
                        } else {
                            format!("{} • {}", item.id, item.version)
                        };
                        ui.label(
                            RichText::new(ver_text)
                                .font(FontId::proportional(12.0))
                                .color(TEXT_MUTED),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Details").clicked() {
                            self.selected_app = Some(item.clone());
                        }
                        if item.has_update() {
                            if ui
                                .add_enabled(
                                    !self.task_running,
                                    egui::Button::new(RichText::new("Update").color(Color32::WHITE))
                                        .fill(ACCENT),
                                )
                                .clicked()
                            {
                                self.trigger_action(StoreAction::UpdateOne(item.clone()), ui.ctx());
                            }
                        } else if item.installed {
                            if ui
                                .add_enabled(!self.task_running, egui::Button::new("Uninstall"))
                                .clicked()
                            {
                                self.trigger_action(StoreAction::Uninstall(item.clone()), ui.ctx());
                            }
                            if item.source == PackageSource::Flatpak && ui.button("Open").clicked() {
                                launch_app(item);
                            }
                        }
                    });
                });
            });
    }

    fn render_detail_view(&mut self, ui: &mut egui::Ui, item: &AppItem) {
        egui::Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(8))
            .inner_margin(Margin::same(22))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    draw_app_tile(ui, &item.name, item.accent_rgb, 76.0);
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&item.name)
                                .font(FontId::proportional(24.0))
                                .strong()
                                .color(TEXT),
                        );
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(&item.publisher)
                                    .font(FontId::proportional(13.0))
                                    .color(ACCENT),
                            );
                            ui.label(RichText::new("•").color(TEXT_MUTED));
                            draw_source_pill(ui, item.source);
                            ui.label(
                                RichText::new(format!("★ {}", item.rating))
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
                                                RichText::new("Update").color(Color32::WHITE).strong(),
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
                                            RichText::new("Open").color(Color32::WHITE).strong(),
                                        )
                                        .fill(ACCENT),
                                    )
                                    .clicked()
                                {
                                    launch_app(item);
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
                                        RichText::new("Install").color(Color32::WHITE).strong(),
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

        ui.add_space(18.0);
        ui.label(
            RichText::new("Description")
                .font(FontId::proportional(17.0))
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

        ui.add_space(18.0);
        ui.label(
            RichText::new("Additional information")
                .font(FontId::proportional(17.0))
                .strong()
                .color(TEXT),
        );
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(BG_CARD)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::same(14))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::Grid::new("app_meta_grid").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                    ui.label(RichText::new("Package ID").color(TEXT_MUTED));
                    ui.label(RichText::new(&item.id).color(TEXT));
                    ui.end_row();
                    ui.label(RichText::new("Source").color(TEXT_MUTED));
                    ui.label(RichText::new(format!("{} ({})", item.source.label(), item.origin)).color(TEXT));
                    ui.end_row();
                    ui.label(RichText::new("Version").color(TEXT_MUTED));
                    ui.label(RichText::new(if item.version.is_empty() { "Latest" } else { &item.version }).color(TEXT));
                    ui.end_row();
                    if let Some(new_v) = &item.new_version {
                        ui.label(RichText::new("Available update").color(TEXT_MUTED));
                        ui.label(RichText::new(new_v).color(ACCENT));
                        ui.end_row();
                    }
                });
            });
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

fn draw_app_tile(ui: &mut egui::Ui, name: &str, rgb: [u8; 3], size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    let bg = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    ui.painter()
        .rect_filled(rect, CornerRadius::same((size * 0.18) as u8), bg);
    let initials: String = name
        .split_whitespace()
        .take(2)
        .filter_map(|w| w.chars().next())
        .collect::<String>()
        .to_uppercase();
    let label = if initials.is_empty() { "A".to_string() } else { initials };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(size * 0.38),
        Color32::WHITE,
    );
}

fn draw_source_pill(ui: &mut egui::Ui, source: PackageSource) {
    let rgb = source.badge_rgb();
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], 28))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(
                RichText::new(source.label())
                    .font(FontId::proportional(11.0))
                    .color(Color32::from_rgb(rgb[0], rgb[1], rgb[2])),
            );
        });
}

fn draw_ms_store_bag(painter: &egui::Painter, rect: egui::Rect) {
    let body = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 2.0, rect.top() + 6.0),
        egui::pos2(rect.right() - 2.0, rect.bottom() - 1.0),
    );
    painter.rect_filled(body, CornerRadius::same(3), Color32::from_rgb(0xe8, 0xe8, 0xe8));
    painter.rect_stroke(
        body,
        CornerRadius::same(3),
        Stroke::new(1.2, Color32::from_rgb(0x88, 0x88, 0x88)),
        egui::StrokeKind::Inside,
    );
    // 4-color Microsoft grid inside bag
    let cx = body.center().x;
    let cy = body.center().y + 1.0;
    let s = 3.8;
    let gap = 1.2;
    let tiles = [
        (egui::pos2(cx - s - gap * 0.5, cy - s - gap * 0.5), Color32::from_rgb(0xf2, 0x50, 0x22)),
        (egui::pos2(cx + gap * 0.5, cy - s - gap * 0.5), Color32::from_rgb(0x7f, 0xba, 0x00)),
        (egui::pos2(cx - s - gap * 0.5, cy + gap * 0.5), Color32::from_rgb(0x00, 0xa4, 0xef)),
        (egui::pos2(cx + gap * 0.5, cy + gap * 0.5), Color32::from_rgb(0xff, 0xb9, 0x00)),
    ];
    for (pos, col) in tiles {
        painter.rect_filled(egui::Rect::from_min_size(pos, Vec2::splat(s)), 0.0, col);
    }
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
            .with_title("Microsoft Store")
            .with_app_id("microsoft-store")
            .with_inner_size([1060.0, 720.0])
            .with_min_inner_size([720.0, 500.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Microsoft Store",
        options,
        Box::new(|cc| Ok(Box::new(StoreApp::new(cc)))),
    )
}
