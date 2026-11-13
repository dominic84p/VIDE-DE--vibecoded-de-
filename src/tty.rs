//! Real hardware backend: DRM/KMS for display, libinput for input, libseat
//! (via logind) for device access and VT switching.
//!
//! Single GPU, single monitor for now: the first connected connector wins.

use std::{cell::RefCell, rc::Rc};

use smithay::{
    backend::{
        allocator::{
            gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
            Fourcc,
        },
        drm::{
            compositor::FrameFlags,
            exporter::gbm::GbmFramebufferExporter,
            output::{DrmOutput, DrmOutputManager, DrmOutputRenderElements},
            DrmDevice, DrmDeviceFd, DrmEvent, DrmNode, NodeType,
        },
        egl::{EGLContext, EGLDisplay},
        libinput::{LibinputInputBackend, LibinputSessionInterface},
        renderer::gles::GlesRenderer,
        session::{libseat::LibSeatSession, Event as SessionEvent, Session},
        udev::{all_gpus, primary_gpu},
    },
    output::{Mode as WlMode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        calloop::EventLoop,
        drm::control::{connector, Device as _, Mode as DrmMode, ModeTypeFlags},
        input::Libinput,
        rustix::fs::OFlags,
    },
    utils::DeviceFd,
};

use smithay::{
    backend::input::InputEvent,
    reexports::input::{AccelProfile, Device as InputDevice},
    wayland::drm_syncobj::{supports_syncobj_eventfd, DrmSyncobjState},
};

/// Parse "1920x1080@144" (the @HZ part optional) into (width, height, Hz-or-0).
fn parse_mode(s: &str) -> Option<(u16, u16, i32)> {
    let (res, hz) = s.split_once('@').unwrap_or((s, ""));
    let (w, h) = res.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?, hz.trim().parse().unwrap_or(0)))
}

/// The mode Settings asked for if the monitor has it, else its preferred one.
fn pick_mode(modes: &[DrmMode], wanted: Option<(u16, u16, i32)>) -> DrmMode {
    wanted
        .and_then(|(w, h, hz)| {
            modes
                .iter()
                .find(|m| m.size() == (w, h) && (hz == 0 || (m.vrefresh() as i32 - hz).abs() <= 2))
                .or_else(|| modes.iter().find(|m| m.size() == (w, h)))
        })
        .copied()
        .or_else(|| modes.iter().find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED)).copied())
        .unwrap_or(modes[0])
}

/// Outputs scale in whole steps; fractional scaling needs wp-fractional-scale, which we don't have.
fn output_scale(c: &vide_config::Config) -> i32 {
    (c.display_scale.clamp(0.5, 3.0).round() as i32).max(1)
}

/// Acceleration profile and speed from our settings. KDE normally does this; without
/// it libinput's default adaptive curve makes high-DPI mice fly off.
fn apply_mouse(d: &mut InputDevice, c: &vide_config::Config) {
    if d.config_accel_is_available() {
        let profile = if c.mouse_flat { AccelProfile::Flat } else { AccelProfile::Adaptive };
        let _ = d.config_accel_set_profile(profile);
        let _ = d.config_accel_set_speed(c.mouse_speed);
        tracing::info!("mouse: {} -> {:?} speed {:.2}", d.name(), profile, c.mouse_speed);
    }
    if d.config_tap_finger_count() > 0 {
        let _ = d.config_tap_set_enabled(c.tap_to_click);
    }
    if d.config_scroll_has_natural_scroll() {
        let _ = d.config_scroll_set_natural_scroll_enabled(c.natural_scroll);
    }
    if d.config_dwt_is_available() {
        let _ = d.config_dwt_set_enabled(c.disable_while_typing);
    }
}

use crate::{
    render::{self, Elem},
    state::State,
};

type Out = DrmOutput<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;
type Manager = DrmOutputManager<GbmAllocator<DrmDeviceFd>, GbmFramebufferExporter<DrmDeviceFd>, (), DrmDeviceFd>;

struct Tty {
    session: LibSeatSession,
    libinput: Libinput,
    manager: Manager,
    output: Out,
    /// The wl_output we advertise, and the monitor's modes, for switching at runtime.
    wl_output: Output,
    modes: Vec<DrmMode>,
    mode: DrmMode,
    renderer: GlesRenderer,
    /// A page flip is in flight; don't queue another until its vblank.
    frame_pending: bool,
    active: bool,
}

