//! wlr-layer-shell: lets separate programs put surfaces on the desktop's layers:
//! wallpaper/desktop icons (background), panels and docks (top), lock screens and
//! notifications (overlay). Our own `vide-desktop` uses it; we don't draw desktop
//! icons in the compositor.

use smithay::{
    delegate_layer_shell,
    desktop::{layer_map_for_output, LayerSurface as MappedLayer, WindowSurfaceType},
    output::Output,
    reexports::wayland_server::protocol::{wl_output::WlOutput, wl_surface::WlSurface},
    utils::{Logical, Point},
    wayland::{
        compositor::with_states,
        shell::wlr_layer::{Layer, LayerSurface, LayerSurfaceData, WlrLayerShellHandler, WlrLayerShellState},
    },
};

use crate::state::State;

impl WlrLayerShellHandler for State {
    fn shell_state(&mut self) -> &mut WlrLayerShellState {
        &mut self.layer_shell_state
    }

    fn new_layer_surface(&mut self, surface: LayerSurface, _output: Option<WlOutput>, _layer: Layer, namespace: String) {
        // One output for now: everything goes on it.
        let Some(output) = self.output.clone() else { return };
        let mut map = layer_map_for_output(&output);
        if let Err(e) = map.map_layer(&MappedLayer::new(surface, namespace.clone())) {
            tracing::warn!("layer surface {namespace}: {e}");
        }
        tracing::info!("layer surface mapped: {namespace}");
        self.needs_redraw = true;
    }

    fn layer_destroyed(&mut self, surface: LayerSurface) {
        let Some(output) = self.output.clone() else { return };
        let mut map = layer_map_for_output(&output);
        let found = map.layers().find(|l| l.layer_surface() == &surface).cloned();
        if let Some(l) = found {
            map.unmap_layer(&l);
        }
        self.needs_redraw = true;
    }
}

impl State {
    /// On commit: lay layers out again, and answer a new layer's first commit with
    /// its configure (its size, from its anchors and our output).
    pub fn layer_commit(&mut self, surface: &WlSurface) {
        let Some(output) = self.output.clone() else { return };
        let mut map = layer_map_for_output(&output);
        let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL).cloned() else { return };
        let initial_sent = with_states(surface, |s| {
            s.data_map.get::<LayerSurfaceData>().map(|d| d.lock().unwrap().initial_configure_sent).unwrap_or(true)
        });
        map.arrange();
        if !initial_sent {
            layer.layer_surface().send_configure();
        }
    }

    /// Layer surfaces on `layers`, front to back, with their positions.
    pub fn layer_surfaces(&self, layers: &[Layer]) -> Vec<(WlSurface, Point<i32, Logical>)> {
        let Some(output) = &self.output else { return Vec::new() };
        let map = layer_map_for_output(output);
        let mut out = Vec::new();
        for &kind in layers {
            for l in map.layers_on(kind).rev() {
                if let Some(geo) = map.layer_geometry(l) {
                    out.push((l.wl_surface().clone(), geo.loc));
                }
            }
        }
        out
    }

    /// The topmost client surface on `layers` under `point`, with its global origin.
    pub fn layer_hit(&self, layers: &[Layer], point: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        let output = self.output.as_ref()?;
        let map = layer_map_for_output(output);
        for &kind in layers {
            for l in map.layers_on(kind).rev() {
                let Some(geo) = map.layer_geometry(l) else { continue };
                if let Some((s, loc)) = l.surface_under(point - geo.loc.to_f64(), WindowSurfaceType::ALL) {
                    return Some((s, (loc + geo.loc).to_f64()));
                }
            }
        }
        None
    }

    /// Does a desktop program provide the background? Then we skip our own wallpaper.
    pub fn has_background_layer(&self) -> bool {
        self.output.as_ref().is_some_and(|o| layer_map_for_output(o).layers_on(Layer::Background).next().is_some())
    }

    /// Re-lay out layers after the output changed size.
    pub fn arrange_layers(&self) {
        if let Some(o) = &self.output {
            layer_map_for_output(o).arrange();
        }
    }
}

pub fn set_output(state: &mut State, output: &Output) {
    state.output = Some(output.clone());
}

delegate_layer_shell!(State);
