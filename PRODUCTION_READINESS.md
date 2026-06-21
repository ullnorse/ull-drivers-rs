# Production Readiness Audit

Date: 2026-06-21

Scope: workspace-level review of `ull-drivers-rs`, including `ull-sht3x` and `ull-ssd1306`, with emphasis on what still needs to improve before this repo feels comfortably production-ready as a public embedded-driver workspace.

## Current Baseline

This repo is already in better shape than most early embedded crates.

- The workspace is small and focused.
- The driver APIs are strongly typed and clearly shaped by device behavior rather than by generic boilerplate.
- The crate docs are useful and technically specific.
- `unsafe` is forbidden workspace-wide in `Cargo.toml`.
- Unit tests are substantial and check real command sequences and failure cases.
- Doctests pass.
- A `no_std` embedded target check passes.

Checks run during this review:

- `cargo fmt --check --all`: passed
- `cargo test --all-targets --all-features`: passed
- `cargo test --workspace --doc --all-features`: passed
- `cargo check --workspace --all-features --target riscv32imac-unknown-none-elf`: passed
- `cargo clippy --workspace --all-targets --all-features`: passes with 1 warning
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: fails

The gap is not basic correctness. The gap is production discipline around maintainability, automation, and support boundaries.

## What Production Ready Should Mean Here

For this repo, "production ready" should mean:

- The crates build and test reproducibly in CI.
- Supported targets, features, and toolchain expectations are explicit.
- Internal code layout is maintainable enough for future fixes without fear.
- Public APIs are exercised from the outside, not only through inline unit tests.
- Releases can be cut predictably with versioning and changelog discipline.
- Users know what is supported, what is out of scope, and how to report issues.

## Priority 0: Fix Before Calling It Production Ready

### 1. Restore a zero-warning lint baseline

Current state:

- Strict clippy fails on `drivers/ull-ssd1306/src/lib.rs:1008-1012` because of `clippy::type_complexity`.
- The recent history includes `1bc3c3b chore: make ull-ssd1306 clippy clean`, so the repo has already treated lint cleanliness as important.

Why this matters:

- A public library repo should have an unambiguous "green" quality gate.
- Once warnings are tolerated, future cleanup becomes optional and drift accelerates.

What to improve:

- Make `cargo clippy --workspace --all-targets --all-features -- -D warnings` pass again.
- Prefer fixing the signature by introducing a clearer internal type alias instead of adding a blanket `#[allow]`.
- Add this command to CI so the baseline cannot silently regress.

Done when:

- Strict clippy passes locally and in CI.

### 2. Add CI for formatting, linting, docs, tests, and embedded target builds

Current state:

- There is no visible `.github/workflows/` directory.
- Quality checks are documented in `README.md:42-47`, but they are not enforced automatically.

Why this matters:

- Production-ready repos are not judged by whether checks can be run manually once.
- They are judged by whether every future change is automatically held to the same bar.

What to improve:

