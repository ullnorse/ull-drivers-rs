# Contributing

## Scope

This workspace contains small, first-party embedded driver crates built around
`embedded-hal` traits. Changes should keep the crates:

- `no_std` by default
- strongly typed where device constraints are known
- explicit about bus traffic, timing, and failure behavior
- small enough to understand without hidden layers

## Local Checks

Run these before opening a pull request:

```bash
cargo fmt --check --all
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --workspace --doc --all-features
cargo check --workspace --all-features --target riscv32imac-unknown-none-elf
cargo hack check -p ull-sht3x --feature-powerset --locked --no-dev-deps
cargo hack check -p ull-ssd1306 --feature-powerset --locked --no-dev-deps
```

Install `cargo-hack` if you do not already have it:

```bash
cargo install cargo-hack
```

## Change Expectations

- Keep changes as small as possible while still solving the problem.
- Add or update tests when behavior changes.
- Update README or crate docs when public APIs, features, or support policies change.
- Preserve sync and async API parity unless there is a documented reason to diverge.
- Avoid introducing a shared internal crate unless multiple drivers genuinely need it.

## Commit Style

The existing history uses short conventional prefixes such as:

- `fix:`
- `test:`
- `docs:`
- `chore:`

Match that style and keep each commit focused on one logical change.

## Pull Requests

- Explain the behavior change, not just the code change.
- Call out any datasheet assumptions or hardware-specific constraints.
- Mention which local checks you ran.
- If hardware validation was manual, describe the board or setup used.
