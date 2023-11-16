Card/IO firmware
================

This repository contains the firmware source code for the Card/IO open source ECG device, built with
an ESP32-S3 MCU.

This firmware is in its early stages of development.

Setup
-----

Tools you need to build the firmware:

- Espressif's Xtensa-enabled rust compiler - [espup](https://github.com/esp-rs/espup)
  > Make sure to run `. ~/export-esp.sh` before trying to work with the firmware
- `cargo install cargo-espflash`
- `cargo install cargo-watch`

### Enable External / USB JTAG selector solder bridge

- `pip install esptool`
- `python -m espefuse burn_efuse --port COM4 STRAP_JTAG_SEL 1`

Commands
--------

- `cargo xtask -h`: Prints information about available commands. Most of the commands have short
  aliasses, listed below.
- `cargo xbuild <hw>`: Build the firmware for a `<hw>` version board.
- `cargo xrun <hw>`: Build and run the firmware on a `<hw>` version board.
- `cargo monitor`: Connect to the Card/IO device and display serial output.
  `<hw>` can be omitted, or one of: `v1`, `v2`, `v4`, `v6_s3`, `v6_c6`. Defaults to `v4`.
- `cargo xcheck <hw>`: runs `cargo check`
- `cargo xclippy <hw>`: runs `cargo clippy`
- `cargo xdoc <hw> [--open]`: runs `cargo doc` and optionally opens the generated documentation.
- `cargo xtest`: runs `cargo test`.
- `cargo example <package> <example> [--watch]`: runs an example.
  Use `--watch` to enable automatic reload when a file changes.
- To run the config site on your PC, run `cargo example config-site simple --watch`
  and open `127.0.0.1:8080` in a browser.
