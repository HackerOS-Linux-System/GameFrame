# Gameframe

**Steam Gaming Mode compositor for legacy hardware**  
Rust + [Smithay](https://github.com/Smithay/smithay) · Apache-2.0

Gameframe replicates the Steam Gaming Mode / gamescope experience on older
machines that cannot run gamescope due to missing Vulkan extensions or kernel
features. It is a full Wayland compositor built on Smithay 0.7.

---

## Supported hardware

| Vendor | Generations | Kernel driver | Mesa |
|--------|------------|---------------|------|
| **AMD** | HD 5000–9000, R5/R7/R9 200–400 (GCN1–4), RX 400–500 | `amdgpu` / `radeon` | RadeonSI + RADV |
| **Nvidia** | GTX 9xx (Maxwell), GTX 10xx (Pascal) | `nouveau` + firmware **or** proprietary `nvidia` | NV50/NVC0 + NVK (Vulkan) |
| **Intel** | Gen 6–12: Sandy Bridge → Alder Lake | `i915` / `xe` | `crocus` (Gen6–8) · `iris` (Gen9–12) |
| | incl. **UHD 620** (Whiskey/Cannon Lake) | | |
| | incl. **UHD 630** (Coffee Lake) | | |
| | incl. **UHD 770** (Alder Lake) | | |
| | incl. **Iris Plus** G4/G7 (Ice Lake) | | |

---

## Features

- 🎮 Full-screen Steam Gaming Mode session (every app forced fullscreen)
- 📺 DRM/KMS atomic modesetting with damage tracking
- 🔁 VRR / FreeSync / Adaptive-Sync (AMD + Intel Gen11+)
- 🖥️ XWayland integration for Steam and legacy X11 games
- 📊 In-session overlay (tiny-skia): FPS, GPU/CPU temp, usage bars, toasts
- ⌨️ Input grab with libinput · gamepad / Steam Input pass-through
- 🔔 Notification toasts (app launch, Steam events)
- ⚙️ TOML configuration (`~/.config/gameframe/config.toml`)
- 🔌 PRIME render offload (dGPU render → iGPU display)
- 🪑 libseat session (no root required)

---

## Quick start

### Build dependencies

```bash
# Arch / Manjaro / Bazzite
sudo pacman -S rust libseat libinput mesa drm gbm

# Fedora / SteamOS-like
sudo dnf install rust cargo libseat-devel libinput-devel mesa-libGL-devel

# Debian / Ubuntu
sudo apt install rustup libseat-dev libinput-dev libgles2-mesa-dev \
                 libgbm-dev libudev-dev libxkbcommon-dev xwayland
```

### Build & install

```bash
git clone https://github.com/gameframe-project/gameframe
cd gameframe
cargo build --release
sudo install -Dm755 target/release/gameframe /usr/local/bin/gameframe
```

### Run

```bash
# Auto-detect GPU, start with Steam Big Picture
gameframe start --exec "steam -gamepadui" --xwayland

# Force AMD backend, cap at 60 FPS, scale 1.5x
gameframe --gpu amd --fps-cap 60 --scale 1.5 start --exec "steam -gamepadui"

# Force Nvidia proprietary (make sure nvidia-drm.modeset=1 is set)
gameframe --gpu nvidia start

# Intel UHD 630 laptop – VRR might not be available, disable it
gameframe --gpu intel --no-vrr start

# Show detected GPUs
gameframe gpu-info

# Dump current configuration
gameframe config dump

# Edit configuration
gameframe config edit
```

---

## Configuration

Default location: `~/.config/gameframe/config.toml`

```toml
[gpu]
# vendor = "amd"          # auto-detect if omitted
# drm_device = "/dev/dri/card0"
prime = false
prefer_nouveau = false    # set true to prefer nouveau over nvidia prop.

[display]
fps_cap = 0               # 0 = uncapped (VRR drives pacing)
hdr = false
vrr = true
# preferred_mode = "1920x1080@60"
scale = 1.0

[session]
xwayland = true           # required for Steam and most games
# initial_exec = "steam -gamepadui"
idle_timeout = 0          # seconds; 0 = disabled

[overlay]
fps_counter = true
gpu_temp    = true
gpu_usage   = true
cpu_usage   = true
ram_usage   = true
position    = "TopLeft"   # TopLeft | TopRight | BottomLeft | BottomRight
width  = 220
height = 130

[input]
repeat_delay = 400        # ms
repeat_rate  = 30         # repeats/second
```

---

## Keyboard shortcuts

| Shortcut | Action |
|----------|--------|
| `Super + Esc` | Toggle overlay / quick-access menu |
| `Ctrl + Alt + Backspace` | Kill session |

---

## Architecture

```
gameframe-cli        clap entry point; merges CLI flags with config
├── gameframe-core   Smithay compositor, calloop event loop,
│   ├── compositor   DRM device init, GBM, EGL, GlesRenderer, outputs
│   ├── state        Central GameframeState (all Smithay delegates)
│   ├── output       Per-connector Output + DrmCompositor + damage tracking
│   ├── session      SessionOptions, run/stop/status
│   ├── frame        FramePacer (FPS cap + VRR)
│   └── xwayland     XWayland lifecycle
├── gameframe-gpu    GPU detection (sysfs), vendor quirks
│   ├── amd          amdgpu/radeon, FreeSync, RADV check
│   ├── nvidia       nouveau firmware check, KMS modeset check
│   └── intel        Gen6–12 identification, mesa driver, UHD 620/630/770
├── gameframe-input  libinput, keybindings, grab management
└── gameframe-overlay tiny-skia HUD renderer → Wayland SHM buffer
```

---

## Nvidia notes

For the proprietary driver, add to kernel cmdline:
```
nvidia-drm.modeset=1
```

For nouveau (open-source):
```bash
# Install firmware (Fedora/Ubuntu)
sudo dnf install linux-firmware       # or
sudo apt install linux-firmware-nonfree
```

Maxwell/Pascal nouveau 3D requires signed firmware blobs.
Without them modesetting works but 3D acceleration is unavailable.

---

## Intel UHD notes

- **UHD 620** (Whiskey Lake / 8th gen U): Gen 9.5, `iris` Mesa, no VRR
- **UHD 630** (Coffee Lake / 8th–9th gen): Gen 9.5, `iris` Mesa, no VRR  
- **UHD 770** (Alder Lake / 12th gen): Gen 12, `iris` Mesa, VRR via PSR2
- **Iris Plus G7** (Ice Lake / 10th gen): Gen 11, `iris` Mesa, VRR via PSR2

For iGPU-only laptops (no dGPU), gameframe works out of the box.
For Optimus laptops (Intel iGPU + Nvidia dGPU), enable PRIME in config:
```toml
[gpu]
prime = true
```

---

## License

Apache-2.0 – see [LICENSE](LICENSE)
