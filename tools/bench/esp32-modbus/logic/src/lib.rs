#![no_std]
//! Hardware-independent logic of the ESP32 bench firmware (docs/23 §4).
//!
//! The firmware crate only moves bytes and pins; everything that can be
//! decided without the chip lives here so that it is tested on the host:
//!
//! - [`map`]: the bench register table and the Modbus PDU server for it;
//! - [`rtu`]: Modbus RTU framing (CRC-16 and the inter-frame silence);
//! - [`fallback`]: the device fallback that turns the LED off after 2 s
//!   without a request;
//! - [`Device`]: the three put together, driven by a microsecond clock that
//!   the caller supplies.
//!
//! The crate does no I/O, reads no clock and never allocates.

pub mod fallback;
pub mod map;
pub mod rtu;

use fallback::Fallback;
use map::{Inputs, Registers};
use rtu::MAX_ADU;

/// Version of the bench firmware, served in holding register 0.
pub const FW_VERSION: u16 = 1;

/// Modbus unit identifier the firmware answers to.
pub const UNIT_ID: u8 = 1;

/// Baud rate of the Modbus RTU line on `UART0`.
pub const BAUD: u32 = 115_200;

/// Banner written once on the console at boot, before `UART0` serves Modbus.
///
/// This build is RTU only: Wi-Fi is off and the RSSI register reads 0.
pub const BANNER: &str = "bench-esp32 v1 · wifi off · modbus-rtu uart0 115200 8N1\r\n";

/// Bench device state: register table, fallback timer and RTU addressing.
#[derive(Debug, Clone)]
pub struct Device {
    registers: Registers,
    fallback: Fallback,
}

impl Device {
    /// Creates the device at boot time `now_us`, with the LED off.
    pub const fn new(now_us: u64) -> Self {
        Self {
            registers: Registers::new(),
            fallback: Fallback::new(now_us),
        }
    }

    /// Returns the commanded LED state (coil 0).
    pub const fn led(&self) -> bool {
        self.registers.led()
    }

    /// Returns whether the device fallback currently holds the LED off.
    pub const fn in_fallback(&self) -> bool {
        self.fallback.is_active()
    }

    /// Updates the input values sampled by the firmware.
    pub fn set_inputs(&mut self, inputs: Inputs) {
        self.registers.set_inputs(inputs);
    }

    /// Advances the fallback timer to `now_us`.
    ///
    /// Returns `true` exactly once per silence, when the fallback forces the
    /// LED off.
    pub fn tick(&mut self, now_us: u64) -> bool {
        if self.fallback.tick(now_us) {
            self.registers.set_led(false);
            true
        } else {
            false
        }
    }

