# ull-drivers-rs

Personal collection of `no_std` embedded drivers under the `ull-*` naming
scheme.

The point is simple: keep drivers I understand, can use on hardware, and can
change without fighting layers I do not need. The main path should be practical
and clear, with lower-level escape hatches when experimenting with device
behavior makes sense.

## Goals

- first-party drivers I fully understand and maintain
- `no_std` by default
- `embedded-hal` 1.0 compatibility
- one crate per device family
- practical examples and tests
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

## Notes

- This workspace currently keeps shared code inside each driver until multiple crates expose a real common layer.
- Workspace-wide settings currently centralize `edition`, `license`, and repository metadata.
- Workspace-wide Rust linting forbids `unsafe` code.

## Local Checks

The repo pins Rust `1.85.0`. Useful checks while changing code:

```bash
cargo fmt --check --all
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
```
