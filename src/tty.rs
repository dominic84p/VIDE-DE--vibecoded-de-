//! Real hardware backend: DRM/KMS for display, libinput for input, libseat
//! (via logind) for device access and VT switching.
//!
//! One GPU, every connected monitor (each on its own CRTC), with hotplug: udev tells us
//! when a connector changes and we rescan.

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
        udev::{all_gpus, primary_gpu, UdevBackend, UdevEvent},
    },
    output::{Mode as WlMode, Output, PhysicalProperties, Subpixel},
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

/// One monitor: its connector, the CRTC driving it, and its frame state.
struct Screen {
    connector: connector::Handle,
    output: Out,
    /// The wl_output we advertise, and the monitor's modes, for switching at runtime.
    wl_output: Output,
    modes: Vec<DrmMode>,
    mode: DrmMode,
    /// A page flip is in flight; don't queue another until its vblank.
    frame_pending: bool,
    /// Something changed since its last frame.
    dirty: bool,
}

struct Tty {
    session: LibSeatSession,
    libinput: Libinput,
    manager: Manager,
    fd: DrmDeviceFd,
    /// Primary first: the order monitors were found in.
    screens: Vec<Screen>,
    renderer: GlesRenderer,
    active: bool,
}

/// Settings → Display changed the mode or scale: switch the primary monitor to the mode,
/// and every monitor to the scale. A mode the monitor refuses leaves the old one in place.
fn apply_display(t: &mut Tty, state: &mut State) {
    let Tty { screens, renderer, .. } = t;
    for (i, sc) in screens.iter_mut().enumerate() {
        if i == 0 {
            let want = pick_mode(&sc.modes, parse_mode(&state.config.display_mode));
            if want != sc.mode {
                match sc.output.use_mode::<_, Elem>(want, renderer, &DrmOutputRenderElements::default()) {
                    Ok(()) => {
                        tracing::info!("display mode: {}x{}@{}", want.size().0, want.size().1, want.vrefresh());
                        sc.mode = want;
                    }
                    Err(e) => tracing::warn!("couldn't switch to {}x{}@{}: {e:?}", want.size().0, want.size().1, want.vrefresh()),
                }
            }
        }
        state.set_output_mode(&sc.wl_output.clone(), WlMode::from(sc.mode));
    }
}

