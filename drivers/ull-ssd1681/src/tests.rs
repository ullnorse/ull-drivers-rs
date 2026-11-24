extern crate std;

use super::*;
#[cfg(feature = "async")]
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};
use embedded_hal::delay::DelayNs;
use embedded_hal::digital::{InputPin, OutputPin};
use embedded_hal::spi::{ErrorKind, ErrorType, Operation, SpiDevice};
use std::cell::RefCell;
use std::collections::VecDeque;
#[cfg(feature = "async")]
use std::pin::pin;
use std::rc::Rc;
use std::vec;
use std::vec::Vec;

#[derive(Debug, Clone, Eq, PartialEq)]
enum Event {
    Dc(bool),
    Rst(bool),
    Spi(Vec<u8>),
    Busy(bool),
    DelayMs(u32),
}

type Log = Rc<RefCell<Vec<Event>>>;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
struct MockError;

impl embedded_hal::spi::Error for MockError {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

impl embedded_hal::digital::Error for MockError {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}

struct MockSpi {
    log: Log,
    fail: bool,
}

impl ErrorType for MockSpi {
    type Error = MockError;
}

impl SpiDevice for MockSpi {
    fn transaction(
        &mut self,
        operations: &mut [Operation<'_, u8>],
    ) -> core::result::Result<(), MockError> {
        if self.fail {
            return Err(MockError);
        }
        for operation in operations {
            if let Operation::Write(bytes) = operation {
                self.log.borrow_mut().push(Event::Spi(bytes.to_vec()));
            }
        }
        Ok(())
    }
}

#[cfg(feature = "async")]
impl embedded_hal_async::spi::SpiDevice for MockSpi {
    async fn transaction(
        &mut self,
        operations: &mut [embedded_hal_async::spi::Operation<'_, u8>],
    ) -> core::result::Result<(), MockError> {
        if self.fail {
            return Err(MockError);
        }
        for operation in operations {
            if let embedded_hal_async::spi::Operation::Write(bytes) = operation {
                self.log.borrow_mut().push(Event::Spi(bytes.to_vec()));
            }
        }
        Ok(())
    }
}

struct MockOutput {
    log: Log,
    is_dc: bool,
    fail: bool,
}

impl embedded_hal::digital::ErrorType for MockOutput {
    type Error = MockError;
}

impl MockOutput {
    fn record(&mut self, level: bool) -> core::result::Result<(), MockError> {
        if self.fail {
            return Err(MockError);
        }
        let event = if self.is_dc {
            Event::Dc(level)
        } else {
            Event::Rst(level)
        };
        self.log.borrow_mut().push(event);
        Ok(())
    }
}

impl OutputPin for MockOutput {
    fn set_low(&mut self) -> core::result::Result<(), MockError> {
        self.record(false)
    }

    fn set_high(&mut self) -> core::result::Result<(), MockError> {
        self.record(true)
    }
}

/// BUSY input: returns the scripted levels, then `default`.
struct MockBusy {
    log: Log,
    script: VecDeque<bool>,
    default: bool,
}

impl embedded_hal::digital::ErrorType for MockBusy {
    type Error = MockError;
}

impl InputPin for MockBusy {
    fn is_high(&mut self) -> core::result::Result<bool, MockError> {
        let level = self.script.pop_front().unwrap_or(self.default);
        self.log.borrow_mut().push(Event::Busy(level));
        Ok(level)
    }

    fn is_low(&mut self) -> core::result::Result<bool, MockError> {
        self.is_high().map(|level| !level)
    }
}

struct MockDelay {
    log: Log,
}

impl DelayNs for MockDelay {
    fn delay_ns(&mut self, ns: u32) {
        self.log.borrow_mut().push(Event::DelayMs(ns / 1_000_000));
    }

    fn delay_ms(&mut self, ms: u32) {
        self.log.borrow_mut().push(Event::DelayMs(ms));
    }
}

#[cfg(feature = "async")]
impl embedded_hal_async::delay::DelayNs for MockDelay {
    async fn delay_ns(&mut self, ns: u32) {
        self.log.borrow_mut().push(Event::DelayMs(ns / 1_000_000));
    }

