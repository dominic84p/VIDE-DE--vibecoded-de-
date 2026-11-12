//! Task Mangler: VIDE-DE's task manager, modelled on the Windows 10 Task Manager.
//! Processes tab (sortable, with totals in the headers, End task) and a
//! Performance tab (CPU / Memory with live graphs). Opened with Ctrl+Shift+Esc.

use std::{collections::VecDeque, time::{Duration, Instant}};

use eframe::egui::{self, Color32, RichText, Sense, Stroke, Vec2};
use sysinfo::{Pid, ProcessesToUpdate, Signal, System};

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
/// Heat-map cells like Windows: the busier, the more yellow/orange.
const HEAT: [Color32; 4] = [
    Color32::from_rgb(0xff, 0xf4, 0xc4),
    Color32::from_rgb(0xff, 0xe9, 0x9c),
    Color32::from_rgb(0xff, 0xd2, 0x6b),
    Color32::from_rgb(0xff, 0xa5, 0x4a),
];
const HISTORY: usize = 60;
const ROW_H: f32 = 24.0;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Task Mangler")
            .with_app_id("task-mangler")
            .with_inner_size([820.0, 620.0])
            .with_min_inner_size([480.0, 360.0]),
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
}

#[derive(Clone, Copy, PartialEq)]
enum Col {
    Name,
    Cpu,
    Memory,
    Disk,
}

#[derive(Clone, Copy, PartialEq)]
enum Graph {
    Cpu,
    Memory,
}

struct Row {
    pid: Pid,
    name: String,
    cpu: f32,
    memory: u64,
    disk: u64,
}

struct Mangler {
    sys: System,
    rows: Vec<Row>,
    last: Instant,
    tab: Tab,
    sort: (Col, bool),
    selected: Option<Pid>,
    graph: Graph,
    cpu_hist: VecDeque<f32>,
    mem_hist: VecDeque<f32>,
    cores: usize,
    status: Option<String>,
}

impl Mangler {
    fn new() -> Self {
        let sys = System::new_all().expect("can't read system information from /proc");
        let cores = sys.cpus().len().max(1);
        let mut m = Self {
            sys,
            rows: Vec::new(),
            last: Instant::now() - Duration::from_secs(10),
            tab: Tab::Processes,
            sort: (Col::Cpu, false),
            selected: None,
            graph: Graph::Cpu,
            cpu_hist: VecDeque::from(vec![0.0; HISTORY]),
            mem_hist: VecDeque::from(vec![0.0; HISTORY]),
            cores,
            status: None,
        };
        m.refresh();
        m
    }

    fn refresh(&mut self) {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.sys.refresh_processes(ProcessesToUpdate::All, true);
        let cores = self.cores as f32;
        self.rows = self
            .sys
            .processes()
            .iter()
            .filter(|(_, p)| p.thread_kind().is_none())
            .map(|(pid, p)| Row {
                pid: *pid,
                name: p.name().to_string_lossy().into_owned(),
                // Per-process CPU is per core; Task Manager shows share of the whole machine.
                cpu: p.cpu_usage() / cores,
                memory: p.memory(),
                disk: { let d = p.disk_usage(); d.read_bytes + d.written_bytes },
            })
            .collect();
        self.sort_rows();
        push(&mut self.cpu_hist, self.sys.global_cpu_usage());
        let mem = self.mem_pct();
        push(&mut self.mem_hist, mem);
        self.last = Instant::now();
    }

    fn mem_pct(&self) -> f32 {
        self.sys.used_memory() as f32 / self.sys.total_memory().max(1) as f32 * 100.0
    }

    fn sort_rows(&mut self) {
        let (col, asc) = self.sort;
        self.rows.sort_by(|a, b| {
            let o = match col {
                Col::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                Col::Cpu => a.cpu.total_cmp(&b.cpu),
                Col::Memory => a.memory.cmp(&b.memory),
                Col::Disk => a.disk.cmp(&b.disk),
            };
            if asc { o } else { o.reverse() }
        });
    }

    /// SIGTERM, then SIGKILL if it's still there after two seconds.
    fn end_task(&mut self) {
        let Some(pid) = self.selected else { return };
        if pid.as_u32() == std::process::id() {
            return;
        }
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
        self.selected = None;
    }
}

fn push(h: &mut VecDeque<f32>, v: f32) {
    h.push_back(v);
    while h.len() > HISTORY {
        h.pop_front();
    }
}

