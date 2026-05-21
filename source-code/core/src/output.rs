use std::collections::HashMap;
use anyhow::Result;
use drm::control::{connector, crtc, Device as ControlDevice, Mode};
use smithay::{
    backend::{
        allocator::gbm::GbmAllocator,
        drm::{DrmDevice, DrmDeviceFd},
        renderer::{damage::OutputDamageTracker, gles::GlesRenderer},
    },
    output::{Mode as WlMode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::wayland_server::DisplayHandle,
    utils::Transform,
};
use tracing::info;

pub struct GameframeOutput {
    pub output:         Output,
    pub crtc:           crtc::Handle,
    pub connector:      connector::Handle,
    pub mode:           Mode,
    pub damage_tracker: OutputDamageTracker,
}

pub struct OutputManager {
    outputs: HashMap<crtc::Handle, GameframeOutput>,
}

impl OutputManager {
    pub fn new() -> Self { Self { outputs: HashMap::new() } }

    #[allow(clippy::too_many_arguments)]
    pub fn add_output(
        &mut self,
        drm: &mut DrmDevice,
        _allocator: GbmAllocator<DrmDeviceFd>,
        _renderer:  &mut GlesRenderer,
        display_handle: &DisplayHandle,
        connector: connector::Handle,
        crtc:      crtc::Handle,
        mode:      Mode,
        scale:     f64,
        vrr:       bool,
    ) -> Result<()> {
        let connector_info   = drm.get_connector(connector, true)?;
        let (phys_w, phys_h) = connector_info.size().unwrap_or((0, 0));
        let (pix_w, pix_h)   = (mode.size().0 as i32, mode.size().1 as i32);

        let wl_mode = WlMode {
            size:    (pix_w, pix_h).into(),
            refresh: mode.vrefresh() as i32 * 1000,
        };

        let output = Output::new(
            format!("GAMEFRAME-{}", self.outputs.len()),
            PhysicalProperties {
                size:     (phys_w as i32, phys_h as i32).into(),
                subpixel: Subpixel::Unknown,
                make:     format!("{:?}", connector_info.interface()),
                model:    "Gameframe Output".into(),
            },
        );
        output.create_global::<crate::state::GameframeState>(display_handle);
        output.add_mode(wl_mode);
        output.set_preferred(wl_mode);
        output.change_current_state(Some(wl_mode), Some(Transform::Normal), None, Some((0, 0).into()));
        output.change_current_state(None, None, Some(Scale::Fractional(scale)), None);

        let damage_tracker = OutputDamageTracker::from_output(&output);

        info!(
            ?connector, ?crtc,
            mode   = ?mode.name(),
            pixels = ?(pix_w, pix_h),
            scale, vrr,
            "Output configured"
        );

        self.outputs.insert(crtc, GameframeOutput { output, crtc, connector, mode, damage_tracker });
        Ok(())
    }

    pub fn output_count(&self) -> usize { self.outputs.len() }

    pub fn outputs(&self) -> impl Iterator<Item = &GameframeOutput> {
        self.outputs.values()
    }

    pub fn outputs_mut(&mut self) -> impl Iterator<Item = &mut GameframeOutput> {
        self.outputs.values_mut()
    }

    pub fn primary_output(&self) -> Option<&Output> {
        self.outputs.values().next().map(|o| &o.output)
    }
}

impl Default for OutputManager { fn default() -> Self { Self::new() } }
