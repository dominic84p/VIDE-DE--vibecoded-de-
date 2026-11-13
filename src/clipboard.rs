//! Keeping the clipboard when the app you copied from closes, as Windoze (and KDE's Klipper)
//! do. On Wayland the copying app itself holds the clipboard; when it quits, Smithay keeps
//! offering its dead source and paste comes back empty. So when an app copies, we fetch the
//! useful formats in the background, and if that app (or the X11 app that owned it) goes
//! away, we put our copy back on the clipboard.

use std::{
    collections::HashMap,
    io::Read,
    os::unix::{io::OwnedFd, net::UnixStream},
    sync::Arc,
    time::Duration,
};

use smithay::{
    reexports::{
        calloop::channel::{self, Sender},
        wayland_server::backend::ClientId,
    },
    wayland::selection::SelectionTarget,
};

use crate::state::{SelectionOwner, State};

/// Formats worth keeping. Text comes in many names for the same bytes; the first one offered
/// is fetched and served for all of them.
const TEXT: [&str; 5] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain", "STRING", "TEXT"];
const OTHERS: [&str; 4] = ["image/png", "text/uri-list", "x-special/gnome-copied-files", "text/html"];
/// Password managers mark what they copy; that must vanish with them, never be kept.
const SECRET: [&str; 2] = ["x-kde-passwordManagerHint", "secret"];
const MAX_BYTES: usize = 64 << 20;

/// Who holds the clipboard we're mirroring.
#[derive(Clone, PartialEq)]
enum Owner {
    Client(ClientId),
    X11,
}

#[derive(Default)]
pub struct Clipboard {
    /// Bumped on every new selection, so late fetches of an old one are dropped.
    generation: u64,
    owner: Option<Owner>,
    /// Every mime the owner offered that we'll serve once it's gone.
    offered: Vec<String>,
    /// Fetched so far: mime -> bytes.
    saved: HashMap<String, Arc<Vec<u8>>>,
    tx: Option<Sender<(u64, Vec<String>, Vec<u8>)>>,
}

impl State {
    fn clipboard_sender(&mut self) -> Sender<(u64, Vec<String>, Vec<u8>)> {
        if let Some(tx) = &self.clipboard.tx {
            return tx.clone();
        }
        let (tx, rx) = channel::channel::<(u64, Vec<String>, Vec<u8>)>();
        let _ = self.loop_handle.insert_source(rx, |ev, _, state: &mut State| {
            if let channel::Event::Msg((generation, mimes, bytes)) = ev {
                if generation == state.clipboard.generation && !bytes.is_empty() {
                    let bytes = Arc::new(bytes);
                    for m in mimes {
                        state.clipboard.saved.insert(m, bytes.clone());
                    }
                }
            }
        });
        self.clipboard.tx = Some(tx.clone());
        tx
    }

    /// An app (`x11`: an X11 app) just set the clipboard, offering `mimes`.
    pub fn clipboard_changed(&mut self, mimes: &[String], x11: bool) {
        let c = &mut self.clipboard;
        c.generation += 1;
        c.saved.clear();
        c.offered.clear();
        c.owner = None;
        if mimes.is_empty() || mimes.iter().any(|m| SECRET.iter().any(|s| m.contains(s))) {
            return;
        }
        // Data-device selections can only be set by the focused app, so that's the owner.
        c.owner = if x11 {
            Some(Owner::X11)
        } else {
            use smithay::reexports::wayland_server::Resource;
            self.keyboard
                .current_focus()
                .and_then(|f| f.surface())
                .and_then(|s| self.dh.get_client(s.id()).ok())
                .map(|client| Owner::Client(client.id()))
        };
        if self.clipboard.owner.is_none() {
            return;
        }

        let mut fetch: Vec<(String, Vec<String>)> = Vec::new();
        let text: Vec<String> = mimes.iter().filter(|m| TEXT.contains(&m.as_str())).cloned().collect();
        if let Some(first) = text.first() {
            fetch.push((first.clone(), text.clone()));
        }
        for m in mimes.iter().filter(|m| OTHERS.contains(&m.as_str())) {
            fetch.push((m.clone(), vec![m.clone()]));
        }
        self.clipboard.offered = fetch.iter().flat_map(|(_, served)| served.clone()).collect();

        // Ask after Smithay has stored the new selection (it tells us just before).
        let generation = self.clipboard.generation;
        self.loop_handle.insert_idle(move |state| {
            if state.clipboard.generation != generation {
                return;
            }
            let tx = state.clipboard_sender();
            for (mime, served) in fetch {
                let Ok((ours, theirs)) = UnixStream::pair() else { continue };
                let fd = OwnedFd::from(theirs);
                if x11 {
                    if let Some(xwm) = state.xwm.as_mut() {
                        let _ = xwm.send_selection(SelectionTarget::Clipboard, mime, fd, state.loop_handle.clone());
                    }
                } else {
                    let _ = smithay::wayland::selection::data_device::request_data_device_client_selection(&state.seat, mime, fd);
                }
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let _ = ours.set_read_timeout(Some(Duration::from_secs(3)));
                    let mut bytes = Vec::new();
                    if (&ours).take(MAX_BYTES as u64).read_to_end(&mut bytes).is_ok() {
                        let _ = tx.send((generation, served, bytes));
                    }
                });
            }
        });
    }

    /// We put something on the clipboard ourselves: stop mirroring the app that had it, so its
    /// old copy can't come back when it closes.
    pub fn clipboard_forget(&mut self) {
        self.clipboard.generation += 1;
        self.clipboard.owner = None;
        self.clipboard.saved.clear();
        self.clipboard.offered.clear();
    }

    /// After clients are dispatched: did the app holding the clipboard just go away?
    pub fn clipboard_check_owner(&mut self) {
        let Some(Owner::Client(id)) = &self.clipboard.owner else { return };
        if self.dh.backend_handle().get_client_data(id.clone()).is_err() {
            self.clipboard_restore();
        }
    }

    /// The X11 app holding the clipboard dropped it (most often: it quit).
    pub fn clipboard_x11_cleared(&mut self) {
        if self.clipboard.owner == Some(Owner::X11) {
            self.clipboard_restore();
        }
    }

    fn clipboard_restore(&mut self) {
        let c = &mut self.clipboard;
        c.owner = None;
        let saved: HashMap<String, Arc<Vec<u8>>> = c.offered.iter().filter_map(|m| c.saved.get(m).map(|b| (m.clone(), b.clone()))).collect();
        if saved.is_empty() {
            return;
        }
        let mimes: Vec<String> = c.offered.iter().filter(|m| saved.contains_key(*m)).cloned().collect();
        tracing::info!("clipboard owner went away; keeping its copy ({})", mimes.join(", "));
        smithay::wayland::selection::data_device::set_data_device_selection(
            &self.dh,
            &self.seat,
            mimes.clone(),
            SelectionOwner::Saved(Arc::new(saved)),
        );
        if let Some(xwm) = self.xwm.as_mut() {
            let _ = xwm.new_selection(SelectionTarget::Clipboard, Some(mimes));
        }
    }
}
