use anyhow::Result as AnyResult;
use clap::{Parser, Subcommand, ValueEnum};

use duct::{cmd, Expression};

#[derive(Debug, Subcommand)]
pub enum Subcommands {
    /// Builds the firmware.
    Build {
        /// Which hardware version to build for.
        hw: Option<HardwareVersion>,

        #[arg(long)]
        timings: bool,

        /// Whether to build with Wi-Fi support.
        #[arg(long)]
        with_wifi: bool,
    },

    /// Runs tests.
    Test,

    /// Builds, flashes and runs the firmware on a connected device.
    Run {
        /// Which hardware version to run on.
        hw: Option<HardwareVersion>,

        profile: Option<Profile>,

        /// Whether to build with Wi-Fi support.
        #[arg(long)]
        with_wifi: bool,
    },

    /// Checks the project for errors.
    Check {
        /// Which hardware version to check for.
        hw: Option<HardwareVersion>,
        /// Whether to check Wi-Fi code.
        #[arg(long)]
        with_wifi: bool,
    },

    /// Builds the documentation.
    Doc {
        /// Which hardware version to build for.
        hw: Option<HardwareVersion>,

        /// Whether to open the documentation in a browser.
        #[clap(long)]
        open: bool,
    },

    /// Runs extra checks (clippy).
    ExtraCheck {
        /// Which hardware version to check for.
        hw: Option<HardwareVersion>,
    },

    /// Runs an example.
    Example {
        /// Which package to run the example from.
        package: String,

        /// Which example to run.
        name: String,

        /// Whether to watch for changes and re-run.
        #[clap(long)]
        watch: bool,
    },
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HardwareVersion {
    V4,
    V6S3,
    #[default]
    V6C6,
    V8S3,
}

impl HardwareVersion {
    fn feature(&self) -> &str {
        match self {
            HardwareVersion::V4 => "hw_v4",
            HardwareVersion::V6S3 => "hw_v6,esp32s3",
            HardwareVersion::V6C6 => "hw_v6,esp32c6",
            HardwareVersion::V8S3 => "hw_v8,esp32s3",
        }
    }

    fn soc(&self) -> SocConfig {
        match self {
            HardwareVersion::V6C6 => SocConfig::C6,
            _ => SocConfig::S3,
        }
    }

