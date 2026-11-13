//! Task Mangler: VIDE-DE's task manager, a copy of the Windoze 10 Task Manager.
//!
//! File / Options / View menus and the seven tabs: Processes (Apps, Background and
//! Windoze processes with the heat map and totals in the headers), Performance (CPU,
//! Memory, disks, network adapters and the GPU, each with its graph and details),
//! App history, Startup (XDG autostart), Users, Details and Services (systemd).
//! "Fewer details" shrinks it to the list of apps. Opened with Ctrl+Shift+Esc.
//!
//! Apps are processes that own a window: the compositor lists them over `$VIDE_IPC`.

use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    io::{Read, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant, SystemTime},
};

use eframe::egui::{self, Align, Align2, Color32, FontId, Id, Key, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use sysinfo::{Disks, Networks, Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, Signal, System, UpdateKind, Users};

const WHITE: Color32 = Color32::WHITE;
const BORDER: Color32 = Color32::from_rgb(0xe5, 0xe5, 0xe5);
const TAB_LINE: Color32 = Color32::from_rgb(0xd9, 0xd9, 0xd9);
const HOVER: Color32 = Color32::from_rgb(0xe5, 0xf3, 0xff);
const SELECTED: Color32 = Color32::from_rgb(0xcc, 0xe8, 0xff);
const SELECTED_LINE: Color32 = Color32::from_rgb(0x99, 0xd1, 0xff);
const TEXT: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
const DIM: Color32 = Color32::from_rgb(0x6d, 0x6d, 0x6d);
const FAINT: Color32 = Color32::from_rgb(0xa0, 0xa0, 0xa0);
const GROUP_BLUE: Color32 = Color32::from_rgb(0x1e, 0x39, 0x91);
const LINK_BLUE: Color32 = Color32::from_rgb(0x00, 0x66, 0xcc);
const CPU_LINE: Color32 = Color32::from_rgb(0x11, 0x7d, 0xbb);
const CPU_FILL: Color32 = Color32::from_rgb(0xf1, 0xf6, 0xfa);
const CPU_GRID: Color32 = Color32::from_rgb(0xd9, 0xea, 0xf4);
const MEM_LINE: Color32 = Color32::from_rgb(0x8b, 0x12, 0xae);
const MEM_FILL: Color32 = Color32::from_rgb(0xf4, 0xf2, 0xf4);
const MEM_GRID: Color32 = Color32::from_rgb(0xec, 0xde, 0xf0);
const DISK_LINE: Color32 = Color32::from_rgb(0x4d, 0xa6, 0x0c);
const DISK_FILL: Color32 = Color32::from_rgb(0xef, 0xf7, 0xe9);
const DISK_GRID: Color32 = Color32::from_rgb(0xdb, 0xed, 0xce);
const NET_LINE: Color32 = Color32::from_rgb(0xa7, 0x4f, 0x01);
const NET_FILL: Color32 = Color32::from_rgb(0xfc, 0xf3, 0xeb);
const NET_GRID: Color32 = Color32::from_rgb(0xf2, 0xdf, 0xcc);
/// The heat map, from idle (pale yellow) to busy (orange), as Task Manager paints it.
const HEAT: [(f32, Color32); 5] = [
    (0.00, Color32::from_rgb(0xff, 0xf4, 0xc4)),
    (0.10, Color32::from_rgb(0xff, 0xe9, 0xa5)),
    (0.30, Color32::from_rgb(0xff, 0xd6, 0x80)),
    (0.60, Color32::from_rgb(0xff, 0xb6, 0x4d)),
    (0.90, Color32::from_rgb(0xf9, 0x95, 0x2a)),
];
const HISTORY: usize = 61;
const ROW_H: f32 = 24.0;
const GROUP_H: f32 = 32.0;

fn main() -> eframe::Result {
    let settings = Settings::load();
    let size = if settings.fewer { [300.0, 320.0] } else { [1040.0, 700.0] };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Task Mangler")
            .with_app_id("task-mangler")
            .with_inner_size(size)
            .with_min_inner_size([260.0, 240.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Task Mangler",
        options,
        Box::new(move |cc| {
            vide_fonts::install_egui!(eframe::egui, cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            cc.egui_ctx.all_styles_mut(|s| s.interaction.selectable_labels = false);
            // Noto Sans is what the shell uses, and the nearest installed thing to Seg-whoa UI.
            if let Ok(data) = std::fs::read("/usr/share/fonts/noto/NotoSans-Regular.ttf") {
                cc.egui_ctx.add_font(egui::FontInsert::new(
                    "noto-sans",
                    egui::FontData::from_owned(data),
                    vec![egui::InsertFontFamily { family: egui::FontFamily::Proportional, priority: egui::FontPriority::Highest }],
                ));
            }
            Ok(Box::new(Mangler::new(settings)))
        }),
    )
}

// ---- Settings ----

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Processes,
    Performance,
    AppHistory,
    Startup,
    Users,
    Details,
    Services,
}

const TABS: [(Tab, &str); 7] = [
    (Tab::Processes, "Processes"),
    (Tab::Performance, "Performance"),
    (Tab::AppHistory, "App history"),
    (Tab::Startup, "Startup"),
    (Tab::Users, "Users"),
    (Tab::Details, "Details"),
    (Tab::Services, "Services"),
];

#[derive(Clone, Copy, PartialEq)]
enum Speed {
    High,
    Normal,
    Low,
    Paused,
}

impl Speed {
    fn interval(self) -> Option<Duration> {
        match self {
            Speed::High => Some(Duration::from_millis(500)),
            Speed::Normal => Some(Duration::from_secs(1)),
            Speed::Low => Some(Duration::from_secs(4)),
            Speed::Paused => None,
        }
    }
}

/// What Task Manager remembers between runs: ~/.config/vide-de/task-mangler.
struct Settings {
    fewer: bool,
    default_tab: Tab,
    on_top: bool,
    minimize_on_use: bool,
    speed: Speed,
    group: bool,
    cols: Vec<PCol>,
}

impl Settings {
    fn path() -> PathBuf {
        config_dir().join("vide-de/task-mangler")
    }

    fn load() -> Self {
        let mut s = Settings { fewer: false, default_tab: Tab::Processes, on_top: false, minimize_on_use: true, speed: Speed::Normal, group: true, cols: DEFAULT_COLS.to_vec() };
        let Ok(text) = std::fs::read_to_string(Self::path()) else { return s };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            match k {
                "fewer" => s.fewer = v == "1",
                "tab" => s.default_tab = TABS.iter().find(|(_, n)| *n == v).map(|(t, _)| *t).unwrap_or(Tab::Processes),
                "on_top" => s.on_top = v == "1",
                "minimize_on_use" => s.minimize_on_use = v == "1",
                "speed" => s.speed = match v { "high" => Speed::High, "low" => Speed::Low, "paused" => Speed::Paused, _ => Speed::Normal },
                "group" => s.group = v == "1",
                "cols" => s.cols = v.split(',').filter_map(|n| PCOLS.iter().find(|c| c.label == n).map(|c| c.col)).collect(),
                _ => {}
            }
        }
        if !s.cols.contains(&PCol::Name) {
            s.cols.insert(0, PCol::Name);
        }
        s
    }

    fn save(&self) {
        let speed = match self.speed { Speed::High => "high", Speed::Normal => "normal", Speed::Low => "low", Speed::Paused => "paused" };
        let tab = TABS.iter().find(|(t, _)| *t == self.default_tab).map(|(_, n)| *n).unwrap_or("Processes");
        let cols: Vec<&str> = self.cols.iter().filter_map(|c| PCOLS.iter().find(|p| p.col == *c)).map(|p| p.label).collect();
        let text = format!(
            "fewer={}\ntab={tab}\non_top={}\nminimize_on_use={}\nspeed={speed}\ngroup={}\ncols={}\n",
            self.fewer as u8, self.on_top as u8, self.minimize_on_use as u8, self.group as u8, cols.join(",")
        );
        let p = Self::path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, text);
    }
}

// ---- Processes tab columns ----

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum PCol {
    Name,
    Type,
    Status,
    Publisher,
    Pid,
    ProcName,
    CmdLine,
    Cpu,
    Memory,
    Disk,
    Network,
    Gpu,
    GpuEngine,
    Power,
    PowerTrend,
}

struct PColInfo {
    col: PCol,
    label: &'static str,
    width: f32,
    /// Numbers are right-aligned, heat-mapped and sort high-to-low first.
    numeric: bool,
}

const PCOLS: [PColInfo; 15] = [
    PColInfo { col: PCol::Name, label: "Name", width: 0.0, numeric: false },
    PColInfo { col: PCol::Type, label: "Type", width: 130.0, numeric: false },
    PColInfo { col: PCol::Status, label: "Status", width: 90.0, numeric: false },
    PColInfo { col: PCol::Publisher, label: "Publisher", width: 120.0, numeric: false },
    PColInfo { col: PCol::Pid, label: "PID", width: 64.0, numeric: false },
    PColInfo { col: PCol::ProcName, label: "Process name", width: 130.0, numeric: false },
    PColInfo { col: PCol::CmdLine, label: "Command line", width: 260.0, numeric: false },
    PColInfo { col: PCol::Cpu, label: "CPU", width: 70.0, numeric: true },
    PColInfo { col: PCol::Memory, label: "Memory", width: 92.0, numeric: true },
    PColInfo { col: PCol::Disk, label: "Disk", width: 80.0, numeric: true },
    PColInfo { col: PCol::Network, label: "Network", width: 80.0, numeric: true },
    PColInfo { col: PCol::Gpu, label: "GPU", width: 66.0, numeric: true },
    PColInfo { col: PCol::GpuEngine, label: "GPU engine", width: 104.0, numeric: false },
    PColInfo { col: PCol::Power, label: "Power usage", width: 96.0, numeric: true },
    PColInfo { col: PCol::PowerTrend, label: "Power usage trend", width: 120.0, numeric: true },
];

const DEFAULT_COLS: [PCol; 10] = [PCol::Name, PCol::Status, PCol::Cpu, PCol::Memory, PCol::Disk, PCol::Network, PCol::Gpu, PCol::GpuEngine, PCol::Power, PCol::PowerTrend];
const USER_COLS: [PCol; 6] = [PCol::Name, PCol::Status, PCol::Cpu, PCol::Memory, PCol::Disk, PCol::Network];

fn pcol(c: PCol) -> &'static PColInfo {
    PCOLS.iter().find(|p| p.col == c).unwrap()
}

// ---- Data ----

/// One process, as of the last refresh.
#[derive(Clone)]
struct Proc {
    pid: u32,
    ppid: u32,
    name: String,
    exe: Option<PathBuf>,
    cmd: String,
    uid: u32,
    /// Share of the whole machine, like Task Manager (sysinfo's is per core).
    cpu: f32,
    mem: u64,
    /// Bytes per second.
    disk: f64,
    gpu: f32,
    threads: usize,
    status: ProcessStatus,
    cpu_ms: u64,
}

#[derive(Clone, Copy, Default)]
struct Stats {
    cpu: f32,
    mem: u64,
    disk: f64,
    net: f64,
    gpu: f32,
    power: f32,
    trend: f32,
}

impl Stats {
    fn add(&mut self, o: &Stats) {
        self.cpu += o.cpu;
        self.mem += o.mem;
        self.disk += o.disk;
        self.net += o.net;
        self.gpu += o.gpu;
        self.power += o.power;
        self.trend += o.trend;
    }
}

/// A window from the compositor: `windows` over the control socket.
#[derive(Clone)]
struct Win {
    id: u64,
    pid: u32,
    app_id: String,
    title: String,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum RowKey {
    Group(u8),
    App(u32),
    Proc(u32),
    Win(u64),
    /// Several processes of one program, folded like Task Manager does.
    Named(u8, String),
    User(u32),
}

#[derive(Clone)]
struct Row {
    key: RowKey,
    depth: u8,
    name: String,
    icon: Option<String>,
    /// Processes inside, shown as "(n)" when more than one.
    count: usize,
    expandable: bool,
    stats: Option<Stats>,
    status: String,
    kind: &'static str,
    pids: Vec<u32>,
    windows: Vec<u64>,
    pid: Option<u32>,
}

/// A data series for a graph, oldest first.
#[derive(Clone)]
struct Hist(VecDeque<f32>);

impl Hist {
    fn new() -> Self {
        Hist(VecDeque::from(vec![0.0; HISTORY]))
    }
    fn push(&mut self, v: f32) {
        self.0.push_back(v);
        while self.0.len() > HISTORY {
            self.0.pop_front();
        }
    }
    fn last(&self) -> f32 {
        self.0.back().copied().unwrap_or(0.0)
    }
    fn max(&self) -> f32 {
        self.0.iter().copied().fold(0.0, f32::max)
    }
}

struct DiskDev {
    dev: String,
    model: String,
    ssd: bool,
    size: u64,
    mounts: Vec<String>,
    formatted: u64,
    system: bool,
    page_file: bool,
    last: Option<(Instant, [u64; 6])>,
    active: Hist,
    read: Hist,
    write: Hist,
    resp_ms: f32,
}

struct NetDev {
    iface: String,
    wifi: bool,
    up: bool,
    speed_mbps: u64,
    ipv4: String,
    ipv6: String,
    send: Hist,
    recv: Hist,
}

#[derive(Clone, Default)]
struct GpuSample {
    name: String,
    util: f32,
    copy: f32,
    enc: f32,
    dec: f32,
    mem_used: u64,
    mem_total: u64,
    temp: Option<f32>,
    driver: String,
    bus: String,
    per_pid: HashMap<u32, f32>,
}

struct GpuDev {
    sample: GpuSample,
    util: Hist,
    copy: Hist,
    enc: Hist,
    dec: Hist,
    mem: Hist,
}

#[derive(Clone)]
struct Service {
    name: String,
    description: String,
    running: bool,
    pid: Option<u32>,
    group: String,
}

#[derive(Clone)]
struct StartupItem {
    name: String,
    publisher: String,
    enabled: bool,
    file: String,
    path: PathBuf,
    exec: String,
    icon: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum PerfPage {
    Cpu,
    Memory,
    Disk(usize),
    Net(usize),
    Gpu,
}

/// Sorting for the plain tables (Details, Services, Startup, App history).
#[derive(Clone)]
enum SortKey {
    T(String),
    N(f64),
}

struct TRow {
    id: u64,
    icon: Option<String>,
    cells: Vec<(String, SortKey)>,
}

#[derive(Default)]
struct TState {
    sort: Option<(usize, bool)>,
    selected: Option<u64>,
    scroll_to: Option<u64>,
}

struct Mangler {
    settings: Settings,
    sys: System,
    networks: Networks,
    users: HashMap<u32, String>,
    my_uid: u32,
    procs: HashMap<u32, Proc>,
    windows: Vec<Win>,
    apps: AppIndex,
    last: Instant,
    tick: u64,
    tab: Tab,
    // Processes and Users tabs.
    sort: (PCol, bool),
    expanded: HashSet<RowKey>,
    selected: Option<RowKey>,
    rows: Vec<Row>,
    user_rows: Vec<Row>,
    power_trend: HashMap<u32, f32>,
    net_total: f64,
    disk_total: f32,
    // Performance.
    page: PerfPage,
    logical: bool,
    cpu: Hist,
    cores: Vec<Hist>,
    mem: Hist,
    disks: Vec<DiskDev>,
    nets: Vec<NetDev>,
    gpu: Option<GpuDev>,
    gpu_feed: Arc<Mutex<Option<GpuSample>>>,
    cpu_info: CpuInfo,
    meminfo: HashMap<String, u64>,
    // Other tabs.
    details: TState,
    services: TState,
    service_list: Arc<Mutex<Option<Vec<Service>>>>,
    services_asked: Option<Instant>,
    startup: TState,
    startup_list: Vec<StartupItem>,
    bios_time: Arc<Mutex<Option<String>>>,
    history: TState,
    app_history: AppHistory,
    // Dialogs.
    run_dialog: Option<(String, bool)>,
    props: Option<u32>,
    status: Option<String>,
    scroll_to_row: Option<RowKey>,
}

#[derive(Default)]
struct CpuInfo {
    brand: String,
    base: String,
    sockets: usize,
    cores: usize,
    logical: usize,
    virt: &'static str,
    caches: [u64; 3],
}

impl Mangler {
    fn new(settings: Settings) -> Self {
        let sys = System::new_all().expect("can't read system information from /proc");
        let my_uid = unsafe { libc::getuid() };
        let users = Users::new_with_refreshed_list().map(|u| u.iter().map(|u| (**u.id(), u.name().to_string())).collect()).unwrap_or_default();
        let gpu_feed = Arc::new(Mutex::new(None));
        gpu_thread(gpu_feed.clone());
        let bios_time = Arc::new(Mutex::new(None));
        {
            let b = bios_time.clone();
            std::thread::spawn(move || *b.lock().unwrap() = bios_seconds());
        }
        let cores = sys.cpus().len().max(1);
        let tab = settings.default_tab;
        let sort_init = (PCol::Name, true);
        let mut m = Self {
            sys,
            networks: Networks::new_with_refreshed_list().unwrap_or_else(|_| Networks::new().unwrap()),
            users,
            my_uid,
            procs: HashMap::new(),
            windows: Vec::new(),
            apps: AppIndex::load(),
            last: Instant::now() - Duration::from_secs(10),
            tick: 0,
            tab,
            sort: sort_init,
            expanded: HashSet::new(),
            selected: None,
            rows: Vec::new(),
            user_rows: Vec::new(),
            power_trend: HashMap::new(),
            net_total: 0.0,
            disk_total: 0.0,
            page: PerfPage::Cpu,
            logical: false,
            cpu: Hist::new(),
            cores: vec![Hist::new(); cores],
            mem: Hist::new(),
            disks: disk_devices(),
            nets: Vec::new(),
            gpu: None,
            gpu_feed,
            cpu_info: CpuInfo::default(),
            meminfo: HashMap::new(),
            details: TState { sort: None, ..Default::default() },
            services: TState { sort: Some((0, true)), ..Default::default() },
            service_list: Arc::new(Mutex::new(None)),
            services_asked: None,
            startup: TState { sort: Some((0, true)), ..Default::default() },
            startup_list: Vec::new(),
            bios_time,
            history: TState { sort: Some((0, true)), ..Default::default() },
            app_history: AppHistory::load(),
            run_dialog: None,
            props: None,
            status: None,
            scroll_to_row: None,
            settings,
        };
        m.cpu_info = cpu_info(&m.sys);
        m.startup_list = startup_items(&m.apps);
        m.refresh();
        m
    }

