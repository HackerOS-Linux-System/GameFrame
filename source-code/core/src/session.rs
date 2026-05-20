use std::path::PathBuf;
use anyhow::Result;
use tracing::{error, info, warn};
use gameframe_gpu::GpuVendor;
use crate::{compositor, Config};

pub struct SessionOptions {
    pub gpu_vendor:   Option<GpuVendor>,
    pub drm_device:   Option<PathBuf>,
    pub initial_exec: Option<String>,
    pub config:       Config,
}

pub async fn run_session(opts: SessionOptions) -> Result<()> {
    info!(
        gpu      = %opts.gpu_vendor.as_ref().map(|v| v.to_string()).unwrap_or_else(|| "auto".into()),
        fps_cap  = opts.config.display.fps_cap,
        hdr      = opts.config.display.hdr,
        vrr      = opts.config.display.vrr,
        xwayland = opts.config.session.xwayland,
        "Starting Gameframe session"
    );

    // Pre-flight GPU checks
    match gameframe_gpu::detect_primary() {
        Ok(Some(gpu)) => {
            info!(name = %gpu.name, vendor = %gpu.vendor, driver = %gpu.driver, "Primary GPU");
            match &gpu.vendor {
                GpuVendor::Nvidia => {
                    gameframe_gpu::nvidia::check_nouveau_firmware();
                    gameframe_gpu::nvidia::check_proprietary_kms();
                }
                GpuVendor::Amd => {
                    if !gameframe_gpu::amd::check_radv() {
                        warn!("RADV Vulkan ICD not found");
                    }
                }
                GpuVendor::Intel => {
                    gameframe_gpu::intel::log_intel_backend(gpu.device_id);
                }
                GpuVendor::Software => {}
            }
        }
        Ok(None) => warn!("No GPU detected – software renderer"),
        Err(e)   => warn!("GPU detection error: {e}"),
    }

    tokio::task::spawn_blocking(move || compositor::run(&opts))
        .await?
        .map_err(|e| { error!(?e, "Compositor error"); e })
}

pub async fn stop_session() -> Result<()> {
    let lock = lock_path();
    if lock.exists() {
        let pid: i32 = std::fs::read_to_string(&lock)?.trim().parse()?;
        // SAFETY: kill with SIGTERM is always safe to call
        unsafe { libc::kill(pid, libc::SIGTERM); }
        info!(pid, "Sent SIGTERM to session");
        std::fs::remove_file(lock).ok();
    } else {
        println!("No active Gameframe session found.");
    }
    Ok(())
}

pub async fn print_status() -> Result<()> {
    let lock = lock_path();
    if lock.exists() {
        let pid = std::fs::read_to_string(&lock)?;
        println!("Gameframe running (PID {})", pid.trim());
        if let Ok(Some(gpu)) = gameframe_gpu::detect_primary() {
            println!("  GPU: {} [{}]", gpu.name, gpu.driver);
        }
    } else {
        println!("No active Gameframe session.");
    }
    Ok(())
}

fn lock_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join("gameframe.lock")
}