    fn flash_size(&self) -> u32 {
        2
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Profile {
    Debug,
    Release,
}

#[derive(Debug, Parser)]
#[clap(about, version, propagate_version = true)]
pub struct Cli {
    #[clap(subcommand)]
    pub subcommand: Subcommands,
}

fn cargo(toolchain: &'static str, command: &[&str]) -> Expression {
    println!(
        "🛠️  Running command: cargo +{toolchain} {}",
        command.join(" ")
    );

    let mut args_vec = vec!["run", toolchain, "cargo"];
    args_vec.extend(command);

    cmd("rustup", args_vec)
}

fn build(config: BuildConfig, timings: bool) -> AnyResult<()> {
    let build_flags = config.build_flags();
    let build_flags = build_flags.iter().map(|s| s.as_str()).collect::<Vec<_>>();

    if timings {
        let mut command = vec!["build", "--timings"];

        command.extend_from_slice(&build_flags);

        cargo(config.soc.toolchain(), &command).run()?;
    }

    let flash_size = format!("-s{}mb", config.version.flash_size());
    let mut command = vec![
        "espflash",
        "save-image",
        "--chip",
        config.soc.chip(),
        &flash_size,
        "target/card_io_fw.bin",
    ];
    command.extend_from_slice(&build_flags);

    cargo(config.soc.toolchain(), &command).run()?;

    Ok(())
}

fn run(config: BuildConfig) -> AnyResult<()> {
    let build_flags = config.build_flags();
    let build_flags = build_flags.iter().map(|s| s.as_str()).collect::<Vec<_>>();

    println!("🛠️  Building firmware");

    build(config, false)?;

    println!("💾  Flashing firmware");

    let mut args = vec!["run"];
    args.extend_from_slice(&build_flags);

    cargo(config.soc.toolchain(), &args).run()?;

    Ok(())
}

fn checks(config: BuildConfig) -> AnyResult<()> {
    let build_flags = config.build_flags();
    let build_flags = build_flags.iter().map(|s| s.as_str()).collect::<Vec<_>>();

    let mut args = vec!["check"];
    args.extend_from_slice(&build_flags);

    cargo(config.soc.toolchain(), &args).run()?;

    Ok(())
}

fn docs(config: BuildConfig, open: bool) -> AnyResult<()> {
    let build_flags = config.build_flags();
    let build_flags = build_flags.iter().map(|s| s.as_str()).collect::<Vec<_>>();

    let mut args = vec!["doc"];
    args.extend_from_slice(&build_flags);

    if open {
        args.push("--open");
    }

    cargo(config.soc.toolchain(), &args).run()?;

    Ok(())
}

fn extra_checks(config: BuildConfig) -> AnyResult<()> {
    cargo(config.soc.toolchain(), &["fmt", "--check"]).run()?;

    let build_flags = config.build_flags();
    let build_flags = build_flags.iter().map(|s| s.as_str()).collect::<Vec<_>>();

    let mut args = vec!["clippy"];
    args.extend_from_slice(&build_flags);

    cargo(config.soc.toolchain(), &args).run()?;

    Ok(())
}

fn test() -> AnyResult<()> {
    let packages = ["signal-processing"];

    let mut args = vec!["test", "--features=dyn_filter"];

    for p in packages {
        args.push("-p");
        args.push(p);
    }

    cargo("esp", &args).run()?;

    Ok(())
}

fn example(package: String, name: String, watch: bool) -> AnyResult<()> {
    let mut args = vec!["run", "--example", &name, "-p", &package];

    // Add required features, etc.
    match (package.as_str(), name.as_str()) {
        ("config-site", "simple") => args.extend_from_slice(&["--features=std,log"]),
        _ => {}
    }

    let program;
    if watch {
        program = args.join(" ");
        args = vec!["watch", "-x", &program];
    }

    // We want to run examples with the default toolchain, not the ESP one.
    cmd("cargo", &args).run()?;

    Ok(())
}

fn main() -> AnyResult<()> {
    let cli = Cli::parse();

    match cli.subcommand {
        Subcommands::Build {
            hw,
            with_wifi,
            timings,
        } => build(BuildConfig::new(hw, None, with_wifi), timings),
        Subcommands::Test => test(),
        Subcommands::Run {
            hw,
            profile,
            with_wifi,
        } => run(BuildConfig::new(hw, profile, with_wifi)),
        Subcommands::Check { hw, with_wifi } => checks(BuildConfig::new(hw, None, with_wifi)),
        Subcommands::Doc { hw, open } => docs(BuildConfig::new(hw, None, true), open),
        Subcommands::ExtraCheck { hw } => extra_checks(BuildConfig::new(hw, None, true)),
        Subcommands::Example {
            package,
            name,
            watch,
        } => example(package, name, watch),
    }
}

#[derive(Clone, Copy, Parser, Debug)]
enum SocConfig {
    S3,
    C5,
    C6,
    C61,
}

impl SocConfig {
    fn chip(self) -> &'static str {
        match self {
            SocConfig::S3 => "esp32s3",
            SocConfig::C5 => "esp32c5",
            SocConfig::C6 => "esp32c6",
            SocConfig::C61 => "esp32c61",
        }
    }

    fn triple(self) -> &'static str {
        match self {
            SocConfig::S3 => "xtensa-esp32s3-none",
            SocConfig::C5 | SocConfig::C6 | SocConfig::C61 => "riscv32imac-unknown-none",
        }
    }

    fn target(self) -> String {
        format!("{}-elf", self.triple())
    }

    fn toolchain(self) -> &'static str {
        match self {
            SocConfig::S3 => "esp",
            SocConfig::C5 | SocConfig::C6 | SocConfig::C61 => "nightly",
        }
    }
}

#[derive(Clone, Copy)]
struct BuildConfig {
    version: HardwareVersion,
    profile: Profile,
    soc: SocConfig,
    with_wifi: bool,
}

impl BuildConfig {
    fn new(hw: Option<HardwareVersion>, variant: Option<Profile>, with_wifi: bool) -> BuildConfig {
        let hw = hw.unwrap_or_default();
        Self {
            version: hw,
            soc: hw.soc(),
            profile: variant.unwrap_or(Profile::Debug),
            with_wifi,
        }
    }

    fn build_flags(self) -> Vec<String> {
        let mut flags = vec![
            format!("--target={}", self.soc.target()),
            format!("--features={}", {
                let mut features = vec![];
                features.push(self.version.feature());
                if self.with_wifi {
                    features.push("wifi");
                }
                features.join(",")
            }),
            String::from("-Zbuild-std=core,alloc"),
        ];

        if self.profile == Profile::Release {
            flags.push(String::from("--release"));
            flags.push(String::from("-Zbuild-std-features=panic_immediate_abort"));
        }

        flags
    }
}