/// Settings → Display changed the mode or scale: switch to it, tell clients, and re-fit
/// the windows. A mode the monitor refuses leaves the old one in place.
fn apply_display(t: &mut Tty, state: &mut State) {
    let want = pick_mode(&t.modes, parse_mode(&state.config.display_mode));
    if want != t.mode {
        match t.output.use_mode::<_, Elem>(want, &mut t.renderer, &DrmOutputRenderElements::default()) {
            Ok(()) => {
                tracing::info!("display mode: {}x{}@{}", want.size().0, want.size().1, want.vrefresh());
                t.mode = want;
            }
            Err(e) => {
                tracing::warn!("couldn't switch to {}x{}@{}: {e:?}", want.size().0, want.size().1, want.vrefresh());
                return;
            }
        }
    }
    let wl_mode = WlMode::from(t.mode);
    let scale = output_scale(&state.config);
    t.wl_output.change_current_state(Some(wl_mode), None, Some(Scale::Integer(scale)), None);
    t.wl_output.set_preferred(wl_mode);
    state.output_size = wl_mode.size.to_logical(scale);
    state.pointer_loc.x = state.pointer_loc.x.clamp(0.0, state.output_size.w as f64 - 1.0);
    state.pointer_loc.y = state.pointer_loc.y.clamp(0.0, state.output_size.h as f64 - 1.0);
    state.arrange_layers();
    let (work, full) = (state.work_area(), state.output_rect());
    state.wm.refit(work, full);
    // Cached chrome was painted for the old size.
    state.taskbar = None;
    state.start.menu = None;
    state.wallpaper = None;
    state.needs_redraw = true;
}