/// Bring up monitors that were plugged in and let go of ones that were unplugged.
fn scan_connectors(t: &mut Tty, state: &mut State) {
    let Ok(res) = t.fd.resource_handles() else { return };
    let connected: Vec<connector::Info> = res
        .connectors()
        .iter()
        .filter_map(|c| t.fd.get_connector(*c, false).ok())
        .filter(|c| c.state() == connector::State::Connected && !c.modes().is_empty())
        .collect();

    // Unplugged.
    let gone: Vec<usize> = (0..t.screens.len()).filter(|&i| !connected.iter().any(|c| c.handle() == t.screens[i].connector)).collect();
    for i in gone.into_iter().rev() {
        let sc = t.screens.remove(i);
        state.remove_monitor(&sc.wl_output);
        // Dropping the DrmOutput frees its CRTC.
    }

    // Plugged in.
    let known: Vec<connector::Handle> = t.screens.iter().map(|s| s.connector).collect();
    for conn in connected.iter().filter(|c| !known.contains(&c.handle())) {
        let used: Vec<_> = t.screens.iter().map(|s| s.output.crtc()).collect();
        let Some(crtc) = conn
            .encoders()
            .iter()
            .filter_map(|e| t.fd.get_encoder(*e).ok())
            .flat_map(|enc| res.filter_crtcs(enc.possible_crtcs()))
            .find(|c| !used.contains(c))
        else {
            tracing::warn!("no free CRTC for {:?}-{}", conn.interface(), conn.interface_id());
            continue;
        };
        let primary = t.screens.is_empty();
        // Settings → Display can pin a WIDTHxHEIGHT@HZ mode for the primary; others use their preferred one.
        let modes: Vec<DrmMode> = conn.modes().to_vec();
        let mode = pick_mode(&modes, if primary { parse_mode(&state.config.display_mode) } else { None });
        let name = format!("{:?}-{}", conn.interface(), conn.interface_id());
        tracing::info!("output {name}: {}x{}@{}", mode.size().0, mode.size().1, mode.vrefresh());

        let (mm_w, mm_h) = conn.size().unwrap_or((0, 0));
        let output = Output::new(
            name.clone(),
            PhysicalProperties { size: (mm_w as i32, mm_h as i32).into(), subpixel: Subpixel::Unknown, make: "unknown".into(), model: "unknown".into() },
        );
        let wl_mode = WlMode::from(mode);
        output.change_current_state(Some(wl_mode), None, None, None);
        let drm_output = match t.manager.initialize_output::<_, Elem>(crtc, mode, &[conn.handle()], &output, None, &mut t.renderer, &DrmOutputRenderElements::default()) {
            Ok(o) => o,
            Err(e) => {
                tracing::warn!("initialize_output {name}: {e:?}");
                continue;
            }
        };
        let _global = output.create_global::<State>(&state.dh);
        state.add_monitor(&output, wl_mode);
        if primary {
            state.pointer_loc = (state.output_size.w as f64 / 2.0, state.output_size.h as f64 / 2.0).into();
        }
        t.screens.push(Screen { connector: conn.handle(), output: drm_output, wl_output: output, modes, mode, frame_pending: false, dirty: true });
    }
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
    let gpu_id = std::fs::metadata(&gpu).map(|m| std::os::unix::fs::MetadataExt::rdev(&m)).ok();
    let fd = session.open(&gpu, OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK)?;
    let fd = DrmDeviceFd::new(DeviceFd::from(fd));
    let (drm, drm_notifier) = DrmDevice::new(fd.clone(), true)?;
    let gbm = GbmDevice::new(fd.clone())?;

    let egl = unsafe { EGLDisplay::new(gbm.clone())? };
    let ctx = EGLContext::new(&egl)?;
    let render_formats = ctx.dmabuf_render_formats().clone();
    let renderer = unsafe { GlesRenderer::new(ctx)? };
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

    let manager: Manager = DrmOutputManager::new(
        drm,
        GbmAllocator::new(gbm.clone(), GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT),
        GbmFramebufferExporter::new(gbm.clone(), None),
        Some(gbm),
        [Fourcc::Argb8888, Fourcc::Xrgb8888],
        render_formats,
    );

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

    let tty = Rc::new(RefCell::new(Tty { session, libinput, manager, fd, screens: Vec::new(), renderer, active: true }));

    // Monitors
    scan_connectors(&mut tty.borrow_mut(), state);
    if tty.borrow().screens.is_empty() {
        return Err("no connected monitor".into());
    }

    // A monitor plugged in or out: udev says our GPU changed.
    match UdevBackend::new(&seat) {
        Ok(udev) => {
            let t = tty.clone();
            event_loop.handle().insert_source(udev, move |event, _, state| {
                if let UdevEvent::Changed { device_id } = event {
                    let mut t = t.borrow_mut();
                    if Some(device_id) == gpu_id && t.active {
                        tracing::info!("monitors changed; rescanning");
                        crate::supervisor::guard("monitor hotplug", || scan_connectors(&mut t, state));
                    }
                }
            })?;
        }
        Err(e) => tracing::warn!("udev: {e}; monitors plugged in later won't be noticed"),
    }

    // Page flip finished: that monitor may draw its next frame.
    let t = tty.clone();
    event_loop.handle().insert_source(drm_notifier, move |event, _, state| match event {
        DrmEvent::VBlank(crtc) => {
            let mut t = t.borrow_mut();
            if let Some(sc) = t.screens.iter_mut().find(|s| s.output.crtc() == crtc) {
                if let Err(e) = sc.output.frame_submitted() {
                    tracing::warn!("frame_submitted: {e:?}");
                }
                sc.frame_pending = false;
            }
            crate::health::flip_done();
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
                for sc in &mut t.screens {
                    sc.frame_pending = false;
                }
                // Monitors may have been swapped while we were away.
                scan_connectors(&mut t, state);
                state.needs_redraw = true;
            }
        }
    })?;

    while state.running {
        crate::health::enter(crate::health::Phase::Dispatch);
        event_loop.dispatch(None, state)?;
        crate::health::enter(crate::health::Phase::Render);

        if std::mem::take(&mut state.mouse_config_dirty) {
            for d in devices.borrow_mut().iter_mut() {
                apply_mouse(d, &state.config);
            }
        }

        let mut t = tty.borrow_mut();
        // Wait for page flips in flight: a modeset mid-flip would stall the output.
        if state.display_dirty && t.active && t.screens.iter().all(|s| !s.frame_pending) {
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

        if std::mem::take(&mut state.needs_redraw) {
            for sc in &mut t.screens {
                sc.dirty = true;
            }
        }
        if t.active {
            let Tty { screens, renderer, .. } = &mut *t;
            for (i, sc) in screens.iter_mut().enumerate().filter(|(_, s)| s.dirty && !s.frame_pending) {
                sc.dirty = false;
                let Some(view) = state.monitors.iter().find(|m| m.output == sc.wl_output).map(|m| m.geo) else { continue };
                let elements = render::elements(renderer, state, view);
                // Composite everything by default: hardware cursor/overlay planes are flaky on
                // NVIDIA. "Ultra Performance Mode" lets a fullscreen app's buffer be scanned out
                // directly; the explicit-sync blocker already held its commit until the GPU
                // finished, so the frame is complete by the time it can be picked.
                let flags = if state.config.direct_scanout { FrameFlags::ALLOW_PRIMARY_PLANE_SCANOUT } else { FrameFlags::empty() };
                match sc.output.render_frame(renderer, &elements, render::CLEAR, flags) {
                    Ok(res) if !res.is_empty => match sc.output.queue_frame(()) {
                        Ok(()) => {
                            sc.frame_pending = true;
                            crate::health::flip_queued();
                            if i == 0 {
                                crate::screenshot::screencast_tick(renderer, state);
                            }
                        }
                        Err(e) => tracing::warn!("queue_frame: {e:?}"),
                    },
                    Ok(_) => render::send_frames(state, state.start_time.elapsed().as_millis() as u32),
                    Err(e) => tracing::warn!("render_frame: {e:?}"),
                }
            }
        }
        drop(t);

        crate::health::enter(crate::health::Phase::Flush);
        state.dh.flush_clients()?;
    }
    Ok(())
}
