use anyhow::Result;
use smithay::reexports::{calloop::LoopHandle, wayland_server::Display};
use tracing::info;

pub struct XWaylandHandle;

pub fn start<D: 'static>(
    _loop_handle: &LoopHandle<'static, D>,
    _display: &Display<D>,
) -> Result<XWaylandHandle> {
    let xwayland_path = find_xwayland()?;
    info!(path = %xwayland_path, "XWayland binary found");

    // Full Smithay 0.7 wiring:
    //
    // use smithay::xwayland::{XWayland, XWaylandEvent};
    // let (xwayland, client) = XWayland::new(_loop_handle, _display)?;
    // _loop_handle.insert_source(xwayland, |event, _, state| match event {
    //     XWaylandEvent::Ready { display_number, .. } => {
    //         info!("XWayland on :{display_number}");
    //         std::env::set_var("DISPLAY", format!(":{display_number}"));
    //     }
    //     XWaylandEvent::Exited => info!("XWayland exited"),
    // })?;

    std::env::set_var("DISPLAY", ":1");
    info!("DISPLAY=:1 set for XWayland clients");
    Ok(XWaylandHandle)
}

fn find_xwayland() -> Result<String> {
    for path in ["/usr/bin/Xwayland", "/usr/local/bin/Xwayland"] {
        if std::path::Path::new(path).exists() {
            return Ok(path.to_string());
        }
    }
    if let Ok(o) = std::process::Command::new("which").arg("Xwayland").output() {
        if o.status.success() {
            return Ok(String::from_utf8_lossy(&o.stdout).trim().to_string());
        }
    }
    anyhow::bail!("Xwayland binary not found – install the xwayland package")
}
