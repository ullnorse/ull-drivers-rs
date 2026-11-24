#![allow(dead_code)]

extern crate std;

use crate::*;
#[cfg(feature = "async")]
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use embedded_hal::{
    delay::DelayNs,
    i2c::{ErrorKind, ErrorType, I2c, NoAcknowledgeSource, Operation, SevenBitAddress},
};
#[cfg(feature = "async")]
use std::pin::pin;
use std::vec::Vec;

// ---- bus mock (same approach as ull-sht3x) ----

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct MockI2cError(ErrorKind);

impl MockI2cError {
    const fn other() -> Self {
        Self(ErrorKind::Other)
    }

    const fn no_ack() -> Self {
        Self(ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address))
    }
}

impl embedded_hal::i2c::Error for MockI2cError {
    fn kind(&self) -> ErrorKind {
        self.0
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum Event {
    Write { address: u8, bytes: Vec<u8> },
    Read { address: u8, len: usize },
}

#[derive(Debug)]
enum ReadResponse {
    Data(Vec<u8>),
    Error(MockI2cError),
}

#[derive(Debug, Default)]
struct MockI2c {
    events: Vec<Event>,
    reads: Vec<ReadResponse>,
    fail_writes: bool,
}

#[allow(dead_code)]
impl MockI2c {
    fn new(reads: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self {
            events: Vec::new(),
            reads: reads.into_iter().map(ReadResponse::Data).collect(),
            fail_writes: false,
        }
    }

    fn with_read_responses(reads: impl IntoIterator<Item = ReadResponse>) -> Self {
        Self {
            events: Vec::new(),
            reads: reads.into_iter().collect(),
            fail_writes: false,
        }
    }

    fn failing_writes() -> Self {
        Self {
            fail_writes: true,
            ..Self::default()
        }
    }

    /// Only the write payloads, in order.
    fn writes(&self) -> Vec<Vec<u8>> {
        self.events
            .iter()
            .filter_map(|event| match event {
                Event::Write { bytes, .. } => Some(bytes.clone()),
                Event::Read { .. } => None,
            })
            .collect()
    }

    fn run(&mut self, address: u8, op: MockOp<'_>) -> core::result::Result<(), MockI2cError> {
        match op {
            MockOp::Read(buffer) => {
                self.events.push(Event::Read {
                    address,
                    len: buffer.len(),
                });
                match self.reads.remove(0) {
                    ReadResponse::Data(read) => buffer.copy_from_slice(&read),
                    ReadResponse::Error(error) => return Err(error),
                }
            }
            MockOp::Write(bytes) => {
                self.events.push(Event::Write {
                    address,
                    bytes: bytes.to_vec(),
                });
                if self.fail_writes {
                    return Err(MockI2cError::other());
                }
            }
        }
        Ok(())
    }
}

enum MockOp<'a> {
    Read(&'a mut [u8]),
    Write(&'a [u8]),
}

impl ErrorType for MockI2c {
    type Error = MockI2cError;
}

impl I2c<SevenBitAddress> for MockI2c {
    fn transaction(
        &mut self,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> core::result::Result<(), Self::Error> {
        for operation in operations {
            match operation {
                Operation::Read(buffer) => self.run(address, MockOp::Read(buffer))?,
                Operation::Write(bytes) => self.run(address, MockOp::Write(bytes))?,
            }
        }
        Ok(())
    }
}

#[cfg(feature = "async")]
impl embedded_hal_async::i2c::I2c<embedded_hal_async::i2c::SevenBitAddress> for MockI2c {
    async fn transaction(
        &mut self,
        address: u8,
        operations: &mut [embedded_hal_async::i2c::Operation<'_>],
    ) -> core::result::Result<(), Self::Error> {
        for operation in operations {
            match operation {
                embedded_hal_async::i2c::Operation::Read(buffer) => {
                    self.run(address, MockOp::Read(buffer))?
                }
                embedded_hal_async::i2c::Operation::Write(bytes) => {
                    self.run(address, MockOp::Write(bytes))?
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
struct MockDelay {
    delayed_ms: Vec<u32>,
}

impl DelayNs for MockDelay {
    fn delay_ns(&mut self, _ns: u32) {}

    fn delay_ms(&mut self, ms: u32) {
        self.delayed_ms.push(ms);
    }
}

#[cfg(feature = "async")]
#[derive(Debug, Default)]
struct MockAsyncDelay {
    delayed_ms: Vec<u32>,
}

#[cfg(feature = "async")]
impl embedded_hal_async::delay::DelayNs for MockAsyncDelay {
    async fn delay_ns(&mut self, _ns: u32) {}

    async fn delay_ms(&mut self, ms: u32) {
        self.delayed_ms.push(ms);
    }
}

#[cfg(feature = "async")]
fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    let mut future = pin!(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => core::hint::spin_loop(),
        }
    }
}
// ---- end mock ----

fn word(value: u16) -> Vec<u8> {
    Vec::from(value.to_be_bytes())
}

fn write_event(bytes: &[u8]) -> Event {
    Event::Write {
        address: 0x36,
        bytes: Vec::from(bytes),
    }
}

fn read_event() -> Event {
    Event::Read {
        address: 0x36,
        len: 2,
    }
}

#[test]
fn address_is_fixed() {
    assert_eq!(Address::default().as_u8(), 0x36);
    assert_eq!(Address::custom(0x36), Some(Address::DEFAULT));
    assert_eq!(Address::custom(0x37), None);
}

#[test]
fn vcell_conversion_matches_78_125_uv_per_lsb() {
    // 3.7 V / 78.125 uV = 47360 = 0xB900 exactly.
    assert_eq!(vcell_microvolts(0xB900), 3_700_000);
    assert_eq!(vcell_microvolts(0), 0);
    // 65535 * 78.125 uV = 5_119_921.875 uV, truncated.
    assert_eq!(vcell_microvolts(0xFFFF), 5_119_921);
}

#[test]
fn soc_conversion_matches_1_percent_per_256() {
    assert_eq!(soc_hundredths(0x3200), 5000); // 50 %
    assert_eq!(soc_hundredths(0x0180), 150); // 1.5 %
    assert_eq!(soc_hundredths(0x6400), 10_000); // 100 %
    assert_eq!(soc_hundredths(0x0001), 0); // 1/256 % = 0.39 -> 0 hundredths
    assert_eq!(soc_hundredths(0x0003), 1); // 3/256 % = 1.17 hundredths
}

#[test]
fn charge_rate_conversion_is_signed() {
    assert_eq!(charge_rate_milli_percent_per_hour(1), 208);
    assert_eq!(charge_rate_milli_percent_per_hour(-1), -208);
    assert_eq!(charge_rate_milli_percent_per_hour(100), 20_800);
    assert_eq!(charge_rate_milli_percent_per_hour(i16::MIN), -6_815_744);
}

#[test]
fn reads_use_one_write_read_transaction() {
    let mut gauge = Max17048::new(MockI2c::new([word(0xB900)]));
    assert_eq!(gauge.vcell_raw().unwrap(), 0xB900);
    assert_eq!(
        gauge.release().events,
        Vec::from([write_event(&[0x02]), read_event()])
    );
}

#[test]
fn reads_converted_values_from_the_right_registers() {
    let mut gauge = Max17048::new(MockI2c::new([
        word(0xB900),
        word(0xB900),
        word(0x3200),
        word(0x3200),
        word(0xFFFF),
        word(0x0012),
    ]));
    assert_eq!(gauge.vcell_microvolts().unwrap(), 3_700_000);
    assert_eq!(gauge.vcell_millivolts().unwrap(), 3_700);
    assert_eq!(gauge.soc_hundredths().unwrap(), 5000);
    assert!((gauge.soc_percent().unwrap() - 50.0).abs() < 1e-4);
    assert_eq!(gauge.charge_rate_milli_percent_per_hour().unwrap(), -208);
    assert_eq!(gauge.version().unwrap(), 0x0012);
    assert_eq!(
        gauge.release().writes(),
        Vec::from([
            Vec::from([0x02]),
            Vec::from([0x02]),
            Vec::from([0x04]),
            Vec::from([0x04]),
            Vec::from([0x16]),
            Vec::from([0x08]),
        ])
    );
}

#[test]
fn float_conversions() {
    let mut gauge = Max17048::new(MockI2c::new([word(0xB900), word(0x0100)]));
    assert!((gauge.vcell_volts().unwrap() - 3.7).abs() < 1e-4);
    assert!((gauge.charge_rate_percent_per_hour().unwrap() - 53.248).abs() < 1e-3);
}

#[test]
fn low_soc_threshold_preserves_other_config_bits() {
    // Reset value 0x971C has ATHD = 0x1C (4 %). 10 % -> ATHD = 22 = 0x16.
    let mut gauge = Max17048::new(MockI2c::new([word(0x971C)]));
    gauge.set_low_soc_threshold(10).unwrap();
    assert_eq!(
        gauge.release().writes(),
        Vec::from([Vec::from([0x0C]), Vec::from([0x0C, 0x97, 0x16])])
    );
}

#[test]
fn low_soc_threshold_limits() {
    for (percent, athd) in [(1u8, 0x1Fu8), (32, 0x00)] {
        let mut gauge = Max17048::new(MockI2c::new([word(0x971C)]));
        gauge.set_low_soc_threshold(percent).unwrap();
        assert_eq!(
            gauge.release().writes()[1],
            Vec::from([0x0C, 0x97, athd]),
            "{percent}"
        );
    }
    for percent in [0u8, 33, 255] {
        let mut gauge = Max17048::new(MockI2c::default());
        assert_eq!(
            gauge.set_low_soc_threshold(percent),
            Err(Error::InvalidThreshold)
        );
        assert!(gauge.release().events.is_empty());
    }
}

#[test]
fn config_decoding() {
    let config = Config(Config::RESET_VALUE);
    assert_eq!(config.rcomp(), 0x97);
    assert_eq!(config.low_soc_threshold_percent(), 4);
    assert!(!config.alert());
    assert!(!config.sleep());
    assert!(!config.soc_change_alert_enabled());
    assert!(Config(0x97FF).alert() && Config(0x97FF).sleep());
}

#[test]
fn soc_change_alert_toggles_alsc_only() {
    let mut gauge = Max17048::new(MockI2c::new([word(0x971C), word(0x975C)]));
    gauge.set_soc_change_alert(true).unwrap();
    gauge.set_soc_change_alert(false).unwrap();
    let writes = gauge.release().writes();
    assert_eq!(writes[1], Vec::from([0x0C, 0x97, 0x5C]));
    assert_eq!(writes[3], Vec::from([0x0C, 0x97, 0x1C]));
}

#[test]
fn voltage_alert_encodes_20mv_units() {
    // 3000 mV -> 150 = 0x96, 4200 mV -> 210 = 0xD2.
    let mut gauge = Max17048::new(MockI2c::default());
    gauge
        .set_voltage_alert(VoltageAlert {
            min_mv: 3000,
            max_mv: 4200,
        })
        .unwrap();
    assert_eq!(
        gauge.release().writes(),
        Vec::from([Vec::from([0x14, 0x96, 0xD2])])
    );
}

#[test]
fn voltage_alert_truncates_and_limits() {
    let mut gauge = Max17048::new(MockI2c::default());
    gauge
        .set_voltage_alert(VoltageAlert {
            min_mv: 3019,
            max_mv: 5100,
        })
        .unwrap();
    assert_eq!(
        gauge.release().writes(),
        Vec::from([Vec::from([0x14, 0x96, 0xFF])])
    );

    let mut gauge = Max17048::new(MockI2c::default());
    assert_eq!(
        gauge.set_voltage_alert(VoltageAlert {
            min_mv: 0,
            max_mv: 5120
        }),
        Err(Error::InvalidThreshold)
    );
    assert!(gauge.release().events.is_empty());
}

#[test]
fn voltage_alert_reads_back_and_disabled_matches_reset_value() {
    let mut gauge = Max17048::new(MockI2c::new([word(0x96D2), word(0x00FF)]));
    assert_eq!(
        gauge.voltage_alert().unwrap(),
        VoltageAlert {
            min_mv: 3000,
            max_mv: 4200
        }
    );
    assert_eq!(gauge.voltage_alert().unwrap(), VoltageAlert::DISABLED);
}

#[test]
fn status_decoding() {
    let status = Status(0x5500);
    assert!(status.voltage_reset_alert_enabled());
    assert!(status.reset_indicator());
    assert!(status.soc_low());
    assert!(status.voltage_low());
    assert!(!status.voltage_high());
    assert!(!status.voltage_reset());
    assert!(!status.soc_change());
    assert!(status.any_alert());

    assert!(Status(0x0100).reset_indicator());
    assert!(!Status(0x0100).any_alert());
    assert!(Status(0x0200).voltage_high());
    assert!(Status(0x0800).voltage_reset());
    assert!(Status(0x2000).soc_change());
}

#[test]
fn clear_alert_clears_flags_then_config_alrt_and_returns_pending_status() {
    // STATUS: EnVR + HD + VL + RI (+ junk in the don't-care low byte).
    // CONFIG: reset value with ALRT (bit 5) set.
    let mut gauge = Max17048::new(MockI2c::new([word(0x55AA), word(0x973C)]));
    let pending = gauge.clear_alert().unwrap();
    assert!(pending.soc_low() && pending.voltage_low());
    assert_eq!(
        gauge.release().events,
        Vec::from([
            write_event(&[0x1A]),
            read_event(),
            // HD and VL cleared, EnVR and RI preserved, low byte zeroed.
            write_event(&[0x1A, 0x41, 0x00]),
            write_event(&[0x0C]),
            read_event(),
            // Only ALRT cleared; RCOMP and ATHD untouched.
            write_event(&[0x0C, 0x97, 0x1C]),
        ])
    );
}

#[test]
fn clear_alert_clears_every_alert_flag() {
    let mut gauge = Max17048::new(MockI2c::new([word(0x7F00), word(0x973C)]));
    gauge.clear_alert().unwrap();
    assert_eq!(gauge.release().writes()[1], Vec::from([0x1A, 0x41, 0x00]));
}

#[test]
fn clear_alert_still_clears_config_alrt_when_status_is_empty() {
    let mut gauge = Max17048::new(MockI2c::new([word(0x0000), word(0x973C)]));
    let pending = gauge.clear_alert().unwrap();
    assert!(!pending.any_alert());
    assert_eq!(gauge.release().writes()[3], Vec::from([0x0C, 0x97, 0x1C]));
}

#[test]
fn clear_alert_preserves_sleep_and_alsc() {
    let mut gauge = Max17048::new(MockI2c::new([word(0x0200), word(0x97FC)]));
    gauge.clear_alert().unwrap();
    assert_eq!(gauge.release().writes()[3], Vec::from([0x0C, 0x97, 0xDC]));
}

#[test]
fn clear_alert_stops_on_bus_error() {
    let mut gauge = Max17048::new(MockI2c::with_read_responses([ReadResponse::Error(
        MockI2cError::no_ack(),
    )]));
    assert_eq!(gauge.clear_alert(), Err(Error::Bus(MockI2cError::no_ack())));
    assert_eq!(gauge.release().writes(), Vec::from([Vec::from([0x1A])]));
}

#[test]
fn clear_reset_indicator_only_clears_ri() {
    let mut gauge = Max17048::new(MockI2c::new([word(0x4F00)]));
    gauge.clear_reset_indicator().unwrap();
    assert_eq!(gauge.release().writes()[1], Vec::from([0x1A, 0x4E, 0x00]));
}

#[test]
fn alert_asserted_reads_config_alrt() {
    let mut gauge = Max17048::new(MockI2c::new([word(0x973C), word(0x971C)]));
    assert!(gauge.alert_asserted().unwrap());
    assert!(!gauge.alert_asserted().unwrap());
}

#[test]
fn hibernate_control_writes_hibrt() {
    let mut gauge = Max17048::new(MockI2c::default());
    gauge.enter_hibernate().unwrap();
    gauge.exit_hibernate().unwrap();
    gauge.set_auto_hibernate().unwrap();
    assert_eq!(
        gauge.release().writes(),
        Vec::from([
            Vec::from([0x0A, 0xFF, 0xFF]),
            Vec::from([0x0A, 0x00, 0x00]),
            Vec::from([0x0A, 0x80, 0x30]),
        ])
    );
}

#[test]
fn hibernate_status_reads_mode_hibstat() {
    let mut gauge = Max17048::new(MockI2c::new([word(0x1000), word(0x0000)]));
    assert!(gauge.is_hibernating().unwrap());
    assert!(!gauge.is_hibernating().unwrap());
    assert_eq!(gauge.release().writes()[0], Vec::from([0x06]));
}

#[test]
fn sleep_sets_ensleep_then_config_sleep_and_wake_clears_it() {
    let mut gauge = Max17048::new(MockI2c::new([word(0x971C), word(0x979C)]));
    gauge.enter_sleep().unwrap();
    gauge.wake().unwrap();
    assert_eq!(
        gauge.release().writes(),
        Vec::from([
            Vec::from([0x06, 0x20, 0x00]),
            Vec::from([0x0C]),
            Vec::from([0x0C, 0x97, 0x9C]),
            Vec::from([0x0C]),
            Vec::from([0x0C, 0x97, 0x1C]),
        ])
    );
}

#[test]
fn raw_register_access() {
    let mut gauge = Max17048::new(MockI2c::new([word(0x1234)]));
    assert_eq!(gauge.read_register(0x40).unwrap(), 0x1234);
    gauge.write_register(0xFE, 0x5400).unwrap();
    assert_eq!(
        gauge.release().writes(),
        Vec::from([Vec::from([0x40]), Vec::from([0xFE, 0x54, 0x00])])
    );
}

#[test]
fn bus_errors_propagate() {
    let mut gauge = Max17048::new(MockI2c::failing_writes());
    assert_eq!(gauge.vcell_raw(), Err(Error::Bus(MockI2cError::other())));
    assert_eq!(
        gauge.write_register(0x0C, 0),
        Err(Error::Bus(MockI2cError::other()))
    );
}

#[test]
fn formats_errors_for_logs() {
    assert_eq!(
        std::format!("{}", Error::<MockI2cError>::InvalidThreshold),
        "threshold out of range"
    );
}

fn assert_send<T: Send>() {}

#[test]
fn driver_is_send_when_i2c_is_send() {
    assert_send::<Max17048<MockI2c>>();
}

#[cfg(feature = "async")]
#[test]
fn blocking_and_async_apis_produce_identical_bus_traffic() {
    fn reads() -> Vec<Vec<u8>> {
        [
            0xB900, 0x3200, 0xFFFF, 0x0012, 0x971C, 0x971C, 0x96D2, 0x55AA, 0x973C, 0x4F00, 0x1000,
            0x971C, 0x979C,
        ]
        .into_iter()
        .map(word)
        .collect()
    }

    let mut gauge = Max17048::new(MockI2c::new(reads()));
    let results = (
        gauge.vcell_microvolts().unwrap(),
        gauge.soc_hundredths().unwrap(),
        gauge.charge_rate_milli_percent_per_hour().unwrap(),
        gauge.version().unwrap(),
    );
    gauge.set_low_soc_threshold(10).unwrap();
    gauge.set_soc_change_alert(true).unwrap();
    gauge
        .set_voltage_alert(VoltageAlert {
            min_mv: 3000,
            max_mv: 4200,
        })
        .unwrap();
    let voltage_alert = gauge.voltage_alert().unwrap();
    let pending = gauge.clear_alert().unwrap();
    gauge.clear_reset_indicator().unwrap();
    let hibernating = gauge.is_hibernating().unwrap();
    gauge.enter_hibernate().unwrap();
    gauge.exit_hibernate().unwrap();
    gauge.set_auto_hibernate().unwrap();
    gauge.enter_sleep().unwrap();
    gauge.wake().unwrap();
    gauge.write_register(0xFE, 0x5400).unwrap();
    let blocking_i2c = gauge.release();

    let mut gauge = Max17048::new(MockI2c::new(reads()));
    let async_results = (
        block_on(gauge.vcell_microvolts_async()).unwrap(),
        block_on(gauge.soc_hundredths_async()).unwrap(),
        block_on(gauge.charge_rate_milli_percent_per_hour_async()).unwrap(),
        block_on(gauge.version_async()).unwrap(),
    );
    block_on(gauge.set_low_soc_threshold_async(10)).unwrap();
    block_on(gauge.set_soc_change_alert_async(true)).unwrap();
    block_on(gauge.set_voltage_alert_async(VoltageAlert {
        min_mv: 3000,
        max_mv: 4200,
    }))
    .unwrap();
    assert_eq!(
        block_on(gauge.voltage_alert_async()).unwrap(),
        voltage_alert
    );
    assert_eq!(block_on(gauge.clear_alert_async()).unwrap(), pending);
    block_on(gauge.clear_reset_indicator_async()).unwrap();
    assert_eq!(block_on(gauge.is_hibernating_async()).unwrap(), hibernating);
    block_on(gauge.enter_hibernate_async()).unwrap();
    block_on(gauge.exit_hibernate_async()).unwrap();
    block_on(gauge.set_auto_hibernate_async()).unwrap();
    block_on(gauge.enter_sleep_async()).unwrap();
    block_on(gauge.wake_async()).unwrap();
    block_on(gauge.write_register_async(0xFE, 0x5400)).unwrap();
    let async_i2c = gauge.release();

    assert_eq!(results, async_results);
    assert_eq!(blocking_i2c.events, async_i2c.events);
    assert!(async_i2c.reads.is_empty());
}