    /// Handles one complete RTU frame received at `now_us`.
    ///
    /// Frames with a bad CRC, too short to hold an address, a function code
    /// and a CRC, or addressed to another unit are dropped silently and do
    /// not feed the fallback timer. Every other frame is a valid request: it
    /// feeds the timer, is executed, and is answered in `out` unless it was a
    /// broadcast. Returns the length of the response to send, if any.
    pub fn handle_frame(
        &mut self,
        frame: &[u8],
        now_us: u64,
        out: &mut [u8; MAX_ADU],
    ) -> Option<usize> {
        let (unit, pdu) = rtu::open(frame)?;
        let broadcast = unit == 0;
        if !broadcast && unit != UNIT_ID {
            return None;
        }
        self.fallback.on_request(now_us);
        let (body, _) = out.split_at_mut(MAX_ADU - rtu::CRC_LEN);
        let (head, response) = body.split_at_mut(1);
        let len = self.registers.serve(pdu, response);
        if broadcast {
            return None;
        }
        head[0] = UNIT_ID;
        Some(rtu::seal(out, 1 + len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtu::crc16;

    fn request(bytes: &[u8]) -> ([u8; MAX_ADU], usize) {
        let mut frame = [0u8; MAX_ADU];
        frame[..bytes.len()].copy_from_slice(bytes);
        let len = rtu::seal(&mut frame, bytes.len());
        (frame, len)
    }

    fn exchange(device: &mut Device, bytes: &[u8], now_us: u64) -> Option<([u8; MAX_ADU], usize)> {
        let (frame, len) = request(bytes);
        let mut out = [0u8; MAX_ADU];
        let n = device.handle_frame(&frame[..len], now_us, &mut out)?;
        Some((out, n))
    }

    #[test]
    fn write_then_read_led_coil() {
        let mut device = Device::new(0);
        let (out, n) = exchange(&mut device, &[1, 5, 0, 0, 0xFF, 0], 10).unwrap();
        assert_eq!(&out[..6], &[1, 5, 0, 0, 0xFF, 0]);
        assert_eq!(n, 8);
        assert!(device.led());
        let (out, n) = exchange(&mut device, &[1, 1, 0, 0, 0, 1], 20).unwrap();
        assert_eq!(&out[..n - 2], &[1, 1, 1, 1]);
        assert_eq!(crc16(&out[..n - 2]).to_le_bytes(), [out[n - 2], out[n - 1]]);
    }

    #[test]
    fn known_response_bytes_for_fw_version() {
        let mut device = Device::new(0);
        let (out, n) = exchange(&mut device, &[1, 3, 0, 0, 0, 1], 0).unwrap();
        assert_eq!(&out[..n], &[0x01, 0x03, 0x02, 0x00, 0x01, 0x79, 0x84]);
    }

    #[test]
    fn other_units_and_bad_crc_are_ignored_and_do_not_feed_fallback() {
        let mut device = Device::new(0);
        exchange(&mut device, &[1, 5, 0, 0, 0xFF, 0], 0).unwrap();
        assert!(exchange(&mut device, &[2, 1, 0, 0, 0, 1], 1_500_000).is_none());
        let (mut frame, len) = request(&[1, 1, 0, 0, 0, 1]);
        frame[len - 1] ^= 0xFF;
        let mut out = [0u8; MAX_ADU];
        assert!(
            device
                .handle_frame(&frame[..len], 1_600_000, &mut out)
                .is_none()
        );
        assert!(device.tick(2_000_000));
        assert!(!device.led());
    }

    #[test]
    fn broadcast_write_is_applied_without_response() {
        let mut device = Device::new(0);
        assert!(exchange(&mut device, &[0, 5, 0, 0, 0xFF, 0], 0).is_none());
        assert!(device.led());
    }

    #[test]
    fn led_goes_off_two_seconds_after_the_last_request() {
        let mut device = Device::new(0);
        exchange(&mut device, &[1, 5, 0, 0, 0xFF, 0], 100_000).unwrap();
        for t in (100_000..2_100_000).step_by(100_000) {
            assert!(!device.tick(t));
            assert!(device.led());
        }
        assert!(device.tick(2_100_000));
        assert!(!device.led());
        assert!(device.in_fallback());
        assert!(!device.tick(5_000_000));
        let (out, n) = exchange(&mut device, &[1, 1, 0, 0, 0, 1], 5_000_000).unwrap();
        assert_eq!(&out[..n - 2], &[1, 1, 1, 0]);
        assert!(!device.in_fallback());
    }

    #[test]
    fn polling_keeps_the_led_on() {
        let mut device = Device::new(0);
        exchange(&mut device, &[1, 5, 0, 0, 0xFF, 0], 0).unwrap();
        for t in (100_000..10_000_000).step_by(100_000) {
            exchange(&mut device, &[1, 4, 0, 0, 0, 4], t).unwrap();
            assert!(!device.tick(t + 50_000));
        }
        assert!(device.led());
    }

    #[test]
    fn exception_responses_still_count_as_requests() {
        let mut device = Device::new(0);
        exchange(&mut device, &[1, 5, 0, 0, 0xFF, 0], 0).unwrap();
        let (out, n) = exchange(&mut device, &[1, 6, 0, 0, 0, 7], 1_900_000).unwrap();
        assert_eq!(&out[..n - 2], &[1, 0x86, 1]);
        assert!(!device.tick(3_000_000));
        assert!(device.led());
    }

    #[test]
    fn banner_names_the_transport() {
        assert!(BANNER.starts_with("bench-esp32 v1 "));
        assert!(BANNER.contains("modbus-rtu uart0 115200 8N1"));
        assert!(BANNER.ends_with("\r\n"));
    }
}
