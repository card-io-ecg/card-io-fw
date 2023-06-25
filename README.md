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

Tips
----

To run the config site on your PC, run `cargo watch -x "run -p config-site --example simple"` and open `127.0.0.1:8080`.
