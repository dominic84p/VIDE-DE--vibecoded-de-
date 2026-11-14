//! wlr-layer-shell: lets separate programs put surfaces on the desktop's layers:
//! wallpaper/desktop icons (background), panels and docks (top), lock screens and
//! notifications (overlay). Our own `vide-desktop` uses it; we don't draw desktop
//! icons in the compositor. Each monitor has its own layer map; a surface that names no
//! monitor goes on the primary.

use smithay::{
    delegate_layer_shell,
    desktop::{layer_map_for_output, LayerSurface as MappedLayer, WindowSurfaceType},
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

    fn new_layer_surface(&mut self, surface: LayerSurface, output: Option<WlOutput>, _layer: Layer, namespace: String) {
        // The monitor the client asked for, else the primary.
        let asked = output.and_then(|o| self.monitors.iter().find(|m| m.output.owns(&o)).map(|m| m.output.clone()));
        let Some(output) = asked.or_else(|| self.output.clone()) else { return };
        let mut map = layer_map_for_output(&output);
        if let Err(e) = map.map_layer(&MappedLayer::new(surface, namespace.clone())) {
            tracing::warn!("layer surface {namespace}: {e}");
        }
        tracing::info!("layer surface mapped: {namespace} on {}", output.name());
        self.needs_redraw = true;
    }

    fn layer_destroyed(&mut self, surface: LayerSurface) {
        for m in &self.monitors {
            let mut map = layer_map_for_output(&m.output);
            let found = map.layers().find(|l| l.layer_surface() == &surface).cloned();
            if let Some(l) = found {
                map.unmap_layer(&l);
            }
        }
        self.needs_redraw = true;
    }
}

impl State {
    /// On commit: lay layers out again, and answer a new layer's first commit with
    /// its configure (its size, from its anchors and its monitor).
    pub fn layer_commit(&mut self, surface: &WlSurface) {
        for m in &self.monitors {
            let mut map = layer_map_for_output(&m.output);
            let Some(layer) = map.layer_for_surface(surface, WindowSurfaceType::TOPLEVEL).cloned() else { continue };
            let initial_sent = with_states(surface, |s| {
                s.data_map.get::<LayerSurfaceData>().map(|d| d.lock().unwrap().initial_configure_sent).unwrap_or(true)
            });
            map.arrange();
            if !initial_sent {
                layer.layer_surface().send_configure();
            }
            return;
        }
    }

    /// Layer surfaces on `layers`, front to back, with their global positions.
    pub fn layer_surfaces(&self, layers: &[Layer]) -> Vec<(WlSurface, Point<i32, Logical>)> {
        let mut out = Vec::new();
        for &kind in layers {
            for m in &self.monitors {
                let map = layer_map_for_output(&m.output);
                for l in map.layers_on(kind).rev() {
                    if let Some(geo) = map.layer_geometry(l) {
                        out.push((l.wl_surface().clone(), m.geo.loc + geo.loc));
                    }
                }
            }
        }
        out
    }

    /// The topmost client surface on `layers` under `point`, with its global origin.
    pub fn layer_hit(&self, layers: &[Layer], point: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        for &kind in layers {
            for m in self.monitors.iter().filter(|m| m.geo.to_f64().contains(point)) {
                let map = layer_map_for_output(&m.output);
                for l in map.layers_on(kind).rev() {
                    let Some(geo) = map.layer_geometry(l) else { continue };
                    let origin = m.geo.loc + geo.loc;
                    if let Some((s, loc)) = l.surface_under(point - origin.to_f64(), WindowSurfaceType::ALL) {
                        return Some((s, (loc + origin).to_f64()));
                    }
                }
            }
        }
        None
    }

    /// Does a desktop program provide monitor `i`'s background? Then we skip our own wallpaper there.
    pub fn has_background_layer(&self, i: usize) -> bool {
        self.monitors.get(i).is_some_and(|m| layer_map_for_output(&m.output).layers_on(Layer::Background).next().is_some())
    }

    /// Re-lay out layers after a monitor changed size.
    pub fn arrange_layers(&self) {
        for m in &self.monitors {
            layer_map_for_output(&m.output).arrange();
        }
    }
}

delegate_layer_shell!(State);
