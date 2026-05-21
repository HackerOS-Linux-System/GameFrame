use anyhow::Result;
use smithay::{
    backend::renderer::{gles::GlesRenderer, ImportDma},
    reexports::wayland_server::DisplayHandle,
    wayland::dmabuf::{DmabufGlobal, DmabufState},
};
use tracing::{debug, warn};

use crate::state::GameframeState;

pub fn init_dmabuf_global(
    renderer:       &GlesRenderer,
    dmabuf_state:   &mut DmabufState,
    display_handle: &DisplayHandle,
) -> Result<DmabufGlobal> {
    // FIX: into_iter() instead of .iter().collect() so Item = Format, not &Format
    let formats: Vec<_> = renderer.dmabuf_formats().into_iter().collect();

    if formats.is_empty() {
        warn!("GlesRenderer has no DMABUF formats");
    } else {
        debug!("Advertising {} DMABUF formats", formats.len());
    }

    let global = dmabuf_state
        .create_global::<GameframeState>(display_handle, formats);

    Ok(global)
}