    // ---- Refresh ----

    fn refresh(&mut self) {
        let dt = self.last.elapsed().as_secs_f64().clamp(0.1, 10.0);
        self.last = Instant::now();
        self.tick += 1;
        self.sys.refresh_cpu_all();
        self.sys.refresh_memory();
        self.sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_disk_usage()
                .with_tasks()
                .with_gpu_usage()
                .with_user(UpdateKind::OnlyIfNotSet)
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_cmd(UpdateKind::OnlyIfNotSet),
        );
        self.networks.refresh(true);
        self.windows = list_windows();
        self.meminfo = meminfo();

        let gpu_sample = self.gpu_feed.lock().unwrap().clone();
        let ncpu = self.sys.cpus().len().max(1) as f32;
        let gpu_pids = gpu_sample.as_ref().map(|g| g.per_pid.clone()).unwrap_or_default();
        self.procs = self
            .sys
            .processes()
            .iter()
            .filter(|(_, p)| p.thread_kind().is_none())
            .map(|(pid, p)| {
                let d = p.disk_usage();
                let pid = pid.as_u32();
                // The kernel cuts names at 15 characters; the executable has the full one.
                let mut name = p.name().to_string_lossy().into_owned();
                if name.len() == 15 {
                    let full = p.exe().and_then(Path::file_name).or_else(|| p.cmd().first().and_then(|c| Path::new(c).file_name()));
                    if let Some(f) = full.map(|f| f.to_string_lossy().into_owned()).filter(|f| f.starts_with(&name)) {
                        name = f;
                    }
                }
                let proc_ = Proc {
                    pid,
                    ppid: p.parent().map(|x| x.as_u32()).unwrap_or(0),
                    name,
                    exe: p.exe().map(Path::to_path_buf),
                    cmd: p.cmd().iter().map(|c| c.to_string_lossy()).collect::<Vec<_>>().join(" "),
                    uid: p.user_id().map(|u| **u).unwrap_or(0),
                    cpu: p.cpu_usage() / ncpu,
                    mem: p.memory(),
                    disk: (d.read_bytes + d.written_bytes) as f64 / dt,
                    gpu: p.gpu_usage().unwrap_or(0.0).max(gpu_pids.get(&pid).copied().unwrap_or(0.0)),
                    threads: p.tasks().map(|t| t.len()).unwrap_or(1),
                    status: p.status(),
                    cpu_ms: p.accumulated_cpu_time(),
                };
                (pid, proc_)
            })
            .collect();
        // Power: a CPU/GPU/disk score; the trend is that score smoothed over ~2 minutes.
        let alpha = (dt / 120.0).min(1.0) as f32;
        let mut trend = HashMap::new();
        for p in self.procs.values() {
            let score = power_score(p);
            let old = self.power_trend.get(&p.pid).copied().unwrap_or(score);
            trend.insert(p.pid, old + (score - old) * alpha);
        }
        self.power_trend = trend;

        // Performance.
        self.cpu.push(self.sys.global_cpu_usage());
        for (h, c) in self.cores.iter_mut().zip(self.sys.cpus()) {
            h.push(c.usage());
        }
        self.mem.push(self.sys.used_memory() as f32 / self.sys.total_memory().max(1) as f32 * 100.0);
        let stats = diskstats();
        for d in &mut self.disks {
            let Some(now) = stats.get(&d.dev).copied() else { continue };
            if let Some((t, prev)) = d.last {
                let ms = t.elapsed().as_secs_f64() * 1000.0;
                let delta = |i: usize| now[i].saturating_sub(prev[i]) as f64;
                d.active.push((delta(4) / ms * 100.0).min(100.0) as f32);
                d.read.push((delta(1) * 512.0 / ms * 1000.0) as f32);
                d.write.push((delta(3) * 512.0 / ms * 1000.0) as f32);
                let ios = delta(0) + delta(2);
                d.resp_ms = if ios > 0.0 { (delta(5) / ios) as f32 } else { 0.0 };
            }
            d.last = Some((Instant::now(), now));
        }
        self.disk_total = self.disks.iter().map(|d| d.active.last()).fold(0.0, f32::max);
        self.refresh_nets(dt);
        if let Some(g) = gpu_sample {
            let dev = self.gpu.get_or_insert_with(|| GpuDev { sample: GpuSample::default(), util: Hist::new(), copy: Hist::new(), enc: Hist::new(), dec: Hist::new(), mem: Hist::new() });
            dev.util.push(g.util);
            dev.copy.push(g.copy);
            dev.enc.push(g.enc);
            dev.dec.push(g.dec);
            dev.mem.push(g.mem_used as f32 / g.mem_total.max(1) as f32 * 100.0);
            dev.sample = g;
        }
        self.app_history.update(&self.procs, &self.apps, self.my_uid);
        self.build_rows();
        if self.tab == Tab::Services && self.services_asked.is_none_or(|t| t.elapsed() > Duration::from_secs(5)) {
            self.ask_services();
        }
    }

    fn refresh_nets(&mut self, dt: f64) {
        let mut nets: Vec<NetDev> = Vec::new();
        for (name, data) in self.networks.iter() {
            let n = name.to_string_lossy().into_owned();
            // Physical adapters only, like Task Manager: no loopback, bridges or tunnels.
            if !Path::new(&format!("/sys/class/net/{n}/device")).exists() {
                continue;
            }
            let mut dev = self.nets.iter().position(|d| d.iface == n).map(|i| self.nets.swap_remove(i)).unwrap_or_else(|| NetDev {
                wifi: Path::new(&format!("/sys/class/net/{n}/wireless")).exists(),
                iface: n.clone(),
                up: false,
                speed_mbps: 0,
                ipv4: String::new(),
                ipv6: String::new(),
                send: Hist::new(),
                recv: Hist::new(),
            });
            dev.up = std::fs::read_to_string(format!("/sys/class/net/{n}/operstate")).is_ok_and(|s| s.trim() == "up");
            dev.speed_mbps = std::fs::read_to_string(format!("/sys/class/net/{n}/speed")).ok().and_then(|s| s.trim().parse::<i64>().ok()).filter(|&s| s > 0).unwrap_or(0) as u64;
            dev.ipv4 = data.ip_networks().iter().find(|i| i.addr.is_ipv4()).map(|i| i.addr.to_string()).unwrap_or_default();
            dev.ipv6 = data.ip_networks().iter().find(|i| i.addr.is_ipv6()).map(|i| i.addr.to_string()).unwrap_or_default();
            dev.send.push((data.transmitted() as f64 * 8.0 / dt) as f32);
            dev.recv.push((data.received() as f64 * 8.0 / dt) as f32);
            nets.push(dev);
        }
        nets.sort_by(|a, b| b.up.cmp(&a.up).then(a.wifi.cmp(&b.wifi)).then(a.iface.cmp(&b.iface)));
        // Network column total: share of the fastest link that's up.
        let link = nets.iter().filter(|n| n.up).map(|n| n.speed_mbps).max().unwrap_or(0).max(100) as f64 * 1e6;
        let used: f64 = nets.iter().map(|n| (n.send.last() + n.recv.last()) as f64).sum();
        self.net_total = (used / link * 100.0).min(100.0);
        self.nets = nets;
        if let PerfPage::Net(i) = self.page
            && i >= self.nets.len()
        {
            self.page = PerfPage::Cpu;
        }
    }

    fn ask_services(&mut self) {
        self.services_asked = Some(Instant::now());
        let out = self.service_list.clone();
        std::thread::spawn(move || {
            let list = services();
            *out.lock().unwrap() = Some(list);
        });
    }

    fn proc_stats(&self, pid: u32) -> Stats {
        let Some(p) = self.procs.get(&pid) else { return Stats::default() };
        Stats { cpu: p.cpu, mem: p.mem, disk: p.disk, net: 0.0, gpu: p.gpu, power: power_score(p), trend: self.power_trend.get(&pid).copied().unwrap_or(0.0) }
    }

    fn is_kernel(p: &Proc) -> bool {
        p.pid == 2 || p.ppid == 2
    }

    fn display_name(&self, p: &Proc) -> String {
        self.apps.by_proc(p).map(|a| a.name.clone()).unwrap_or_else(|| p.name.clone())
    }

    fn proc_icon(&self, p: &Proc) -> Option<String> {
        self.apps.by_proc(p).and_then(|a| a.icon.clone()).or_else(|| icon_path("application-x-executable"))
    }

    fn proc_row(&self, pid: u32, depth: u8, kind: &'static str) -> Option<Row> {
        let p = self.procs.get(&pid)?;
        Some(Row {
            key: RowKey::Proc(pid),
            depth,
            name: self.display_name(p),
            icon: self.proc_icon(p),
            count: 1,
            expandable: false,
            stats: Some(self.proc_stats(pid)),
            status: if p.status == ProcessStatus::Stop { "Suspended".into() } else { String::new() },
            kind,
            pids: vec![pid],
            windows: Vec::new(),
            pid: Some(pid),
        })
    }

    fn build_rows(&mut self) {
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for p in self.procs.values() {
            children.entry(p.ppid).or_default().push(p.pid);
        }
        let mut claimed: HashSet<u32> = HashSet::new();

        // Apps: each window-owning process, with its child processes folded in.
        let mut by_pid: BTreeMap<u32, Vec<Win>> = BTreeMap::new();
        for w in &self.windows {
            if w.pid > 1 && self.procs.contains_key(&w.pid) {
                by_pid.entry(w.pid).or_default().push(w.clone());
            }
        }
        let mut apps: Vec<(Row, Vec<Row>)> = Vec::new();
        for (pid, wins) in &by_pid {
            let mut tree = vec![*pid];
            let mut i = 0;
            while i < tree.len() {
                for &c in children.get(&tree[i]).map(Vec::as_slice).unwrap_or(&[]) {
                    if !by_pid.contains_key(&c) && !claimed.contains(&c) {
                        tree.push(c);
                    }
                }
                i += 1;
            }
            claimed.extend(&tree);
            let p = &self.procs[pid];
            let info = self.apps.by_app_id(&wins[0].app_id).or_else(|| self.apps.by_proc(p));
            let mut stats = Stats::default();
            for &t in &tree {
                stats.add(&self.proc_stats(t));
            }
            let key = RowKey::App(*pid);
            let mut kids = Vec::new();
            for w in wins {
                kids.push(Row {
                    key: RowKey::Win(w.id),
                    depth: 1,
                    name: w.title.clone(),
                    icon: None,
                    count: 0,
                    expandable: false,
                    stats: None,
                    status: String::new(),
                    kind: "App",
                    pids: vec![*pid],
                    windows: vec![w.id],
                    pid: Some(*pid),
                });
            }
            if tree.len() > 1 {
                for &t in &tree {
                    if let Some(r) = self.proc_row(t, 1, "App") {
                        kids.push(r);
                    }
                }
            }
            apps.push((
                Row {
                    key,
                    depth: 0,
                    name: info.map(|a| a.name.clone()).unwrap_or_else(|| p.name.clone()),
                    icon: info.and_then(|a| a.icon.clone()).or_else(|| icon_path("application-x-executable")),
                    count: tree.len(),
                    expandable: true,
                    stats: Some(stats),
                    status: if p.status == ProcessStatus::Stop { "Suspended".into() } else { String::new() },
                    kind: "App",
                    pids: tree,
                    windows: wins.iter().map(|w| w.id).collect(),
                    pid: Some(*pid),
                },
                kids,
            ));
        }

        // Background and Windoze processes: the rest, with same-named processes folded.
        let fold = |group: u8, kind: &'static str, pick: &dyn Fn(&Proc) -> bool| -> Vec<(Row, Vec<Row>)> {
            let mut by_name: BTreeMap<String, Vec<u32>> = BTreeMap::new();
            for p in self.procs.values() {
                if !claimed.contains(&p.pid) && !Self::is_kernel(p) && pick(p) {
                    by_name.entry(self.display_name(p)).or_default().push(p.pid);
                }
            }
            let mut out = Vec::new();
            for (name, mut pids) in by_name {
                pids.sort();
                if pids.len() == 1 {
                    if let Some(r) = self.proc_row(pids[0], 0, kind) {
                        out.push((r, Vec::new()));
                    }
                    continue;
                }
                let mut stats = Stats::default();
                for &p in &pids {
                    stats.add(&self.proc_stats(p));
                }
                let kids: Vec<Row> = pids.iter().filter_map(|&p| self.proc_row(p, 1, kind)).collect();
                let first = &self.procs[&pids[0]];
                out.push((
                    Row {
                        key: RowKey::Named(group, name.clone()),
                        depth: 0,
                        name,
                        icon: self.proc_icon(first),
                        count: pids.len(),
                        expandable: true,
                        stats: Some(stats),
                        status: String::new(),
                        kind,
                        pids: pids.clone(),
                        windows: Vec::new(),
                        pid: None,
                    },
                    kids,
                ));
            }
            out
        };
        let me = self.my_uid;
        let background = fold(1, "Background process", &|p: &Proc| p.uid == me);
        let system = fold(2, "Windoze process", &|p: &Proc| p.uid != me);
        claimed.clear();

        let sort = self.sort;
        let cmp = |a: &Row, b: &Row| row_cmp(a, b, sort);
        let mut rows = Vec::new();
        let emit = |rows: &mut Vec<Row>, mut items: Vec<(Row, Vec<Row>)>, expanded: &HashSet<RowKey>| {
            items.sort_by(|a, b| cmp(&a.0, &b.0));
            for (r, mut kids) in items {
                let open = expanded.contains(&r.key);
                rows.push(r);
                if open {
                    // Window titles stay first; the processes under them sort.
                    let split = kids.iter().position(|k| !matches!(k.key, RowKey::Win(_))).unwrap_or(kids.len());
                    kids[split..].sort_by(|a, b| cmp(a, b));
                    rows.extend(kids);
                }
            }
        };
        if self.settings.group {
            for (g, label, items) in [(0u8, "Apps", apps), (1, "Background processes", background), (2, "Windoze processes", system)] {
                if items.is_empty() {
                    continue;
                }
                rows.push(Row {
                    key: RowKey::Group(g),
                    depth: 0,
                    name: format!("{label} ({})", items.len()),
                    icon: None,
                    count: 0,
                    expandable: false,
                    stats: None,
                    status: String::new(),
                    kind: "",
                    pids: Vec::new(),
                    windows: Vec::new(),
                    pid: None,
                });
                emit(&mut rows, items, &self.expanded);
            }
        } else {
            let mut all = apps;
            all.extend(background);
            all.extend(system);
            emit(&mut rows, all, &self.expanded);
        }
        self.rows = rows;

