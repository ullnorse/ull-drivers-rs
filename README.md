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

## Support Policy

- Minimum supported Rust version: `1.85`
- Default build mode: `no_std`
- CI checks the pinned Rust `1.85.0` host toolchain plus `riscv32imac-unknown-none-elf`
- CI checks every crate feature combination with `cargo-hack`
- These crates are still `0.x`; breaking API changes are allowed before `1.0`

Support means the repository actively checks these combinations in CI:

- workspace formatting, clippy, tests, doctests, and package verification on the pinned `1.85.0` host toolchain
- embedded builds for `riscv32imac-unknown-none-elf`
- feature-matrix builds for `ull-sht3x` and `ull-ssd1306`

## Maintenance Commands

```bash
cargo fmt --check --all
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo test --locked --workspace --doc --all-features
cargo check --locked --workspace --all-features --target riscv32imac-unknown-none-elf
cargo package --workspace --allow-dirty --locked
cargo hack check -p ull-sht3x --feature-powerset --locked --no-dev-deps
cargo hack check -p ull-ssd1306 --feature-powerset --locked --no-dev-deps
```

Install `cargo-hack` first if you want to run the feature-matrix checks locally:

```bash
cargo install cargo-hack
```

## Repository Docs

- `CHANGELOG.md`: user-visible changes across workspace releases
- `CONTRIBUTING.md`: local workflow and change expectations
- `SECURITY.md`: how to report security issues