    async fn delay_ms(&mut self, ms: u32) {
        self.log.borrow_mut().push(Event::DelayMs(ms));
    }
}

type TestDriver = Ssd1681<MockSpi, MockOutput, MockOutput, MockBusy, MockDelay>;

#[derive(Default)]
struct Failures {
    spi: bool,
    dc: bool,
    rst: bool,
}

fn build(busy: &[bool], default: bool, failures: Failures, config: Config) -> (TestDriver, Log) {
    let log: Log = Rc::default();
    let driver = Ssd1681::with_config(
        MockSpi {
            log: log.clone(),
            fail: failures.spi,
        },
        MockOutput {
            log: log.clone(),
            is_dc: true,
            fail: failures.dc,
        },
        MockOutput {
            log: log.clone(),
            is_dc: false,
            fail: failures.rst,
        },
        MockBusy {
            log: log.clone(),
            script: busy.iter().copied().collect(),
            default,
        },
        MockDelay { log: log.clone() },
        config,
    );
    (driver, log)
}

fn driver_with(busy: &[bool], default: bool) -> (TestDriver, Log) {
    build(busy, default, Failures::default(), Config::default())
}

fn driver() -> (TestDriver, Log) {
    driver_with(&[], false)
}

fn take(log: &Log) -> Vec<Event> {
    core::mem::take(&mut *log.borrow_mut())
}

/// Events of one command: D/C low, command byte, then D/C high and data.
fn cmd(command: u8, data: &[u8]) -> Vec<Event> {
    let mut events = vec![Event::Dc(false), Event::Spi(vec![command])];
    if !data.is_empty() {
        events.push(Event::Dc(true));
        events.push(Event::Spi(data.to_vec()));
    }
    events
}

fn idle() -> Vec<Event> {
    vec![Event::Busy(false)]
}

fn join(parts: &[Vec<Event>]) -> Vec<Event> {
    parts.iter().flatten().cloned().collect()
}

fn reset_events() -> Vec<Event> {
    vec![
        Event::Rst(false),
        Event::DelayMs(10),
        Event::Rst(true),
        Event::DelayMs(10),
    ]
}

fn reset_and_config() -> Vec<Event> {
    join(&[
        reset_events(),
        cmd(0x12, &[]),
        vec![Event::DelayMs(10)],
        idle(),
        cmd(0x01, &[0xC7, 0x00, 0x00]),
        cmd(0x11, &[0x03]),
        cmd(0x44, &[0x00, 0x18]),
        cmd(0x45, &[0x00, 0x00, 0xC7, 0x00]),
        cmd(0x3C, &[0x05]),
        cmd(0x18, &[0x80]),
    ])
}

fn load_lut() -> Vec<Event> {
    join(&[cmd(0x22, &[0xB1]), cmd(0x20, &[]), idle()])
}

fn auto_fill() -> Vec<Event> {
    join(&[cmd(0x47, &[0xF7]), idle(), cmd(0x46, &[0xF7]), idle()])
}

fn ram_write(command: u8, frame: &[u8]) -> Vec<Event> {
    join(&[
        cmd(0x4E, &[0x00]),
        cmd(0x4F, &[0x00, 0x00]),
        cmd(command, frame),
    ])
}

fn full_refresh_events() -> Vec<Event> {
    join(&[
        cmd(0x3C, &[0x05]),
        cmd(0x22, &[0xF7]),
        cmd(0x20, &[]),
        idle(),
    ])
}

fn partial_refresh_events() -> Vec<Event> {
    join(&[
        cmd(0x3C, &[0x80]),
        cmd(0x22, &[0xFF]),
        cmd(0x20, &[]),
        idle(),
    ])
}

fn test_frame() -> [u8; FRAME_LEN] {
    let mut frame = [0xFF; FRAME_LEN];
    frame[0] = 0x7F;
    frame[FRAME_LEN - 1] = 0xFE;
    frame
}

#[test]
fn constants_match_the_panel() {
    assert_eq!(WIDTH, 200);
    assert_eq!(HEIGHT, 200);
    assert_eq!(BYTES_PER_ROW, 25);
    assert_eq!(FRAME_LEN, 5000);
}

#[test]
fn default_config_timeouts_exceed_typical_times() {
    let config = Config::default();
    assert!(config.full_refresh_timeout_ms > FULL_REFRESH_TYPICAL_MS);
    assert!(config.partial_refresh_timeout_ms > PARTIAL_REFRESH_TYPICAL_MS);
    assert!(config.command_timeout_ms >= 1_000);
}

#[test]
fn deep_sleep_mode_codes() {
    assert_eq!(DeepSleepMode::Mode1.as_u8(), 0x01);
    assert_eq!(DeepSleepMode::Mode2.as_u8(), 0x03);
}

#[test]
fn init_sends_reset_config_fill_and_lut_load() {
    let (mut epd, log) = driver();
    epd.init().unwrap();
    assert_eq!(
        take(&log),
        join(&[reset_and_config(), auto_fill(), load_lut()])
    );
}

#[test]
fn wake_skips_the_ram_fill() {
    let (mut epd, log) = driver();
    epd.wake().unwrap();
    assert_eq!(take(&log), join(&[reset_and_config(), load_lut()]));
}

#[test]
fn clear_fills_both_rams_white_then_refreshes() {
    let (mut epd, log) = driver();
    epd.clear().unwrap();
    assert_eq!(take(&log), join(&[auto_fill(), full_refresh_events()]));
}

#[test]
fn write_frame_targets_bw_ram_with_cursor_reset() {
    let (mut epd, log) = driver();
    let frame = test_frame();
    epd.write_frame(&frame).unwrap();
    assert_eq!(take(&log), ram_write(0x24, &frame));
}

#[test]
fn write_previous_frame_targets_red_ram() {
    let (mut epd, log) = driver();
    let frame = test_frame();
    epd.write_previous_frame(&frame).unwrap();
    assert_eq!(take(&log), ram_write(0x26, &frame));
}

#[test]
fn full_refresh_sequence() {
    let (mut epd, log) = driver();
    epd.full_refresh().unwrap();
    assert_eq!(take(&log), full_refresh_events());
}

#[test]
fn partial_refresh_sequence() {
    let (mut epd, log) = driver();
    epd.partial_refresh().unwrap();
    assert_eq!(take(&log), partial_refresh_events());
}

#[test]
fn display_full_writes_both_rams_before_refresh() {
    let (mut epd, log) = driver();
    let frame = test_frame();
    epd.display_full(&frame).unwrap();
    assert_eq!(
        take(&log),
        join(&[
            ram_write(0x24, &frame),
            ram_write(0x26, &frame),
            full_refresh_events()
        ])
    );
}

#[test]
fn display_partial_refreshes_then_updates_previous_ram() {
    let (mut epd, log) = driver();
    let frame = test_frame();
    epd.display_partial(&frame).unwrap();
    assert_eq!(
        take(&log),
        join(&[
            ram_write(0x24, &frame),
            partial_refresh_events(),
            ram_write(0x26, &frame)
        ])
    );
}

#[test]
fn deep_sleep_sends_mode_and_does_not_wait() {
    let (mut epd, log) = driver_with(&[], true);
    epd.deep_sleep(DeepSleepMode::Mode1).unwrap();
    assert_eq!(take(&log), cmd(0x10, &[0x01]));
    epd.deep_sleep(DeepSleepMode::Mode2).unwrap();
    assert_eq!(take(&log), cmd(0x10, &[0x03]));
}

#[test]
fn busy_wait_polls_until_low() {
    let (mut epd, log) = driver_with(&[true, true, false], false);
    epd.wait_until_idle(10).unwrap();
    assert_eq!(
        take(&log),
        vec![
            Event::Busy(true),
            Event::DelayMs(1),
            Event::Busy(true),
            Event::DelayMs(1),
            Event::Busy(false),
        ]
    );
}

#[test]
fn busy_timeout_returns_error_after_the_limit() {
    let (mut epd, log) = driver_with(&[], true);
    assert_eq!(epd.wait_until_idle(3), Err(Error::BusyTimeout));
    let events = take(&log);
    let polls = events
        .iter()
        .filter(|e| matches!(e, Event::Busy(_)))
        .count();
    let waits: u32 = events
        .iter()
        .map(|e| if let Event::DelayMs(ms) = e { *ms } else { 0 })
        .sum();
    assert_eq!(polls, 4);
    assert_eq!(waits, 3);
}

#[test]
fn stuck_busy_fails_a_refresh_with_the_refresh_timeout() {
    let (mut epd, log) = driver_with(&[], true);
    assert_eq!(epd.full_refresh(), Err(Error::BusyTimeout));
    let waited: u32 = take(&log)
        .iter()
        .map(|e| if let Event::DelayMs(ms) = e { *ms } else { 0 })
        .sum();
    assert_eq!(waited, Config::default().full_refresh_timeout_ms);
}

#[test]
fn bus_error_is_reported() {
    let failures = Failures {
        spi: true,
        ..Failures::default()
    };
    let (mut epd, _log) = build(&[], false, failures, Config::default());
    assert_eq!(epd.write_command(0x12, &[]), Err(Error::Bus(MockError)));
}

#[test]
fn pin_errors_name_the_pin() {
    let failures = Failures {
        dc: true,
        ..Failures::default()
    };
    let (mut epd, _log) = build(&[], false, failures, Config::default());
    assert_eq!(
        epd.write_command(0x12, &[]),
        Err(Error::Pin(Pin::DataCommand))
    );

    let failures = Failures {
        rst: true,
        ..Failures::default()
    };
    let (mut epd, _log) = build(&[], false, failures, Config::default());
    assert_eq!(epd.hardware_reset(), Err(Error::Pin(Pin::Reset)));
}

#[test]
fn custom_config_sets_timeouts() {
    let config = Config {
        command_timeout_ms: 5,
        full_refresh_timeout_ms: 7,
        partial_refresh_timeout_ms: 9,
    };
    let (mut epd, log) = build(&[], true, Failures::default(), config);
    assert_eq!(epd.config(), config);
    assert_eq!(epd.partial_refresh(), Err(Error::BusyTimeout));
    let waited: u32 = take(&log)
        .iter()
        .map(|e| if let Event::DelayMs(ms) = e { *ms } else { 0 })
        .sum();
    assert_eq!(waited, 9);
}

#[test]
fn error_display_is_readable() {
    use std::string::ToString;
    assert_eq!(
        Error::<MockError>::BusyTimeout.to_string(),
        "e-paper BUSY timeout"
    );
}

#[cfg(feature = "async")]
fn block_on<F: Future>(future: F) -> F::Output {
    let mut context = Context::from_waker(Waker::noop());
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

#[cfg(feature = "async")]
#[test]
fn async_traffic_matches_blocking() {
    let frame = test_frame();

    let (mut blocking, blocking_log) = driver();
    blocking.init().unwrap();
    blocking.display_full(&frame).unwrap();
    blocking.display_partial(&frame).unwrap();
    blocking.clear().unwrap();
    blocking.deep_sleep(DeepSleepMode::Mode1).unwrap();
    blocking.wake().unwrap();

    let (mut asynchronous, async_log) = driver();
    block_on(async {
        asynchronous.init_async().await.unwrap();
        asynchronous.display_full_async(&frame).await.unwrap();
        asynchronous.display_partial_async(&frame).await.unwrap();
        asynchronous.clear_async().await.unwrap();
        asynchronous
            .deep_sleep_async(DeepSleepMode::Mode1)
            .await
            .unwrap();
        asynchronous.wake_async().await.unwrap();
    });

    assert_eq!(take(&async_log), take(&blocking_log));
}

#[cfg(feature = "async")]
#[test]
fn async_busy_timeout() {
    let (mut epd, _log) = driver_with(&[], true);
    assert_eq!(
        block_on(epd.partial_refresh_async()),
        Err(Error::BusyTimeout)
    );
}
