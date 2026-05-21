use serde::{Deserialize, Serialize};
use gameframe_gpu::GpuVendor;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub gpu:     GpuConfig,
    pub display: DisplayConfig,
    pub session: SessionConfig,
    pub overlay: OverlayConfig,
    pub input:   InputConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GpuConfig {
    pub vendor:         Option<GpuVendor>,
    pub drm_device:     Option<std::path::PathBuf>,
    pub prime:          bool,
    pub prefer_nouveau: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    pub fps_cap:        u32,
    pub hdr:            bool,
    pub vrr:            bool,
    pub preferred_mode: Option<String>,
    pub rotation:       u32,
    pub scale:          f64,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self { fps_cap: 0, hdr: false, vrr: true, preferred_mode: None, rotation: 0, scale: 1.0 }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    pub initial_exec:  Option<String>,
    pub idle_timeout:  u64,
    pub xwayland:      bool,
    pub env:           std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    pub fps_counter: bool,
    pub gpu_temp:    bool,
    pub gpu_usage:   bool,
    pub cpu_usage:   bool,
    pub ram_usage:   bool,
    pub position:    OverlayPosition,
    pub width:       u32,
    pub height:      u32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            fps_counter: true, gpu_temp: true, gpu_usage: true,
            cpu_usage: true, ram_usage: true,
            position: OverlayPosition::TopLeft,
            width: 220, height: 130,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum OverlayPosition {
    #[default] TopLeft, TopRight, BottomLeft, BottomRight,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    pub repeat_delay: u32,
    pub repeat_rate:  u32,
}

impl Default for InputConfig {
    fn default() -> Self { Self { repeat_delay: 400, repeat_rate: 30 } }
}
