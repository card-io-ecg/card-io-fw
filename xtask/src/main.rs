use anyhow::Result as AnyResult;
use clap::{Parser, Subcommand};

use duct::{cmd, Expression};

#[derive(Debug, Subcommand)]
pub enum Subcommands {
    Build,
    Test,
    Run,
    Check,
    Doc {
        #[clap(long)]
        open: bool,
    },
    ExtraCheck,
    Example {
        package: String,
        name: String,
    },
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

fn build() -> AnyResult<()> {
    cargo(&[
        "build",
        "--target=xtensa-esp32s3-none-elf",
        "-Zbuild-std=core,alloc",
        "--release",
    ])
    .run()?;

    Ok(())
}

fn run() -> AnyResult<()> {
    cargo(&[
        "espflash",
        "flash",
        "-M",
        "--release",
        "--target=xtensa-esp32s3-none-elf",
        "-Zbuild-std=core,alloc",
    ])
    .run()?;

    Ok(())
}

fn checks() -> AnyResult<()> {
    cargo(&[
        "check",
        "--target=xtensa-esp32s3-none-elf",
        "-Zbuild-std=core,alloc",
    ])
    .run()?;

    Ok(())
}

fn docs(open: bool) -> AnyResult<()> {
    let mut args = vec![
        "doc",
        "--target=xtensa-esp32s3-none-elf",
        "-Zbuild-std=core,alloc",
    ];

    if open {
        args.push("--open");
    }

    cargo(&args).run()?;

    Ok(())
}

fn extra_checks() -> AnyResult<()> {
    cargo(&["fmt", "--check"]).run()?;
    cargo(&[
        "clippy",
        "--target=xtensa-esp32s3-none-elf",
        "-Zbuild-std=core,alloc",
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
        Subcommands::Build => build(),
        Subcommands::Test => test(),
        Subcommands::Run => run(),
        Subcommands::Check => checks(),
        Subcommands::Doc { open } => docs(open),
        Subcommands::ExtraCheck => extra_checks(),
        Subcommands::Example { package, name } => example(package, name),
    }
}