fn human(b: u64) -> String {
    let mb = b as f64 / 1024.0 / 1024.0;
    if mb >= 1024.0 { format!("{:.1} GB", mb / 1024.0) } else { format!("{mb:.1} MB") }
}

fn heat(fraction: f32) -> Color32 {
    let i = ((fraction * 4.0) as usize).min(3);
    if fraction < 0.02 { WHITE } else { HEAT[i] }
}

/// A Task-Manager-style area graph: grid, filled area, line on top.
fn graph(ui: &mut egui::Ui, size: Vec2, hist: &VecDeque<f32>, line: Color32, fill: Color32, grid: bool) -> egui::Response {
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
    resp
}

impl eframe::App for Mangler {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut v = egui::Visuals::light();
        v.panel_fill = WHITE;
        ctx.set_visuals(v);
        if self.last.elapsed() >= Duration::from_secs(1) {
            self.refresh();
        }
        ctx.request_repaint_after(Duration::from_millis(500));

        // Tabs, Windows-style: plain text, the active one underlined.
        egui::Panel::top("tabs").frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| {
            ui.horizontal(|ui| {
                for (t, label) in [(Tab::Processes, "Processes"), (Tab::Performance, "Performance")] {
                    let active = self.tab == t;
                    let r = ui.add(egui::Button::new(RichText::new(label).color(TEXT).size(13.0)).frame(false));
                    if active {
                        ui.painter().hline(r.rect.x_range(), r.rect.bottom() + 2.0, Stroke::new(2.0, CPU_LINE));
                    }
                    if r.clicked() {
                        self.tab = t;
                    }
                    ui.add_space(8.0);
                }
            });
            ui.painter().hline(ui.max_rect().x_range(), ui.min_rect().bottom() + 4.0, Stroke::new(1.0, BORDER));
        });

        egui::Panel::bottom("footer").frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin::symmetric(10, 8))).show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(s) = &self.status {
                    ui.label(RichText::new(s).color(DIM).size(12.0));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.tab == Tab::Processes && ui.add_enabled(self.selected.is_some(), egui::Button::new("End task").min_size(Vec2::new(90.0, 26.0))).clicked() {
                        self.end_task();
                    }
                });
            });
        });

        egui::CentralPanel::default().frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin::symmetric(8, 4))).show(ui, |ui| match self.tab {
            Tab::Processes => self.processes(ui),
            Tab::Performance => self.performance(ui),
        });
    }
}