- Add a CI workflow that runs on push and pull request.
- At minimum, run:
  - `cargo fmt --check --all`
  - `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  - `cargo test --all-targets --all-features`
  - `cargo test --workspace --doc --all-features`
  - `cargo check --workspace --all-features --target riscv32imac-unknown-none-elf`
- Add a feature-matrix job. `cargo hack` is a good fit because both crates expose optional features.
- Consider a second toolchain job for the minimum supported Rust version once MSRV is declared.

Done when:

- CI exists, is green, and covers the commands above.

### 3. Declare the support matrix explicitly

Current state:

- Both crate manifests are clean but minimal: `drivers/ull-sht3x/Cargo.toml:1-24` and `drivers/ull-ssd1306/Cargo.toml:1-26`.
- There is no `rust-version` field.
- There is no repo-level statement of supported targets, feature combinations, or semver expectations.

Why this matters:

- Library users need to know what you test and what you merely believe should work.
- Embedded crates especially need clarity around `no_std`, target coverage, async support, and HAL assumptions.

What to improve:

- Add `rust-version` to each crate.
- Document the supported target policy in the root README or a dedicated support policy section.
- Document supported feature combinations.
- Decide whether docs.rs should build all features, and if so, add `package.metadata.docs.rs` to each crate.
- State semver intent clearly: experimental `0.x` API, or "breaking changes can happen until 1.0".

Done when:

- Users can answer "what Rust version, which targets, and which feature combinations are supported?" from the repo alone.

### 4. Break up the monolithic `lib.rs` files

Current state:

- `drivers/ull-sht3x/src/lib.rs` is about 1843 lines.
- `drivers/ull-ssd1306/src/lib.rs` is about 2885 lines.
- Large portions of both files are test code:
  - `drivers/ull-sht3x/src/lib.rs:1227-1843`
  - `drivers/ull-ssd1306/src/lib.rs:2020-2885`

Why this matters:

- The current code is understandable, but production readiness is partly about how easy the next five fixes will be.
- Large single-file libraries slow review, increase merge friction, and make design boundaries less obvious.

What to improve:

- Move test code out of `lib.rs` into `src/tests.rs` or `tests/*.rs`.
- Split production code into a few stable modules.
- Do not over-engineer the split. Use modules to clarify concepts, not to maximize file count.

Suggested shape for `ull-sht3x`:

- `address.rs`
- `types.rs`
- `commands.rs`
- `driver.rs`
- `tests.rs`

Suggested shape for `ull-ssd1306`:

- `types.rs`
- `config.rs`
- `scroll.rs`
- `buffered.rs`
- `raw.rs`
- `init.rs`
- `tests.rs`

Done when:

- Each crate's main `lib.rs` is a compact entry point rather than the whole codebase.

### 5. Add outward-facing API coverage and examples

Current state:

- There are no `examples/` directories.
- There are no integration `tests/` directories.
- The inline unit tests are strong, but they test internals from inside the crate.

Why this matters:

- Production-ready libraries should demonstrate the public API from the outside.
- Examples are both user documentation and compile-checked usage contracts.

What to improve:

- Add at least one example per crate:
  - `ull-sht3x`: single measurement and fixed-point measurement
  - `ull-ssd1306`: init, draw, flush, and partial flush
- Add integration tests that import the crates as users would.
- Keep unit tests for exact command sequencing and private helper behavior.
- Use examples as smoke tests for docs clarity, not just as copied README snippets.

Done when:

- A new user can copy a real example instead of mentally stitching together the README.

## Priority 1: Important Improvements After the Blockers

### 6. Reduce sync/async duplication without hiding the code behind macros

Current state:

- `ull-sht3x` duplicates large sync and async surfaces in `drivers/ull-sht3x/src/lib.rs:423-1143`.
- `ull-ssd1306` duplicates large sync and async surfaces in `drivers/ull-ssd1306/src/lib.rs:838-1849`.

Why this matters:

- Duplication is manageable right now, but it is the main long-term drift risk in both crates.
- A future bug fix can easily land in one API surface and not the other.

What to improve:

- Keep separate public sync and async APIs if that remains the clearest public shape.
- Extract shared private helpers for:
  - command selection
  - validation
  - address-window calculations
  - scroll parameter encoding
  - pure data conversions
- Avoid large macro-generated APIs unless they clearly reduce complexity.

Done when:

- Shared logic lives in one place, while the public API remains readable.

### 7. Add feature-matrix testing rather than testing only "all features"

Current state:

- The documented and manually run checks cover `--all-features` well.
- That does not guarantee that every smaller feature combination works.

Why this matters:

- Optional features are part of the public contract.
- `all-features` can hide bugs in individual combinations.

What to improve:

- Add `cargo hack` to CI.
- Test at least:
  - `--no-default-features`
  - each feature individually
  - `--all-features`
  - a few representative combinations such as `async + defmt`, `async + serde`, and `graphics + async` for `ull-ssd1306`

Done when:

- Feature support is enforced rather than assumed.

### 8. Tighten package metadata for crates.io and docs.rs

Current state:

- Both crate manifests have solid descriptions, keywords, and categories.
- They still omit some metadata commonly expected in mature public crates.

What to improve:

- Add `rust-version`.
- Consider adding explicit `readme = "README.md"` for clarity.
- Consider adding `documentation` and `homepage` if those URLs are stable and useful.
- Add `package.metadata.docs.rs` if you want docs.rs to build with selected features.
- Decide whether the current package contents should stay minimal or whether examples should be included in the published package.

Done when:

- The published crates clearly advertise how they should be built and documented.

### 9. Add release discipline: changelog, tagging, and release automation

Current state:

- There is no visible `CHANGELOG.md`.
- There is no release workflow file or documented release process.

Why this matters:

- A production-ready library should make updates legible to downstream users.
- Embedded consumers often pin versions carefully and need crisp change history.

What to improve:

- Add `CHANGELOG.md`.
- Document a release process in the root README or a maintenance guide.
- Choose whether releases are manual, tag-driven, or automated with a tool like `release-plz`.
- Consider `cargo-semver-checks` as a release gate once the APIs settle.

Done when:

- Cutting a release is repeatable and leaves behind usable change notes.

### 10. Add contribution and security policy files

Current state:

- No `CONTRIBUTING.md`
- No `SECURITY.md`
- No `CODE_OF_CONDUCT.md`

Why this matters:

- Even a solo-maintained public repo looks more production-grade when expectations are explicit.
- Security reporting especially should not rely on guesswork.

What to improve:

- Add `CONTRIBUTING.md` with local commands, style expectations, and PR scope guidance.
- Add `SECURITY.md` with a simple vulnerability reporting path.
- Add `CODE_OF_CONDUCT.md` only if you want a standard public collaboration posture.

Done when:

- External contributors and reporters know how to interact with the repo.

### 11. Add dependency and supply-chain checks

Current state:

- There is no visible `cargo-deny`, `cargo-audit`, Dependabot, or Renovate setup.

Why this matters:

- Production readiness is not only about code style; it is also about ongoing dependency hygiene.

What to improve:

- Add `cargo-deny` or `cargo-audit` to CI.
- Add dependency update automation such as Dependabot.
- If license policy matters, use `cargo-deny` to enforce it.

Done when:

- Dependency risk is checked continuously rather than manually.

## Priority 2: Useful Polish After Core Readiness Is In Place

### 12. Add hardware validation guidance or a manual HIL path

Current state:

- Tests are mock-based.
- That is good and fast, but it does not substitute for real-device confidence.

What to improve:

- Add a short `HARDWARE_TESTING.md` or README section explaining how maintainers validate against real hardware.
- If feasible, add ignored or manually triggered board-level smoke tests.
- Even a documented manual checklist is better than an implicit assumption that hardware was probably tried once.

### 13. Track size and codegen regressions if tiny-footprint use matters

Current state:

- There is no explicit binary size or code size tracking.

What to improve:

- If these crates target very small MCUs, add a lightweight size-check workflow or at least one representative example binary whose size can be tracked over time.

### 14. Add deeper property testing where it buys confidence

Current state:

- The deterministic tests are good.

What to improve:

- Consider `proptest` for conversion invariants and argument-range behavior.
- Consider fuzzing or randomized testing for parsing helpers if the code grows more complex.

This is not a production blocker today, but it can add confidence cheaply later.

### 15. Improve docs.rs polish for optional features

Current state:

- The crates use `README.md` as crate-level docs, which is a good start.

What to improve:

- If feature-gated items become more numerous, consider docs.rs metadata and `doc_cfg` presentation so users can see which APIs belong to which features.

## What Should Stay As-Is

Not every "professional-looking" repo convention would improve this project.

- Keep the typed API design. It is a real strength.
- Keep README-driven crate docs unless that becomes a maintenance burden.
- Keep the "no shared core crate until it is justified" approach from `README.md:38-39`.
- Do not replace readable code with large macro systems just to reduce line count.
- Do not add abstraction layers only to make the repo look more enterprise-like.

The goal is a production-ready small library repo, not a process-heavy framework.

## Recommended File Additions

Repo-level additions:

- `.github/workflows/ci.yml`
- `.github/dependabot.yml`
- `CONTRIBUTING.md`
- `SECURITY.md`
- `CHANGELOG.md`
- `HARDWARE_TESTING.md` or a comparable section in the root README
- `deny.toml` if using `cargo-deny`
- `justfile` or `Makefile` if you want one command surface for maintainers

Crate-level additions:

- `drivers/ull-sht3x/examples/`
- `drivers/ull-ssd1306/examples/`
- `drivers/ull-sht3x/tests/`
- `drivers/ull-ssd1306/tests/`
- optional `src/tests.rs` if tests remain unit-style but should leave `lib.rs`

## Suggested Rollout Order

1. Fix the clippy warning and restore `-D warnings`.
2. Add CI with host, docs, and embedded-target jobs.
3. Add `rust-version`, support-matrix documentation, and feature-matrix testing.
4. Move tests out of `lib.rs` and split source into modules.
5. Add examples and integration tests.
6. Add changelog, release process, contribution policy, and security policy.
7. Add dependency automation and optional supply-chain checks.
8. Add HIL guidance and any size-tracking or property-testing polish.

## Exit Criteria

This repo should feel production ready when all of the following are true:

- `cargo fmt --check --all` is enforced in CI.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` is enforced in CI and green.
- `cargo test --all-targets --all-features` is enforced in CI and green.
- `cargo test --workspace --doc --all-features` is enforced in CI and green.
- At least one `no_std` embedded target build is enforced in CI and green.
- Feature combinations are tested, not only `--all-features`.
- Each crate declares `rust-version`.
- Supported targets and semver expectations are documented.
- The source is modular enough that future changes do not require scrolling through thousands of lines.
- There are real examples and at least minimal public-API integration tests.
- Release and security processes are documented.

At that point, the repo would not just contain good code. It would behave like a maintainable public library that other teams can trust.
