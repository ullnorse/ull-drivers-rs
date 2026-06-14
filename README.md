# ull-drivers-rs

Personal collection of first-party `no_std` embedded drivers under the `ull-*`
naming scheme.

The point of this repository is simple: build and maintain my own embedded
drivers instead of depending on third-party driver implementations. Each driver
crate is meant to be small, understandable, tested, and based on
`embedded-hal` traits rather than vendor-specific APIs.

## Goals

- first-party drivers I fully understand and maintain
- `no_std` by default
- `embedded-hal` 1.0 compatibility
- one crate per device family
- practical docs and tests from the start
- no shared crate until multiple drivers actually need one

## Layout

```text
ull-drivers-rs/
├── drivers/
│   ├── ull-sht3x/
│   └── ull-ssd1306/
├── Cargo.toml
└── LICENSE
```

## Crates

- `drivers/ull-sht3x`: `embedded-hal` 1.0 I2C driver for Sensirion SHT3x-DIS humidity and temperature sensors.
- `drivers/ull-ssd1306`: `embedded-hal` 1.0 SSD1306 display driver with optional `embedded-graphics-core` support.

## Workspace Notes

- This workspace currently keeps shared code inside each driver until multiple crates expose a real common layer.
- Workspace-wide settings currently centralize `edition`, `license`, and repository metadata.
- Workspace-wide Rust linting forbids `unsafe` code.

## Commands

```bash
cargo check --workspace
cargo test --all-targets --all-features
```