        // Users: one row per signed-in (human) user, expandable to their processes.
        let mut by_user: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for p in self.procs.values() {
            if !Self::is_kernel(p) && (p.uid >= 1000 && p.uid < 60000 || p.uid == me) {
                by_user.entry(p.uid).or_default().push(p.pid);
            }
        }
        let mut items = Vec::new();
        for (uid, pids) in by_user {
            let mut stats = Stats::default();
            for &p in &pids {
                stats.add(&self.proc_stats(p));
            }
            let kids: Vec<Row> = pids.iter().filter_map(|&p| self.proc_row(p, 1, "")).collect();
            let name = self.users.get(&uid).cloned().unwrap_or_else(|| uid.to_string());
            items.push((
                Row {
                    key: RowKey::User(uid),
                    depth: 0,
                    name,
                    icon: icon_path("user-identity").or_else(|| icon_path("user")),
                    count: pids.len(),
                    expandable: true,
                    stats: Some(stats),
                    status: String::new(),
                    kind: "",
                    pids,
                    windows: Vec::new(),
                    pid: None,
                },
                kids,
            ));
        }
        let mut urows = Vec::new();
        emit(&mut urows, items, &self.expanded);
        self.user_rows = urows;
    }

    // ---- Actions ----

    fn selected_row(&self) -> Option<Row> {
        let key = self.selected.as_ref()?;
        let rows = if self.tab == Tab::Users { &self.user_rows } else { &self.rows };
        rows.iter().find(|r| r.key == *key).cloned()
    }

    /// End task: apps get asked to close their windows; plain processes get
    /// SIGTERM, then SIGKILL if they're still there two seconds later.
    fn end_task(&mut self, row: &Row) {
        if !row.windows.is_empty() && matches!(row.key, RowKey::App(_) | RowKey::Win(_)) {
            for w in &row.windows {
                ipc(&format!("close-window {w}"));
            }
            return;
        }
        let pids: Vec<u32> = row.pids.iter().copied().filter(|&p| p > 1 && p != std::process::id()).collect();
        self.kill(&pids, &row.name);
    }

