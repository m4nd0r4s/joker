//! The bench register table (docs/23 §4) and the Modbus PDU server for it.
//!
//! | Register | FC | Type | Signal |
//! |---|---|---|---|
//! | coil 0 | 1 (read), 5 (write) | `bool` | board LED |
//! | discrete input 0 | 2 | `bool` | `BOOT` button, already inverted: 1 = pressed |
//! | input register 0 | 4 | `u16` | touch sensor reading |
//! | input register 1 | 4 | `i16` | Wi-Fi RSSI in dBm, 0 without Wi-Fi |
//! | input registers 2–3 | 4 | `u32` | seconds since boot, high word first |
//! | holding register 0 | 3 | `u16` | bench firmware version |
//!
//! Function codes other than 1, 2, 3, 4 and 5 are answered with exception 1
//! (illegal function): the table has no writable register.

use crate::FW_VERSION;

/// Number of coils in the table.
pub const COILS: u16 = 1;
/// Number of discrete inputs in the table.
pub const DISCRETE_INPUTS: u16 = 1;
/// Number of input registers in the table.
pub const INPUT_REGISTERS: u16 = 4;
/// Number of holding registers in the table.
pub const HOLDING_REGISTERS: u16 = 1;

const MAX_READ_BITS: u16 = 2000;
const MAX_READ_REGISTERS: u16 = 125;
const COIL_ON: u16 = 0xFF00;
const COIL_OFF: u16 = 0x0000;

/// Modbus exception codes used by the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Exception {
    /// The function code is not supported.
    IllegalFunction = 1,
    /// The address range is outside the table.
    IllegalDataAddress = 2,
    /// The quantity, the coil value or the PDU length is invalid.
    IllegalDataValue = 3,
}

/// Values sampled from the hardware, refreshed by the firmware loop.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Inputs {
    /// `BOOT` button pressed.
    pub button: bool,
    /// Raw touch counter of T0; it drops when the pin is touched.
    pub touch: u16,
    /// Wi-Fi RSSI in dBm, 0 without Wi-Fi.
    pub rssi: i16,
    /// Seconds since boot.
    pub uptime_s: u32,
}

impl Inputs {
    fn input_register(&self, address: u16) -> u16 {
        match address {
            0 => self.touch,
            1 => u16::from_be_bytes(self.rssi.to_be_bytes()),
            2 => (self.uptime_s >> 16) as u16,
            _ => (self.uptime_s & 0xFFFF) as u16,
        }
    }
}

/// The register table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Registers {
    led: bool,
    inputs: Inputs,
}

impl Registers {
    /// Creates the table at boot: LED off, inputs zero.
    pub const fn new() -> Self {
        Self {
            led: false,
            inputs: Inputs {
                button: false,
                touch: 0,
                rssi: 0,
                uptime_s: 0,
            },
        }
    }

    /// Returns coil 0, the commanded LED state.
    pub const fn led(&self) -> bool {
        self.led
    }

    /// Sets coil 0.
    pub fn set_led(&mut self, on: bool) {
        self.led = on;
    }

    /// Returns the current input values.
    pub const fn inputs(&self) -> Inputs {
        self.inputs
    }

    /// Replaces the input values.
    pub fn set_inputs(&mut self, inputs: Inputs) {
        self.inputs = inputs;
    }

    /// Executes a request PDU and writes the response PDU into `out`.
    ///
    /// Returns the response length. `out` must hold at least 253 bytes, the
    /// largest Modbus PDU; a shorter buffer truncates the response.
    pub fn serve(&mut self, pdu: &[u8], out: &mut [u8]) -> usize {
        let Some(&function) = pdu.first() else {
            return 0;
        };
        match self.execute(function, &pdu[1..], out) {
            Ok(len) => len,
            Err(exception) => write(out, &[function | 0x80, exception as u8]),
        }
    }

    fn execute(&mut self, function: u8, data: &[u8], out: &mut [u8]) -> Result<usize, Exception> {
        match function {
            1 => {
                let (start, count) = read_range(data, MAX_READ_BITS, COILS)?;
                Ok(write_bits(out, function, start, count, |_| self.led))
            }
            2 => {
                let (start, count) = read_range(data, MAX_READ_BITS, DISCRETE_INPUTS)?;
                let button = self.inputs.button;
                Ok(write_bits(out, function, start, count, |_| button))
            }
            3 => {
                let (start, count) = read_range(data, MAX_READ_REGISTERS, HOLDING_REGISTERS)?;
                Ok(write_registers(out, function, start, count, |_| FW_VERSION))
            }
            4 => {
                let (start, count) = read_range(data, MAX_READ_REGISTERS, INPUT_REGISTERS)?;
                let inputs = self.inputs;
                Ok(write_registers(out, function, start, count, |a| {
                    inputs.input_register(a)
                }))
            }
            5 => {
                let [a0, a1, v0, v1] = *data else {
                    return Err(Exception::IllegalDataValue);
                };
                let address = u16::from_be_bytes([a0, a1]);
                let on = match u16::from_be_bytes([v0, v1]) {
                    COIL_ON => true,
                    COIL_OFF => false,
                    _ => return Err(Exception::IllegalDataValue),
                };
                if address >= COILS {
                    return Err(Exception::IllegalDataAddress);
                }
                self.led = on;
                Ok(write(out, &[function, a0, a1, v0, v1]))
            }
            _ => Err(Exception::IllegalFunction),
        }
    }
}

fn read_range(data: &[u8], max_count: u16, size: u16) -> Result<(u16, u16), Exception> {
    let [s0, s1, c0, c1] = *data else {
        return Err(Exception::IllegalDataValue);
    };
    let start = u16::from_be_bytes([s0, s1]);
    let count = u16::from_be_bytes([c0, c1]);
    if count == 0 || count > max_count {
        return Err(Exception::IllegalDataValue);
    }
    if u32::from(start) + u32::from(count) > u32::from(size) {
        return Err(Exception::IllegalDataAddress);
    }
    Ok((start, count))
}

