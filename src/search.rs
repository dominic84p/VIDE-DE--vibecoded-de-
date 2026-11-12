//! What the start menu lists. Empty query: pinned apps, most used, then every app
//! under A–Z headers, like Windows 10. Typing: a calculator answer, apps (fuzzy,
//! ranked by how often you launch them), Settings pages, commands, then files.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use crate::{apps::App, pins::Pins, ui::PowerAction};

#[derive(Clone, PartialEq, Debug)]
pub enum Entry {
    Header(String),
    App(usize),
    /// A Settings page: (title, `--page` name).
    Setting(&'static str, &'static str),
    File(PathBuf),
    /// "= 42": Enter copies the answer.
    Calc(String),
    Power(PowerAction),
    /// "Search the web" for the query.
    Web(String),
}

/// The search pane's tabs, as in Windows 10.
pub const FILTERS: [&str; 5] = ["All", "Apps", "Documents", "Settings", "Web"];

/// Windows' search layout: the top hit under "Best match", then the rest by kind,
/// limited to one kind when a tab other than All is picked.
pub fn arrange(entries: Vec<Entry>, filter: usize) -> Vec<Entry> {
    let keep = |e: &Entry| match filter {
        1 => matches!(e, Entry::App(_)),
        2 => matches!(e, Entry::File(p) if !p.is_dir()),
        3 => matches!(e, Entry::Setting(..)),
        4 => matches!(e, Entry::Web(_)),
        _ => true,
    };
    let mut groups: Vec<(String, Vec<Entry>)> = Vec::new();
    for e in entries {
        match e {
            Entry::Header(h) => groups.push((h, Vec::new())),
            e if keep(&e) => match groups.last_mut() {
                Some(g) => g.1.push(e),
                None => groups.push((String::new(), vec![e])),
            },
            _ => {}
        }
    }
    let mut out = Vec::new();
    // Web results are the best match only when nothing else matched.
    let best = groups.iter().position(|g| !g.1.is_empty() && g.0 != "Search the web").or_else(|| groups.iter().position(|g| !g.1.is_empty()));
    if let Some(g) = best {
        out.push(Entry::Header("Best match".into()));
        out.push(groups[g].1.remove(0));
    }
    for (h, items) in groups {
        if items.is_empty() {
            continue;
        }
        let h = if h == "Files and folders" && filter == 2 { "Documents".into() } else { h };
        out.push(Entry::Header(h));
        out.extend(items);
    }
    out
}

/// "Top apps" when the search pane opens empty: the most launched, else pinned.
pub fn top_apps(apps: &[App], pins: &Pins) -> Vec<Entry> {
    let mut used: Vec<(usize, u32)> = pins.launches.iter().filter_map(|(id, &n)| apps.iter().position(|a| a.id == *id).map(|i| (i, n))).collect();
    used.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut out: Vec<usize> = used.into_iter().map(|(i, _)| i).collect();
    for id in &pins.start {
        if let Some(i) = apps.iter().position(|a| a.id == *id)
            && !out.contains(&i)
        {
            out.push(i);
        }
    }
    out.truncate(8);
    out.into_iter().map(Entry::App).collect()
}

impl Entry {
    pub fn selectable(&self) -> bool {
        !matches!(self, Entry::Header(_))
    }
}

/// Settings pages search can jump to, with words people might type for them.
const SETTINGS: &[(&str, &str, &str)] = &[
    ("Personalization", "personalization", "wallpaper background picture theme colors"),
    ("Display", "display", "screen resolution refresh rate scale scaling night light monitor"),
    ("Sound", "sound", "audio volume speakers headphones output microphone"),
    ("Network & Wi-Fi", "network", "wifi wireless internet ethernet"),
    ("Bluetooth", "bluetooth", "devices headphones pair"),
    ("Mouse", "mouse", "pointer acceleration speed cursor"),
    ("Gaming", "gaming", "ultra performance mode direct scanout fps games"),
    ("Start", "start", "kde apps menu search"),
    ("About", "about", "system info version pc"),
];

const COMMANDS: &[(&str, &str)] = &[
    ("shut down", "shutdown power off turn off"),
    ("restart", "reboot"),
    ("sign out", "log out logout log off"),
];

