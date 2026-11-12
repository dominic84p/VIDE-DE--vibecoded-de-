//! Nested backend: we run as a window inside another compositor. For development.

use smithay::{
    backend::{
        egl::EGLDevice,
        renderer::{damage::OutputDamageTracker, gles::GlesRenderer},
        winit::{self, WinitEvent},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::Transform,
};

use crate::{render, state::State};

pub fn run(event_loop: &mut EventLoop<'static, State>, state: &mut State) -> Result<(), Box<dyn std::error::Error>> {
    let (mut backend, winit) = winit::init::<GlesRenderer>()?;
    let render_node = EGLDevice::device_for_display(backend.renderer().egl_context().display())
        .ok()
        .and_then(|d| d.try_get_render_node().ok().flatten());
    state.init_dmabuf(backend.renderer(), render_node);
    // We draw our own cursor.
    backend.window().set_cursor_visible(false);

    let size = backend.window_size();
    let output = Output::new(
        "vibe-nested".into(),
        PhysicalProperties { size: (0, 0).into(), subpixel: Subpixel::Unknown, make: "vibecoded".into(), model: "nested".into() },
    );
    let _global = output.create_global::<State>(&state.dh);
    crate::layers::set_output(state, &output);
    let mode = Mode { size, refresh: 60_000 };
    output.change_current_state(Some(mode), Some(Transform::Flipped180), None, Some((0, 0).into()));
    output.set_preferred(mode);
    state.output_size = size.to_logical(1);

    let mut damage_tracker = OutputDamageTracker::from_output(&output);

    event_loop.handle().insert_source(winit, move |event, _, state| match event {
        WinitEvent::Resized { size, .. } => {
            let mode = Mode { size, refresh: 60_000 };
            output.change_current_state(Some(mode), None, None, None);
            output.set_preferred(mode);
            state.output_size = size.to_logical(1);
            state.needs_redraw = true;
        }
        WinitEvent::Input(event) => state.handle_input(event),
        WinitEvent::Redraw => state.needs_redraw = true,
        WinitEvent::CloseRequested => state.running = false,
        WinitEvent::Focus(_) => {}
    })?;

    while state.running {
        event_loop.dispatch(None, state)?;

        if let Some(region) = state.screenshot_requested.take() {
            crate::screenshot::take(backend.renderer(), state, region);
        }

        if state.needs_redraw {
            state.needs_redraw = false;
            let age = backend.buffer_age().unwrap_or(0);
            let (renderer, mut fb) = backend.bind()?;
            let elements = render::elements(renderer, state);
            let result = damage_tracker.render_output(renderer, &mut fb, age, &elements, render::CLEAR)?;
            let damage = result.damage.cloned();
            drop(fb);
            if let Some(damage) = damage {
                // Blocks until vsync, which is what paces us.
                backend.submit(Some(&damage))?;
            }
            crate::screenshot::screencast_tick(backend.renderer(), state);
            render::send_frames(state, state.start_time.elapsed().as_millis() as u32);
        }

        state.dh.flush_clients()?;
    }
    Ok(())
}
