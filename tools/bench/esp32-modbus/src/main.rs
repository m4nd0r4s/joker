#![no_std]
#![no_main]
//! RTU-only bench firmware for the original ESP32 (docs/23 §4).
//!
//! Serves the bench register table as Modbus RTU unit 1 on `UART0` (the USB
//! bridge of the board) at 115 200 baud, 8N1, and turns the LED off when no
//! valid request arrives for 2 s. All decisions live in
//! [`bench_esp32_logic`]; this binary only samples the pins, moves bytes and
//! drives the LED.
//!
//! Pins: LED on GPIO2, `BOOT` button on GPIO0 (active low), touch pad T0 on
//! GPIO4, `UART0` on GPIO1 (TX) and GPIO3 (RX).

use bench_esp32_logic::map::Inputs;
use bench_esp32_logic::rtu::{Framer, MAX_ADU};
use bench_esp32_logic::{BANNER, BAUD, Device};
use esp_hal::Blocking;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use esp_hal::main;
use esp_hal::time::Instant;
use esp_hal::touch::{Touch, TouchPad};
use esp_hal::uart::{Config, Uart};

esp_bootloader_esp_idf::esp_app_desc!();

/// Resets the chip on panic.
///
/// After a reset every GPIO is an input and GPIO2 is pulled down, so the LED
/// is off: a panic never leaves the output in its last commanded state.
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    esp_hal::system::software_reset()
}

/// Microseconds since boot.
fn now_us() -> u64 {
    Instant::now().duration_since_epoch().as_micros()
}

/// Whole seconds since boot, saturating at `u32::MAX`.
fn uptime_s(now_us: u64) -> u32 {
    u32::try_from(now_us / 1_000_000).unwrap_or(u32::MAX)
}

/// Writes all of `bytes` and waits until they have left the transmitter.
///
/// A transmit error drops the rest of the response; the master sees a
/// timeout and retries.
fn send(uart: &mut Uart<'_, Blocking>, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        match uart.write(bytes) {
            Ok(written) => bytes = &bytes[written..],
            Err(_) => return,
        }
    }
    let _ = uart.flush();
}

#[main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    let mut led = Output::new(peripherals.GPIO2, Level::Low, OutputConfig::default());
    let button = Input::new(
        peripherals.GPIO0,
        InputConfig::default().with_pull(Pull::Up),
    );
    let touch = Touch::continuous_mode(peripherals.TOUCH, None);
    let mut touch_pad = TouchPad::new(peripherals.GPIO4, &touch);

    let Ok(uart) = Uart::new(peripherals.UART0, Config::default().with_baudrate(BAUD)) else {
        esp_hal::system::software_reset()
    };
    let mut uart = uart.with_rx(peripherals.GPIO3).with_tx(peripherals.GPIO1);

    send(&mut uart, BANNER.as_bytes());

    let mut device = Device::new(now_us());
    let mut framer = Framer::new();
    let mut chunk = [0u8; 128];
    let mut response = [0u8; MAX_ADU];
    let mut touch_value = 0u16;

    loop {
        let now = now_us();
        match uart.read_buffered(&mut chunk) {
            Ok(read) => framer.push(&chunk[..read], now),
            Err(_) => framer.mark_error(now),
        }

        if let Some(value) = touch_pad.try_read() {
            touch_value = value;
        }
        device.set_inputs(Inputs {
            button: button.is_low(),
            touch: touch_value,
            rssi: 0,
            uptime_s: uptime_s(now),
        });

        if let Some(frame) = framer.poll(now)
            && let Some(len) = device.handle_frame(frame, now, &mut response)
        {
            send(&mut uart, &response[..len]);
        }

        device.tick(now_us());
        led.set_level(Level::from(device.led()));
    }
}