impl Mangler {
    fn processes(&mut self, ui: &mut egui::Ui) {
        let total_w = ui.available_width();
        let num_w = 90.0;
        // Leave room for the scrollbar so the last column isn't clipped.
        let name_w = total_w - num_w * 3.0 - 28.0;
        let cpu = self.sys.global_cpu_usage();
        let mem = self.mem_pct();
        let headers = [
            (Col::Name, String::new(), String::new(), name_w),
            (Col::Cpu, format!("{cpu:.0}%"), "CPU".into(), num_w),
            (Col::Memory, format!("{mem:.0}%"), "Memory".into(), num_w),
            (Col::Disk, "".into(), "Disk".into(), num_w),
        ];
        // Header: big total over the column name, like Windows.
        ui.horizontal(|ui| {
            for (col, big, small, w) in headers {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 40.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 0.0, HOVER);
                }
                let align = if col == Col::Name { egui::Align2::LEFT_BOTTOM } else { egui::Align2::RIGHT_BOTTOM };
                let x = if col == Col::Name { rect.left() + 6.0 } else { rect.right() - 6.0 };
                if !big.is_empty() {
                    ui.painter().text(egui::pos2(x, rect.top() + 20.0), align, &big, egui::FontId::proportional(17.0), TEXT);
                }
                let label = if col == Col::Name { "Name".to_string() } else { small };
                ui.painter().text(egui::pos2(x, rect.bottom() - 4.0), align, &label, egui::FontId::proportional(12.0), DIM);
                if resp.clicked() {
                    self.sort = if self.sort.0 == col { (col, !self.sort.1) } else { (col, col == Col::Name) };
                    self.sort_rows();
                }
                ui.painter().vline(rect.right(), rect.y_range(), Stroke::new(1.0, BORDER));
            }
        });
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, BORDER));

        let total_mem = self.sys.total_memory().max(1) as f32;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, ROW_H, self.rows.len(), |ui, range| {
            for r in &self.rows[range] {
                let (rect, resp) = ui.allocate_exact_size(Vec2::new(total_w, ROW_H), Sense::click());
                let sel = self.selected == Some(r.pid);
                let p = ui.painter();
                if sel {
                    p.rect_filled(rect, 0.0, SELECTED);
                } else if resp.hovered() {
                    p.rect_filled(rect, 0.0, HOVER);
                }
                let mut x = rect.left();
                p.text(egui::pos2(x + 6.0, rect.center().y), egui::Align2::LEFT_CENTER, &r.name, egui::FontId::proportional(13.0), TEXT);
                x += name_w;
                for (frac, text) in [
                    (r.cpu / 100.0, format!("{:.1}%", r.cpu)),
                    (r.memory as f32 / total_mem * 8.0, human(r.memory)),
                    ((r.disk as f32 / 10_000_000.0).min(1.0), format!("{:.1} MB/s", r.disk as f32 / 1_048_576.0)),
                ] {
                    let cell = egui::Rect::from_min_size(egui::pos2(x, rect.top()), Vec2::new(num_w, ROW_H));
                    if !sel {
                        p.rect_filled(cell, 0.0, heat(frac));
                    }
                    p.text(egui::pos2(cell.right() - 6.0, cell.center().y), egui::Align2::RIGHT_CENTER, text, egui::FontId::proportional(12.5), TEXT);
                    x += num_w;
                }
                if resp.clicked() {
                    self.selected = Some(r.pid);
                }
            }
        });
    }

    fn performance(&mut self, ui: &mut egui::Ui) {
        let cpu = self.sys.global_cpu_usage();
        let mem = self.mem_pct();
        let brand = self.sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
        ui.horizontal_top(|ui| {
            // Left list: small graph + label per resource.
            ui.vertical(|ui| {
                ui.set_width(200.0);
                for (g, title, sub, hist, line, fill) in [
                    (Graph::Cpu, "CPU", format!("{cpu:.0}%"), &self.cpu_hist, CPU_LINE, CPU_FILL),
                    (Graph::Memory, "Memory", format!("{}/{} ({mem:.0}%)", human(self.sys.used_memory()), human(self.sys.total_memory())), &self.mem_hist, MEM_LINE, MEM_FILL),
                ] {
                    let selected = self.graph == g;
                    let frame = egui::Frame::new().fill(if selected { SELECTED } else { WHITE }).inner_margin(6.0);
                    let r = frame
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                graph(ui, Vec2::new(70.0, 46.0), hist, line, fill, false);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(title).color(TEXT).size(15.0));
                                    ui.label(RichText::new(sub).color(DIM).size(11.5));
                                });
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if r.clicked() {
                        self.graph = g;
                    }
                }
            });
            ui.separator();
            // Right: the big graph and stats.
            ui.vertical(|ui| {
                let (title, right, hist, line, fill) = match self.graph {
                    Graph::Cpu => ("CPU", brand.clone(), &self.cpu_hist, CPU_LINE, CPU_FILL),
                    Graph::Memory => ("Memory", human(self.sys.total_memory()), &self.mem_hist, MEM_LINE, MEM_FILL),
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).color(TEXT).size(26.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(right).color(TEXT).size(14.0));
                    });
                });
                ui.label(RichText::new("% Utilization over 60 seconds").color(DIM).size(11.5));
                let w = ui.available_width();
                graph(ui, Vec2::new(w, (ui.available_height() - 120.0).max(140.0)), hist, line, fill, true);
                ui.add_space(10.0);
                let stat = |ui: &mut egui::Ui, k: &str, v: String| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(k).color(DIM).size(11.5));
                        ui.label(RichText::new(v).color(TEXT).size(19.0));
                    });
                    ui.add_space(24.0);
                };
                ui.horizontal(|ui| match self.graph {
                    Graph::Cpu => {
                        stat(ui, "Utilization", format!("{cpu:.0}%"));
                        let mhz = self.sys.cpus().first().map(|c| c.frequency()).unwrap_or(0);
                        stat(ui, "Speed", format!("{:.2} GHz", mhz as f32 / 1000.0));
                        stat(ui, "Processes", self.rows.len().to_string());
                        stat(ui, "Logical processors", self.cores.to_string());
                        let up = System::uptime().unwrap_or(0);
                        stat(ui, "Up time", format!("{}:{:02}:{:02}:{:02}", up / 86400, up / 3600 % 24, up / 60 % 60, up % 60));
                    }
                    Graph::Memory => {
                        stat(ui, "In use", human(self.sys.used_memory()));
                        stat(ui, "Available", human(self.sys.available_memory()));
                        stat(ui, "Total", human(self.sys.total_memory()));
                    }
                });
            });
        });
    }
}
