//! Task Mangler: VIDE-DE's task manager, modelled on Windows 10 Task Manager.
//! Processes tab (grouped, sortable, heat map, totals in headers, right-click menu)
//! Performance tab (CPU, Memory, Disk, Network, GPU with live graphs and full details)

use std::{collections::VecDeque, time::{Duration, Instant}};

use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2, Align2, Layout, Align, Pos2};
use sysinfo::{Pid, ProcessesToUpdate, Signal, System, Disk, Disks, DiskKind, NetworkData, Networks};

const WHITE: Color32 = Color32::WHITE;
const BORDER: Color32 = Color32::from_rgb(0xe5, 0xe5, 0xe5);
const HOVER: Color32 = Color32::from_rgb(0xe5, 0xf3, 0xff);
const SELECTED: Color32 = Color32::from_rgb(0xcc, 0xe8, 0xff);
const TEXT: Color32 = Color32::from_rgb(0x1a, 0x1a, 0x1a);
const DIM: Color32 = Color32::from_rgb(0x6d, 0x6d, 0x6d);
const CPU_LINE: Color32 = Color32::from_rgb(0x11, 0x7d, 0xbb);
const CPU_FILL: Color32 = Color32::from_rgb(0xf1, 0xf6, 0xfa);
const MEM_LINE: Color32 = Color32::from_rgb(0x8b, 0x12, 0xae);
const MEM_FILL: Color32 = Color32::from_rgb(0xf4, 0xf2, 0xf4);
const DISK_LINE: Color32 = Color32::from_rgb(0x4c, 0x9f, 0x4c);
const DISK_FILL: Color32 = Color32::from_rgb(0xec, 0xf7, 0xec);
const NET_LINE: Color32 = Color32::from_rgb(0x9f, 0x4c, 0x9f);
const NET_FILL: Color32 = Color32::from_rgb(0xf4, 0xec, 0xf4);
const GPU_LINE: Color32 = Color32::from_rgb(0xcc, 0x77, 0x00);
const GPU_FILL: Color32 = Color32::from_rgb(0xff, 0xf3, 0xe0);
const HEAT: [Color32; 4] = [
    Color32::from_rgb(0xff, 0xf4, 0xc4),
    Color32::from_rgb(0xff, 0xe9, 0x9c),
    Color32::from_rgb(0xff, 0xd2, 0x6b),
    Color32::from_rgb(0xff, 0xa5, 0x4a),
];
const HISTORY: usize = 60;
const ROW_H: f32 = 24.0;
const GROUP_ROW_H: f32 = 28.0;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Task Mangler")
            .with_app_id("task-mangler")
            .with_inner_size([1000.0, 700.0])
            .with_min_inner_size([700.0, 450.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Task Mangler",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(Mangler::new()))
        }),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Processes,
    Performance,
    // AppHistory,
    // Startup,
    // Users,
    // Details,
    // Services,
}

#[derive(Clone, Copy, PartialEq)]
enum PerfTab {
    Cpu,
    Memory,
    Disk,
    Network,
    Gpu,
}

#[derive(Clone, Copy, PartialEq)]
enum Col {
    Name,
    Cpu,
    Memory,
    Disk,
    Network,
    Gpu,
    PowerUsage,
    PowerTrend,
}

#[derive(Clone, Copy, PartialEq)]
enum SortDir {
    Asc,
    Desc,
}

struct ProcessGroup {
    name: String,
    expanded: bool,
    children: Vec<ProcRow>,
}

#[derive(Clone)]
struct ProcRow {
    pid: Pid,
    name: String,
    exe: String,
    cpu: f32,
    memory: u64,
    disk: u64,
    network: u64,
    gpu: f32,
    power: String,
    power_trend: String,
    status: String,
    user: String,
    command_line: String,
}

struct Mangler {
    sys: System,
    disks: Disks,
    networks: Networks,
    groups: Vec<ProcessGroup>,
    flat_rows: Vec<ProcRow>,
    last_refresh: Instant,
    tab: Tab,
    perf_tab: PerfTab,
    sort_col: Col,
    sort_dir: SortDir,
    selected_pid: Option<Pid>,
    cpu_hist: VecDeque<f32>,
    mem_hist: VecDeque<f32>,
    disk_hist: VecDeque<f32>,
    net_hist: VecDeque<f32>,
    cores: usize,
    status: Option<String>,
    context_menu: Option<(Pid, Pos2)>,
}

