//! Nested backend: we run as a window inside another compositor. For development.
//!
//! `VIBE_NESTED_MONITORS=N` splits the window into N monitors side by side, so
//! multi-monitor behaviour can be tried without the hardware.

use smithay::{
    backend::{
        egl::EGLDevice,
        renderer::{damage::OutputDamageTracker, gles::GlesRenderer},
        winit::{self, WinitEvent},
    },
    output::{Mode, Output, PhysicalProperties, Subpixel},
    reexports::calloop::EventLoop,
    utils::{Physical, Size, Transform},
};

use crate::{render, state::State};

/// Give each pretend monitor its share of the window's width.
fn split(state: &mut State, outputs: &[Output], size: Size<i32, Physical>) {
    let n = outputs.len() as i32;
    for (i, o) in outputs.iter().enumerate() {
        let i = i as i32;
        let w = size.w * (i + 1) / n - size.w * i / n;
        state.add_monitor(o, Mode { size: (w, size.h).into(), refresh: 60_000 });
    }
}

pub fn run(event_loop: &mut EventLoop<'static, State>, state: &mut State) -> Result<(), Box<dyn std::error::Error>> {
    let (mut backend, winit) = winit::init::<GlesRenderer>()?;
    let render_node = EGLDevice::device_for_display(backend.renderer().egl_context().display())
        .ok()
        .and_then(|d| d.try_get_render_node().ok().flatten());
    state.init_dmabuf(backend.renderer(), render_node);
    // We draw our own cursor.
    backend.window().set_cursor_visible(false);

    let count = std::env::var("VIBE_NESTED_MONITORS").ok().and_then(|n| n.parse::<usize>().ok()).unwrap_or(1).clamp(1, 4);
    let outputs: Vec<Output> = (1..=count)
        .map(|i| {
            let name = if count == 1 { "vibe-nested".to_string() } else { format!("vibe-nested-{i}") };
            let o = Output::new(name, PhysicalProperties { size: (0, 0).into(), subpixel: Subpixel::Unknown, make: "vibecoded".into(), model: "nested".into() });
            o.change_current_state(None, Some(Transform::Flipped180), None, None);
            let _global = o.create_global::<State>(&state.dh);
            o
        })
        .collect();
    split(state, &outputs, backend.window_size());

    // One tracker for the whole window, which shows every monitor; rebuilt when it resizes.
    let window_tracker = |size: Size<i32, Physical>, scale: f64| OutputDamageTracker::new(size, scale, Transform::Flipped180);
    let mut tracked = (backend.window_size(), state.output_scale);
    let mut damage_tracker = window_tracker(tracked.0, tracked.1);

    let outs = outputs.clone();
    event_loop.handle().insert_source(winit, move |event, _, state| match event {
        WinitEvent::Resized { size, .. } => split(state, &outs, size),
        WinitEvent::Input(event) => {
            crate::supervisor::guard("input", || state.handle_input(event));
        }
        WinitEvent::Redraw => state.needs_redraw = true,
        WinitEvent::CloseRequested => state.running = false,
        WinitEvent::Focus(_) => {}
    })?;

    while state.running {
        crate::health::enter(crate::health::Phase::Dispatch);
        event_loop.dispatch(None, state)?;
        crate::health::enter(crate::health::Phase::Render);

        // Settings → Display scale, so it can be tried nested too.
        if std::mem::take(&mut state.display_dirty) {
            split(state, &outputs, backend.window_size());
        }
        if let Some(region) = state.screenshot_requested.take() {
            crate::screenshot::take(backend.renderer(), state, region);
        }

        if state.needs_redraw {
            state.needs_redraw = false;
            if tracked != (backend.window_size(), state.output_scale) {
                tracked = (backend.window_size(), state.output_scale);
                damage_tracker = window_tracker(tracked.0, tracked.1);
            }
            let age = backend.buffer_age().unwrap_or(0);
            let (renderer, mut fb) = backend.bind()?;
            let view = state.layout_rect();
            let elements = render::elements(renderer, state, view);
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

        crate::health::enter(crate::health::Phase::Flush);
        state.dh.flush_clients()?;
    }
    Ok(())
}
