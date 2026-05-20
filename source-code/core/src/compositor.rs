use std::{os::unix::io::OwnedFd, path::PathBuf, sync::Arc, time::Duration};

use anyhow::{Context, Result};
use drm::control::{connector, crtc, Device as ControlDevice, ModeTypeFlags};
use tracing::{debug, error, info, warn};

use smithay::{
    backend::{
        allocator::gbm::{GbmAllocator, GbmBufferFlags},
        drm::{DrmDevice, DrmDeviceFd, DrmEvent, DrmNode},
        egl::{EGLContext, EGLDisplay},
        // ImportDma must be in scope to call renderer.dmabuf_formats()
        renderer::{gles::GlesRenderer, ImportDma},
        session::{libseat::LibSeatSession, Session},
    },
    reexports::{
        calloop::{
            timer::{TimeoutAction, Timer},
            EventLoop, LoopHandle, LoopSignal,
        },
        // FIX 1: use smithay's own re-exported rustix, not the standalone crate.
        // The compiler error said: "expected `smithay::reexports::rustix::fs::OFlags`"
        rustix::fs::OFlags,
        wayland_server::Display,
    },
    utils::DeviceFd,
    wayland::socket::ListeningSocketSource,
};

use gameframe_gpu::GpuVendor;

use crate::{
    frame::FramePacer,
    output::OutputManager,
    session::SessionOptions,
    state::{GameframeClientData, GameframeState},
    xwayland,
};

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn run(opts: &SessionOptions) -> Result<()> {
    // ── 1. calloop EventLoop ──────────────────────────────────────────────────
    let mut event_loop: EventLoop<GameframeState> =
        EventLoop::try_new().context("EventLoop::try_new")?;

    // SAFETY: event_loop outlives all loop_handle users inside this function
    let loop_handle: LoopHandle<'static, GameframeState> =
        unsafe { std::mem::transmute(event_loop.handle()) };

    // ── 2. Wayland display + UNIX socket ──────────────────────────────────────
    let mut display: Display<GameframeState> =
        Display::new().context("Wayland Display::new")?;

    let socket_source = ListeningSocketSource::new_auto()
        .context("Wayland ListeningSocket")?;
    let socket_name = socket_source.socket_name().to_string_lossy().into_owned();
    info!(%socket_name, "Wayland socket ready");

    {
        let lh = loop_handle.clone();
        lh.insert_source(socket_source, |client_stream, _, state| {
            state
                .display_handle
                .insert_client(client_stream, Arc::new(GameframeClientData::default()))
                .expect("insert_client");
        })
        .context("Wayland socket source")?;
    }

    // ── 3. libseat session ────────────────────────────────────────────────────
    // LibSeatSessionNotifier is !Send+!Sync so InsertError<it> can't convert
    // to anyhow::Error via ?. We ignore the result – seat events are optional.
    let (mut session, notifier) =
        LibSeatSession::new().context("LibSeat session")?;

    let _ = loop_handle.insert_source(notifier, |event, _, _state| {
        debug!(?event, "libseat event");
    });

    info!(seat = %session.seat(), "libseat session opened");

    // ── 4. GameframeState ─────────────────────────────────────────────────────
    let mut state = GameframeState::new(
        &mut display,
        loop_handle.clone(),
        opts.config.clone(),
        socket_name.clone(),
    );

    // ── 5. DRM device ─────────────────────────────────────────────────────────
    let drm_path = resolve_drm_node(&opts.drm_device, &opts.gpu_vendor)?;
    info!(path = %drm_path.display(), "Opening DRM device");

    let drm_node = DrmNode::from_path(&drm_path)
        .with_context(|| format!("DrmNode::from_path({})", drm_path.display()))?;

    // FIX 1 applied: OFlags now comes from smithay::reexports::rustix (see import above)
    let drm_fd: OwnedFd = session
        .open(
            &drm_path,
            OFlags::RDWR | OFlags::CLOEXEC | OFlags::NOCTTY | OFlags::NONBLOCK,
        )
        .context("session.open DRM")?;

    let drm_device_fd = DrmDeviceFd::new(DeviceFd::from(drm_fd));
    let (mut drm, drm_notifier) =
        DrmDevice::new(drm_device_fd.clone(), true).context("DrmDevice::new")?;

    // ── 6. Vendor quirks ──────────────────────────────────────────────────────
    apply_vendor_quirks(&opts.gpu_vendor);

    // ── 7. GBM ───────────────────────────────────────────────────────────────
    let gbm_device =
        gbm::Device::new(drm_device_fd.clone()).context("GBM device")?;
    let gbm_allocator = GbmAllocator::new(
        gbm_device.clone(),
        GbmBufferFlags::RENDERING | GbmBufferFlags::SCANOUT,
    );

    // ── 8. EGL + GLES renderer ────────────────────────────────────────────────
    // SAFETY: requires valid GBM device pointer
    let egl_display =
        unsafe { EGLDisplay::new(gbm_device.clone()).context("EGLDisplay::new")? };

    let egl_context = EGLContext::new(&egl_display).context("EGLContext::new")?;
    info!(drm = %drm_path.display(), "EGL display created");

    // SAFETY: makes EGLContext current on this thread
    let mut renderer: GlesRenderer =
        unsafe { GlesRenderer::new(egl_context).context("GlesRenderer::new")? };

    // FormatSet in Smithay 0.7 has no .len() – iterate and count instead
    let dmabuf_fmt_count = renderer.dmabuf_formats().iter().count();
    info!(dmabuf_formats = dmabuf_fmt_count, "GLES renderer ready");

    // ── 9. Enumerate connectors / outputs ─────────────────────────────────────
    let drm_resources =
        drm.resource_handles().context("DRM resource_handles")?;
    let mut output_manager = OutputManager::new();

    for &connector_handle in drm_resources.connectors() {
        let connector_info = drm.get_connector(connector_handle, false)?;
        if connector_info.state() != connector::State::Connected {
            debug!(?connector_handle, "not connected, skipping");
            continue;
        }

        let crtc_handle =
            find_crtc_for_connector(&drm, &drm_resources, &connector_info)?;
        let mode =
            select_mode(&connector_info, opts.config.display.preferred_mode.as_deref())?;

        info!(
            ?connector_handle, ?crtc_handle,
            mode = ?mode.name(), "Setting up output"
        );

        output_manager.add_output(
            &mut drm,
            gbm_allocator.clone(),
            &mut renderer,
            &state.display_handle,
            connector_handle,
            crtc_handle,
            mode,
            opts.config.display.scale,
            opts.config.display.vrr,
        )?;
    }

    if output_manager.output_count() == 0 {
        warn!("No connected outputs – starting headless");
    }

    // ── 10. DRM vblank source ─────────────────────────────────────────────────
    loop_handle
        .insert_source(drm_notifier, move |event, _meta, _state| {
            match event {
                DrmEvent::VBlank(crtc) => debug!(?crtc, "VBlank"),
                DrmEvent::Error(e)     => error!("DRM error: {e}"),
            }
        })
        .context("DRM notifier source")?;

    // ── 11. Frame pacing timer ────────────────────────────────────────────────
    let fps_cap = opts.config.display.fps_cap;
    let initial_interval = if fps_cap > 0 {
        Duration::from_secs_f64(1.0 / fps_cap as f64)
    } else {
        Duration::from_millis(4)
    };

    // Timer insert_source also returns !Send InsertError – ignore the result
    let _ = loop_handle.insert_source(
        Timer::from_duration(initial_interval),
        {
            let mut pacer = FramePacer::new(fps_cap);
            move |_deadline, _, _state| {
                TimeoutAction::ToDuration(pacer.next_interval())
            }
        },
    );

    // ── 12. XWayland ──────────────────────────────────────────────────────────
    if opts.config.session.xwayland {
        match xwayland::start(&loop_handle, &display) {
            Ok(_)  => info!("XWayland started"),
            Err(e) => warn!("XWayland failed: {e}"),
        }
    }

    // ── 13. Initial application ───────────────────────────────────────────────
    if let Some(ref exec) = opts
        .initial_exec
        .clone()
        .or_else(|| opts.config.session.initial_exec.clone())
    {
        spawn_app(exec, &socket_name, &opts.config.session.env)?;
    }

    // ── 14. Main event loop ───────────────────────────────────────────────────
    info!("Event loop running (Super+Esc=overlay, Ctrl+Alt+Backspace=quit)");
    let signal: LoopSignal = event_loop.get_signal();

    event_loop.run(
        Some(Duration::from_millis(4)),
        &mut state,
        |state| {
            display.flush_clients().ok();
            if !state.running {
                signal.stop();
            }
        },
    )?;

    info!("Event loop exited");
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn resolve_drm_node(
    forced: &Option<PathBuf>,
    vendor: &Option<GpuVendor>,
) -> Result<PathBuf> {
    if let Some(p) = forced {
        return Ok(p.clone());
    }
    if let Some(gpu) = gameframe_gpu::detect_primary()? {
        if let Some(v) = vendor {
            if &gpu.vendor != v {
                if let Some(m) = gameframe_gpu::detect_all()?
                    .into_iter()
                    .find(|g| &g.vendor == v)
                {
                    return Ok(m.drm_node);
                }
                warn!(%v, "Forced vendor not found – using primary GPU");
            }
        }
        return Ok(gpu.drm_node);
    }
    warn!("No GPU detected – falling back to /dev/dri/card0");
    Ok(PathBuf::from("/dev/dri/card0"))
}

fn apply_vendor_quirks(vendor: &Option<GpuVendor>) {
    match vendor.as_ref() {
        Some(GpuVendor::Nvidia) => {
            gameframe_gpu::nvidia::check_proprietary_kms();
            gameframe_gpu::nvidia::check_nouveau_firmware();
        }
        Some(GpuVendor::Software) => {
            warn!("Software renderer – no GPU acceleration");
        }
        _ => {}
    }
}

fn find_crtc_for_connector(
    drm: &DrmDevice,
    resources: &drm::control::ResourceHandles,
    connector: &drm::control::connector::Info,
) -> Result<crtc::Handle> {
    for enc_handle in connector.encoders() {
        if let Ok(enc) = drm.get_encoder(*enc_handle) {
            // FIX 3: CrtcListFilter.0 is private in drm-rs 0.14.
            // CrtcListFilter implements BitAnd<CrtcListFilter>, not BitAnd<u32>.
            // We use SAFETY: CrtcListFilter is repr(transparent) over u32,
            // so transmute is sound and the only public way to read the bits.
            let possible_bits: u32 = unsafe {
                std::mem::transmute(enc.possible_crtcs())
            };
            for (idx, crtc_handle) in resources.crtcs().iter().enumerate() {
                if possible_bits & (1u32 << idx) != 0 {
                    return Ok(*crtc_handle);
                }
            }
        }
    }
    anyhow::bail!("No CRTC for connector {:?}", connector.handle())
}

fn select_mode(
    connector: &connector::Info,
    preferred: Option<&str>,
) -> Result<drm::control::Mode> {
    let modes = connector.modes();
    if modes.is_empty() {
        anyhow::bail!("No modes for connector {:?}", connector.handle());
    }
    if let Some(pref) = preferred {
        if let Some(m) = modes.iter().find(|m| m.name().to_string_lossy() == pref) {
            return Ok(*m);
        }
        warn!("Mode '{pref}' not found – using EDID preferred");
    }
    if let Some(m) = modes.iter().find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED)) {
        return Ok(*m);
    }
    Ok(*modes.iter()
        .max_by_key(|m| m.size().0 as u32 * m.size().1 as u32)
        .unwrap())
}

fn spawn_app(
    exec: &str,
    wayland_display: &str,
    extra_env: &std::collections::HashMap<String, String>,
) -> Result<()> {
    info!(%exec, "Spawning application");
    let mut cmd = std::process::Command::new("sh");
    cmd.args(["-c", exec])
        .env("WAYLAND_DISPLAY", wayland_display)
        .env("XDG_SESSION_TYPE", "wayland")
        .env("GDK_BACKEND", "wayland")
        .env("QT_QPA_PLATFORM", "wayland")
        .env("SDL_VIDEODRIVER", "wayland")
        .env("CLUTTER_BACKEND", "wayland");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd.spawn()
        .with_context(|| format!("Failed to spawn: {exec}"))?;
    Ok(())
}
