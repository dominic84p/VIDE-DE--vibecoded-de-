//! Keyboard focus target: a Wayland surface, or an X11 window (which also
//! needs the X server told about focus, not just its Wayland surface).

use std::borrow::Cow;

use smithay::{
    backend::input::KeyState,
    desktop::PopupKind,
    input::{
        keyboard::{KeyboardTarget, KeysymHandle, ModifiersState},
        Seat,
    },
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{IsAlive, Serial},
    wayland::seat::WaylandFocus,
    xwayland::X11Surface,
};

use crate::state::State;

#[derive(Clone, PartialEq, Debug)]
pub enum FocusTarget {
    Wayland(WlSurface),
    X11(X11Surface),
}

impl FocusTarget {
    pub fn surface(&self) -> Option<WlSurface> {
        match self {
            FocusTarget::Wayland(s) => Some(s.clone()),
            FocusTarget::X11(x) => x.wl_surface(),
        }
    }
}

impl IsAlive for FocusTarget {
    fn alive(&self) -> bool {
        match self {
            FocusTarget::Wayland(s) => s.alive(),
            FocusTarget::X11(x) => x.alive(),
        }
    }
}

impl WaylandFocus for FocusTarget {
    fn wl_surface(&self) -> Option<Cow<'_, WlSurface>> {
        match self {
            FocusTarget::Wayland(s) => Some(Cow::Borrowed(s)),
            FocusTarget::X11(x) => x.wl_surface().map(Cow::Owned),
        }
    }
}

impl From<PopupKind> for FocusTarget {
    fn from(p: PopupKind) -> Self {
        FocusTarget::Wayland(p.wl_surface().clone())
    }
}

impl From<FocusTarget> for WlSurface {
    fn from(f: FocusTarget) -> Self {
        // Popup grabs only ever hand us Wayland targets; an unassociated X11 window has no surface.
        f.surface().expect("focus target without a wl_surface")
    }
}

macro_rules! each {
    ($self:ident, $t:ident => $e:expr) => {
        match $self {
            FocusTarget::Wayland($t) => $e,
            FocusTarget::X11($t) => $e,
        }
    };
}

impl KeyboardTarget<State> for FocusTarget {
    fn enter(&self, seat: &Seat<State>, data: &mut State, keys: Vec<KeysymHandle<'_>>, serial: Serial) {
        each!(self, t => KeyboardTarget::enter(t, seat, data, keys, serial))
    }

    fn leave(&self, seat: &Seat<State>, data: &mut State, serial: Serial) {
        each!(self, t => KeyboardTarget::leave(t, seat, data, serial))
    }

    fn key(&self, seat: &Seat<State>, data: &mut State, key: KeysymHandle<'_>, state: KeyState, serial: Serial, time: u32) {
        each!(self, t => KeyboardTarget::key(t, seat, data, key, state, serial, time))
    }

    fn modifiers(&self, seat: &Seat<State>, data: &mut State, modifiers: ModifiersState, serial: Serial) {
        each!(self, t => KeyboardTarget::modifiers(t, seat, data, modifiers, serial))
    }
}
