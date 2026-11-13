//! Installing and removing apps. Flatpak's output is read only to drive a progress bar;
//! it never reaches the UI. Installs go to the user's own installation, so no password.

use std::{
    collections::HashSet,
    io::Read,
    process::{Command, Stdio},
    sync::mpsc,
};

#[derive(Clone, Debug)]
pub struct Installed {
    pub id: String,
    pub name: String,
    pub version: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Install,
    Uninstall,
    Update,
    UpdateAll,
}

#[derive(Debug)]
pub enum TaskEvent {
    Progress { percent: Option<f32>, phase: &'static str },
    Done { ok: bool },
}

/// Flatpak ids are dotted names; reject anything else before it reaches a command line.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() < 200 && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn output(args: &[&str]) -> Option<String> {
    let out = Command::new("flatpak").args(args).stdin(Stdio::null()).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn list_installed() -> Vec<Installed> {
    let Some(text) = output(&["list", "--app", "--columns=application,name,version"]) else { return Vec::new() };
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let id = f.next()?.trim();
            valid_id(id).then(|| Installed {
                id: id.to_string(),
                name: f.next().unwrap_or(id).trim().to_string(),
                version: f.next().unwrap_or_default().trim().to_string(),
            })
        })
        .collect()
}

pub fn list_updates() -> HashSet<String> {
    output(&["remote-ls", "--updates", "--app", "--columns=application"])
        .map(|t| t.lines().map(|l| l.trim().to_string()).filter(|l| valid_id(l)).collect())
        .unwrap_or_default()
}

/// Local copy of an installed app's icon, if flatpak exported one.
pub fn local_icon(id: &str) -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    let roots = [format!("{home}/.local/share/flatpak/exports/share/icons"), "/var/lib/flatpak/exports/share/icons".to_string()];
    for root in roots {
        for size in ["128x128", "256x256", "64x64", "512x512", "48x48"] {
            let p = format!("{root}/hicolor/{size}/apps/{id}.png");
            if std::path::Path::new(&p).exists() {
                return Some(p);
            }
        }
    }
    None
}

pub fn launch(id: &str) {
    if valid_id(id) {
        let _ = Command::new("flatpak").args(["run", id]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
}

/// The user-level Flathub remote's name, adding it if this account doesn't have one.
fn user_remote() -> Option<String> {
    let remotes = output(&["remotes", "--user", "--columns=name,url"]).unwrap_or_default();
    for line in remotes.lines() {
        let mut f = line.split('\t');
        if let (Some(name), Some(url)) = (f.next(), f.next()) {
            if url.contains("flathub.org") {
                return Some(name.trim().to_string());
            }
        }
    }
    let ok = Command::new("flatpak")
        .args(["remote-add", "--user", "--if-not-exists", "flathub", "https://dl.flathub.org/repo/flathub.flatpakrepo"])
        .stdin(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    ok.then(|| "flathub".to_string())
}

/// "45%" anywhere in a line of flatpak's output.
fn percent_in(line: &str) -> Option<f32> {
    let end = line.rfind('%')?;
    let digits: String = line[..end].chars().rev().take_while(|c| c.is_ascii_digit()).collect::<Vec<_>>().into_iter().rev().collect();
    digits.parse::<f32>().ok().filter(|p| *p <= 100.0)
}

fn phase_in(line: &str, default: &'static str) -> &'static str {
    let l = line.to_ascii_lowercase();
    if l.contains("download") || l.contains("pulling") || l.contains("fetch") {
        "Downloading"
    } else if l.contains("uninstall") {
        "Removing"
    } else if l.contains("updating") {
        "Updating"
    } else if l.contains("installing") {
        "Installing"
    } else {
        default
    }
}

/// Run a task to completion, reporting progress. Blocks; call it from a worker thread.
pub fn run(kind: Kind, id: &str, emit: &dyn Fn(TaskEvent)) {
    let default = match kind {
        Kind::Install => "Installing",
        Kind::Uninstall => "Removing",
        Kind::Update | Kind::UpdateAll => "Updating",
    };
    emit(TaskEvent::Progress { percent: None, phase: default });
    let mut args: Vec<String> = match kind {
        Kind::Install => {
            let Some(remote) = user_remote() else { return emit(TaskEvent::Done { ok: false }) };
            vec!["install".into(), "--user".into(), "-y".into(), "--noninteractive".into(), remote, id.into()]
        }
        Kind::Uninstall => vec!["uninstall".into(), "-y".into(), "--noninteractive".into(), id.into()],
        Kind::Update => vec!["update".into(), "-y".into(), "--noninteractive".into(), id.into()],
        Kind::UpdateAll => vec!["update".into(), "-y".into(), "--noninteractive".into()],
    };
    if !matches!(kind, Kind::UpdateAll) && !valid_id(id) {
        return emit(TaskEvent::Done { ok: false });
    }
    args.shrink_to_fit();

    let child = Command::new("flatpak").args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    let Ok(mut child) = child else { return emit(TaskEvent::Done { ok: false }) };

    // Both streams, split on newlines and carriage returns (progress redraws in place).
    let (line_tx, line_rx) = mpsc::channel::<String>();
    let mut readers = Vec::new();
    for mut stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)].into_iter().flatten() {
        let line_tx = line_tx.clone();
        readers.push(std::thread::spawn(move || {
            let (mut buf, mut line) = ([0u8; 512], Vec::new());
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                for &b in &buf[..n] {
                    if b == b'\n' || b == b'\r' {
                        if !line.is_empty() {
                            let _ = line_tx.send(String::from_utf8_lossy(&line).into_owned());
                            line.clear();
                        }
                    } else {
                        line.push(b);
                    }
                }
            }
        }));
    }
    drop(line_tx);

    let mut tail: Vec<String> = Vec::new();
    let mut phase = default;
    let mut percent = None;
    for line in line_rx {
        phase = phase_in(&line, phase);
        percent = percent_in(&line).map(|p| p / 100.0).or(percent);
        emit(TaskEvent::Progress { percent, phase });
        tail.push(line);
        if tail.len() > 20 {
            tail.remove(0);
        }
    }
    for r in readers {
        let _ = r.join();
    }
    let ok = child.wait().is_ok_and(|s| s.success());
    if !ok {
        // For whoever launched us from a terminal; never shown in the window.
        eprintln!("window-shopping: {kind:?} {id} failed:\n{}", tail.join("\n"));
    }
    emit(TaskEvent::Done { ok });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_percent() {
        assert!(valid_id("net.leatherclient.LeatherClient"));
        assert!(!valid_id("x; rm -rf ~"));
        assert_eq!(percent_in("Installing… ██░░ 45%"), Some(45.0));
        assert_eq!(percent_in("no progress here"), None);
    }
}