pub fn run(event_loop: &mut EventLoop<'static, State>, state: &mut State) -> Result<(), Box<dyn std::error::Error>> {
    state.tty = true;

    let (mut session, session_notifier) = LibSeatSession::new().map_err(|e| {
        format!("couldn't take the seat ({e}). Is VIDE-DE or another desktop already running on this VT?")
    })?;
    let seat = session.seat();

    // GPU
    let gpu = primary_gpu(&seat)?
        .or_else(|| all_gpus(&seat).ok()?.into_iter().next())
        .ok_or("no GPU found")?;
    tracing::info!("using GPU {}", gpu.display());
    let fd = session.open(&gpu, OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK)?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (drm, drm_notifier) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd.clone())?;

    let egl = unsafe { EGLDisplay::new(gbm.clone())? };
    let ctx = EGLContext::new(&egl)?;
    let render_formats = ctx.dmabuf_render_formats().clone();
    let mut renderer = unsafe { GlesRenderer::new(ctx)? };
    let render_node = DrmNode::from_file(&fd)
        .ok()
        .and_then(|n| n.node_with_type(NodeType::Render).and_then(|r| r.ok()).or(Some(n)));
    state.init_dmabuf(&renderer, render_node);
    if supports_syncobj_eventfd(&fd) {
        state.syncobj_state = Some(DrmSyncobjState::new::<State>(&state.dh, fd.clone()));
        tracing::info!("explicit sync (linux-drm-syncobj) enabled");
    } else {
        tracing::warn!("no syncobj eventfd support; NVIDIA clients may flicker");
    }

    let mut manager: Manager = DrmOutputManager::new(
        drm,
        GbmAllocator::new(gbm.clone(), GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT),
        GbmFramebufferExporter::new(gbm.clone(), None),
        Some(gbm),
        [Fourcc::Argb8888, Fourcc::Xrgb8888],
        render_formats,
    );

    // Monitor
    let res = fd.resource_handles()?;
    let conn = res
        .connectors()
        .iter()
        .filter_map(|c| fd.get_connector(*c, false).ok())
        .find(|c| c.state() == connector::State::Connected && !c.modes().is_empty())
        .ok_or("no connected monitor")?;
    // Settings → Display can pin a WIDTHxHEIGHT@HZ mode; otherwise use the monitor's preferred one.
    let modes: Vec<DrmMode> = conn.modes().to_vec();
    let mode = pick_mode(&modes, parse_mode(&state.config.display_mode));
    let crtc = conn
        .encoders()
        .iter()
        .filter_map(|e| fd.get_encoder(*e).ok())
        .flat_map(|enc| res.filter_crtcs(enc.possible_crtcs()))
        .next()
        .ok_or("no CRTC for monitor")?;
    let name = format!("{:?}-{}", conn.interface(), conn.interface_id());
    tracing::info!("output {name}: {}x{}@{}", mode.size().0, mode.size().1, mode.vrefresh());

    let (mm_w, mm_h) = conn.size().unwrap_or((0, 0));
    let output = Output::new(
        name,
        PhysicalProperties {
            size: (mm_w as i32, mm_h as i32).into(),
            subpixel: Subpixel::Unknown,
            make: "unknown".into(),
            model: "unknown".into(),
        },
    );
    let wl_mode = WlMode::from(mode);
    let scale = output_scale(&state.config);
    output.change_current_state(Some(wl_mode), None, Some(Scale::Integer(scale)), Some((0, 0).into()));
    output.set_preferred(wl_mode);
    let _global = output.create_global::<State>(&state.dh);
    crate::layers::set_output(state, &output);
    state.output_size = wl_mode.size.to_logical(scale);
    state.pointer_loc = (state.output_size.w as f64 / 2.0, state.output_size.h as f64 / 2.0).into();

    let drm_output = manager
        .initialize_output::<_, Elem>(
            crtc,
            mode,
            &[conn.handle()],
            &output,
            None,
            &mut renderer,
            &DrmOutputRenderElements::default(),
        )
        .map_err(|e| format!("initialize_output: {e:?}"))?;

    // Input
    // Pointer devices, so mouse settings can be re-applied when they change.
    let devices: Rc<RefCell<Vec<InputDevice>>> = Default::default();
    let mut libinput = Libinput::new_with_udev(LibinputSessionInterface::from(session.clone()));
    libinput.udev_assign_seat(&seat).map_err(|_| "libinput: failed to assign seat")?;
    event_loop
        .handle()
        .insert_source(LibinputInputBackend::new(libinput.clone()), {
            let devices = devices.clone();
            move |event, _, state| {
                if let InputEvent::DeviceAdded { device } = &event {
                    let mut d = device.clone();
                    apply_mouse(&mut d, &state.config);
                    devices.borrow_mut().push(d);
                }
                if let InputEvent::DeviceRemoved { device } = &event {
                    devices.borrow_mut().retain(|d| d != device);
                }
                crate::supervisor::guard("input", || state.handle_input(event));
            }
        })?;

    let tty = Rc::new(RefCell::new(Tty {
        session,
        libinput,
        manager,
        output: drm_output,
        wl_output: output.clone(),
        modes,
        mode,
        renderer,
        frame_pending: false,
        active: true,
    }));

    // Page flip finished: we may draw the next frame.
    let t = tty.clone();
    event_loop.handle().insert_source(drm_notifier, move |event, _, state| match event {
        DrmEvent::VBlank(_) => {
            let mut t = t.borrow_mut();
            if let Err(e) = t.output.frame_submitted() {
                tracing::warn!("frame_submitted: {e:?}");
            }
            t.frame_pending = false;
            render::send_frames(state, state.start_time.elapsed().as_millis() as u32);
        }
        DrmEvent::Error(e) => tracing::error!("drm: {e}"),
    })?;

    // VT switched away from / back to us.
    let t = tty.clone();
    event_loop.handle().insert_source(session_notifier, move |event, _, state| {
        let mut t = t.borrow_mut();
        match event {
            SessionEvent::PauseSession => {
                tracing::info!("session paused");
                t.active = false;
                t.libinput.suspend();
                t.manager.pause();
            }
            SessionEvent::ActivateSession => {
                tracing::info!("session resumed");
                if t.libinput.resume().is_err() {
                    tracing::error!("libinput resume failed");
                }
                if let Err(e) = t.manager.activate(false) {
                    tracing::error!("drm activate: {e}");
                }
                t.active = true;
                t.frame_pending = false;
                state.needs_redraw = true;
            }
        }
    })?;

    while state.running {
        event_loop.dispatch(None, state)?;

        if std::mem::take(&mut state.mouse_config_dirty) {
            for d in devices.borrow_mut().iter_mut() {
                apply_mouse(d, &state.config);
            }
        }

        let mut t = tty.borrow_mut();
        // Wait for any page flip in flight: a modeset mid-flip would stall the output.
        if state.display_dirty && t.active && !t.frame_pending {
            state.display_dirty = false;
            crate::supervisor::guard("display change", || apply_display(&mut t, state));
        }
        if let Some(region) = state.screenshot_requested.take() {
            if t.active {
                crate::screenshot::take(&mut t.renderer, state, region);
            }
        }
        if let Some(vt) = state.pending_vt.take() {
            if let Err(e) = t.session.change_vt(vt) {
                tracing::warn!("change_vt({vt}): {e}");
            }
        }

        if state.needs_redraw && t.active && !t.frame_pending {
            state.needs_redraw = false;
            let t = &mut *t;
            let elements = render::elements(&mut t.renderer, state);
            // Composite everything by default: hardware cursor/overlay planes are flaky on
            // NVIDIA. "Ultra Performance Mode" lets a fullscreen app's buffer be scanned out
            // directly; the explicit-sync blocker already held its commit until the GPU
            // finished, so the frame is complete by the time it can be picked.
            let flags = if state.config.direct_scanout { FrameFlags::ALLOW_PRIMARY_PLANE_SCANOUT } else { FrameFlags::empty() };
            match t.output.render_frame(&mut t.renderer, &elements, render::CLEAR, flags) {
                Ok(res) if !res.is_empty => match t.output.queue_frame(()) {
                    Ok(()) => {
                        t.frame_pending = true;
                        crate::screenshot::screencast_tick(&mut t.renderer, state);
                    }
                    Err(e) => tracing::warn!("queue_frame: {e:?}"),
                },
                Ok(_) => render::send_frames(state, state.start_time.elapsed().as_millis() as u32),
                Err(e) => tracing::warn!("render_frame: {e:?}"),
            }
        }
        drop(t);

        state.dh.flush_clients()?;
    }
    Ok(())
}