/// How well `query` matches `hay`; None if it doesn't. Whole-substring matches beat
/// scattered letters, and matching at the start of a word beats the middle.
pub fn fuzzy(hay: &str, query: &str) -> Option<i32> {
    let h = hay.to_lowercase();
    let q = query.to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    if let Some(at) = h.find(&q) {
        let word_start = at == 0 || !h[..at].chars().last().is_some_and(char::is_alphanumeric);
        return Some(200 + if at == 0 { 100 } else { 0 } + if word_start { 50 } else { 0 } - h.len().min(50) as i32);
    }
    // Letters in order, e.g. "fexp" for "File Exploder". Initials score highest.
    let mut score = 0;
    let mut chars = h.char_indices().peekable();
    let mut prev_end = 0;
    for qc in q.chars() {
        if qc == ' ' {
            continue;
        }
        loop {
            let (i, c) = chars.next()?;
            if c == qc {
                let word_start = i == 0 || !h[..i].chars().last().is_some_and(char::is_alphanumeric);
                score += if word_start { 20 } else if i == prev_end { 10 } else { 1 };
                prev_end = i + c.len_utf8();
                break;
            }
        }
    }
    (score >= q.len() as i32 * 3).then_some(score)
}

pub fn results(query: &str, apps: &[App], hide_kde: bool, pins: &Pins, files: &FileIndex) -> Vec<Entry> {
    let visible = |a: &&App| !(hide_kde && a.kde);
    let find = |id: &str| apps.iter().position(|a| a.id == id);
    let mut out = Vec::new();
    let q = query.trim();

    if q.is_empty() {
        // Pinned apps are Start's tiles, as in Windows 10, so the list leaves them in A-Z only.
        let pinned: Vec<usize> = Vec::new();
        let mut used: Vec<(usize, u32)> = pins
            .launches
            .iter()
            .filter_map(|(id, &n)| find(id).map(|i| (i, n)))
            .filter(|(i, _)| !pinned.contains(i) && visible(&&apps[*i]))
            .collect();
        used.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        if !used.is_empty() {
            out.push(Entry::Header("Most used".into()));
            out.extend(used.iter().take(5).map(|&(i, _)| Entry::App(i)));
        }
        let mut all: Vec<usize> = apps.iter().enumerate().filter(|(_, a)| visible(a)).map(|(i, _)| i).collect();
        all.sort_by_key(|&i| apps[i].name.to_lowercase());
        let mut letter = None;
        for i in all {
            let first = apps[i].name.chars().next().map(|c| if c.is_alphabetic() { c.to_ascii_uppercase() } else { '#' });
            if first != letter {
                letter = first;
                out.push(Entry::Header(first.unwrap_or('#').to_string()));
            }
            out.push(Entry::App(i));
        }
        return out;
    }

    if let Some(v) = calc(q) {
        out.push(Entry::Header("Calculator".into()));
        out.push(Entry::Calc(format_number(v)));
    }

    let mut ranked: Vec<(i32, usize)> = apps
        .iter()
        .enumerate()
        .filter(|(_, a)| visible(a))
        .filter_map(|(i, a)| {
            let s = fuzzy(&a.name, q).or_else(|| fuzzy(&a.id, q).map(|s| s / 2))?;
            let used = pins.launches.get(&a.id).copied().unwrap_or(0) as i32;
            Some((s + (used * 8).min(80), i))
        })
        .collect();
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(apps[a.1].name.cmp(&apps[b.1].name)));
    if !ranked.is_empty() {
        out.push(Entry::Header("Apps".into()));
        out.extend(ranked.iter().take(8).map(|&(_, i)| Entry::App(i)));
    }

    let settings: Vec<Entry> = SETTINGS
        .iter()
        .filter(|(title, _, words)| fuzzy(title, q).is_some() || words.split(' ').any(|w| w.starts_with(&q.to_lowercase())))
        .map(|&(title, page, _)| Entry::Setting(title, page))
        .collect();
    if !settings.is_empty() {
        out.push(Entry::Header("Settings".into()));
        out.extend(settings);
    }

    let commands: Vec<Entry> = COMMANDS
        .iter()
        .filter(|(name, words)| {
            let ql = q.to_lowercase();
            name.starts_with(&ql) || words.split(' ').any(|w| w.starts_with(&ql)) && ql.len() >= 3
        })
        .map(|(name, _)| match *name {
            "shut down" => Entry::Power(PowerAction::ShutDown),
            "restart" => Entry::Power(PowerAction::Restart),
            _ => Entry::Power(PowerAction::SignOut),
        })
        .collect();
    if !commands.is_empty() {
        out.push(Entry::Header("Commands".into()));
        out.extend(commands);
    }

    let files = files.search(q, 8);
    if !files.is_empty() {
        out.push(Entry::Header("Files and folders".into()));
        out.extend(files.into_iter().map(Entry::File));
    }
    out.push(Entry::Header("Search the web".into()));
    out.push(Entry::Web(q.to_string()));
    out
}

// ---------------------------------------------------------------- file index

