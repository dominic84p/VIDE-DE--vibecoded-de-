//! Your own notification sounds: pick a file with the desktop's file chooser (the
//! xdg-desktop-portal one, so it's the same dialog every app uses), and it's copied into
//! `vide_config::custom_sounds_dir()`, where Settings and the compositor both find it.

use std::{collections::HashMap, path::PathBuf, sync::mpsc};

use zbus::zvariant::{OwnedValue, Value};

/// Ask the portal for a sound file, on a thread (the dialog is modal for as long as you
/// browse). The answer is the new sound's id, or why there isn't one; `None` if you cancelled.
pub fn pick() -> mpsc::Receiver<Option<Result<String, String>>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let answer = match open_file() {
            Ok(Some(src)) => Some(import(&src)),
            Ok(None) => None,
            Err(e) => Some(Err(format!("Couldn't open the file picker: {e}"))),
        };
        let _ = tx.send(answer);
    });
    rx
}

/// Copy a sound into our folder, keeping its name (numbered if one's already there).
fn import(src: &PathBuf) -> Result<String, String> {
    let dir = vide_config::custom_sounds_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn't make {}: {e}", dir.display()))?;
    let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "sound".into());
    let ext = src.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !vide_config::CUSTOM_SOUND_TYPES.contains(&ext.as_str()) {
        return Err(format!("{} isn't a sound file VIDE-DE can play (wav, ogg, flac, mp3, opus).", src.display()));
    }
    let mut name = format!("{stem}.{ext}");
    for n in 2.. {
        if !dir.join(&name).exists() {
            break;
        }
        name = format!("{stem} ({n}).{ext}");
    }
    std::fs::copy(src, dir.join(&name)).map_err(|e| format!("Couldn't copy {}: {e}", src.display()))?;
    Ok(format!("custom:{name}"))
}

/// org.freedesktop.portal.FileChooser.OpenFile, waiting for its Response.
fn open_file() -> zbus::Result<Option<PathBuf>> {
    let conn = zbus::blocking::Connection::session()?;
    // The portal answers on a Request object whose path we can predict from our bus name and
    // a token; listen there before asking, or a fast answer could arrive first.
    let sender = conn.unique_name().map(|n| n.trim_start_matches(':').replace('.', "_")).unwrap_or_default();
    let token = format!("vide_sound_{}", std::process::id());
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let request = zbus::blocking::Proxy::new(&conn, "org.freedesktop.portal.Desktop", path.as_str(), "org.freedesktop.portal.Request")?;
    let mut responses = request.receive_signal("Response")?;

    let chooser = zbus::blocking::Proxy::new(&conn, "org.freedesktop.portal.Desktop", "/org/freedesktop/portal/desktop", "org.freedesktop.portal.FileChooser")?;
    let patterns: Vec<(u32, String)> = vide_config::CUSTOM_SOUND_TYPES.iter().map(|e| (0u32, format!("*.{e}"))).collect();
    let filters = vec![("Sounds".to_string(), patterns)];
    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("handle_token", Value::from(token.as_str()));
    opts.insert("filters", Value::from(filters.clone()));
    opts.insert("current_filter", Value::from(filters[0].clone()));
    let _: zbus::zvariant::OwnedObjectPath = chooser.call("OpenFile", &("", "Add a notification sound", opts))?;

    let Some(msg) = responses.next() else { return Ok(None) };
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize()?;
    if code != 0 {
        return Ok(None);
    }
    let uris: Vec<String> = results.get("uris").and_then(|v| v.try_clone().ok()).and_then(|v| v.try_into().ok()).unwrap_or_default();
    Ok(uris.first().and_then(|u| u.strip_prefix("file://")).map(|p| PathBuf::from(percent_decode(p))))
}

/// `%20` and friends in a file:// URI back to bytes.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok())) {
            (b'%', Some(c)) => {
                out.push(c);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