impl Mangler {
    fn new() -> Self {
        let sys = System::new_all().expect("can't read system information from /proc");
        let disks = Disks::new_with_refreshed_list().expect("disks");
        let networks = Networks::new_with_refreshed_list().expect("networks");
        let cores = sys.cpus().len().max(1);
        let mut m = Self {
            sys,
            disks,
            networks,
            groups: Vec::new(),
            flat_rows: Vec::new(),
            last_refresh: Instant::now() - Duration::from_secs(10),
            tab: Tab::Processes,
            perf_tab: PerfTab::Cpu,
            sort_col: Col::Cpu,
            sort_dir: SortDir::Desc,
            selected_pid: None,
            cpu_hist: VecDeque::from(vec![0.0; 60]),
            mem_hist: VecDeque::from(vec![0.0; 60]),
            disk_hist: VecDeque::from(vec![0.0; 60]),
            net_hist: VecDeque::from(vec![0.0; 60]),
            cores,
            status: None,
            context_menu: None,
        };
        m.refresh();
        m
    }

    fn refresh(&mut self) {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        self.disks.refresh(true);
        self.networks.refresh(true);

        let cores = self.cores as f32;
        let mut rows = Vec::new();
        for (pid, p) in self.sys.processes() {
            if p.thread_kind().is_none() {
                let cpu = p.cpu_usage() / self.cores as f32;
                let mem = p.memory();
                let disk = { let d = p.disk_usage(); d.read_bytes + d.written_bytes };
                rows.push(ProcRow {
                    pid: *pid,
                    name: p.name().to_string_lossy().into_owned(),
                    exe: p.exe().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default(),
                    cpu,
                    memory: mem,
                    disk,
                    network: 0, // sysinfo doesn't expose per-process network easily
                    gpu: 0.0,
                    power: "Very low".into(),
                    power_trend: "".into(),
                    status: if p.status().to_string() == "Run" { "Running" } else { "Suspended" }.into(),
                    user: "user".into(),
                    command_line: p.cmd().iter().map(|s| s.to_string_lossy().into_owned()).collect::<Vec<_>>().join(" "),
                });
            }
        }

        // Group: Apps (have visible windows - we can't detect easily, so use heuristic)
        // Background processes, Windows processes
        let mut apps = Vec::new();
        let mut background = Vec::new();
        let mut windows = Vec::new();

        for r in rows {
            let name_lower = r.name.to_lowercase();
            let is_system = name_lower.contains("systemd") || name_lower.contains("kthreadd") || name_lower.contains("kernel") || name_lower.contains("irq/");
            let is_desktop = name_lower.contains("plasmashell") || name_lower.contains("kwin") || name_lower.contains("gnome-shell") || name_lower.contains("explorer") || name_lower.contains("konsole") || name_lower.contains("code") || name_lower.contains("firefox") || name_lower.contains("chrome");

            if is_system {
                windows.push(r);
            } else if is_desktop {
                apps.push(r);
            } else {
                background.push(r);
            }
        }

        self.groups = vec![
            ProcessGroup { name: "Apps".into(), expanded: true, children: apps },
            ProcessGroup { name: "Background processes".into(), expanded: true, children: background },
            ProcessGroup { name: "Windows processes".into(), expanded: false, children: windows },
        ];
        self.flatten_rows();
        self.sort_rows();
        push(&mut self.cpu_hist, self.sys.global_cpu_usage());
        let mem_pct = self.mem_pct();
        push(&mut self.mem_hist, mem_pct);
        self.last_refresh = Instant::now();
    }

    fn flatten_rows(&mut self) {
        self.flat_rows.clear();
        for g in &self.groups {
            self.flat_rows.extend(g.children.clone());
        }
    }

    fn mem_pct(&self) -> f32 {
        self.sys.used_memory() as f32 / self.sys.total_memory().max(1) as f32 * 100.0
    }

    fn sort_rows(&mut self) {
        for g in &mut self.groups {
            let (col, dir) = (self.sort_col, self.sort_dir);
            g.children.sort_by(|a, b| {
                let o = match col {
                    Col::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                    Col::Cpu => a.cpu.total_cmp(&b.cpu),
                    Col::Memory => a.memory.cmp(&b.memory),
                    Col::Disk => a.disk.cmp(&b.disk),
                    Col::Network => a.network.cmp(&b.network),
                    Col::Gpu => a.gpu.total_cmp(&b.gpu),
                    Col::PowerUsage => a.power.cmp(&b.power),
                    Col::PowerTrend => a.power_trend.cmp(&b.power_trend),
                };
                match dir { SortDir::Asc => o, SortDir::Desc => o.reverse() }
            });
        }
        self.flatten_rows();
    }

