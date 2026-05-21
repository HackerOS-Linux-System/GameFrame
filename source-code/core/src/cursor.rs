use smithay::{
    backend::renderer::gles::GlesRenderer,
    input::pointer::CursorImageStatus,
    reexports::wayland_server::Resource,   // FIX: .id() requires Resource trait in scope
    utils::{Logical, Point},
};
use tracing::debug;

/// Render the pointer cursor as a software overlay.
pub fn render_software_cursor(
    _renderer:        &mut GlesRenderer,
    cursor_status:    &CursorImageStatus,
    pointer_location: Point<f64, Logical>,
    _output_scale:    f64,
) {
    match cursor_status {
        CursorImageStatus::Hidden => {}
        CursorImageStatus::Named(_name) => {
            let _ = pointer_location;
        }
        CursorImageStatus::Surface(surface) => {
            // FIX: import Resource so .id() is available
            debug!("client cursor surface: {:?}", surface.id());
        }
    }
}

/// DRM hardware cursor plane stub.
pub struct HardwareCursor {
    pub available: bool,
}

impl HardwareCursor {
    pub fn new(available: bool) -> Self { Self { available } }

    pub fn upload(&mut self, _status: &CursorImageStatus) {}
    pub fn move_to(&self, _x: i32, _y: i32) {}
}