    fn kill(&mut self, pids: &[u32], name: &str) {
        let mut failed = false;
        for &pid in pids {
            match self.sys.process(Pid::from_u32(pid)).map(|p| p.kill_with(Signal::Term)) {
                Some(Some(true)) => {}
                _ => failed = true,
            }
        }
        if failed {
            self.status = Some(format!("Unable to end {name}: Access is denied."));
        }
        let pids = pids.to_vec();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(2));
            for pid in pids {
                if Path::new(&format!("/proc/{pid}")).exists() {
                    let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).status();
                }
            }
        });
    }

    fn end_tree(&mut self, pid: u32) {
        let mut tree = vec![pid];
        let mut i = 0;
        while i < tree.len() {
            let parent = tree[i];
            tree.extend(self.procs.values().filter(|p| p.ppid == parent).map(|p| p.pid));
            i += 1;
        }
        let name = self.procs.get(&pid).map(|p| p.name.clone()).unwrap_or_default();
        self.kill(&tree, &name);
    }

    fn switch_to(&mut self, ctx: &egui::Context, row: &Row) {
        if let Some(w) = row.windows.first() {
            ipc(&format!("focus-window {w}"));
            if self.settings.minimize_on_use {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
        }
    }

    fn go_to_details(&mut self, pid: u32) {
        self.tab = Tab::Details;
        self.details.selected = Some(pid as u64);
        self.details.scroll_to = Some(pid as u64);
    }

    fn set_fewer(&mut self, ctx: &egui::Context, fewer: bool) {
        self.settings.fewer = fewer;
        self.settings.save();
        let size = if fewer { vec2(300.0, 320.0) } else { vec2(1040.0, 700.0) };
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
    }

    fn toggle_expand(&mut self, key: &RowKey) {
        if !self.expanded.remove(key) {
            self.expanded.insert(key.clone());
        }
        self.build_rows();
    }

    // ---- Chrome ----

    fn menu_bar(&mut self, ui: &mut Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Run new task").clicked() {
                    self.run_dialog = Some((String::new(), false));
                    ui.close();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Options", |ui| {
                if ui.checkbox(&mut self.settings.on_top, "Always on top").changed() {
                    let level = if self.settings.on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
                    self.settings.save();
                }
                if ui.checkbox(&mut self.settings.minimize_on_use, "Minimize on use").changed() {
                    self.settings.save();
                }
                ui.separator();
                ui.menu_button("Set default tab", |ui| {
                    for (t, label) in TABS {
                        if ui.radio(self.settings.default_tab == t, label).clicked() {
                            self.settings.default_tab = t;
                            self.settings.save();
                            ui.close();
                        }
                    }
                });
            });
            ui.menu_button("View", |ui| {
                if ui.button("Refresh now").clicked() {
                    self.refresh();
                    ui.close();
                }
                ui.menu_button("Update speed", |ui| {
                    for (s, label) in [(Speed::High, "High"), (Speed::Normal, "Normal"), (Speed::Low, "Low"), (Speed::Paused, "Paused")] {
                        if ui.radio(self.settings.speed == s, label).clicked() {
                            self.settings.speed = s;
                            self.settings.save();
                            ui.close();
                        }
                    }
                });
                ui.separator();
                if ui.checkbox(&mut self.settings.group, "Group by type").changed() {
                    self.settings.save();
                    self.build_rows();
                }
                if ui.button("Expand all").clicked() {
                    let keys: Vec<RowKey> = self.rows.iter().chain(&self.user_rows).filter(|r| r.expandable).map(|r| r.key.clone()).collect();
                    self.expanded.extend(keys);
                    self.build_rows();
                    ui.close();
                }
                if ui.button("Collapse all").clicked() {
                    self.expanded.clear();
                    self.build_rows();
                    ui.close();
                }
            });
        });
    }

    fn tabs(&mut self, ui: &mut Ui) {
        let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), 25.0), Sense::hover());
        let p = ui.painter();
        p.hline(strip.x_range(), strip.bottom() - 0.5, (1.0, TAB_LINE));
        let mut x = strip.left() + 6.0;
        for (t, label) in TABS {
            let w = text_w(ui, label, 12.0) + 16.0;
            let active = self.tab == t;
            let r = if active {
                Rect::from_min_max(pos2(x - 2.0, strip.top()), pos2(x + w + 2.0, strip.bottom()))
            } else {
                Rect::from_min_max(pos2(x, strip.top() + 2.0), pos2(x + w, strip.bottom() - 1.0))
            };
            let resp = ui.interact(r, Id::unique(("tab", label)), Sense::click());
            let p = ui.painter();
            if active {
                p.rect_filled(r, 0.0, WHITE);
                let s = Stroke::new(1.0, TAB_LINE);
                p.vline(r.left() + 0.5, r.y_range(), s);
                p.vline(r.right() - 0.5, r.y_range(), s);
                p.hline(r.x_range(), r.top() + 0.5, s);
            } else if resp.hovered() {
                p.rect_filled(r, 0.0, Color32::from_rgb(0xd8, 0xea, 0xf9));
                p.rect_stroke(r, 0.0, (1.0, Color32::from_rgb(0x7e, 0xb4, 0xea)), StrokeKind::Inside);
            }
            p.text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(12.0), TEXT);
            if resp.clicked() {
                self.tab = t;
                if t == Tab::Services {
                    self.ask_services();
                }
                if t == Tab::Startup {
                    self.startup_list = startup_items(&self.apps);
                }
            }
            x += w + 1.0;
        }
    }

    fn footer(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.set_height(30.0);
            if fewer_link(ui, false, "Fewer details") {
                self.set_fewer(ctx, true);
            }
            if let Some(s) = &self.status {
                ui.add_space(16.0);
                ui.label(egui::RichText::new(s).color(DIM).size(12.0));
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| match self.tab {
                Tab::Processes | Tab::Details => {
                    let row = if self.tab == Tab::Processes { self.selected_row() } else { None };
                    let enabled = if self.tab == Tab::Processes { row.as_ref().is_some_and(|r| !r.pids.is_empty()) } else { self.details.selected.is_some() };
                    if win_button(ui, if self.tab == Tab::Details { "End task" } else { "End task" }, enabled) {
                        match row {
                            Some(r) => self.end_task(&r),
                            None => {
                                if let Some(pid) = self.details.selected {
                                    let name = self.procs.get(&(pid as u32)).map(|p| p.name.clone()).unwrap_or_default();
                                    self.kill(&[pid as u32], &name);
                                }
                            }
                        }
                    }
                }
                Tab::Performance => {
                    ui.add_space(4.0);
                    if link(ui, "Open Resource Monitor") {
                        let tool = if which("htop") { "htop" } else { "top" };
                        spawn_terminal(&[tool]);
                    }
                    resource_icon(ui);
                }
                Tab::Startup => {
                    let item = self.startup.selected.and_then(|id| self.startup_list.get(id as usize)).cloned();
                    let label = if item.as_ref().is_some_and(|i| !i.enabled) { "Enable" } else { "Disable" };
                    if win_button(ui, label, item.is_some())
                        && let Some(it) = item
                    {
                        set_autostart(&it, !it.enabled);
                        self.startup_list = startup_items(&self.apps);
                    }
                }
                Tab::Users => {
                    win_button(ui, "Disconnect", false);
                }
                Tab::Services => {
                    ui.add_space(4.0);
                    if link(ui, "Open Services") {
                        spawn_terminal(&["systemctl", "list-units", "--type=service", "--all"]);
                    }
                    resource_icon(ui);
                }
                Tab::AppHistory => {}
            });
        });
    }

    // ---- Processes / Users ----

    fn heat_table(&mut self, ui: &mut Ui, ctx: &egui::Context, users: bool) {
        let cols: Vec<PCol> = if users { USER_COLS.to_vec() } else { self.settings.cols.clone() };
        let rows = if users { std::mem::take(&mut self.user_rows) } else { std::mem::take(&mut self.rows) };
        let fixed: f32 = cols.iter().filter(|c| **c != PCol::Name).map(|c| pcol(*c).width).sum();
        let name_w = (ui.available_width() - fixed - 14.0).max(180.0);
        let width = |c: PCol| if c == PCol::Name { name_w } else { pcol(c).width };

        // Header: the total above the name, like Windoze, and a caret over the sort column.
        let totals: HashMap<PCol, f32> = [
            (PCol::Cpu, self.sys.global_cpu_usage()),
            (PCol::Memory, self.mem.last()),
            (PCol::Disk, self.disk_total),
            (PCol::Network, self.net_total as f32),
            (PCol::Gpu, self.gpu.as_ref().map(|g| g.util.last()).unwrap_or(0.0)),
        ]
        .into_iter()
        .collect();
        let (hrect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::hover());
        let mut x = hrect.left();
        let mut header_menu = None;
        for &c in &cols {
            let w = width(c);
            let r = Rect::from_min_size(pos2(x, hrect.top()), vec2(w, hrect.height()));
            let resp = ui.interact(r, Id::unique(("pcol", users, c as u8)), Sense::click());
            let info = pcol(c);
            let total = totals.get(&c).copied();
            let p = ui.painter();
            if let Some(t) = total.filter(|t| *t >= 50.0) {
                p.rect_filled(r, 0.0, heat(t / 100.0));
            } else if resp.hovered() {
                p.rect_filled(r, 0.0, HOVER);
            }
            let label = if users && c == PCol::Name { "User" } else { info.label };
            let (ax, align) = if info.numeric { (r.right() - 8.0, Align2::RIGHT_BOTTOM) } else { (r.left() + 8.0, Align2::LEFT_BOTTOM) };
            if let Some(t) = total {
                p.text(pos2(ax, r.top() + 27.0), align, format!("{t:.0}%"), FontId::proportional(17.0), if t >= 50.0 { TEXT } else { Color32::from_rgb(0x1a, 0x1a, 0x1a) });
            }
            paint_text(ui, pos2(ax, r.bottom() - 4.0), align, label, 12.0, if total.is_some() { DIM } else { TEXT }, w - 14.0, 1);
            if self.sort.0 == c {
                chevron(ui.painter(), pos2(r.center().x, r.top() + 5.0), if self.sort.1 { Dir::Up } else { Dir::Down }, 3.0, FAINT);
            }
            ui.painter().vline(r.right() - 0.5, r.y_range(), (1.0, BORDER));
            if resp.clicked() {
                self.sort = if self.sort.0 == c { (c, !self.sort.1) } else { (c, !info.numeric) };
            }
            if !users {
                header_menu.get_or_insert(resp.clone());
                resp.context_menu(|ui| self.column_menu(ui));
            }
            x += w;
        }
        ui.painter().hline(hrect.x_range(), hrect.bottom() - 0.5, (1.0, BORDER));
        let _ = header_menu;

        let mut toggle = None;
        let mut activate = None;
        egui::ScrollArea::vertical().id_salt(("heat", users)).auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let total_mem = self.sys.total_memory().max(1) as f32;
            for r in &rows {
                let h = if matches!(r.key, RowKey::Group(_)) { GROUP_H } else { ROW_H };
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
                if self.scroll_to_row.as_ref() == Some(&r.key) {
                    ui.scroll_to_rect(rect, None);
                    self.scroll_to_row = None;
                }
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let p = ui.painter().clone();
                if let RowKey::Group(_) = r.key {
                    paint_text(ui, pos2(rect.left() + 10.0, rect.bottom() - 7.0), Align2::LEFT_BOTTOM, &r.name, 15.0, GROUP_BLUE, name_w, 1);
                    continue;
                }
                let sel = self.selected.as_ref() == Some(&r.key);
                let mut x = rect.left();
                for &c in &cols {
                    let w = width(c);
                    let cell = Rect::from_min_size(pos2(x, rect.top()), vec2(w, h));
                    let info = pcol(c);
                    if let (Some(s), true) = (r.stats, info.numeric) {
                        let frac = match c {
                            PCol::Cpu => s.cpu / 100.0,
                            PCol::Memory => s.mem as f32 / total_mem * 4.0,
                            PCol::Disk => (s.disk / 50e6) as f32,
                            PCol::Network => (s.net / 10e6) as f32,
                            PCol::Gpu => s.gpu / 100.0,
                            PCol::Power => power_level(s.power) as f32 / 4.0,
                            PCol::PowerTrend => power_level(s.trend) as f32 / 4.0,
                            _ => 0.0,
                        };
                        p.rect_filled(cell, 0.0, heat(frac));
                    }
                    x += w;
                }
                if sel {
                    p.rect_filled(rect, 0.0, SELECTED);
                    p.rect_stroke(rect, 0.0, (1.0, SELECTED_LINE), StrokeKind::Inside);
                } else if resp.hovered() {
                    p.rect_filled(rect, 0.0, HOVER.gamma_multiply(0.85));
                }
                let mut x = rect.left();
                let cy = rect.center().y;
                for &c in &cols {
                    let w = width(c);
                    let info = pcol(c);
                    if c == PCol::Name {
                        let indent = r.depth as f32 * 22.0;
                        if r.expandable {
                            let cr = Rect::from_center_size(pos2(x + 12.0 + indent, cy), vec2(18.0, h));
                            let chev = ui.interact(cr, Id::unique(("chev", &r.key)), Sense::click());
                            chevron(&p, cr.center(), if self.expanded.contains(&r.key) { Dir::Down } else { Dir::Right }, 3.5, if chev.hovered() { CPU_LINE } else { DIM });
                            if chev.clicked() {
                                toggle = Some(r.key.clone());
                            }
                        }
                        let ix = x + 24.0 + indent;
                        match &r.icon {
                            Some(icon) => paint_icon(ui, icon, Rect::from_min_size(pos2(ix, cy - 8.0), vec2(16.0, 16.0))),
                            None if matches!(r.key, RowKey::Win(_)) => window_glyph(&p, Rect::from_min_size(pos2(ix, cy - 7.0), vec2(16.0, 14.0))),
                            None => {}
                        }
                        let label = if r.count > 1 { format!("{} ({})", r.name, r.count) } else { r.name.clone() };
                        paint_text(ui, pos2(ix + 24.0, cy), Align2::LEFT_CENTER, &label, 12.0, TEXT, w - (ix - x) - 30.0, 1);
                    } else {
                        let s = r.stats.unwrap_or_default();
                        let text = match c {
                            PCol::Status => r.status.clone(),
                            PCol::Type => r.kind.to_string(),
                            PCol::Pid => r.pid.map(|p| p.to_string()).unwrap_or_default(),
                            PCol::ProcName => r.pid.and_then(|p| self.procs.get(&p)).map(|p| p.name.clone()).unwrap_or_default(),
                            PCol::CmdLine => r.pid.and_then(|p| self.procs.get(&p)).map(|p| p.cmd.clone()).unwrap_or_default(),
                            PCol::Publisher => String::new(),
                            _ if r.stats.is_none() => String::new(),
                            PCol::Cpu => pct(s.cpu),
                            PCol::Memory => mb(s.mem),
                            PCol::Disk => format!("{} MB/s", one_dec(s.disk / 1048576.0)),
                            PCol::Network => format!("{} Mbps", one_dec(s.net / 1e6)),
                            PCol::Gpu => pct(s.gpu),
                            PCol::GpuEngine => if s.gpu > 0.0 { "GPU 0 - 3D".into() } else { String::new() },
                            PCol::Power => POWER[power_level(s.power)].into(),
                            PCol::PowerTrend => POWER[power_level(s.trend)].into(),
                            PCol::Name => String::new(),
                        };
                        if info.numeric {
                            paint_text(ui, pos2(x + w - 8.0, cy), Align2::RIGHT_CENTER, &text, 12.0, TEXT, w - 12.0, 1);
                        } else {
                            paint_text(ui, pos2(x + 8.0, cy), Align2::LEFT_CENTER, &text, 12.0, TEXT, w - 12.0, 1);
                        }
                    }
                    x += w;
                }
                if resp.clicked() || resp.secondary_clicked() {
                    self.selected = Some(r.key.clone());
                }
                if resp.double_clicked() {
                    activate = Some(r.clone());
                }
                resp.context_menu(|ui| self.row_menu(ui, ctx, r));
            }
            // Empty space below: deselect.
            let (_, rest) = ui.allocate_exact_size(vec2(ui.available_width(), ui.available_height().max(10.0)), Sense::click());
            if rest.clicked() {
                self.selected = None;
            }
        });
        if users {
            self.user_rows = rows;
        } else {
            self.rows = rows;
        }
        if let Some(k) = toggle {
            self.toggle_expand(&k);
        }
        if let Some(r) = activate {
            if r.expandable {
                self.toggle_expand(&r.key);
            } else if !r.windows.is_empty() {
                self.switch_to(ctx, &r);
            }
        }
    }

    fn column_menu(&mut self, ui: &mut Ui) {
        for info in PCOLS.iter().filter(|c| c.col != PCol::Name) {
            let mut on = self.settings.cols.contains(&info.col);
            if ui.checkbox(&mut on, info.label).changed() {
                if on {
                    // Keep Task Manager's column order regardless of click order.
                    self.settings.cols.push(info.col);
                    self.settings.cols.sort_by_key(|c| PCOLS.iter().position(|p| p.col == *c));
                } else {
                    self.settings.cols.retain(|c| *c != info.col);
                }
                self.settings.save();
            }
        }
    }

    fn row_menu(&mut self, ui: &mut Ui, ctx: &egui::Context, r: &Row) {
        ui.set_min_width(200.0);
        if r.expandable {
            let label = if self.expanded.contains(&r.key) { "Collapse" } else { "Expand" };
            if ui.button(egui::RichText::new(label).strong()).clicked() {
                self.toggle_expand(&r.key);
                ui.close();
            }
        }
        if !r.windows.is_empty() {
            if ui.button("Switch to").clicked() {
                self.switch_to(ctx, r);
                ui.close();
            }
            ui.separator();
        }
        if ui.add_enabled(!r.pids.is_empty(), egui::Button::new("End task")).clicked() {
            self.end_task(r);
            ui.close();
        }
        ui.separator();
        let pid = r.pid.or_else(|| r.pids.first().copied());
        let exe = pid.and_then(|p| self.procs.get(&p)).and_then(|p| p.exe.clone());
        if ui.add_enabled(pid.is_some(), egui::Button::new("Go to details")).clicked()
            && let Some(p) = pid
        {
            self.go_to_details(p);
            ui.close();
        }
        if ui.add_enabled(exe.is_some(), egui::Button::new("Open file location")).clicked()
            && let Some(e) = &exe
        {
            open_location(e);
            ui.close();
        }
        if ui.button("Search online").clicked() {
            search_online(&r.name);
            ui.close();
        }
        if ui.add_enabled(pid.is_some(), egui::Button::new("Properties")).clicked() {
            self.props = pid;
            ui.close();
        }
    }

    // ---- Fewer details ----

    fn fewer_view(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let apps: Vec<Row> = self.rows.iter().filter(|r| matches!(r.key, RowKey::App(_))).cloned().collect();
        egui::Panel::bottom("fewer-footer").frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin::symmetric(10, 8))).show_separator_line(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                if fewer_link(ui, true, "More details") {
                    self.set_fewer(ctx, false);
                }
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let row = self.selected_row().filter(|r| matches!(r.key, RowKey::App(_)));
                    if win_button(ui, "End task", row.is_some())
                        && let Some(r) = row
                    {
                        self.end_task(&r);
                    }
                });
            });
        });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin::symmetric(4, 4))).show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            if apps.is_empty() {
                ui.add_space(10.0);
                ui.label(egui::RichText::new("There are no running apps").color(DIM).size(12.0));
            }
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for r in &apps {
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                    let sel = self.selected.as_ref() == Some(&r.key);
                    if sel {
                        ui.painter().rect_filled(rect, 0.0, SELECTED);
                        ui.painter().rect_stroke(rect, 0.0, (1.0, SELECTED_LINE), StrokeKind::Inside);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, 0.0, HOVER);
                    }
                    if let Some(i) = &r.icon {
                        paint_icon(ui, i, Rect::from_min_size(pos2(rect.left() + 6.0, rect.center().y - 8.0), vec2(16.0, 16.0)));
                    }
                    paint_text(ui, pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, &r.name, 12.0, TEXT, rect.width() - 36.0, 1);
                    if resp.clicked() || resp.secondary_clicked() {
                        self.selected = Some(r.key.clone());
                    }
                    if resp.double_clicked() {
                        self.switch_to(ctx, r);
                    }
                    resp.context_menu(|ui| {
                        if ui.button(egui::RichText::new("Switch to").strong()).clicked() {
                            self.switch_to(ctx, r);
                            ui.close();
                        }
                        if ui.button("End task").clicked() {
                            self.end_task(r);
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Run new task").clicked() {
                            self.run_dialog = Some((String::new(), false));
                            ui.close();
                        }
                        if ui.checkbox(&mut self.settings.on_top, "Always on top").changed() {
                            let level = if self.settings.on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
                            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
                            self.settings.save();
                        }
                        ui.separator();
                        let exe = r.pid.and_then(|p| self.procs.get(&p)).and_then(|p| p.exe.clone());
                        if ui.add_enabled(exe.is_some(), egui::Button::new("Open file location")).clicked()
                            && let Some(e) = &exe
                        {
                            open_location(e);
                            ui.close();
                        }
                        if ui.button("Search online").clicked() {
                            search_online(&r.name);
                            ui.close();
                        }
                        if ui.button("Properties").clicked() {
                            self.props = r.pid;
                            ui.close();
                        }
                    });
                }
            });
        });
    }

    // ---- Performance ----

    fn performance(&mut self, ui: &mut Ui) {
        let full = ui.available_rect_before_wrap();
        let side = Rect::from_min_max(full.min, pos2(full.left() + 230.0, full.bottom()));
        let main = Rect::from_min_max(pos2(side.right() + 1.0, full.top()), full.max);
        ui.painter().vline(side.right() + 0.5, full.y_range(), (1.0, BORDER));

        // Sidebar tiles.
        let mut tiles: Vec<(PerfPage, String, Vec<String>, Hist, Option<Hist>, Color32, Color32, f32)> = Vec::new();
        let ghz = |m: &Mangler| m.sys.cpus().iter().map(|c| c.frequency()).max().unwrap_or(0) as f32 / 1000.0;
        tiles.push((PerfPage::Cpu, "CPU".into(), vec![format!("{:.0}%  {:.2} GHz", self.cpu.last(), ghz(self))], self.cpu.clone(), None, CPU_LINE, CPU_FILL, 100.0));
        let (used, total) = (self.sys.used_memory(), self.sys.total_memory());
        tiles.push((PerfPage::Memory, "Memory".into(), vec![format!("{}/{} ({:.0}%)", gb(used), gb(total), self.mem.last())], self.mem.clone(), None, MEM_LINE, MEM_FILL, 100.0));
        for (i, d) in self.disks.iter().enumerate() {
            tiles.push((
                PerfPage::Disk(i),
                disk_title(i, d),
                vec![if d.ssd { "SSD".into() } else { "HDD".into() }, format!("{:.0}%", d.active.last())],
                d.active.clone(),
                None,
                DISK_LINE,
                DISK_FILL,
                100.0,
            ));
        }
        for (i, n) in self.nets.iter().enumerate() {
            let sub = if n.up { format!("S: {} R: {}", bits(n.send.last()), bits(n.recv.last())) } else { "Not connected".into() };
            let max = nice_bits(n.send.max().max(n.recv.max()));
            tiles.push((PerfPage::Net(i), if n.wifi { "Wi-Fi".into() } else { "Ethernet".into() }, vec![n.iface.clone(), sub], n.recv.clone(), Some(n.send.clone()), NET_LINE, NET_FILL, max));
        }
        if let Some(g) = &self.gpu {
            let temp = g.sample.temp.map(|t| format!(" ({t:.0} °C)")).unwrap_or_default();
            tiles.push((PerfPage::Gpu, "GPU 0".into(), vec![g.sample.name.clone(), format!("{:.0}%{temp}", g.util.last())], g.util.clone(), None, CPU_LINE, CPU_FILL, 100.0));
        }
        let mut y = side.top() + 4.0;
        for (page, title, subs, hist, second, line, fill, max) in tiles {
            let h = 22.0 + subs.len() as f32 * 16.0 + 14.0;
            let h = h.max(62.0);
            let r = Rect::from_min_size(pos2(side.left() + 2.0, y), vec2(side.width() - 4.0, h));
            let resp = ui.interact(r, Id::unique(("perf-tile", format!("{:?}", page_id(page)))), Sense::click());
            if self.page == page {
                ui.painter().rect_filled(r, 0.0, SELECTED);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, 0.0, HOVER);
            }
            let g = Rect::from_min_size(pos2(r.left() + 8.0, r.top() + 8.0), vec2(68.0, h - 16.0));
            let mut series = vec![(&hist, false)];
            if let Some(s) = &second {
                series.push((s, true));
            }
            graph(ui, g, &series, max, line, fill, None, 0);
            paint_text(ui, pos2(g.right() + 10.0, r.top() + 10.0), Align2::LEFT_TOP, &title, 15.0, TEXT, r.right() - g.right() - 14.0, 1);
            for (k, s) in subs.iter().enumerate() {
                paint_text(ui, pos2(g.right() + 10.0, r.top() + 32.0 + k as f32 * 16.0), Align2::LEFT_TOP, s, 12.0, DIM, r.right() - g.right() - 14.0, 1);
            }
            if resp.clicked() {
                self.page = page;
            }
            y += h + 2.0;
        }

        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(main.shrink2(vec2(18.0, 10.0))).layout(egui::Layout::top_down(Align::Min)));
        match self.page {
            PerfPage::Cpu => self.perf_cpu(&mut child),
            PerfPage::Memory => self.perf_mem(&mut child),
            PerfPage::Disk(i) => self.perf_disk(&mut child, i),
            PerfPage::Net(i) => self.perf_net(&mut child, i),
            PerfPage::Gpu => self.perf_gpu(&mut child),
        }
        ui.allocate_rect(full, Sense::hover());
    }

    fn perf_title(ui: &mut Ui, title: &str, right: &str) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
        ui.painter().text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, title, FontId::proportional(26.0), TEXT);
        paint_text(ui, pos2(r.right(), r.center().y + 3.0), Align2::RIGHT_CENTER, right, 15.0, TEXT, r.width() * 0.6, 1);
    }

    /// The label row above a graph and the "60 seconds ... 0" row below it.
    fn big_graph(&self, ui: &mut Ui, left: &str, right: &str, series: &[(&Hist, bool)], max: f32, line: Color32, fill: Color32, grid: Color32, h: f32) -> egui::Response {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
        ui.painter().text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, left, FontId::proportional(12.0), DIM);
        ui.painter().text(pos2(r.right(), r.center().y), Align2::RIGHT_CENTER, right, FontId::proportional(12.0), DIM);
        let (g, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
        graph(ui, g, series, max, line, fill, Some(grid), self.tick);
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
        ui.painter().text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, "60 seconds", FontId::proportional(12.0), DIM);
        ui.painter().text(pos2(r.right(), r.center().y), Align2::RIGHT_CENTER, "0", FontId::proportional(12.0), DIM);
        resp
    }

    fn perf_cpu(&mut self, ui: &mut Ui) {
        Self::perf_title(ui, "CPU", &self.cpu_info.brand);
        let h = (ui.available_height() - 230.0).max(120.0);
        let resp = if self.logical {
            let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
            ui.painter().text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, "% Utilization over 60 seconds", FontId::proportional(12.0), DIM);
            ui.painter().text(pos2(r.right(), r.center().y), Align2::RIGHT_CENTER, "100%", FontId::proportional(12.0), DIM);
            let (area, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h + 18.0), Sense::click());
            let n = self.cores.len().max(1);
            let cols = (n as f32).sqrt().ceil() as usize;
            let rows = n.div_ceil(cols);
            let (cw, ch) = (area.width() / cols as f32, area.height() / rows as f32);
            for (i, c) in self.cores.iter().enumerate() {
                let r = Rect::from_min_size(area.min + vec2((i % cols) as f32 * cw, (i / cols) as f32 * ch), vec2(cw, ch)).shrink(2.0);
                graph(ui, r, &[(c, false)], 100.0, CPU_LINE, CPU_FILL, Some(CPU_GRID), self.tick);
            }
            resp
        } else {
            self.big_graph(ui, "% Utilization", "100%", &[(&self.cpu, false)], 100.0, CPU_LINE, CPU_FILL, CPU_GRID, h)
        };
        resp.context_menu(|ui| {
            ui.menu_button("Change graph to", |ui| {
                if ui.radio(!self.logical, "Overall utilization").clicked() {
                    self.logical = false;
                    ui.close();
                }
                if ui.radio(self.logical, "Logical processors").clicked() {
                    self.logical = true;
                    ui.close();
                }
            });
        });
        ui.add_space(10.0);
        let ghz = self.sys.cpus().iter().map(|c| c.frequency()).max().unwrap_or(0) as f32 / 1000.0;
        let procs = self.procs.len();
        let threads: usize = self.procs.values().map(|p| p.threads).sum();
        let handles: usize = self.sys.processes().values().filter_map(|p| p.open_files()).sum();
        let up = System::uptime().unwrap_or(0);
        let c = &self.cpu_info;
        let stats = vec![
            vec![("Utilization", format!("{:.0}%", self.cpu.last())), ("Speed", format!("{ghz:.2} GHz"))],
            vec![("Processes", procs.to_string()), ("Threads", threads.to_string()), ("Handles", handles.to_string())],
            vec![("Up time", format!("{}:{:02}:{:02}:{:02}", up / 86400, up / 3600 % 24, up / 60 % 60, up % 60))],
        ];
        let table = vec![
            ("Base speed:", c.base.clone()),
            ("Sockets:", c.sockets.to_string()),
            ("Cores:", c.cores.to_string()),
            ("Logical processors:", c.logical.to_string()),
            ("Virtualization:", c.virt.to_string()),
            ("L1 cache:", cache(c.caches[0])),
            ("L2 cache:", cache(c.caches[1])),
            ("L3 cache:", cache(c.caches[2])),
        ];
        stat_block(ui, &stats, &table);
    }

    fn perf_mem(&mut self, ui: &mut Ui) {
        let total = self.sys.total_memory();
        Self::perf_title(ui, "Memory", &gb(total));
        let h = (ui.available_height() - 290.0).max(110.0);
        self.big_graph(ui, "Memory usage", &gb(total), &[(&self.mem, false)], 100.0, MEM_LINE, MEM_FILL, MEM_GRID, h);
        ui.add_space(6.0);
        let mi = |k: &str| self.meminfo.get(k).copied().unwrap_or(0) * 1024;
        let in_use = total.saturating_sub(mi("MemAvailable"));
        let modified = mi("Dirty") + mi("Writeback");
        let free = mi("MemFree");
        let standby = total.saturating_sub(in_use + modified + free);
        // Memory composition: In use / Modified / Standby / Free.
        ui.label(egui::RichText::new("Memory composition").color(DIM).size(12.0));
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::hover());
        let p = ui.painter();
        let mut x = bar.left();
        for (bytes, fill, stroke) in [(in_use, MEM_LINE.gamma_multiply(0.45), true), (modified, MEM_LINE.gamma_multiply(0.25), true), (standby, MEM_FILL, true), (free, WHITE, false)] {
            let w = bar.width() * bytes as f32 / total.max(1) as f32;
            let r = Rect::from_min_size(pos2(x, bar.top()), vec2(w, bar.height()));
            p.rect_filled(r, 0.0, fill);
            if stroke && w > 1.0 {
                p.vline(r.right(), bar.y_range(), (1.0, MEM_LINE));
            }
            x += w;
        }
        p.rect_stroke(bar, 0.0, (1.0, MEM_LINE), StrokeKind::Inside);
        ui.add_space(10.0);
        let compressed = mi("Zswap");
        let stats = vec![
            vec![("In use (Compressed)", format!("{} ({})", gb(in_use), mb_short(compressed))), ("Available", gb(mi("MemAvailable")))],
            vec![("Committed", format!("{}/{}", gb(mi("Committed_AS")), gb(mi("CommitLimit")))), ("Cached", gb(mi("Cached") + mi("Buffers")))],
            vec![("Paged pool", mb_short(mi("SReclaimable"))), ("Non-paged pool", mb_short(mi("SUnreclaim")))],
        ];
        let table = vec![("Hardware reserved:", mb_short(physical_ram().saturating_sub(total)))];
        stat_block(ui, &stats, &table);
    }

    fn perf_disk(&mut self, ui: &mut Ui, i: usize) {
        let Some(d) = self.disks.get(i) else { return };
        Self::perf_title(ui, &disk_title(i, d), &d.model);
        let h = (ui.available_height() - 330.0).max(90.0);
        self.big_graph(ui, "Active time", "100%", &[(&d.active, false)], 100.0, DISK_LINE, DISK_FILL, DISK_GRID, h);
        ui.add_space(4.0);
        let max = nice_bytes(d.read.max().max(d.write.max()));
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
        ui.painter().text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, "Disk transfer rate", FontId::proportional(12.0), DIM);
        ui.painter().text(pos2(r.right(), r.center().y), Align2::RIGHT_CENTER, bytes_rate(max), FontId::proportional(12.0), DIM);
        let (g, _) = ui.allocate_exact_size(vec2(ui.available_width(), 70.0), Sense::hover());
        graph(ui, g, &[(&d.read, false), (&d.write, true)], max, DISK_LINE, DISK_FILL, Some(DISK_GRID), self.tick);
        ui.add_space(10.0);
        let stats = vec![
            vec![("Active time", format!("{:.0}%", d.active.last())), ("Average response time", format!("{:.1} ms", d.resp_ms))],
            vec![("Read speed", bytes_rate(d.read.last())), ("Write speed", bytes_rate(d.write.last()))],
        ];
        let table = vec![
            ("Capacity:", disk_size(d.size)),
            ("Formatted:", disk_size(d.formatted)),
            ("System disk:", if d.system { "Yes".into() } else { "No".into() }),
            ("Page file:", if d.page_file { "Yes".into() } else { "No".into() }),
            ("Type:", if d.ssd { "SSD".into() } else { "HDD".into() }),
        ];
        stat_block(ui, &stats, &table);
    }

    fn perf_net(&mut self, ui: &mut Ui, i: usize) {
        let Some(n) = self.nets.get(i) else { return };
        Self::perf_title(ui, if n.wifi { "Wi-Fi" } else { "Ethernet" }, &n.iface);
        let max = nice_bits(n.send.max().max(n.recv.max()));
        let h = (ui.available_height() - 210.0).max(110.0);
        self.big_graph(ui, "Throughput", &bits(max), &[(&n.recv, false), (&n.send, true)], max, NET_LINE, NET_FILL, NET_GRID, h);
        ui.add_space(10.0);
        let stats = vec![vec![("Send", bits(n.send.last()))], vec![("Receive", bits(n.recv.last()))]];
        let mut table = vec![("Adapter name:", n.iface.clone())];
        table.push(("Connection type:", if n.wifi { "802.11".into() } else { "Ethernet".into() }));
        if n.speed_mbps > 0 {
            table.push(("Link speed:", if n.speed_mbps >= 1000 { format!("{} Gbps", n.speed_mbps / 1000) } else { format!("{} Mbps", n.speed_mbps) }));
        }
        table.push(("IPv4 address:", n.ipv4.clone()));
        table.push(("IPv6 address:", n.ipv6.clone()));
        stat_block(ui, &stats, &table);
    }

    fn perf_gpu(&mut self, ui: &mut Ui) {
        let Some(g) = &self.gpu else { return };
        Self::perf_title(ui, "GPU", &g.sample.name);
        // The four engine graphs, two by two.
        let avail = ui.available_width();
        let eh = ((ui.available_height() - 300.0) / 2.0).clamp(60.0, 160.0);
        for pair in [[("3D", &g.util), ("Copy", &g.copy)], [("Video Encode", &g.enc), ("Video Decode", &g.dec)]] {
            let (row, _) = ui.allocate_exact_size(vec2(avail, eh + 20.0), Sense::hover());
            for (k, (label, h)) in pair.iter().enumerate() {
                let x = row.left() + k as f32 * (avail / 2.0 + 6.0);
                let w = avail / 2.0 - 6.0;
                ui.painter().text(pos2(x, row.top() + 8.0), Align2::LEFT_CENTER, *label, FontId::proportional(12.0), DIM);
                ui.painter().text(pos2(x + w, row.top() + 8.0), Align2::RIGHT_CENTER, format!("{:.0}%", h.last()), FontId::proportional(12.0), DIM);
                graph(ui, Rect::from_min_size(pos2(x, row.top() + 18.0), vec2(w, eh)), &[(h, false)], 100.0, CPU_LINE, CPU_FILL, Some(CPU_GRID), self.tick);
            }
            ui.add_space(6.0);
        }
        let (r, _) = ui.allocate_exact_size(vec2(avail, 18.0), Sense::hover());
        ui.painter().text(pos2(r.left(), r.center().y), Align2::LEFT_CENTER, "Dedicated GPU memory usage", FontId::proportional(12.0), DIM);
        ui.painter().text(pos2(r.right(), r.center().y), Align2::RIGHT_CENTER, gb(g.sample.mem_total), FontId::proportional(12.0), DIM);
        let (gr, _) = ui.allocate_exact_size(vec2(avail, 50.0), Sense::hover());
        graph(ui, gr, &[(&g.mem, false)], 100.0, CPU_LINE, CPU_FILL, Some(CPU_GRID), self.tick);
        ui.add_space(10.0);
        let s = &g.sample;
        let mut stats = vec![
            vec![("Utilization", format!("{:.0}%", g.util.last())), ("Dedicated GPU memory", format!("{}/{}", gb(s.mem_used), gb(s.mem_total)))],
        ];
        if let Some(t) = s.temp {
            stats.push(vec![("GPU Temperature", format!("{t:.0} °C"))]);
        }
        let table = vec![("Driver version:", s.driver.clone()), ("Physical location:", s.bus.clone())];
        stat_block(ui, &stats, &table);
    }

    // ---- Plain tables ----

    fn details_tab(&mut self, ui: &mut Ui) {
        let cols: [(&str, f32, bool); 9] = [
            ("Name", 200.0, false),
            ("PID", 70.0, true),
            ("Status", 90.0, false),
            ("User name", 110.0, false),
            ("CPU", 50.0, true),
            ("Memory (active private working set)", 150.0, true),
            ("Architecture", 90.0, false),
            ("UAC virtualization", 120.0, false),
            ("Description", 240.0, false),
        ];
        let arch = if std::env::consts::ARCH == "x86_64" { "x64" } else { std::env::consts::ARCH };
        let mut rows: Vec<TRow> = self
            .procs
            .values()
            .filter(|p| !Self::is_kernel(p))
            .map(|p| TRow {
                id: p.pid as u64,
                icon: self.proc_icon(p),
                cells: vec![
                    (p.name.clone(), SortKey::T(p.name.to_lowercase())),
                    (p.pid.to_string(), SortKey::N(p.pid as f64)),
                    (if p.status == ProcessStatus::Stop { "Suspended" } else { "Running" }.into(), SortKey::T(String::new())),
                    (self.users.get(&p.uid).cloned().unwrap_or_else(|| "SYSTEM".into()), SortKey::T(self.users.get(&p.uid).cloned().unwrap_or_default())),
                    (format!("{:02.0}", p.cpu.round()), SortKey::N(p.cpu as f64)),
                    (format!("{} K", commas(p.mem / 1024)), SortKey::N(p.mem as f64)),
                    (arch.to_string(), SortKey::T(String::new())),
                    ("Not allowed".into(), SortKey::T(String::new())),
                    (self.display_name(p), SortKey::T(self.display_name(p).to_lowercase())),
                ],
            })
            .collect();
        let mut st = std::mem::take(&mut self.details);
        if st.sort.is_none() {
            rows.sort_by_key(|r| r.id);
        }
        let out = table(ui, "details", &cols, &mut rows, &mut st);
        self.details = st;
        for (id, resp) in out.rows {
            let pid = id as u32;
            resp.context_menu(|ui| {
                ui.set_min_width(190.0);
                let name = self.procs.get(&pid).map(|p| p.name.clone()).unwrap_or_default();
                if ui.button("End task").clicked() {
                    self.kill(&[pid], &name);
                    ui.close();
                }
                if ui.button("End process tree").clicked() {
                    self.end_tree(pid);
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Set priority", |ui| {
                    for (label, nice) in [("Realtime", -20), ("High", -10), ("Above normal", -5), ("Normal", 0), ("Below normal", 5), ("Low", 19)] {
                        if ui.button(label).clicked() {
                            set_priority(pid, nice);
                            ui.close();
                        }
                    }
                });
                ui.separator();
                let exe = self.procs.get(&pid).and_then(|p| p.exe.clone());
                if ui.add_enabled(exe.is_some(), egui::Button::new("Open file location")).clicked()
                    && let Some(e) = &exe
                {
                    open_location(e);
                    ui.close();
                }
                if ui.button("Search online").clicked() {
                    search_online(&name);
                    ui.close();
                }
                if ui.button("Properties").clicked() {
                    self.props = Some(pid);
                    ui.close();
                }
                if ui.button("Go to service(s)").clicked() {
                    self.tab = Tab::Services;
                    self.ask_services();
                    self.services.selected = self.service_list.lock().unwrap().as_ref().and_then(|l| l.iter().position(|s| s.pid == Some(pid))).map(|i| i as u64);
                    self.services.scroll_to = self.services.selected;
                    ui.close();
                }
            });
        }
    }

    fn services_tab(&mut self, ui: &mut Ui) {
        let list = self.service_list.lock().unwrap().clone().unwrap_or_default();
        let cols: [(&str, f32, bool); 5] = [("Name", 220.0, false), ("PID", 70.0, true), ("Description", 320.0, false), ("Status", 90.0, false), ("Group", 120.0, false)];
        let mut rows: Vec<TRow> = list
            .iter()
            .enumerate()
            .map(|(i, s)| TRow {
                id: i as u64,
                icon: icon_path("preferences-system-services").or_else(|| icon_path("system-run")),
                cells: vec![
                    (s.name.clone(), SortKey::T(s.name.to_lowercase())),
                    (s.pid.map(|p| p.to_string()).unwrap_or_default(), SortKey::N(s.pid.unwrap_or(0) as f64)),
                    (s.description.clone(), SortKey::T(s.description.to_lowercase())),
                    (if s.running { "Running" } else { "Stopped" }.into(), SortKey::T(if s.running { "a" } else { "b" }.into())),
                    (s.group.clone(), SortKey::T(s.group.clone())),
                ],
            })
            .collect();
        let mut st = std::mem::take(&mut self.services);
        let out = table(ui, "services", &cols, &mut rows, &mut st);
        self.services = st;
        for (id, resp) in out.rows {
            let Some(s) = list.get(id as usize).cloned() else { continue };
            resp.context_menu(|ui| {
                ui.set_min_width(170.0);
                for (label, verb, on) in [("Start", "start", !s.running), ("Stop", "stop", s.running), ("Restart", "restart", s.running)] {
                    if ui.add_enabled(on, egui::Button::new(label)).clicked() {
                        // polkit asks for a password through the session's agent if needed.
                        let _ = std::process::Command::new("systemctl").args([verb, &s.name]).spawn();
                        self.services_asked = None;
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("Open Services").clicked() {
                    spawn_terminal(&["systemctl", "status", &s.name]);
                    ui.close();
                }
                if ui.button("Search online").clicked() {
                    search_online(&s.name);
                    ui.close();
                }
                if ui.add_enabled(s.pid.is_some(), egui::Button::new("Go to details")).clicked()
                    && let Some(p) = s.pid
                {
                    self.go_to_details(p);
                    ui.close();
                }
            });
        }
    }

    fn startup_tab(&mut self, ui: &mut Ui) {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
        if let Some(t) = self.bios_time.lock().unwrap().clone() {
            ui.painter().text(pos2(r.right() - 10.0, r.center().y), Align2::RIGHT_CENTER, format!("Last BIOS time:  {t}"), FontId::proportional(12.0), DIM);
        }
        let cols: [(&str, f32, bool); 4] = [("Name", 260.0, false), ("Publisher", 200.0, false), ("Status", 100.0, false), ("Startup impact", 120.0, false)];
        let mut rows: Vec<TRow> = self
            .startup_list
            .iter()
            .enumerate()
            .map(|(i, s)| TRow {
                id: i as u64,
                icon: s.icon.clone().or_else(|| icon_path("application-x-executable")),
                cells: vec![
                    (s.name.clone(), SortKey::T(s.name.to_lowercase())),
                    (s.publisher.clone(), SortKey::T(s.publisher.to_lowercase())),
                    (if s.enabled { "Enabled" } else { "Disabled" }.into(), SortKey::T(if s.enabled { "a" } else { "b" }.into())),
                    ("Not measured".into(), SortKey::T(String::new())),
                ],
            })
            .collect();
        let mut st = std::mem::take(&mut self.startup);
        let out = table(ui, "startup", &cols, &mut rows, &mut st);
        self.startup = st;
        for (id, resp) in out.rows {
            let Some(s) = self.startup_list.get(id as usize).cloned() else { continue };
            resp.context_menu(|ui| {
                ui.set_min_width(170.0);
                if ui.button(if s.enabled { "Disable" } else { "Enable" }).clicked() {
                    set_autostart(&s, !s.enabled);
                    self.startup_list = startup_items(&self.apps);
                    ui.close();
                }
                if ui.button("Open file location").clicked() {
                    open_location(&s.path);
                    ui.close();
                }
                if ui.button("Search online").clicked() {
                    search_online(&s.name);
                    ui.close();
                }
            });
        }
    }

    fn history_tab(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            let since = fmt_date(self.app_history.since);
            ui.label(egui::RichText::new(format!("Resource usage since {since} for current user account.")).color(TEXT).size(12.0));
        });
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            if link(ui, "Delete usage history") {
                self.app_history.clear(&self.procs);
            }
        });
        ui.add_space(6.0);
        let cols: [(&str, f32, bool); 5] = [("Name", 300.0, false), ("CPU time", 100.0, true), ("Network", 100.0, true), ("Metered network", 120.0, true), ("Tile updates", 100.0, true)];
        let totals = self.app_history.totals();
        let mut rows: Vec<TRow> = totals
            .iter()
            .enumerate()
            .map(|(i, (name, ms))| TRow {
                id: i as u64,
                icon: self.apps.by_name(name).and_then(|a| a.icon.clone()),
                cells: vec![
                    (name.clone(), SortKey::T(name.to_lowercase())),
                    (hms(*ms), SortKey::N(*ms as f64)),
                    ("0 MB".into(), SortKey::N(0.0)),
                    ("0 MB".into(), SortKey::N(0.0)),
                    ("0 MB".into(), SortKey::N(0.0)),
                ],
            })
            .collect();
        let mut st = std::mem::take(&mut self.history);
        table(ui, "history", &cols, &mut rows, &mut st);
        self.history = st;
    }

    // ---- Dialogs ----

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some((mut text, mut admin)) = self.run_dialog.take() {
            let mut open = true;
            let mut cancel = false;
            let mut done = false;
            egui::Window::new("Create new task").id(Id::unique("run")).collapsible(false).resizable(false).default_width(400.0).open(&mut open).show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                    if let Some(i) = icon_path("system-run") {
                        paint_icon(ui, &i, r);
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Type the name of a program, folder, document, or Internet resource, and VIDE-DE will open it for you.").color(TEXT).size(12.0));
                });
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label("Open:");
                    let te = ui.add(egui::TextEdit::singleline(&mut text).desired_width(ui.available_width()));
                    if te.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        done = true;
                    }
                    if !te.has_focus() && text.is_empty() {
                        te.request_focus();
                    }
                });
                ui.add_space(6.0);
                ui.checkbox(&mut admin, "Create this task with administrative privileges.");
                ui.add_space(10.0);
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    let b = |ui: &mut Ui, s: &str| ui.add(egui::Button::new(s).min_size(vec2(76.0, 24.0))).clicked();
                    ui.add_enabled(false, egui::Button::new("Browse...").min_size(vec2(76.0, 24.0)));
                    if b(ui, "Cancel") {
                        cancel = true;
                    }
                    if b(ui, "OK") {
                        done = true;
                    }
                });
            });
            if done {
                run_task(text.trim(), admin);
            } else if open && !cancel {
                self.run_dialog = Some((text, admin));
            }
        }

        if let Some(pid) = self.props {
            let mut open = true;
            let p = self.procs.get(&pid).cloned();
            let title = p.as_ref().map(|p| p.name.clone()).unwrap_or_else(|| pid.to_string());
            egui::Window::new(format!("{title} Properties")).id(Id::unique("props")).collapsible(false).resizable(false).default_width(360.0).open(&mut open).show(ctx, |ui| {
                let Some(p) = &p else {
                    ui.label("The process has exited.");
                    return;
                };
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover());
                    if let Some(i) = self.proc_icon(p) {
                        paint_icon(ui, &i, r);
                    }
                    ui.add_space(10.0);
                    ui.label(egui::RichText::new(&p.name).color(TEXT).size(14.0));
                });
                ui.separator();
                let md = p.exe.as_ref().and_then(|e| std::fs::metadata(e).ok());
                egui::Grid::new("pgrid").num_columns(2).spacing([24.0, 6.0]).show(ui, |ui| {
                    let mut row = |k: &str, v: String| {
                        ui.label(k);
                        ui.label(egui::RichText::new(v).color(TEXT));
                        ui.end_row();
                    };
                    row("Type:", "Application".into());
                    row("Description:", self.display_name(p));
                    row("Location:", p.exe.as_ref().and_then(|e| e.parent()).map(|d| d.display().to_string()).unwrap_or_default());
                    row("Size:", md.as_ref().map(|m| mb_short(m.len())).unwrap_or_default());
                    row("PID:", p.pid.to_string());
                    row("Command line:", p.cmd.chars().take(60).collect());
                    row("Modified:", md.and_then(|m| m.modified().ok()).map(fmt_date).unwrap_or_default());
                });
            });
            if !open {
                self.props = None;
            }
        }
    }
}