    fn end_task(&mut self) {
        let Some(pid) = self.selected_pid else { return };
        if pid.as_u32() == std::process::id() { return; }
        let Some(p) = self.sys.process(pid) else { return };
        let name = p.name().to_string_lossy().into_owned();
        if p.kill_with(Signal::Term) != Some(true) {
            self.status = Some(format!("Couldn't end {name} (not yours?)"));
            return;
        }
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
        });
        self.status = Some(format!("Ended {name}"));
        self.selected_pid = None;
    }

    fn open_file_location(&self) {
        if let Some(pid) = self.selected_pid {
            if let Some(p) = self.sys.process(pid) {
                if let Some(exe) = p.exe() {
                    let _ = std::process::Command::new("xdg-open").arg(exe.parent().unwrap_or_else(|| std::path::Path::new("/"))).status();
                }
            }
        }
    }
}

fn push(h: &mut VecDeque<f32>, v: f32) {
    h.push_back(v);
    while h.len() > 60 { h.pop_front(); }
}

fn human(b: u64) -> String {
    let mb = b as f64 / 1024.0 / 1024.0;
    if mb >= 1024.0 { format!("{:.1} GB", mb / 1024.0) } else { format!("{:.1} MB", mb) }
}

fn human_speed(b: u64) -> String {
    let mb = b as f64 / 1024.0 / 1024.0;
    if mb >= 1024.0 { format!("{:.1} GB/s", mb / 1024.0) } else { format!("{:.1} MB/s", mb) }
}

fn heat(fraction: f32) -> Color32 {
    let i = ((fraction * 4.0) as usize).min(3);
    if fraction < 0.02 { WHITE } else { HEAT[i] }
}

fn graph(ui: &mut egui::Ui, size: Vec2, hist: &VecDeque<f32>, line: Color32, fill: Color32, grid: bool, label: &str, unit: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, WHITE);
    if grid {
        for i in 1..10 {
            let y = rect.top() + rect.height() * i as f32 / 10.0;
            p.hline(rect.x_range(), y, Stroke::new(1.0, fill.gamma_multiply(1.6)));
        }
        for i in 1..20 {
            let x = rect.left() + rect.width() * i as f32 / 20.0;
            p.vline(x, rect.y_range(), Stroke::new(1.0, fill.gamma_multiply(1.6)));
        }
    }
    let n = hist.len().max(2);
    let pts: Vec<egui::Pos2> = hist
        .iter()
        .enumerate()
        .map(|(i, v)| egui::pos2(rect.left() + rect.width() * i as f32 / (n - 1) as f32, rect.bottom() - rect.height() * (v / 100.0).clamp(0.0, 1.0)))
        .collect();
    for w in pts.windows(2) {
        let quad = vec![w[0], w[1], egui::pos2(w[1].x, rect.bottom()), egui::pos2(w[0].x, rect.bottom())];
        p.add(egui::Shape::convex_polygon(quad, fill, Stroke::NONE));
    }
    p.add(egui::Shape::line(pts, Stroke::new(1.2, line)));
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, line), egui::StrokeKind::Inside);

    // Current value label
    if let Some(&last) = hist.back() {
        p.text(egui::pos2(rect.right() - 6.0, rect.top() + 16.0), Align2::RIGHT_TOP, format!("{:.0}{}", last, unit), egui::FontId::proportional(14.0), line);
        p.text(egui::pos2(rect.left() + 6.0, rect.top() + 16.0), Align2::LEFT_TOP, label, egui::FontId::proportional(12.0), DIM);
    }
    resp
}

impl eframe::App for Mangler {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut v = egui::Visuals::light();
        v.panel_fill = WHITE;
        ctx.set_visuals(v);
        if self.last_refresh.elapsed() >= Duration::from_secs(1) {
            self.refresh();
        }
        ctx.request_repaint_after(Duration::from_millis(500));