fn write(out: &mut [u8], bytes: &[u8]) -> usize {
    let len = bytes.len().min(out.len());
    out[..len].copy_from_slice(&bytes[..len]);
    len
}

fn write_bits(
    out: &mut [u8],
    function: u8,
    start: u16,
    count: u16,
    bit: impl Fn(u16) -> bool,
) -> usize {
    let bytes = usize::from(count.div_ceil(8));
    let len = (2 + bytes).min(out.len());
    if len < 2 {
        return 0;
    }
    out[0] = function;
    out[1] = bytes as u8;
    out[2..len].fill(0);
    for i in 0..count {
        let index = 2 + usize::from(i / 8);
        if index < len && bit(start + i) {
            out[index] |= 1 << (i % 8);
        }
    }
    len
}

fn write_registers(
    out: &mut [u8],
    function: u8,
    start: u16,
    count: u16,
    register: impl Fn(u16) -> u16,
) -> usize {
    let bytes = 2 * usize::from(count);
    let len = (2 + bytes).min(out.len());
    if len < 2 {
        return 0;
    }
    out[0] = function;
    out[1] = bytes as u8;
    for i in 0..count {
        let index = 2 + 2 * usize::from(i);
        if index + 1 < len {
            out[index..index + 2].copy_from_slice(&register(start + i).to_be_bytes());
        }
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    fn serve(registers: &mut Registers, pdu: &[u8]) -> ([u8; 253], usize) {
        let mut out = [0u8; 253];
        let len = registers.serve(pdu, &mut out);
        (out, len)
    }

    fn sample() -> Registers {
        let mut registers = Registers::new();
        registers.set_inputs(Inputs {
            button: true,
            touch: 612,
            rssi: -67,
            uptime_s: 0x0001_0002,
        });
        registers
    }

    #[test]
    fn reads_all_input_registers() {
        let mut registers = sample();
        let (out, len) = serve(&mut registers, &[4, 0, 0, 0, 4]);
        assert_eq!(
            &out[..len],
            &[4, 8, 0x02, 0x64, 0xFF, 0xBD, 0x00, 0x01, 0x00, 0x02]
        );
    }

    #[test]
    fn reads_uptime_alone() {
        let mut registers = sample();
        let (out, len) = serve(&mut registers, &[4, 0, 2, 0, 2]);
        assert_eq!(&out[..len], &[4, 4, 0x00, 0x01, 0x00, 0x02]);
    }

    #[test]
    fn reads_button_and_fw_version() {
        let mut registers = sample();
        let (out, len) = serve(&mut registers, &[2, 0, 0, 0, 1]);
        assert_eq!(&out[..len], &[2, 1, 1]);
        let (out, len) = serve(&mut registers, &[3, 0, 0, 0, 1]);
        assert_eq!(&out[..len], &[3, 2, 0, FW_VERSION as u8]);
    }

    #[test]
    fn writes_and_reads_the_led_coil() {
        let mut registers = Registers::new();
        let (out, len) = serve(&mut registers, &[5, 0, 0, 0xFF, 0]);
        assert_eq!(&out[..len], &[5, 0, 0, 0xFF, 0]);
        assert!(registers.led());
        let (out, len) = serve(&mut registers, &[1, 0, 0, 0, 1]);
        assert_eq!(&out[..len], &[1, 1, 1]);
        serve(&mut registers, &[5, 0, 0, 0, 0]);
        assert!(!registers.led());
    }

    #[test]
    fn out_of_range_addresses_are_exception_2() {
        let mut registers = sample();
        for pdu in [
            &[1u8, 0, 0, 0, 2][..],
            &[1, 0, 1, 0, 1],
            &[2, 0, 0, 0, 2],
            &[3, 0, 1, 0, 1],
            &[4, 0, 3, 0, 2],
            &[4, 0xFF, 0xFF, 0, 1],
            &[5, 0, 1, 0xFF, 0],
        ] {
            let (out, len) = serve(&mut registers, pdu);
            assert_eq!(&out[..len], &[pdu[0] | 0x80, 2], "{pdu:?}");
        }
    }

    #[test]
    fn bad_quantities_values_and_lengths_are_exception_3() {
        let mut registers = sample();
        for pdu in [
            &[1u8, 0, 0, 0, 0][..],
            &[1, 0, 0, 0x07, 0xD1],
            &[3, 0, 0, 0, 126],
            &[4, 0, 0, 0, 0],
            &[5, 0, 0, 0x12, 0x34],
            &[4, 0, 0, 0],
            &[5, 0, 0, 0xFF, 0, 0],
        ] {
            let (out, len) = serve(&mut registers, pdu);
            assert_eq!(&out[..len], &[pdu[0] | 0x80, 3], "{pdu:?}");
        }
        assert!(!registers.led());
    }

    #[test]
    fn unsupported_functions_are_exception_1() {
        let mut registers = sample();
        for function in [6u8, 15, 16, 23, 43] {
            let (out, len) = serve(&mut registers, &[function, 0, 0, 0, 1]);
            assert_eq!(&out[..len], &[function | 0x80, 1]);
        }
    }

    #[test]
    fn empty_pdu_has_no_response() {
        let mut registers = sample();
        assert_eq!(serve(&mut registers, &[]).1, 0);
    }

    #[test]
    fn negative_rssi_round_trips_as_i16() {
        let inputs = Inputs {
            rssi: -1,
            ..Inputs::default()
        };
        assert_eq!(inputs.input_register(1), 0xFFFF);
    }
}