impl eframe::App for Mangler {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let mut v = egui::Visuals::light();
        v.selection.bg_fill = SELECTED;
        v.selection.stroke = Stroke::new(1.0, TEXT);
        v.panel_fill = WHITE;
        v.window_fill = Color32::from_rgb(0xf0, 0xf0, 0xf0);
        v.window_stroke = Stroke::new(1.0, Color32::from_rgb(0xcc, 0xcc, 0xcc));
        v.window_corner_radius = 0.into();
        v.menu_corner_radius = 0.into();
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(0x40, 0x40, 0x40);
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = 0.into();
        }
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x91, 0xc9, 0xf7);
        v.widgets.hovered.bg_stroke = Stroke::NONE;
        ctx.set_visuals(v);

        if let Some(iv) = self.settings.speed.interval()
            && self.last.elapsed() >= iv
        {
            self.refresh();
        }
        ctx.request_repaint_after(Duration::from_millis(250));

        // Del ends the selected task, F5 refreshes, Ctrl+Tab cycles tabs.
        if !ctx.egui_wants_keyboard_input() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Delete))
                && let Some(r) = self.selected_row()
            {
                self.end_task(&r);
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::F5)) {
                self.refresh();
            }
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, Key::Tab)) {
                let i = TABS.iter().position(|(t, _)| *t == self.tab).unwrap_or(0);
                self.tab = TABS[(i + 1) % TABS.len()].0;
            }
            let step = if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)) {
                1
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)) {
                -1
            } else {
                0
            };
            if step != 0 && matches!(self.tab, Tab::Processes | Tab::Users) {
                let rows = if self.tab == Tab::Users { &self.user_rows } else { &self.rows };
                let selectable: Vec<&Row> = rows.iter().filter(|r| !matches!(r.key, RowKey::Group(_))).collect();
                let cur = self.selected.as_ref().and_then(|k| selectable.iter().position(|r| r.key == *k));
                let next = match cur {
                    None => 0,
                    Some(c) => (c as isize + step).clamp(0, selectable.len() as isize - 1) as usize,
                };
                if let Some(r) = selectable.get(next) {
                    self.selected = Some(r.key.clone());
                    self.scroll_to_row = Some(r.key.clone());
                }
            }
        }

        if self.settings.fewer {
            self.fewer_view(ui, &ctx);
            self.dialogs(&ctx);
            return;
        }

        egui::Panel::top("menu").frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin { left: 4, right: 4, top: 2, bottom: 0 })).show_separator_line(false).show(ui, |ui| {
            self.menu_bar(ui);
        });
        egui::Panel::top("tabs").frame(egui::Frame::new().fill(WHITE)).show_separator_line(false).show(ui, |ui| self.tabs(ui));
        egui::Panel::bottom("footer")
            .frame(egui::Frame::new().fill(WHITE).inner_margin(egui::Margin { left: 10, right: 10, top: 6, bottom: 8 }))
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.painter().hline(ui.max_rect().x_range(), ui.max_rect().top() - 6.0, (1.0, TAB_LINE));
                self.footer(ui, &ctx);
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(WHITE)).show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            match self.tab {
                Tab::Processes => self.heat_table(ui, &ctx, false),
                Tab::Users => self.heat_table(ui, &ctx, true),
                Tab::Performance => self.performance(ui),
                Tab::Details => self.details_tab(ui),
                Tab::Services => self.services_tab(ui),
                Tab::Startup => self.startup_tab(ui),
                Tab::AppHistory => self.history_tab(ui),
            }
        });
        self.dialogs(&ctx);
    }
}