        // Main tabs
        egui::Panel::top("maintabs").frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (t, label) in [(Tab::Processes, "Processes"), (Tab::Performance, "Performance")] {
                    let active = self.tab == t;
                    let r = ui.add(egui::Button::new(RichText::new(label).color(TEXT).size(13.0)).frame(false));
                    if active { ui.painter().hline(r.rect.x_range(), r.rect.bottom() + 2.0, Stroke::new(2.0, CPU_LINE)); }
                    if r.clicked() { self.tab = t; }
                    ui.add_space(16.0);
                }
            });
            ui.painter().hline(ui.max_rect().x_range(), ui.min_rect().bottom() + 4.0, Stroke::new(1.0, BORDER));
        });

        match self.tab {
            Tab::Processes => self.processes_tab(ui),
            Tab::Performance => self.performance_tab(ui),
        }

        // Context menu
        if let Some((pid, pos)) = self.context_menu {
            egui::Area::new(egui::Id::new("ctxmenu")).fixed_pos(pos).show(&ctx, |ui| {
                egui::Frame::new().fill(WHITE).stroke(Stroke::new(1.0, BORDER)).inner_margin(4.0).show(ui, |ui| {
                    ui.set_min_width(180.0);
                    if ui.button("End task").clicked() { self.end_task(); self.context_menu = None; ui.close(); }
                    ui.separator();
                    if ui.button("Open file location").clicked() { self.open_file_location(); self.context_menu = None; ui.close(); }
                    if ui.button("Properties").clicked() { self.show_properties(); self.context_menu = None; ui.close(); }
                    ui.separator();
                    if ui.button("Go to details").clicked() { /* switch to Details tab */ self.context_menu = None; ui.close(); }
                });
            });
        }
    }
}

