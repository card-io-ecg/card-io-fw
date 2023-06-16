use anyhow::Result as AnyResult;
use clap::{Parser, Subcommand, ValueEnum};

use duct::{cmd, Expression};

#[derive(Debug, Subcommand)]
pub enum Subcommands {
    Build {
        hw: Option<HardwareVersion>,
    },
    Test,
    Run {
        hw: Option<HardwareVersion>,
    },
    Check {
        hw: Option<HardwareVersion>,
    },
    Doc {
        hw: Option<HardwareVersion>,
        #[clap(long)]
        open: bool,
    },
    ExtraCheck {
        hw: Option<HardwareVersion>,
    },
    Example {
        package: String,
        name: String,
    },
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum HardwareVersion {
    #[default]
    V1,
    V2,
}

impl HardwareVersion {
    fn feature(&self) -> &str {
        match self {
            HardwareVersion::V1 => "hw_v1",
            HardwareVersion::V2 => "hw_v2",
        }
    }
}

#[derive(Debug, Parser)]
#[clap(about, version, propagate_version = true)]
pub struct Cli {
    #[clap(subcommand)]
    pub subcommand: Subcommands,
}

fn cargo(args: &[&str]) -> Expression {
    let mut args_vec = vec!["run", "esp", "cargo"];
    args_vec.extend(args);
    cmd("rustup", args_vec)
}

fn build(hw: HardwareVersion) -> AnyResult<()> {
    cargo(&[
        "build",
        "--target=xtensa-esp32s3-none-elf",
        "-Zbuild-std=core,alloc",
        "--release",
        &format!("--features={}", hw.feature()),
    ])
    .run()?;

    Ok(())
}

fn run(hw: HardwareVersion) -> AnyResult<()> {
    cargo(&[
        "espflash",
        "flash",
        "-M",
        "--release",
        "--target=xtensa-esp32s3-none-elf",
        "-Zbuild-std=core,alloc",
        &format!("--features={}", hw.feature()),
    ])
    .run()?;

    Ok(())
}

fn checks(hw: HardwareVersion) -> AnyResult<()> {
    cargo(&[
        "check",
        "--target=xtensa-esp32s3-none-elf",
        "-Zbuild-std=core,alloc",
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
        &format!("--features={}", hw.feature()),
    ])
    .run()?;

    Ok(())
}

fn test() -> AnyResult<()> {
    let packages = ["signal-processing"];

    let mut args = vec!["test"];

    for p in packages {
        args.push("-p");
        args.push(p);
    }

    cargo(&args).run()?;

    Ok(())
}

fn example(package: String, name: String) -> AnyResult<()> {
    cargo(&["run", "--example", &name, "-p", &package]).run()?;

    Ok(())
}

fn main() -> AnyResult<()> {
    let cli = Cli::parse();

    match cli.subcommand {
        Subcommands::Build { hw } => build(hw.unwrap_or_default()),
        Subcommands::Test => test(),
        Subcommands::Run { hw } => run(hw.unwrap_or_default()),
        Subcommands::Check { hw } => checks(hw.unwrap_or_default()),
        Subcommands::Doc { open, hw } => docs(open, hw.unwrap_or_default()),
        Subcommands::ExtraCheck { hw } => extra_checks(hw.unwrap_or_default()),
        Subcommands::Example { package, name } => example(package, name),
    }
}