// ---- Widgets ----

struct TableOut {
    rows: Vec<(u64, egui::Response)>,
}

/// A plain sortable list (Details, Services, Startup, App history), styled like
/// Windoze' list views: header with dividers, blue selection, no heat map.
fn table(ui: &mut Ui, id: &str, cols: &[(&str, f32, bool)], rows: &mut [TRow], st: &mut TState) -> TableOut {
    if let Some((c, asc)) = st.sort {
        rows.sort_by(|a, b| {
            let o = match (&a.cells[c].1, &b.cells[c].1) {
                (SortKey::N(x), SortKey::N(y)) => x.total_cmp(y),
                (SortKey::T(x), SortKey::T(y)) => x.cmp(y).then_with(|| a.cells[0].0.cmp(&b.cells[0].0)),
                _ => std::cmp::Ordering::Equal,
            };
            if asc { o } else { o.reverse() }
        });
    }
    let (hr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
    let mut x = hr.left();
    for (k, (label, w, numeric)) in cols.iter().enumerate() {
        let r = Rect::from_min_size(pos2(x, hr.top()), vec2(*w, hr.height()));
        let resp = ui.interact(r, Id::unique(("th", id, k)), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(r, 0.0, HOVER);
        }
        let (ax, align) = if *numeric { (r.right() - 8.0, Align2::RIGHT_CENTER) } else { (r.left() + 8.0, Align2::LEFT_CENTER) };
        paint_text(ui, pos2(ax, r.center().y + 2.0), align, label, 12.0, TEXT, w - 14.0, 1);
        if let Some((c, asc)) = st.sort
            && c == k
        {
            chevron(ui.painter(), pos2(r.center().x, r.top() + 5.0), if asc { Dir::Up } else { Dir::Down }, 3.0, FAINT);
        }
        ui.painter().vline(r.right() - 0.5, r.y_range(), (1.0, BORDER));
        if resp.clicked() {
            st.sort = Some(match st.sort {
                Some((c, asc)) if c == k => (k, !asc),
                _ => (k, !numeric),
            });
        }
        x += w;
    }
    ui.painter().hline(hr.x_range(), hr.bottom() - 0.5, (1.0, BORDER));
    let mut out = TableOut { rows: Vec::new() };
    let total_w: f32 = cols.iter().map(|c| c.1).sum();
    egui::ScrollArea::both().id_salt(("table", id)).auto_shrink(false).show(ui, |ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        for row in rows.iter() {
            let (rect, resp) = ui.allocate_exact_size(vec2(total_w.max(ui.available_width()), 22.0), Sense::click());
            if st.scroll_to == Some(row.id) {
                ui.scroll_to_rect(rect, Some(Align::Center));
                st.scroll_to = None;
            }
            if !ui.is_rect_visible(rect) {
                continue;
            }
            let sel = st.selected == Some(row.id);
            if sel {
                ui.painter().rect_filled(rect, 0.0, SELECTED);
                ui.painter().rect_stroke(rect, 0.0, (1.0, SELECTED_LINE), StrokeKind::Inside);
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 0.0, HOVER);
            }
            let mut x = rect.left();
            for (k, ((text, _), (_, w, numeric))) in row.cells.iter().zip(cols).enumerate() {
                let mut tx = x + 8.0;
                if k == 0
                    && let Some(i) = &row.icon
                {
                    paint_icon(ui, i, Rect::from_min_size(pos2(x + 6.0, rect.center().y - 8.0), vec2(16.0, 16.0)));
                    tx += 22.0;
                }
                if *numeric {
                    paint_text(ui, pos2(x + w - 8.0, rect.center().y), Align2::RIGHT_CENTER, text, 12.0, TEXT, w - 12.0, 1);
                } else {
                    paint_text(ui, pos2(tx, rect.center().y), Align2::LEFT_CENTER, text, 12.0, TEXT, w - (tx - x) - 4.0, 1);
                }
                x += w;
            }
            if resp.clicked() || resp.secondary_clicked() {
                st.selected = Some(row.id);
            }
            out.rows.push((row.id, resp));
        }
        let (_, rest) = ui.allocate_exact_size(vec2(ui.available_width(), ui.available_height().max(10.0)), Sense::click());
        if rest.clicked() {
            st.selected = None;
        }
    });
    out
}

/// Windoze' push button: grey face, blue border on hover.
fn win_button(ui: &mut Ui, label: &str, enabled: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2((text_w(ui, label, 12.0) + 30.0).max(86.0), 24.0), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let (fill, line, text) = if !enabled {
        (Color32::from_rgb(0xcc, 0xcc, 0xcc), Color32::from_rgb(0xbf, 0xbf, 0xbf), Color32::from_rgb(0x83, 0x83, 0x83))
    } else if resp.is_pointer_button_down_on() {
        (Color32::from_rgb(0xcc, 0xe4, 0xf7), Color32::from_rgb(0x00, 0x54, 0x99), TEXT)
    } else if resp.hovered() {
        (Color32::from_rgb(0xe5, 0xf1, 0xfb), Color32::from_rgb(0x00, 0x78, 0xd7), TEXT)
    } else {
        (Color32::from_rgb(0xe1, 0xe1, 0xe1), Color32::from_rgb(0xad, 0xad, 0xad), TEXT)
    };
    p.rect_filled(rect, 0.0, fill);
    p.rect_stroke(rect, 0.0, (1.0, line), StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, label, FontId::proportional(12.0), text);
    resp.clicked()
}

/// "Fewer details" / "More details": a circled chevron and the label.
fn fewer_link(ui: &mut Ui, down: bool, label: &str) -> bool {
    let w = text_w(ui, label, 12.0) + 28.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
    let c = pos2(rect.left() + 9.0, rect.center().y);
    let color = if resp.hovered() { LINK_BLUE } else { Color32::from_rgb(0x50, 0x50, 0x50) };
    ui.painter().circle_stroke(c, 7.5, (1.0, color));
    chevron(ui.painter(), c, if down { Dir::Down } else { Dir::Up }, 3.0, color);
    ui.painter().text(pos2(rect.left() + 22.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.0), if resp.hovered() { LINK_BLUE } else { TEXT });
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

fn link(ui: &mut Ui, label: &str) -> bool {
    let w = text_w(ui, label, 12.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, label, FontId::proportional(12.0), LINK_BLUE);
    if resp.hovered() {
        ui.painter().hline(rect.x_range(), rect.center().y + 8.0, (1.0, LINK_BLUE));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// The little gauge icon before "Open Resource Monitor" / "Open Services".
fn resource_icon(ui: &mut Ui) {
    let (r, _) = ui.allocate_exact_size(vec2(22.0, 20.0), Sense::hover());
    let c = r.center();
    ui.painter().rect_stroke(Rect::from_center_size(c, vec2(14.0, 11.0)), 0.0, (1.0, Color32::from_rgb(0x50, 0x50, 0x50)), StrokeKind::Inside);
    ui.painter().line_segment([c + vec2(-5.0, 3.0), c + vec2(-1.0, -1.0)], (1.0, CPU_LINE));
    ui.painter().line_segment([c + vec2(-1.0, -1.0), c + vec2(2.0, 1.0)], (1.0, CPU_LINE));
    ui.painter().line_segment([c + vec2(2.0, 1.0), c + vec2(5.0, -3.0)], (1.0, CPU_LINE));
}

/// The stats under a Performance graph: big values on the left, a key/value table on the right.
fn stat_block(ui: &mut Ui, stats: &[Vec<(&str, String)>], table: &[(&str, String)]) {
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), ui.available_height().max(150.0)), Sense::hover());
    let p = ui.painter();
    let mut y = area.top();
    for row in stats {
        let mut x = area.left();
        for (k, v) in row {
            p.text(pos2(x, y), Align2::LEFT_TOP, *k, FontId::proportional(12.0), DIM);
            let r = p.text(pos2(x, y + 16.0), Align2::LEFT_TOP, v, FontId::proportional(20.0), TEXT);
            x += (r.width() + 30.0).max(110.0);
        }
        y += 52.0;
    }
    let tx = area.left() + area.width() * 0.52;
    let mut y = area.top();
    let kw = table.iter().map(|(k, _)| text_w(ui, k, 12.0)).fold(0.0, f32::max) + 24.0;
    for (k, v) in table {
        p.text(pos2(tx, y), Align2::LEFT_TOP, *k, FontId::proportional(12.0), DIM);
        paint_text(ui, pos2(tx + kw, y), Align2::LEFT_TOP, v, 12.0, TEXT, area.right() - tx - kw, 1);
        y += 18.0;
    }
}

/// A Task Manager graph: grid that scrolls with time, filled area under the first
/// series, a dashed line for the second (send / write), border in the line colour.
#[allow(clippy::too_many_arguments)]
fn graph(ui: &Ui, rect: Rect, series: &[(&Hist, bool)], max: f32, line: Color32, fill: Color32, grid: Option<Color32>, tick: u64) {
    let p = ui.painter();
    p.rect_filled(rect, 0.0, WHITE);
    if let Some(g) = grid {
        for i in 1..10 {
            p.hline(rect.x_range(), rect.top() + rect.height() * i as f32 / 10.0, (1.0, g));
        }
        let step = rect.width() / 20.0;
        let shift = (tick % 3) as f32 * step / 3.0;
        let mut x = rect.right() - shift;
        while x > rect.left() {
            p.vline(x, rect.y_range(), (1.0, g));
            x -= step;
        }
    }
    let max = max.max(1e-6);
    for (k, (h, dashed)) in series.iter().enumerate() {
        let n = h.0.len().max(2);
        let pts: Vec<Pos2> = h
            .0
            .iter()
            .enumerate()
            .map(|(i, v)| pos2(rect.left() + rect.width() * i as f32 / (n - 1) as f32, rect.bottom() - rect.height() * (v / max).clamp(0.0, 1.0)))
            .collect();
        if k == 0 {
            for w in pts.windows(2) {
                let quad = vec![w[0], w[1], pos2(w[1].x, rect.bottom()), pos2(w[0].x, rect.bottom())];
                p.add(egui::Shape::convex_polygon(quad, fill, Stroke::NONE));
            }
        }
        if *dashed {
            p.extend(egui::Shape::dashed_line(&pts, Stroke::new(1.0, line), 4.0, 3.0));
        } else {
            p.add(egui::Shape::line(pts, Stroke::new(1.0, line)));
        }
    }
    p.rect_stroke(rect, 0.0, (1.0, line), StrokeKind::Inside);
}

