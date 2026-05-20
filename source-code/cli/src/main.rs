use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use tracing::info;

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name        = "gameframe",
    version     = env!("CARGO_PKG_VERSION"),
    about       = "Steam Gaming Mode compositor for legacy AMD/Nvidia/Intel hardware",
    long_about  = "\
Gameframe is a Smithay-based Wayland compositor that replicates the Steam Gaming\n\
Mode experience on older machines that cannot run gamescope.\n\n\
Supported GPU families:\n\
  AMD    – HD 5000-9000, R5/R7/R9 200-400, RX (amdgpu / radeon + Mesa RADV)\n\
  Nvidia – GTX 9xx Maxwell, GTX 10xx Pascal (nouveau or proprietary)\n\
  Intel  – HD/UHD Gen6 (Sandy Bridge) through Gen12 (Alder Lake/UHD 770)\n\
           incl. Intel UHD 620/630/770\n"
)]
struct Cli {
    /// Force GPU vendor (default: auto-detect)
    #[arg(long, value_name = "VENDOR")]
    gpu: Option<CliGpuVendor>,

    /// Target FPS cap (0 = uncapped / VRR)
    #[arg(long, default_value = "0", value_name = "N")]
    fps_cap: u32,

    /// Enable HDR output
    #[arg(long)]
    hdr: bool,

    /// Enable VRR / FreeSync / Adaptive Sync (default: true)
    #[arg(long, default_value = "true")]
    vrr: bool,

    /// Disable VRR even if GPU supports it
    #[arg(long)]
    no_vrr: bool,

    /// Force a specific DRM device (e.g. /dev/dri/card1)
    #[arg(long, value_name = "PATH")]
    drm_device: Option<std::path::PathBuf>,

    /// Output scale factor (1.0 = native, 2.0 = HiDPI)
    #[arg(long, default_value = "1.0", value_name = "FACTOR")]
    scale: f64,

    /// Preferred output mode (e.g. 1920x1080@60)
    #[arg(long, value_name = "WxH@HZ")]
    mode: Option<String>,

    /// Enable XWayland (needed for Steam and most games)
    #[arg(long)]
    xwayland: bool,

    /// Verbosity (-v debug, -vv trace)
    #[arg(short = 'v', action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the Gameframe session [default]
    Start {
        /// Application to launch (e.g. "steam -gamepadui")
        #[arg(long, value_name = "CMD")]
        exec: Option<String>,
    },
    /// Stop a running session
    Stop,
    /// Show session status
    Status,
    /// Show detected GPU information
    GpuInfo,
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// Print effective config (merged defaults + file)
    Dump,
    /// Open config in $EDITOR
    Edit,
    /// Reset config to defaults
    Reset,
    /// Print config file path
    Path,
}

#[derive(ValueEnum, Clone, Debug, PartialEq, Eq)]
enum CliGpuVendor {
    Amd,
    Nvidia,
    Intel,
    Software,
}

impl From<CliGpuVendor> for gameframe_gpu::GpuVendor {
    fn from(v: CliGpuVendor) -> Self {
        match v {
            CliGpuVendor::Amd      => Self::Amd,
            CliGpuVendor::Nvidia   => Self::Nvidia,
            CliGpuVendor::Intel    => Self::Intel,
            CliGpuVendor::Software => Self::Software,
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "gameframe starting"
    );

    let mut config = load_config()?;

    // CLI flags override config values
    if cli.fps_cap > 0           { config.display.fps_cap   = cli.fps_cap; }
    if cli.hdr                   { config.display.hdr        = true; }
    if cli.no_vrr                { config.display.vrr        = false; }
    if cli.scale != 1.0          { config.display.scale      = cli.scale; }
    if let Some(m) = &cli.mode   { config.display.preferred_mode = Some(m.clone()); }
    if cli.xwayland              { config.session.xwayland   = true; }

    match cli.command.unwrap_or(Commands::Start { exec: None }) {
        Commands::Start { exec } => {
            use gameframe_core::{run_session, session::SessionOptions};
            run_session(SessionOptions {
                gpu_vendor:   cli.gpu.map(Into::into),
                drm_device:   cli.drm_device,
                initial_exec: exec,
                config,
            })
            .await?;
        }
        Commands::Stop   => gameframe_core::stop_session().await?,
        Commands::Status => gameframe_core::print_status().await?,
        Commands::GpuInfo => gameframe_gpu::print_gpu_info()?,
        Commands::Config { action } => handle_config_action(action)?,
    }

    Ok(())
}

// ── Config handling ───────────────────────────────────────────────────────────

fn load_config() -> Result<gameframe_core::Config> {
    let path = config_path()?;
    if path.exists() {
        let raw = std::fs::read_to_string(&path)?;
        Ok(toml::from_str(&raw)?)
    } else {
        Ok(gameframe_core::Config::default())
    }
}

fn config_path() -> Result<std::path::PathBuf> {
    use directories::ProjectDirs;
    let dirs = ProjectDirs::from("io", "gameframe", "gameframe")
        .ok_or_else(|| anyhow::anyhow!("Cannot determine config directory"))?;
    Ok(dirs.config_dir().join("config.toml"))
}

fn handle_config_action(action: ConfigAction) -> Result<()> {
    let path = config_path()?;
    match action {
        ConfigAction::Dump => {
            let cfg = load_config()?;
            println!("{}", toml::to_string_pretty(&cfg)?);
        }
        ConfigAction::Edit => {
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nano".into());
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if !path.exists() {
                std::fs::write(&path, toml::to_string_pretty(&gameframe_core::Config::default())?)?;
            }
            std::process::Command::new(&editor).arg(&path).status()?;
        }
        ConfigAction::Reset => {
            if path.exists() { std::fs::remove_file(&path)?; }
            println!("Configuration reset to defaults.");
        }
        ConfigAction::Path => {
            println!("{}", path.display());
        }
    }
    Ok(())
}

// ── Logging ───────────────────────────────────────────────────────────────────

fn init_logging(verbose: u8) {
    let level = match verbose {
        0 => "gameframe=info",
        1 => "gameframe=debug",
        _ => "gameframe=trace,smithay=debug",
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level)),
        )
        .with_target(true)
        .with_thread_ids(false)
        .compact()
        .init();
}
