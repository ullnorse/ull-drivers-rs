# Driver Style

This is the house style for `ull-*` embedded-hal drivers. Drivers use these
patterns unless a device has a concrete reason to differ.

The goal is consistency, not framework code. Each driver stays easy to read,
easy to use on hardware, and small enough to understand without a shared
abstraction layer getting in the way.

## Crate Shape

Each device family gets one crate under `drivers/ull-device/`.

```text
drivers/ull-device/
├── Cargo.toml
├── README.md
├── examples/
├── src/
│   ├── lib.rs
│   ├── types.rs
│   ├── tests.rs
│   └── driver/
│       ├── mod.rs
│       ├── blocking.rs
│       └── async.rs
└── tests/
    └── public_api.rs
```

The standard layout is:

- `lib.rs`: crate docs, module declarations, and public re-exports only.
- `types.rs`: public value types, config types, address types, error types, and pure conversion helpers.
- `driver/mod.rs`: main driver type, typestate markers, command constants, and private shared helpers.
- `driver/blocking.rs`: blocking `embedded-hal` method implementations.
- `driver/async.rs`: async method implementations behind the `async` feature.
- `src/tests.rs`: crate-internal behavior tests.
- `tests/public_api.rs`: external public API smoke tests.
- `examples/`: small hardware-oriented examples.

Shared crates, macro frameworks, and common traits are avoided until multiple
drivers prove they need exactly the same code.

## Public API Shape

The main driver type owns the bus or device interface.

```rust
pub struct Device<BUS, MODE = DefaultMode> {
    bus: BUS,
    address: u8,
    _mode: core::marker::PhantomData<MODE>,
}
```

Field names match the transport when that improves readability, such as `i2c`
for an I2C-only driver. Generic names like `interface` are reserved for drivers
that really support multiple transport kinds.

Drivers expose these basics when applicable:

```rust
impl<BUS> Device<BUS> {
    pub fn new(bus: BUS) -> Self;
    pub fn with_address(bus: BUS, address: Address) -> Self;
}

impl<BUS, MODE> Device<BUS, MODE> {
    pub const fn address(&self) -> u8;
    pub fn release(self) -> BUS;
}
```

`new` creates the default practical configuration. `with_address` selects a
typed address. `release`, not `destroy`, is the standard way to return owned
peripherals.

Operations usually take `&mut self`. Methods consume and return `self` only for
real state transitions, especially typestate transitions.

## Address Types

I2C devices use a typed address wrapper.

```rust
pub struct Address(u8);

impl Address {
    pub const DEFAULT: Self = Self(0x00);
    pub const ALTERNATE: Self = Self(0x00);

    pub const fn custom(address: u8) -> Option<Self>;
    pub const fn as_u8(self) -> u8;
}
```

`custom` only accepts supported addresses. `Default` is implemented as
`Address::DEFAULT`.

## Errors

`Bus` is the transport error variant for all drivers, even I2C-only drivers.
This keeps I2C, SPI, and future transports consistent.

```rust
pub type Result<T, E> = core::result::Result<T, Error<E>>;

pub enum Error<BusError> {
    Bus(BusError),
    DeviceSpecific,
}
```

Separate error types are used only when another fallible resource is involved. For
example, reset-aware display initialization may return both a bus error and a
reset-pin error.

Public errors implement `core::fmt::Display` and `core::error::Error`.
`defmt::Format` derives are gated with `#[cfg_attr(feature = "defmt", derive(defmt::Format))]`.

## Blocking And Async

Blocking APIs are the primary APIs. Async APIs mirror them with an `_async`
suffix and live behind the `async` feature.

```rust
pub fn init(&mut self) -> Result<(), BUS::Error>;
pub async fn init_async(&mut self) -> Result<(), BUS::Error>;
```

Async names stay boring and predictable. A separate async API shape is only used
when the hardware behavior requires it.

## Typestate

Typestate is used when it prevents real invalid device behavior:

- acquisition modes that accept different commands
- raw vs buffered display modes
- scroll-active states that must block RAM writes

Typestate is not used just to make the API look clever. If a normal method with
a runtime argument is clearer and safe enough, the normal method is used.

Typestate marker names are public when users see them in return types.
Zero-sized markers derive at least `Debug`, `Copy`, `Clone`, `Eq`, `PartialEq`,
and `Default`.

## Raw Escape Hatches

Drivers preserve practical lower-level control.

Good examples:

- raw command writes for displays
- raw sensor words before unit conversion
- status-register access
- fixed-point conversion paths for targets without floating-point support

Drivers do not model the whole datasheet up front. They start with the main path
plus the escape hatches that are useful for real hardware work.

## Tests And Examples

Every driver has a `tests/public_api.rs` smoke test that uses the crate as an
external user would. It covers:

- construction with default or alternate address
- one normal blocking flow
- `release()` returning the bus
- one important typestate or raw escape hatch when present
- one async smoke test when the `async` feature exists

Examples are short and hardware-shaped. They show one complete task rather than
exhaustive API demonstrations.

## Documentation

Each crate README states:

- what device family the crate targets
- transport and embedded-hal version
- the simple usage path
- optional features
- hardware or timing caveats that affect real use

Workspace docs describe policy and style. Crate docs describe the driver a user
has in front of them.