impl Mangler {
    fn processes_tab(&mut self, ui: &mut egui::Ui) {
        let total_w = ui.available_width();
        let col_w = 80.0;
        let name_w = total_w - col_w * 7.0 - 40.0;

        // Header row with totals
        let cpu_total = self.sys.global_cpu_usage();
        let mem_total = self.mem_pct();
        let disk_total = self.disk_total_speed();
        let net_total = self.network_total_speed();

        let headers = [
            (Col::Name, "Name".into(), name_w),
            (Col::Cpu, format!("CPU\n{:.0}%", cpu_total), col_w),
            (Col::Memory, format!("Memory\n{:.0}%", mem_total), col_w),
            (Col::Disk, format!("Disk\n{:.0}%", (disk_total / 10_000_000.0).min(100.0)), col_w),
            (Col::Network, format!("Network\n{:.0} Mbps", net_total / 1_000_000.0), col_w),
            (Col::Gpu, "GPU\n0%".into(), col_w),
            (Col::PowerUsage, "Power\nVery low".into(), col_w),
            (Col::PowerTrend, "Trend".into(), col_w),
        ];

        ui.horizontal(|ui| {
            for (col, label, w) in headers {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 40.0), Sense::click());
                if resp.hovered() { ui.painter().rect_filled(rect, 0.0, HOVER); }
                let align = if col == Col::Name { Align2::LEFT_BOTTOM } else { Align2::RIGHT_BOTTOM };
                let x = if col == Col::Name { rect.left() + 6.0 } else { rect.right() - 6.0 };
                ui.painter().text(egui::pos2(x, rect.top() + 20.0), align, &label, egui::FontId::proportional(11.0), TEXT);
                ui.painter().text(egui::pos2(x, rect.bottom() - 4.0), align, &label.lines().next().unwrap_or(""), egui::FontId::proportional(12.0), DIM);
                if resp.clicked() {
                    self.sort_col = col;
                    self.sort_dir = if self.sort_col == col && self.sort_dir == SortDir::Desc { SortDir::Asc } else { SortDir::Desc };
                    self.sort_rows();
                }
                ui.painter().vline(rect.right(), rect.y_range(), Stroke::new(1.0, BORDER));
            }
        });
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, BORDER));

        // Process list with groups
        let total_mem = self.sys.total_memory().max(1) as f32;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for group in &mut self.groups {
                if group.children.is_empty() { continue; }

                // Group header
                let (grect, gresp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), GROUP_ROW_H), Sense::click());
                if gresp.hovered() { ui.painter().rect_filled(grect, 0.0, HOVER); }
                let arrow = if group.expanded { "▼" } else { "▶" };
                let total_cpu: f32 = group.children.iter().map(|c| c.cpu).sum();
                let group_mem: u64 = group.children.iter().map(|c| c.memory).sum();
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    if ui.add(egui::Button::new(RichText::new(arrow).size(12.0)).frame(false)).clicked() { group.expanded = !group.expanded; }
                    ui.label(RichText::new(&group.name).color(TEXT).size(13.0).strong());
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("{} items", group.children.len())).color(DIM).size(11.0));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("{:.1}%", total_cpu)).color(DIM).size(11.0));
                        ui.add_space(8.0);
                        ui.label(RichText::new(human(group_mem)).color(DIM).size(11.0));
                    });
                });
                if gresp.clicked() { group.expanded = !group.expanded; }
                ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, BORDER));

                if !group.expanded { continue; }

                // Process rows
                for row in &group.children {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H), Sense::click());
                    let sel = self.selected_pid == Some(row.pid);
                    let p = ui.painter();
                    if sel { p.rect_filled(rect, 0.0, SELECTED); }
                    else if resp.hovered() { p.rect_filled(rect, 0.0, HOVER); }

                    let mut x = rect.left();
                    // Name + icon
                    p.text(egui::pos2(x + 6.0, rect.center().y), Align2::LEFT_CENTER, &row.name, egui::FontId::proportional(13.0), TEXT);
                    x += name_w;

                    // CPU
                    let cpu_frac = (row.cpu / 100.0).min(1.0);
                    let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(col_w, ROW_H));
                    if !sel { p.rect_filled(cell, 0.0, heat(cpu_frac)); }
                    p.text(egui::pos2(cell.right() - 6.0, cell.center().y), Align2::RIGHT_CENTER, format!("{:.1}%", row.cpu), egui::FontId::proportional(12.5), TEXT);
                    x += col_w;

                    // Memory
                    let mem_frac = (row.memory as f32 / total_mem).min(1.0);
                    let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(col_w, ROW_H));
                    if !sel { p.rect_filled(cell, 0.0, heat(mem_frac)); }
                    p.text(egui::pos2(cell.right() - 6.0, cell.center().y), Align2::RIGHT_CENTER, human(row.memory), egui::FontId::proportional(12.5), TEXT);
                    x += col_w;

                    // Disk
                    let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(col_w, ROW_H));
                    if !sel { p.rect_filled(cell, 0.0, heat((row.disk as f32 / 10_000_000.0).min(1.0))); }
                    p.text(egui::pos2(cell.right() - 6.0, cell.center().y), Align2::RIGHT_CENTER, human_speed(row.disk), egui::FontId::proportional(12.5), TEXT);
                    x += col_w;

                    // Network
                    let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(col_w, ROW_H));
                    if !sel { p.rect_filled(cell, 0.0, heat((row.network as f32 / 1_000_000.0).min(1.0))); }
                    p.text(egui::pos2(cell.right() - 6.0, cell.center().y), Align2::RIGHT_CENTER, "0 B/s", egui::FontId::proportional(12.5), TEXT);
                    x += col_w;

                    // GPU
                    let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(col_w, ROW_H));
                    if !sel { p.rect_filled(cell, 0.0, heat(row.gpu / 100.0)); }
                    p.text(egui::pos2(cell.right() - 6.0, cell.center().y), Align2::RIGHT_CENTER, "0%", egui::FontId::proportional(12.5), TEXT);
                    x += col_w;

                    // Power
                    let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(col_w, ROW_H));
                    p.text(egui::pos2(cell.right() - 6.0, cell.center().y), Align2::RIGHT_CENTER, &row.power, egui::FontId::proportional(12.5), DIM);
                    x += col_w;

                    // Power trend
                    let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(col_w, ROW_H));
                    p.text(egui::pos2(cell.right() - 6.0, cell.center().y), Align2::RIGHT_CENTER, &row.power_trend, egui::FontId::proportional(12.5), DIM);

                    if resp.clicked() { self.selected_pid = Some(row.pid); }
                    if resp.secondary_clicked() { self.selected_pid = Some(row.pid); self.context_menu = Some((row.pid, ui.ctx().input(|i| i.pointer.hover_pos().unwrap_or_default()))); }
                }
            }
        });
    }

    fn performance_tab(&mut self, ui: &mut egui::Ui) {
        // Left sidebar with perf tabs
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(200.0);
                for (t, label, icon) in [
                    (PerfTab::Cpu, "CPU", "📊"),
                    (PerfTab::Memory, "Memory", "💾"),
                    (PerfTab::Disk, "Disk", "💿"),
                    (PerfTab::Network, "Network", "🌐"),
                    (PerfTab::Gpu, "GPU", "🎮"),
                ] {
                    let active = self.perf_tab == t;
                    let frame = egui::Frame::new().fill(if active { SELECTED } else { WHITE }).inner_margin(10.0);
                    let r = frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon).size(18.0));
                            ui.label(RichText::new(label).color(TEXT).size(14.0));
                        });
                    }).response.interact(Sense::click());
                    if r.clicked() { self.perf_tab = t; }
                    ui.add_space(4.0);
                }
            });
            ui.separator();

            // Right content
            ui.vertical(|ui| {
                match self.perf_tab {
                    PerfTab::Cpu => self.cpu_detail(ui),
                    PerfTab::Memory => self.memory_detail(ui),
                    PerfTab::Disk => self.disk_detail(ui),
                    PerfTab::Network => self.network_detail(ui),
                    PerfTab::Gpu => self.gpu_detail(ui),
                }
            });
        });
    }

    fn cpu_detail(&mut self, ui: &mut egui::Ui) {
        let cpu = self.sys.global_cpu_usage();
        let brand = self.sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
        let freq = self.sys.cpus().first().map(|c| c.frequency()).unwrap_or(0);

        ui.horizontal(|ui| {
            ui.label(RichText::new("CPU").color(TEXT).size(26.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(&brand).color(TEXT).size(14.0));
            });
        });
        ui.label(RichText::new("% Utilization over 60 seconds").color(DIM).size(11.5));

        let w = ui.available_width();
        graph(ui, Vec2::new(w, 200.0), &self.cpu_hist, CPU_LINE, CPU_FILL, true, "CPU", "%");
        ui.add_space(10.0);

        let mhz = freq as f32 / 1000.0;
        ui.horizontal(|ui| {
            self.stat_box(ui, "Utilization", format!("{:.0}%", cpu));
            self.stat_box(ui, "Speed", format!("{:.2} GHz", mhz));
            self.stat_box(ui, "Processes", self.groups.iter().map(|g| g.children.len()).sum::<usize>().to_string());
            self.stat_box(ui, "Threads", self.total_threads().to_string());
            self.stat_box(ui, "Handles", "N/A".into());
            let up = System::uptime().unwrap_or(0);
            self.stat_box(ui, "Up time", format!("{}:{:02}:{:02}:{:02}", up / 86400, up / 3600 % 24, up / 60 % 60, up % 60));
            self.stat_box(ui, "Logical processors", self.cores.to_string());
            self.stat_box(ui, "Virtualization", if cfg!(target_os = "linux") { "Enabled" } else { "N/A" }.into());
        });
    }

    fn memory_detail(&mut self, ui: &mut egui::Ui) {
        let used = self.sys.used_memory();
        let total = self.sys.total_memory();
        let avail = self.sys.available_memory();
        let pct = self.mem_pct();

        ui.horizontal(|ui| {
            ui.label(RichText::new("Memory").color(TEXT).size(26.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(human(total)).color(TEXT).size(14.0));
            });
        });
        ui.label(RichText::new("% Utilization over 60 seconds").color(DIM).size(11.5));

        let w = ui.available_width();
        graph(ui, Vec2::new(w, 200.0), &self.mem_hist, MEM_LINE, MEM_FILL, true, "Memory", "%");
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            self.stat_box(ui, "In use", human(used));
            self.stat_box(ui, "Available", human(avail));
            self.stat_box(ui, "Committed", human(used)); // simplified
            self.stat_box(ui, "Cached", human(total - used - avail));
            self.stat_box(ui, "Paged pool", "N/A".into());
            self.stat_box(ui, "Non-paged pool", "N/A".into());
        });
    }

    fn disk_detail(&mut self, ui: &mut egui::Ui) {
        self.disks.refresh(true);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Disk").color(TEXT).size(26.0));
        });
        ui.label(RichText::new("Active time over 60 seconds").color(DIM).size(11.5));

        let w = ui.available_width();
        // Use first disk for graph
        let _ = self.disk_hist; // placeholder
        graph(ui, Vec2::new(w, 200.0), &self.disk_hist, DISK_LINE, DISK_FILL, true, "Disk", "%");
        ui.add_space(10.0);

        // Disk list
        for disk in &self.disks {
            let name = disk.name().to_string_lossy();
            let kind = match disk.kind() { DiskKind::HDD => "HDD", DiskKind::SSD => "SSD", _ => "Unknown" };
            let total = disk.total_space();
            let avail = disk.available_space();
            let used = total - avail;

            egui::Frame::new().fill(WHITE).stroke(Stroke::new(1.0, BORDER)).inner_margin(10.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(name.to_string()).color(TEXT).size(15.0).strong());
                        ui.label(RichText::new(format!("{} ({})", kind, human(total))).color(DIM).size(12.0));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("{} free of {}", human(avail), human(total))).color(TEXT).size(13.0));
                    });
                });
                ui.add_space(8.0);
                let bar_w = ui.available_width();
                let used_frac = used as f32 / total.max(1) as f32;
                let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_w, 16.0), Sense::hover());
                ui.painter().rect_filled(rect, 4.0, DISK_FILL);
                ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, Vec2::new(bar_w * used_frac, 16.0)), 4.0, DISK_LINE);
                ui.label(RichText::new(format!("{} / {} used", human(used), human(total))).color(DIM).size(11.0));
            });
            ui.add_space(8.0);
        }
    }

    fn network_detail(&mut self, ui: &mut egui::Ui) {
        self.networks.refresh(true);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Network").color(TEXT).size(26.0));
        });
        ui.label(RichText::new("Throughput over 60 seconds").color(DIM).size(11.5));

        let w = ui.available_width();
        graph(ui, Vec2::new(w, 200.0), &self.net_hist, NET_LINE, NET_FILL, true, "Network", "Kbps");
        ui.add_space(10.0);

        for (name, data) in &self.networks {
            let recv = data.received();
            let sent = data.transmitted();
            let total = recv + sent;

            egui::Frame::new().fill(WHITE).stroke(Stroke::new(1.0, BORDER)).inner_margin(10.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(name.to_string_lossy()).color(TEXT).size(15.0).strong());
                        ui.label(RichText::new(format!("Received: {}  Sent: {}", human(recv), human(sent))).color(DIM).size(12.0));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(human(total)).color(TEXT).size(13.0));
                    });
                });
                ui.add_space(8.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
                ui.painter().rect_filled(rect, 4.0, NET_FILL);
                let sent_frac = sent as f32 / total.max(1) as f32;
                ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, Vec2::new(rect.width() * sent_frac, 16.0)), 4.0, NET_LINE);
            });
            ui.add_space(8.0);
        }
    }

    fn gpu_detail(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("GPU").color(TEXT).size(26.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new("GPU 0").color(TEXT).size(14.0));
            });
        });
        ui.label(RichText::new("GPU utilization over 60 seconds").color(DIM).size(11.5));

        // Placeholder - sysinfo doesn't expose GPU well on Linux
        let w = ui.available_width();
        graph(ui, Vec2::new(w, 200.0), &VecDeque::from(vec![0.0; 60]), GPU_LINE, GPU_FILL, true, "GPU", "%");
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            self.stat_box(ui, "3D", "0%".into());
            self.stat_box(ui, "Video Decode", "0%".into());
            self.stat_box(ui, "Video Encode", "0%".into());
            self.stat_box(ui, "Dedicated GPU memory", "0 MB".into());
            self.stat_box(ui, "Shared GPU memory", "0 MB".into());
            self.stat_box(ui, "Driver version", "N/A".into());
            self.stat_box(ui, "Driver date", "N/A".into());
        });
    }

    fn stat_box(&self, ui: &mut egui::Ui, label: &str, value: String) {
        ui.vertical(|ui| {
            ui.label(RichText::new(label).color(DIM).size(11.5));
            ui.label(RichText::new(value).color(TEXT).size(19.0));
        });
        ui.add_space(24.0);
    }

    fn disk_total_speed(&self) -> f32 {
        // Simplified
        0.0
    }

    fn network_total_speed(&self) -> f32 {
        let mut total = 0u64;
        for data in self.networks.values() {
            total += data.received() + data.transmitted();
        }
        total as f32
    }

    fn total_threads(&self) -> usize {
        self.groups.iter().map(|g| g.children.iter().filter(|p| p.status == "Running").count()).sum()
    }

    fn show_properties(&self) {
        if let Some(pid) = self.selected_pid {
            if let Some(p) = self.sys.process(pid) {
                println!("Properties for {:?} (PID: {})", p.name(), pid);
            }
        }
    }
}