/// A tiny window outline for window-title rows under an app.
fn window_glyph(p: &egui::Painter, r: Rect) {
    p.rect_stroke(r, 0.0, (1.0, Color32::from_rgb(0x50, 0x50, 0x50)), StrokeKind::Inside);
    p.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 3.0)), 0.0, Color32::from_rgb(0x50, 0x50, 0x50));
}

#[derive(Clone, Copy)]
enum Dir {
    Up,
    Down,
    Right,
}

fn chevron(p: &egui::Painter, c: Pos2, dir: Dir, size: f32, color: Color32) {
    let (a, t, b) = match dir {
        Dir::Down => (vec2(-1.0, -0.5), vec2(0.0, 0.5), vec2(1.0, -0.5)),
        Dir::Up => (vec2(-1.0, 0.5), vec2(0.0, -0.5), vec2(1.0, 0.5)),
        Dir::Right => (vec2(-0.5, -1.0), vec2(0.5, 0.0), vec2(-0.5, 1.0)),
    };
    let s = Stroke::new(1.2, color);
    p.line_segment([c + a * size, c + t * size], s);
    p.line_segment([c + t * size, c + b * size], s);
}

fn text_w(ui: &Ui, s: &str, size: f32) -> f32 {
    ui.painter().layout_no_wrap(s.to_owned(), FontId::proportional(size), TEXT).size().x
}

/// Text cut to `max_w` with an ellipsis, at most `rows` lines, placed by `anchor` at `pos`.
#[allow(clippy::too_many_arguments)]
fn paint_text(ui: &Ui, pos: Pos2, anchor: Align2, s: &str, size: f32, color: Color32, max_w: f32, rows: usize) -> Rect {
    let mut job = egui::text::LayoutJob::simple(s.to_owned(), FontId::proportional(size), color, max_w.max(1.0));
    job.wrap.max_rows = rows;
    job.wrap.overflow_character = Some('…');
    job.halign = anchor.x();
    let g = ui.painter().layout_job(job);
    let rect = anchor.anchor_size(pos, g.size());
    // Centred and right-aligned galleys are laid out around x = 0.
    let origin = match anchor.x() {
        Align::Center => pos2(rect.center().x, rect.top()),
        Align::Max => pos2(rect.right(), rect.top()),
        Align::Min => rect.min,
    };
    ui.painter().galley(origin, g, color);
    rect
}

fn paint_icon(ui: &Ui, path: &str, rect: Rect) {
    egui::Image::new(format!("file://{path}")).fit_to_exact_size(rect.size()).paint_at(ui, rect);
}

fn heat(frac: f32) -> Color32 {
    let f = frac.clamp(0.0, 1.0);
    for w in HEAT.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if f <= b {
            let t = (f - a) / (b - a);
            return lerp(ca, cb, t);
        }
    }
    HEAT[HEAT.len() - 1].1
}

fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

// ---- Formatting ----

const POWER: [&str; 5] = ["Very low", "Low", "Moderate", "High", "Very high"];

fn power_score(p: &Proc) -> f32 {
    p.cpu + p.gpu * 0.5 + (p.disk / 5e6) as f32
}

fn power_level(score: f32) -> usize {
    [0.5, 3.0, 10.0, 25.0].iter().filter(|t| score >= **t).count()
}

fn row_cmp(a: &Row, b: &Row, (col, asc): (PCol, bool)) -> std::cmp::Ordering {
    let (sa, sb) = (a.stats.unwrap_or_default(), b.stats.unwrap_or_default());
    let o = match col {
        PCol::Cpu => sa.cpu.total_cmp(&sb.cpu),
        PCol::Memory => sa.mem.cmp(&sb.mem),
        PCol::Disk => sa.disk.total_cmp(&sb.disk),
        PCol::Network => sa.net.total_cmp(&sb.net),
        PCol::Gpu => sa.gpu.total_cmp(&sb.gpu),
        PCol::Power => sa.power.total_cmp(&sb.power),
        PCol::PowerTrend => sa.trend.total_cmp(&sb.trend),
        PCol::Pid => a.pid.cmp(&b.pid),
        PCol::Status => a.status.cmp(&b.status),
        PCol::Type => a.kind.cmp(b.kind),
        _ => std::cmp::Ordering::Equal,
    };
    let o = o.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    if asc { o } else { o.reverse() }
}

fn pct(v: f32) -> String {
    if v < 0.05 { "0%".into() } else { format!("{v:.1}%") }
}

fn one_dec(v: f64) -> String {
    if v < 0.05 { "0".into() } else { format!("{v:.1}") }
}

/// The Memory column: always MB with a thousands separator, like Windoze.
fn mb(b: u64) -> String {
    let v = b as f64 / 1048576.0;
    let whole = v.trunc() as u64;
    format!("{}.{} MB", commas(whole), ((v.fract() * 10.0).round() as u64).min(9))
}

fn mb_short(b: u64) -> String {
    let v = b as f64 / 1048576.0;
    if v >= 1024.0 { format!("{:.1} GB", v / 1024.0) } else { format!("{v:.0} MB") }
}

fn gb(b: u64) -> String {
    format!("{:.1} GB", b as f64 / (1u64 << 30) as f64)
}

fn disk_size(b: u64) -> String {
    let g = b as f64 / 1e9;
    if g >= 1000.0 { format!("{:.1} TB", g / 1000.0) } else { format!("{g:.0} GB") }
}

fn cache(b: u64) -> String {
    if b >= 1 << 20 { format!("{:.1} MB", b as f64 / 1048576.0) } else { format!("{} KB", b / 1024) }
}

fn bits(v: f32) -> String {
    if v < 1000.0 {
        "0 Kbps".into()
    } else if v < 1e6 {
        format!("{:.1} Kbps", v / 1e3)
    } else if v < 1e9 {
        format!("{:.1} Mbps", v / 1e6)
    } else {
        format!("{:.1} Gbps", v / 1e9)
    }
}

fn bytes_rate(v: f32) -> String {
    if v < 1024.0 * 1024.0 { format!("{:.0} KB/s", v / 1024.0) } else { format!("{:.1} MB/s", v / 1048576.0) }
}

/// Graph scales snap to round numbers, like Task Manager's auto-scaling axis.
fn nice_bits(v: f32) -> f32 {
    [100e3, 500e3, 1e6, 5e6, 10e6, 50e6, 100e6, 500e6, 1e9, 10e9].into_iter().find(|s| v <= *s).unwrap_or(10e9)
}

fn nice_bytes(v: f32) -> f32 {
    [100.0 * 1024.0, 1048576.0, 10.0 * 1048576.0, 100.0 * 1048576.0, 500.0 * 1048576.0, 1024.0 * 1048576.0, 4096.0 * 1048576.0].into_iter().find(|s| v <= *s).unwrap_or(4096.0 * 1048576.0)
}

fn commas(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (k, c) in s.chars().enumerate() {
        if k > 0 && (s.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn hms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

fn fmt_date(t: SystemTime) -> String {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    // Civil-from-days (Howard Hinnant), local offset from libc so no chrono dependency.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = secs as libc::time_t;
    unsafe { libc::localtime_r(&t, &mut tm) };
    format!("{}/{}/{}", tm.tm_mon + 1, tm.tm_mday, tm.tm_year + 1900)
}

fn disk_title(i: usize, d: &DiskDev) -> String {
    if d.mounts.is_empty() { format!("Disk {i}") } else { format!("Disk {i} ({})", d.mounts.join(" ")) }
}

fn page_id(p: PerfPage) -> (u8, usize) {
    match p {
        PerfPage::Cpu => (0, 0),
        PerfPage::Memory => (1, 0),
        PerfPage::Disk(i) => (2, i),
        PerfPage::Net(i) => (3, i),
        PerfPage::Gpu => (4, 0),
    }
}

// ---- System sources ----

fn config_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"))
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// The compositor's windows, via the control socket. Empty outside VIDE-DE.
fn list_windows() -> Vec<Win> {
    let Some(text) = ipc_reply("windows") else { return Vec::new() };
    text.lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(5, '\t').collect();
            Some(Win { id: f.first()?.parse().ok()?, pid: f.get(1)?.parse().ok()?, app_id: f.get(3)?.to_string(), title: f.get(4)?.to_string() })
        })
        .collect()
}

fn ipc_reply(cmd: &str) -> Option<String> {
    let path = std::env::var_os("VIDE_IPC")?;
    let mut s = UnixStream::connect(path).ok()?;
    s.set_read_timeout(Some(Duration::from_millis(300))).ok()?;
    s.write_all(format!("{cmd}\n").as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

fn ipc(cmd: &str) {
    let _ = ipc_reply(cmd);
}

fn meminfo() -> HashMap<String, u64> {
    std::fs::read_to_string("/proc/meminfo")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            Some((k.to_string(), v.split_whitespace().next()?.parse().ok()?))
        })
        .collect()
}

/// Installed RAM as the firmware reports it, for "Hardware reserved".
fn physical_ram() -> u64 {
    // Memory blocks the kernel knows about, online or not.
    let Ok(size) = std::fs::read_to_string("/sys/devices/system/memory/block_size_bytes") else { return 0 };
    let Ok(block) = u64::from_str_radix(size.trim(), 16) else { return 0 };
    std::fs::read_dir("/sys/devices/system/memory").map(|rd| rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("memory")).count() as u64 * block).unwrap_or(0)
}

/// Per-device counters from /proc/diskstats: reads, sectors read, writes,
/// sectors written, ms doing I/O, ms spent on reads + writes.
fn diskstats() -> HashMap<String, [u64; 6]> {
    std::fs::read_to_string("/proc/diskstats")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            if f.len() < 14 {
                return None;
            }
            let n = |i: usize| f[i].parse::<u64>().unwrap_or(0);
            Some((f[2].to_string(), [n(3), n(5), n(7), n(9), n(12), n(6) + n(10)]))
        })
        .collect()
}

/// Physical disks, as Task Manager lists them: no loop, RAM or zram devices.
fn disk_devices() -> Vec<DiskDev> {
    let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
    let swaps = std::fs::read_to_string("/proc/swaps").unwrap_or_default();
    let disks = Disks::new_with_refreshed_list().ok();
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir("/sys/block") else { return out };
    let mut names: Vec<String> = rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    for dev in names {
        if ["loop", "ram", "zram", "dm-", "sr", "fd", "md"].iter().any(|p| dev.starts_with(p)) {
            continue;
        }
        let sys = format!("/sys/block/{dev}");
        let read = |f: &str| std::fs::read_to_string(format!("{sys}/{f}")).unwrap_or_default().trim().to_string();
        let size = read("size").parse::<u64>().unwrap_or(0) * 512;
        if size == 0 {
            continue;
        }
        // Partitions of this disk: sda1, nvme0n1p1...
        let on_disk = |path: &str| {
            let base = path.trim_start_matches("/dev/");
            base.starts_with(&dev) && base != dev || base == dev
        };
        let mut mounts_here = Vec::new();
        let mut system = false;
        for line in mounts.lines() {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() > 1 && on_disk(f[0]) && !mounts_here.contains(&f[1].to_string()) && (f[1] == "/" || f[1].starts_with("/home") || f[1].starts_with("/run/media") || f[1].starts_with("/mnt") || f[1].starts_with("/media")) {
                system |= f[1] == "/";
                mounts_here.push(f[1].replace("\\040", " "));
            }
        }
        let formatted = disks
            .as_ref()
            .map(|d| {
                let mut seen = HashSet::new();
                d.iter().filter(|x| on_disk(&x.name().to_string_lossy()) && seen.insert(x.name().to_owned())).map(|x| x.total_space()).sum()
            })
            .unwrap_or(0);
        out.push(DiskDev {
            model: read("device/model"),
            ssd: read("queue/rotational") == "0",
            size,
            mounts: mounts_here,
            formatted,
            system,
            page_file: swaps.lines().skip(1).any(|l| l.split_whitespace().next().is_some_and(on_disk)),
            dev,
            last: None,
            active: Hist::new(),
            read: Hist::new(),
            write: Hist::new(),
            resp_ms: 0.0,
        });
    }
    out
}

fn cpu_info(sys: &System) -> CpuInfo {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let field = |k: &str| cpuinfo.lines().find(|l| l.starts_with(k)).and_then(|l| l.split_once(':')).map(|(_, v)| v.trim().to_string());
    let brand = sys.cpus().first().map(|c| c.brand().trim().to_string()).filter(|b| !b.is_empty()).or_else(|| field("model name")).unwrap_or_default();
    let flags = field("flags").unwrap_or_default();
    let sockets = cpuinfo.lines().filter(|l| l.starts_with("physical id")).map(|l| l.to_string()).collect::<HashSet<_>>().len().max(1);
    let read_khz = |f: &str| std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu0/cpufreq/{f}")).ok().and_then(|s| s.trim().parse::<u64>().ok());
    // Base speed: Intel publishes it; otherwise the "@ x.xxGHz" in the name, then the max.
    let base = read_khz("base_frequency")
        .map(|k| format!("{:.2} GHz", k as f64 / 1e6))
        .or_else(|| brand.split('@').nth(1).map(|s| s.trim().replace("GHz", " GHz")))
        .or_else(|| read_khz("cpuinfo_max_freq").map(|k| format!("{:.2} GHz", k as f64 / 1e6)))
        .unwrap_or_default();
    let virt = if !(flags.contains(" vmx") || flags.contains(" svm")) {
        "Not capable"
    } else if Path::new("/dev/kvm").exists() {
        "Enabled"
    } else {
        "Disabled"
    };
    let logical = sys.cpus().len();
    // Each cache level's total: size × how many separate copies there are.
    let mut caches = [0u64; 3];
    if let Ok(rd) = std::fs::read_dir("/sys/devices/system/cpu/cpu0/cache") {
        for e in rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("index")) {
            let r = |f: &str| std::fs::read_to_string(e.path().join(f)).unwrap_or_default().trim().to_string();
            let level: usize = r("level").parse().unwrap_or(0);
            let size = r("size");
            let bytes = size.trim_end_matches(['K', 'M']).parse::<u64>().unwrap_or(0) * if size.ends_with('M') { 1 << 20 } else { 1024 };
            let shared = cpu_list_len(&r("shared_cpu_list")).max(1);
            if (1..=3).contains(&level) {
                caches[level - 1] += bytes * (logical / shared).max(1) as u64;
            }
        }
    }
    CpuInfo { brand, base, sockets, cores: System::physical_core_count().unwrap_or(logical), logical, virt, caches }
}

fn cpu_list_len(s: &str) -> usize {
    s.split(',')
        .map(|part| match part.split_once('-') {
            Some((a, b)) => b.parse::<usize>().unwrap_or(0).saturating_sub(a.parse().unwrap_or(0)) + 1,
            None if !part.is_empty() => 1,
            None => 0,
        })
        .sum()
}

/// GPU stats on a thread, since nvidia-smi takes a moment. AMD/Intel cards that
/// expose gpu_busy_percent are read from sysfs instead.
fn gpu_thread(out: Arc<Mutex<Option<GpuSample>>>) {
    std::thread::spawn(move || {
        let mut n = 0u64;
        let mut per_pid = HashMap::new();
        loop {
            let s = nvidia().or_else(amdgpu);
            let Some(mut s) = s else {
                // No GPU we can read; don't keep spawning nvidia-smi.
                return;
            };
            if n.is_multiple_of(2) {
                per_pid = nvidia_pmon();
            }
            s.per_pid = per_pid.clone();
            *out.lock().unwrap() = Some(s);
            n += 1;
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}

fn nvidia() -> Option<GpuSample> {
    let out = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=name,utilization.gpu,utilization.memory,utilization.encoder,utilization.decoder,memory.used,memory.total,temperature.gpu,driver_version,pci.bus_id", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let f: Vec<&str> = text.lines().next()?.split(',').map(str::trim).collect();
    if f.len() < 10 {
        return None;
    }
    let n = |i: usize| f[i].parse::<f32>().unwrap_or(0.0);
    // "00000000:01:00.0" → PCI bus 1, device 0, function 0.
    let bus = {
        let parts: Vec<&str> = f[9].rsplitn(3, [':', '.']).collect();
        match parts.as_slice() {
            [func, dev, rest] => {
                let b = u32::from_str_radix(rest.rsplit(':').next().unwrap_or("0"), 16).unwrap_or(0);
                format!("PCI bus {b}, device {}, function {}", u32::from_str_radix(dev, 16).unwrap_or(0), func)
            }
            _ => f[9].to_string(),
        }
    };
    Some(GpuSample {
        name: f[0].to_string(),
        util: n(1),
        copy: n(2),
        enc: n(3),
        dec: n(4),
        mem_used: (n(5) as u64) << 20,
        mem_total: (n(6) as u64) << 20,
        temp: Some(n(7)),
        driver: f[8].to_string(),
        bus,
        per_pid: HashMap::new(),
    })
}

fn nvidia_pmon() -> HashMap<u32, f32> {
    let Ok(out) = std::process::Command::new("nvidia-smi").args(["pmon", "-c", "1", "-s", "u"]).output() else { return HashMap::new() };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| !l.starts_with('#'))
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            Some((f.get(1)?.parse().ok()?, f.get(3)?.parse().ok()?))
        })
        .collect()
}

