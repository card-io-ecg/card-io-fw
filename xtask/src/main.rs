use std::env;

use anyhow::Result as AnyResult;
use clap::{Parser, Subcommand, ValueEnum};

use duct::{cmd, Expression};

#[derive(Debug, Subcommand)]
pub enum Subcommands {
    /// Builds the firmware.
    Build {
        /// Which hardware version to build for.
        hw: Option<HardwareVersion>,
        variant: Option<BuildVariant>,
    },

    /// Builds the firmware and dumps the assembly.
    Asm {
        /// Which hardware version to build for.
        hw: Option<HardwareVersion>,
    },

    /// Runs tests.
    Test,

    /// Connects to the Card/IO device to display serial output.
    Monitor { variant: Option<MonitorVariant> },

    /// Builds, flashes and runs the firmware on a connected device.
    Run {
        /// Which hardware version to run on.
        hw: Option<HardwareVersion>,
    },

    /// Checks the project for errors.
    Check {
        /// Which hardware version to check for.
        hw: Option<HardwareVersion>,
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
    V1,
    V2,
    #[default]
    V4,
}

impl HardwareVersion {
    fn feature(&self) -> &str {
        match self {
            HardwareVersion::V1 => "hw_v1",
            HardwareVersion::V2 => "hw_v2",
            HardwareVersion::V4 => "hw_v4",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BuildVariant {
    StackSizes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum MonitorVariant {
    Debug,
    Release,
}

#[derive(Debug, Parser)]
#[clap(about, version, propagate_version = true)]
pub struct Cli {
    #[clap(subcommand)]
    pub subcommand: Subcommands,
}

fn cargo(command: &[&str]) -> Expression {
    println!("🛠️  Running command: cargo +esp {}", command.join(" "));

    let mut args_vec = vec!["run", "esp", "cargo"];
    args_vec.extend(command);

    cmd("rustup", args_vec)
}

fn build(hw: HardwareVersion, opt: Option<BuildVariant>, timings: bool) -> AnyResult<()> {
    let build_flags = [
        "--target=xtensa-esp32s3-none-elf",
        &format!("--features={}", hw.feature()),
        "-Zbuild-std=core,alloc",
        "-Zbuild-std-features=panic_immediate_abort",
    ];

    if let Some(option) = opt {
        match option {
            BuildVariant::StackSizes => {
                let mut command = vec!["rustc"];

                command.extend_from_slice(&build_flags);
                command.extend_from_slice(&[
                    "--profile=lto",
                    "--",
                    "-Zemit-stack-sizes",
                    "--emit=llvm-bc",
                ]);

                cargo(&command).run()?;

                return Ok(());
            }
        }
    }

    if timings {
        let mut command = vec!["build", "--release", "--timings"];

        command.extend_from_slice(&build_flags);

        cargo(&command).run()?;
    }

    let mut command = vec![
        "espflash",
        "save-image",
        "--release",
        "--chip",
        "esp32s3",
        "-s2mb",
        "target/card_io_fw.bin",
    ];
    command.extend_from_slice(&build_flags);

    cargo(&command).run()?;

    Ok(())
}

fn run(hw: HardwareVersion) -> AnyResult<()> {
    cargo(&[
        "espflash",
        "flash",
        "-M",
        "--erase-parts=otadata",
        "--target=xtensa-esp32s3-none-elf",
        "-Zbuild-std=core,alloc",
        "-Zbuild-std-features=panic_immediate_abort",
        &format!("--features={}", hw.feature()),
    ])
    .run()?;

    Ok(())
}

fn monitor(variant: MonitorVariant) -> AnyResult<()> {
    let variant = match variant {
        MonitorVariant::Debug => "debug",
        MonitorVariant::Release => "release",
    };

    cargo(&[
        "espflash",
        "monitor",
        "-e",
        &format!("./target/xtensa-esp32s3-none-elf/{variant}/card_io_fw"),
    ])
    .run()?;

    Ok(())
}

fn checks(hw: HardwareVersion) -> AnyResult<()> {
    cargo(&[
        "check",
        "--target=xtensa-esp32s3-none-elf",
        "-Zbuild-std=core,alloc",
        "-Zbuild-std-features=panic_immediate_abort",
        &format!("--features={}", hw.feature()),
    ])
    .run()?;

    Ok(())
}

fn docs(open: bool, hw: HardwareVersion) -> AnyResult<()> {
    let hw = format!("--features={}", hw.feature());
    let mut args = vec![
        "doc",
        "--target=xtensa-esp32s3-none-elf",
        "-Zbuild-std=core,alloc",
        "-Zbuild-std-features=panic_immediate_abort",
        &hw,
    ];

    if open {
        args.push("--open");
    }

    cargo(&args).run()?;

    Ok(())
}

fn extra_checks(hw: HardwareVersion) -> AnyResult<()> {
    cargo(&["fmt", "--check"]).run()?;
    cargo(&[
        "clippy",
        "--target=xtensa-esp32s3-none-elf",
        "-Zbuild-std=core,alloc",
        "-Zbuild-std-features=panic_immediate_abort",
        &format!("--features={}", hw.feature()),
    ])
    .run()?;

    Ok(())
}

fn test() -> AnyResult<()> {
    let packages = ["signal-processing"];

    let mut args = vec!["test", "--features=dyn_filter"];

    for p in packages {
        args.push("-p");
        args.push(p);
    }

    cargo(&args).run()?;

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

fn asm() -> AnyResult<()> {
    cmd!(
        "xtensa-esp32s3-elf-objdump",
        "-d",
        "./target/xtensa-esp32s3-none-elf/release/card_io_fw"
    )
    .stdout_path("target/asm.s")
    .run()?;

    cmd!(
        "xtensa-esp32s3-elf-nm",
        "./target/xtensa-esp32s3-none-elf/release/card_io_fw",
        "-S",
        "--size-sort",
    )
    .stdout_path("target/syms.txt")
    .run()?;

    std::fs::remove_file("target/asm_filt.s").ok();
    std::fs::remove_file("target/syms_filt.txt").ok();

    cmd!("rustfilt", "-i=target/asm.s", "-o=target/asm_filt.s").run()?;
    cmd!("rustfilt", "-i=target/syms.txt", "-o=target/syms_filt.txt").run()?;

    println!("🛠️  Assembly saved to target/asm.s");

    Ok(())
}

fn main() -> AnyResult<()> {
    let cli = Cli::parse();

    env::set_var("DEFMT_LOG", "card_io_fw=debug,info");

    match cli.subcommand {
        Subcommands::Build { hw, variant: opt } => build(hw.unwrap_or_default(), opt, false),
        Subcommands::Test => test(),
        Subcommands::Asm { hw } => {
            build(hw.unwrap_or_default(), None, true)?;
            asm()
        }
        Subcommands::Monitor { variant } => monitor(variant.unwrap_or(MonitorVariant::Debug)),
        Subcommands::Run { hw } => run(hw.unwrap_or_default()),
        Subcommands::Check { hw } => checks(hw.unwrap_or_default()),
        Subcommands::Doc { open, hw } => docs(open, hw.unwrap_or_default()),
        Subcommands::ExtraCheck { hw } => extra_checks(hw.unwrap_or_default()),
        Subcommands::Example {
            package,
            name,
            watch,
        } => example(package, name, watch),
    }
}
