use tracing::{error, info, warn};

pub fn check_nouveau_firmware() -> bool {
    let path = std::path::Path::new("/lib/firmware/nvidia");
    if path.exists() {
        info!("Nouveau firmware found at /lib/firmware/nvidia");
        true
    } else {
        warn!(
            "Nouveau firmware not found! 3D acceleration unavailable. \
             Install linux-firmware or use the proprietary nvidia driver."
        );
        false
    }
}

pub fn check_proprietary_kms() -> bool {
    match std::fs::read_to_string("/sys/module/nvidia_drm/parameters/modeset") {
        Ok(v) if v.trim() == "Y" => { info!("nvidia-drm KMS enabled ✓"); true }
        Ok(_) => {
            error!("nvidia-drm.modeset=0 – add 'nvidia-drm.modeset=1' to kernel cmdline!");
            false
        }
        Err(_) => { warn!("nvidia_drm module not loaded"); false }
    }
}

pub fn active_driver() -> &'static str {
    if std::path::Path::new("/sys/module/nvidia_drm").exists() { "nvidia" }
    else if std::path::Path::new("/sys/module/nouveau").exists() { "nouveau" }
    else { "none" }
}

pub fn check_vulkan_icd() -> bool {
    std::path::Path::new("/usr/share/vulkan/icd.d/nvidia_icd.json").exists()
}
