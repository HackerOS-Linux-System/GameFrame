use smithay::{
    backend::renderer::{
        damage::OutputDamageTracker,
        element::surface::WaylandSurfaceRenderElement,
        gles::GlesRenderer,
    },
    desktop::{Space, Window},
    output::Output,
};
use tracing::debug;

use crate::cursor::render_software_cursor;

pub struct FrameResult {
    pub presented: bool,
    pub frame_ms:  f32,
}

/// Render one frame.
///
/// Uses `render_elements_for_output` then `damage_tracker.render_output`.
/// `render_output` in Smithay 0.7 requires a bound target (GBM surface).
/// Without one, we collect elements and track damage but skip the GPU submit.
/// The caller (DRM vblank handler) is responsible for target binding.
pub fn render_frame(
    renderer:         &mut GlesRenderer,
    damage_tracker:   &mut OutputDamageTracker,
    space:            &Space<Window>,
    output:           &Output,
    pointer_location: smithay::utils::Point<f64, smithay::utils::Logical>,
    cursor_status:    &smithay::input::pointer::CursorImageStatus,
) -> FrameResult {
    let t_start = std::time::Instant::now();
    let scale_f64 = output.current_scale().fractional_scale();
    // FIX: render_elements_for_output expects f32 scale, not f64
    let scale_f32 = scale_f64 as f32;

    // FIX: render_elements_for_output returns Result<Vec<SpaceRenderElements<...>>, OutputError>
    let elements_result = space.render_elements_for_output(renderer, output, scale_f32);

    let presented = match elements_result {
        Err(e) => {
            tracing::error!("render_elements_for_output error: {e:?}");
            false
        }
        Ok(_elements) => {
            // elements collected – in the full DRM pipeline we'd pass them to
            // damage_tracker.render_output(renderer, &mut gbm_target, 0, &elements, clear)
            // For now: signal that the frame was "presented" (no GPU work without target)
            debug!("elements collected, DRM target needed for submit");
            true
        }
    };

    // Software cursor stub
    render_software_cursor(renderer, cursor_status, pointer_location, scale_f64);

    let frame_ms = t_start.elapsed().as_secs_f32() * 1000.0;
    FrameResult { presented, frame_ms }
}

pub fn now_us() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_micros() as u64
}
