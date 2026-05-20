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
    /// Force a specific vendor; None = auto-detect
    pub vendor:     Option<GpuVendor>,
    /// Force a specific DRM card (e.g. /dev/dri/card1)
    pub drm_device: Option<std::path::PathBuf>,
    /// Enable PRIME render offload (dGPU render → iGPU display)
    pub prime:      bool,
    /// Prefer nouveau over proprietary Nvidia driver
    pub prefer_nouveau: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayConfig {
    /// Target FPS cap; 0 = uncapped (VRR drives pacing)
    pub fps_cap:         u32,
    /// Enable HDR output (requires HDR plane support)
    pub hdr:             bool,
    /// Enable VRR / Adaptive Sync / FreeSync
    pub vrr:             bool,
    /// Preferred output mode, e.g. "1920x1080@60"
    pub preferred_mode:  Option<String>,
    /// Force output rotation (0, 90, 180, 270)
    pub rotation:        u32,
    /// Output scale factor (1.0 = native, 2.0 = HiDPI)
    pub scale:           f64,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self { fps_cap: 0, hdr: false, vrr: true, preferred_mode: None, rotation: 0, scale: 1.0 }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Command to exec at startup (Steam, Heroic, etc.)
    pub initial_exec:  Option<String>,
    /// Idle timeout in seconds before DPMS-off; 0 = disabled
    pub idle_timeout:  u64,
    /// Enable XWayland for legacy/Steam games
    pub xwayland:      bool,
    /// Additional environment variables injected into spawned processes
    pub env:           std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    pub fps_counter:    bool,
    pub gpu_temp:       bool,
    pub gpu_usage:      bool,
    pub cpu_usage:      bool,
    pub ram_usage:      bool,
    pub position:       OverlayPosition,
    /// Overlay pixel width
    pub width:          u32,
    /// Overlay pixel height
    pub height:         u32,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            fps_counter: true,
            gpu_temp:    true,
            gpu_usage:   true,
            cpu_usage:   true,
            ram_usage:   true,
            position:    OverlayPosition::TopLeft,
            width:       220,
            height:      130,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub enum OverlayPosition {
    #[default] TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InputConfig {
    /// Milliseconds before key repeat starts
    pub repeat_delay: u32,
    /// Key repeats per second
    pub repeat_rate:  u32,
}