/// Names of files under ~, built on a background thread when Start opens, so typing
/// never waits on the disk.
#[derive(Clone, Default)]
pub struct FileIndex {
    inner: Arc<Mutex<(Vec<PathBuf>, Option<Instant>, bool)>>,
}

const MAX_FILES: usize = 60_000;
const SKIP: &[&str] = &["node_modules", "target", "__pycache__", "venv", "Steam", "steamapps", "snap"];

impl FileIndex {
    /// Rebuild if the index is missing or more than a minute old.
    pub fn refresh(&self) {
        {
            let mut g = self.inner.lock().unwrap();
            if g.2 || g.1.is_some_and(|t| t.elapsed() < Duration::from_secs(60)) {
                return;
            }
            g.2 = true;
        }
        let inner = self.inner.clone();
        std::thread::spawn(move || {
            let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
            let mut found = Vec::new();
            walk(&home, 0, &mut found);
            let mut g = inner.lock().unwrap();
            *g = (found, Some(Instant::now()), false);
        });
    }

    pub fn search(&self, query: &str, max: usize) -> Vec<PathBuf> {
        let q = query.to_lowercase();
        if q.len() < 2 {
            return Vec::new();
        }
        let g = self.inner.lock().unwrap();
        let mut hits: Vec<(i32, &PathBuf)> = g
            .0
            .iter()
            .filter_map(|p| {
                let name = p.file_name()?.to_string_lossy().to_lowercase();
                let at = name.find(&q)?;
                // Prefix matches first, then shallower paths, then shorter names.
                let depth = p.components().count() as i32;
                Some((if at == 0 { 1000 } else { 0 } - depth * 10 - name.len() as i32, p))
            })
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0));
        hits.into_iter().take(max).map(|(_, p)| p.clone()).collect()
    }
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 6 || out.len() >= MAX_FILES {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') || SKIP.contains(&name.as_ref()) {
            continue;
        }
        let path = e.path();
        let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
        out.push(path.clone());
        if is_dir {
            walk(&path, depth + 1, out);
        }
    }
}

// ---------------------------------------------------------------- calculator

/// Evaluate arithmetic like "12*(3+4)/2^2". Only answers when there's an operator,
/// so typing a plain number doesn't hijack the results.
pub fn calc(s: &str) -> Option<f64> {
    let s = s.strip_prefix('=').unwrap_or(s).replace(['x', '×'], "*").replace('÷', "/").replace(',', "");
    if !s.chars().any(|c| "+-*/^%".contains(c)) || !s.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let toks: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    let mut p = Parser { t: &toks, i: 0 };
    let v = p.expr()?;
    (p.i == toks.len() && v.is_finite()).then_some(v)
}

struct Parser<'a> {
    t: &'a [char],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.t.get(self.i).copied()
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = if op == '+' { v + r } else { v - r };
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.power()?;
        while let Some(op @ ('*' | '/' | '%')) = self.peek() {
            self.i += 1;
            let r = self.power()?;
            v = match op {
                '*' => v * r,
                '/' => v / r,
                _ => v % r,
            };
        }
        Some(v)
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.unary()?;
        if self.peek() == Some('^') {
            self.i += 1;
            // Right-associative: 2^3^2 = 2^9.
            return Some(base.powf(self.power()?));
        }
        Some(base)
    }

    fn unary(&mut self) -> Option<f64> {
        if self.peek() == Some('-') {
            self.i += 1;
            return Some(-self.unary()?);
        }
        self.atom()
    }

    fn atom(&mut self) -> Option<f64> {
        if self.peek() == Some('(') {
            self.i += 1;
            let v = self.expr()?;
            if self.peek() == Some(')') {
                self.i += 1;
            }
            return Some(v);
        }
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
            self.i += 1;
        }
        self.t[start..self.i].iter().collect::<String>().parse().ok()
    }
}

pub fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculator() {
        assert_eq!(calc("2+3*4"), Some(14.0));
        assert_eq!(calc("(2+3)*4"), Some(20.0));
        assert_eq!(calc("2^3^2"), Some(512.0));
        assert_eq!(calc("-4+10/4"), Some(-1.5));
        assert_eq!(calc("42"), None);
        assert_eq!(calc("firefox"), None);
        assert_eq!(format_number(0.1 + 0.2), "0.3");
    }

    #[test]
    fn fuzzy_ranks() {
        assert!(fuzzy("Firefox", "fire").unwrap() > fuzzy("Bonfire", "fire").unwrap());
        assert!(fuzzy("File Exploder", "fexp").is_some());
        assert!(fuzzy("Konsole", "xyz").is_none());
    }
}