fn amdgpu() -> Option<GpuSample> {
    for e in std::fs::read_dir("/sys/class/drm").ok()?.flatten() {
        let dev = e.path().join("device");
        let Ok(busy) = std::fs::read_to_string(dev.join("gpu_busy_percent")) else { continue };
        let r = |f: &str| std::fs::read_to_string(dev.join(f)).ok().and_then(|s| s.trim().parse::<u64>().ok()).unwrap_or(0);
        let name = std::fs::read_to_string(dev.join("product_name")).map(|s| s.trim().to_string()).unwrap_or_else(|_| "GPU".into());
        return Some(GpuSample {
            name,
            util: busy.trim().parse().unwrap_or(0.0),
            mem_used: r("mem_info_vram_used"),
            mem_total: r("mem_info_vram_total"),
            bus: std::fs::read_link(&dev).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default(),
            ..Default::default()
        });
    }
    None
}

/// "Last BIOS time" from systemd-analyze's firmware figure.
fn bios_seconds() -> Option<String> {
    let out = std::process::Command::new("systemd-analyze").output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let part = text.split('+').chain(text.split("in ")).find(|p| p.contains("(firmware)"))?;
    let v = part.trim().split_whitespace().next()?.trim_start_matches("in ");
    // "7.322s" or "1min 2.1s".
    let secs = v.strip_suffix('s')?.parse::<f32>().ok()?;
    Some(format!("{secs:.1} seconds"))
}

fn services() -> Vec<Service> {
    let Ok(out) = std::process::Command::new("systemctl").args(["list-units", "--type=service", "--all", "--no-legend", "--plain", "--no-pager"]).output() else { return Vec::new() };
    // Each service's main process, from the cgroup every process sits in.
    let mut pids: HashMap<String, u32> = HashMap::new();
    if let Ok(rd) = std::fs::read_dir("/proc") {
        for e in rd.flatten() {
            let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
            let Ok(cg) = std::fs::read_to_string(e.path().join("cgroup")) else { continue };
            if let Some(unit) = cg.lines().find_map(|l| l.rsplit('/').find(|s| s.ends_with(".service"))) {
                let entry = pids.entry(unit.to_string()).or_insert(pid);
                *entry = (*entry).min(pid);
            }
        }
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let unit = f.next()?.to_string();
            let _load = f.next()?;
            let active = f.next()?;
            let sub = f.next()?;
            let description = f.collect::<Vec<_>>().join(" ");
            let running = active == "active" && sub == "running";
            let name = unit.trim_end_matches(".service").to_string();
            let group = if unit.contains('@') { unit.split('@').next().unwrap_or("").to_string() } else { String::new() };
            Some(Service { pid: if running { pids.get(&unit).copied() } else { None }, name, description, running, group })
        })
        .collect()
}

fn startup_items(apps: &AppIndex) -> Vec<StartupItem> {
    let user_dir = config_dir().join("autostart");
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    // System entries first; the user's copies of the same file override them.
    for dir in [PathBuf::from("/etc/xdg/autostart"), user_dir] {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.ends_with(".desktop") {
                    files.insert(n, e.path());
                }
            }
        }
    }
    let mut out = Vec::new();
    for (file, path) in files {
        let Some(entry) = DesktopEntry::read(&path) else { continue };
        // Entries for other desktops never run here, so they don't belong in the list.
        if let Some(only) = &entry.only_show_in
            && !only.split(';').any(|d| d.eq_ignore_ascii_case("VIDE") || d.eq_ignore_ascii_case("VIDE-DE"))
        {
            continue;
        }
        let enabled = !entry.hidden && entry.autostart_enabled;
        let icon = entry.icon.as_deref().and_then(resolve_icon).or_else(|| apps.by_exec(&entry.exec).and_then(|a| a.icon.clone()));
        out.push(StartupItem { name: entry.name, publisher: entry.publisher, enabled, file, path, exec: entry.exec, icon });
    }
    out
}

/// Enable or disable an autostart entry the XDG way: a copy in ~/.config/autostart
/// with Hidden= set, which shadows the system one.
fn set_autostart(item: &StartupItem, enabled: bool) {
    let dst = config_dir().join("autostart").join(&item.file);
    let text = std::fs::read_to_string(&item.path).unwrap_or_default();
    let mut lines: Vec<String> = text.lines().filter(|l| !l.starts_with("Hidden=") && !l.starts_with("X-GNOME-Autostart-enabled=")).map(String::from).collect();
    let at = lines.iter().position(|l| l.trim() == "[Desktop Entry]").map(|i| i + 1).unwrap_or(0);
    lines.insert(at, format!("Hidden={}", !enabled));
    if let Some(d) = dst.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(dst, lines.join("\n") + "\n");
    let _ = &item.exec;
}

fn set_priority(pid: u32, nice: i32) {
    // Raising priority needs root; pkexec asks through the session's polkit agent.
    let mut cmd = if nice < 0 { std::process::Command::new("pkexec") } else { std::process::Command::new("renice") };
    if nice < 0 {
        cmd.arg("renice");
    }
    let _ = cmd.args(["-n", &nice.to_string(), "-p", &pid.to_string()]).spawn();
}

fn run_task(text: &str, admin: bool) {
    if text.is_empty() {
        return;
    }
    let expanded = match text.strip_prefix('~') {
        Some(rest) => format!("{}{rest}", home().display()),
        None => text.to_string(),
    };
    if Path::new(&expanded).exists() && !Path::new(&expanded).is_file() || expanded.contains("://") {
        let _ = std::process::Command::new("xdg-open").arg(&expanded).spawn();
        return;
    }
    let mut cmd = if admin { std::process::Command::new("pkexec") } else { std::process::Command::new("sh") };
    if admin {
        cmd.args(["sh", "-c", &expanded]);
    } else {
        cmd.args(["-c", &expanded]);
    }
    let _ = cmd.spawn();
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(bin).exists()))
}

fn sibling(bin: &str) -> Option<PathBuf> {
    let ours = std::env::current_exe().ok().and_then(|p| Some(p.parent()?.join(bin))).filter(|p| p.exists());
    ours.or_else(|| std::env::var_os("PATH").and_then(|p| std::env::split_paths(&p).map(|d| d.join(bin)).find(|p| p.exists())))
}

/// Run a command in our terminal (PowerSlop, else Konsole).
fn spawn_terminal(argv: &[&str]) {
    let mut cmd = match sibling("powerslop") {
        Some(p) => {
            let mut c = std::process::Command::new(p);
            c.arg("-e");
            c
        }
        None => {
            let mut c = std::process::Command::new("konsole");
            c.args(["--hold", "-e"]);
            c
        }
    };
    let _ = cmd.args(argv).spawn();
}

/// Show a file's folder in File Exploder (or whatever opens folders).
fn open_location(path: &Path) {
    let dir = if path.is_dir() { path } else { path.parent().unwrap_or(Path::new("/")) };
    match sibling("file-exploder") {
        Some(fe) => {
            let _ = std::process::Command::new(fe).arg(dir).spawn();
        }
        None => {
            let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
        }
    }
}

fn search_online(q: &str) {
    let q: String = q.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c.to_string() } else if c == ' ' { "+".into() } else { format!("%{:02X}", c as u32 & 0xff) }).collect();
    let _ = std::process::Command::new("xdg-open").arg(format!("https://www.bing.com/search?q={q}")).spawn();
}

// ---- Apps, icons ----

#[derive(Clone)]
struct AppInfo {
    name: String,
    icon: Option<String>,
}

/// Installed .desktop entries, so processes show "Firefox" with its icon, not "firefox".
struct AppIndex {
    by_key: HashMap<String, AppInfo>,
}

impl AppIndex {
    fn load() -> Self {
        let mut by_key = HashMap::new();
        let mut dirs = vec![PathBuf::from("/usr/share/applications"), PathBuf::from("/var/lib/flatpak/exports/share/applications")];
        dirs.push(home().join(".local/share/flatpak/exports/share/applications"));
        dirs.push(home().join(".local/share/applications"));
        for dir in dirs {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            for e in rd.flatten() {
                let path = e.path();
                if path.extension().is_none_or(|x| x != "desktop") {
                    continue;
                }
                let Some(d) = DesktopEntry::read(&path) else { continue };
                let info = AppInfo { name: d.name.clone(), icon: d.icon.as_deref().and_then(resolve_icon) };
                let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
                let mut keys = vec![stem.clone()];
                if let Some(last) = stem.rsplit('.').next() {
                    keys.push(last.to_string());
                }
                if let Some(c) = &d.wm_class {
                    keys.push(c.to_lowercase());
                }
                if let Some(bin) = exec_bin(&d.exec) {
                    keys.push(bin);
                }
                for k in keys {
                    // Earlier (system) entries win over later duplicates only if the later one has no icon.
                    if !by_key.contains_key(&k) || info.icon.is_some() {
                        by_key.insert(k, info.clone());
                    }
                }
            }
        }
        AppIndex { by_key }
    }

    fn by_app_id(&self, app_id: &str) -> Option<&AppInfo> {
        let id = app_id.to_lowercase();
        if id.is_empty() {
            return None;
        }
        self.by_key.get(&id).or_else(|| self.by_key.get(id.rsplit('.').next().unwrap_or(&id)))
    }

    fn by_proc(&self, p: &Proc) -> Option<&AppInfo> {
        let exe = p.exe.as_ref().and_then(|e| e.file_name()).map(|n| n.to_string_lossy().to_lowercase());
        exe.as_deref().and_then(|e| self.by_key.get(e)).or_else(|| self.by_key.get(&p.name.to_lowercase()))
    }

    fn by_exec(&self, exec: &str) -> Option<&AppInfo> {
        exec_bin(exec).and_then(|b| self.by_key.get(&b))
    }

    fn by_name(&self, name: &str) -> Option<&AppInfo> {
        self.by_key.values().find(|a| a.name == name)
    }
}

/// The program an Exec= line runs, without env assignments or wrappers.
fn exec_bin(exec: &str) -> Option<String> {
    let mut parts = exec.split_whitespace().filter(|p| !p.contains('=') && !p.starts_with('%'));
    let mut first = parts.next()?;
    if first == "env" || first.ends_with("/env") {
        first = parts.next()?;
    }
    Some(Path::new(first.trim_matches('"')).file_name()?.to_string_lossy().to_lowercase())
}

struct DesktopEntry {
    name: String,
    exec: String,
    icon: Option<String>,
    wm_class: Option<String>,
    publisher: String,
    hidden: bool,
    autostart_enabled: bool,
    only_show_in: Option<String>,
}

impl DesktopEntry {
    fn read(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let mut d = DesktopEntry { name: String::new(), exec: String::new(), icon: None, wm_class: None, publisher: String::new(), hidden: false, autostart_enabled: true, only_show_in: None };
        let mut in_entry = false;
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_entry = line == "[Desktop Entry]";
                continue;
            }
            if !in_entry {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            match k.trim() {
                "Name" => d.name = v.trim().to_string(),
                "Exec" => d.exec = v.trim().to_string(),
                "Icon" => d.icon = Some(v.trim().to_string()),
                "StartupWMClass" => d.wm_class = Some(v.trim().to_string()),
                "X-KDE-Publisher" | "X-Publisher" => d.publisher = v.trim().to_string(),
                "Hidden" => d.hidden = v.trim() == "true",
                "X-GNOME-Autostart-enabled" => d.autostart_enabled = v.trim() != "false",
                "OnlyShowIn" => d.only_show_in = Some(v.trim().to_string()),
                _ => {}
            }
        }
        (!d.name.is_empty()).then_some(d)
    }
}

fn icon_path(name: &str) -> Option<String> {
    resolve_icon(name)
}

/// A theme icon's file, preferring small sizes (these draw at 16px). Cached.
fn resolve_icon(name: &str) -> Option<String> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    cache
        .entry(name.to_string())
        .or_insert_with(|| {
            if name.starts_with('/') {
                return Path::new(name).exists().then(|| name.to_string());
            }
            let h = home();
            let roots = [
                PathBuf::from("/usr/share/icons"),
                h.join(".local/share/icons"),
                PathBuf::from("/var/lib/flatpak/exports/share/icons"),
                h.join(".local/share/flatpak/exports/share/icons"),
            ];
            let mut candidates = Vec::new();
            for root in &roots {
                for s in ["32", "22", "24", "48", "16", "64"] {
                    for d in ["apps", "places", "mimetypes", "devices", "actions", "status"] {
                        candidates.push(root.join(format!("breeze/{d}/{s}/{name}.svg")));
                    }
                }
                for s in ["32x32", "48x48", "24x24", "22x22", "16x16", "64x64", "128x128", "256x256"] {
                    candidates.push(root.join(format!("hicolor/{s}/apps/{name}.png")));
                }
                candidates.push(root.join(format!("hicolor/scalable/apps/{name}.svg")));
            }
            candidates.push(PathBuf::from(format!("/usr/share/pixmaps/{name}.png")));
            candidates.push(PathBuf::from(format!("/usr/share/pixmaps/{name}.svg")));
            candidates.into_iter().find(|p| p.exists()).map(|p| p.display().to_string())
        })
        .clone()
}

// ---- App history ----

/// CPU time per app, kept across runs in ~/.local/state/vide-de/app-history so the
/// App history tab has something to show, like Windoze' usage database.
struct AppHistory {
    since: SystemTime,
    base: BTreeMap<String, u64>,
    /// pid → (app name, CPU ms counted so far, CPU ms when counting started).
    live: HashMap<u32, (String, u64, u64)>,
    saved: Instant,
}

impl AppHistory {
    fn path() -> PathBuf {
        let state = std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/state"));
        state.join("vide-de/app-history")
    }

    fn load() -> Self {
        let mut h = AppHistory { since: SystemTime::now(), base: BTreeMap::new(), live: HashMap::new(), saved: Instant::now() };
        if let Ok(text) = std::fs::read_to_string(Self::path()) {
            for line in text.lines() {
                if let Some(s) = line.strip_prefix("since\t") {
                    h.since = SystemTime::UNIX_EPOCH + Duration::from_secs(s.parse().unwrap_or(0));
                } else if let Some((n, ms)) = line.rsplit_once('\t') {
                    h.base.insert(n.to_string(), ms.parse().unwrap_or(0));
                }
            }
        }
        h
    }

    fn update(&mut self, procs: &HashMap<u32, Proc>, apps: &AppIndex, uid: u32) {
        // Processes that exited: their time moves into the stored totals.
        let gone: Vec<u32> = self.live.keys().filter(|p| !procs.contains_key(p)).copied().collect();
        for pid in gone {
            if let Some((name, ms, start)) = self.live.remove(&pid) {
                *self.base.entry(name).or_default() += ms.saturating_sub(start);
            }
        }
        for p in procs.values().filter(|p| p.uid == uid) {
            let Some(app) = apps.by_proc(p) else { continue };
            let e = self.live.entry(p.pid).or_insert_with(|| (app.name.clone(), p.cpu_ms, 0));
            e.1 = p.cpu_ms;
        }
        if self.saved.elapsed() > Duration::from_secs(30) {
            self.save();
        }
    }

    fn totals(&self) -> Vec<(String, u64)> {
        let mut t = self.base.clone();
        for (name, ms, start) in self.live.values() {
            *t.entry(name.clone()).or_default() += ms.saturating_sub(*start);
        }
        t.into_iter().collect()
    }

    fn clear(&mut self, procs: &HashMap<u32, Proc>) {
        self.base.clear();
        self.since = SystemTime::now();
        for (pid, e) in self.live.iter_mut() {
            e.2 = procs.get(pid).map(|p| p.cpu_ms).unwrap_or(e.1);
        }
        self.save();
    }

    fn save(&mut self) {
        self.saved = Instant::now();
        let since = self.since.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let mut text = format!("since\t{since}\n");
        for (n, ms) in self.totals() {
            text += &format!("{n}\t{ms}\n");
        }
        let p = Self::path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = std::fs::write(p, text);
    }
}
