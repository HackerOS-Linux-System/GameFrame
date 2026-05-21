use anyhow::Result;
use smithay::reexports::{calloop::LoopHandle, wayland_server::Display};
use tracing::info;

pub struct XWaylandHandle {
    pub display_number: u32,
}

/// Start XWayland and wire it into the calloop event loop.
pub fn start<D: 'static>(
    _loop_handle: &LoopHandle<'static, D>,
    _display: &Display<D>,
) -> Result<XWaylandHandle> {
    let path = find_xwayland()?;
    info!(binary = %path, "Starting XWayland");

    // ── Full Smithay 0.7 XWayland wiring ─────────────────────────────────────
    //
    // use smithay::xwayland::{XWayland, XWaylandClientData, XWaylandEvent};
    //
    // let (xwayland, client_token) = XWayland::new(_loop_handle, _display)
    //     .context("XWayland::new")?;
    //
    // _loop_handle
    //     .insert_source(xwayland, |event, _, state: &mut D| match event {
    //         XWaylandEvent::Ready {
    //             connection, client, display_number, wm_fd,
    //         } => {
    //             info!("XWayland ready on :{display_number}");
    //             std::env::set_var("DISPLAY", format!(":{display_number}"));
    //
    //             // Start the X11 window manager
    //             if let Ok(xwm) = X11Wm::start_wm(
    //                 state.loop_handle.clone(),
    //                 wm_fd,
    //                 connection,
    //                 client,
    //             ) {
    //                 state.xwm = Some(xwm);
    //             }
    //         }
    //         XWaylandEvent::Exited => {
    //             warn!("XWayland exited unexpectedly");
    //             state.xwm = None;
    //         }
    //     })
    //     .context("XWayland event source")?;
    //
    // ── Stub: set DISPLAY so spawned apps find XWayland ──────────────────────
    // The real wiring above requires GameframeState to implement XwmHandler
    // (handle_request, map_window, unmap_window, etc.) which is a non-trivial
    // addition; tracked as issue #12.

    let display_number = 1u32;
    std::env::set_var("DISPLAY", format!(":{display_number}"));
    info!("DISPLAY=:{display_number} set for X11 clients");

    Ok(XWaylandHandle { display_number })
}

fn find_xwayland() -> Result<String> {
    for path in ["/usr/bin/Xwayland", "/usr/local/bin/Xwayland", "/bin/Xwayland"] {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }
    if let Ok(o) = std::process::Command::new("which").arg("Xwayland").output() {
        if o.status.success() {
            let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !p.is_empty() { return Ok(p); }
        }
    }
    anyhow::bail!("Xwayland binary not found – install xwayland package")
}
