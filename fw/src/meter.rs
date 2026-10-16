//! B23 Modbus RTU master: UART1 + MAX485, DE/RE on GPIO21.
#![cfg_attr(feature = "sim", allow(unused))]

use anyhow::{ensure, Result};
use esp_idf_svc::hal::delay::{TickType, BLOCK};
use esp_idf_svc::hal::gpio::{AnyIOPin, Gpio17, Gpio18, Gpio21, Output, PinDriver};
use esp_idf_svc::hal::uart::{config::Config, UartDriver, UART1};
use esp_idf_svc::hal::units::Hertz;
use rmodbus::{client::ModbusRequest, ModbusProto};

// Must match the meter panel settings (RS485 → Modbus).
const SLAVE: u8 = 1;
const BAUD: u32 = 19_200;
const TIMEOUT_MS: u64 = 200;

pub struct Meter {
    uart: UartDriver<'static>,
    de: PinDriver<'static, Gpio21, Output>,
    #[cfg(feature = "sim")]
    t0: std::time::Instant,
}

impl Meter {
    pub fn new(uart: UART1, tx: Gpio17, rx: Gpio18, de: Gpio21) -> Result<Self> {
        let cfg = Config::default().baudrate(Hertz(BAUD)).parity_even();
        let uart = UartDriver::new(uart, tx, rx, None::<AnyIOPin>, None::<AnyIOPin>, &cfg)?;
        let mut de = PinDriver::output(de)?;
        de.set_low()?;
        Ok(Self {
            uart,
            de,
            #[cfg(feature = "sim")]
            t0: std::time::Instant::now(),
        })
    }

    #[cfg(feature = "sim")]
    pub fn read(&mut self, start: u16, len: u16) -> Result<Vec<u16>> {
        Ok(sim(start, len, self.t0.elapsed().as_secs_f32()))
    }

    /// FC 03 read of `len` holding registers from `start`.
    #[cfg(not(feature = "sim"))]
    pub fn read(&mut self, start: u16, len: u16) -> Result<Vec<u16>> {
        let mut mreq = ModbusRequest::new(SLAVE, ModbusProto::Rtu);
        let mut req = Vec::new();
        mreq.generate_get_holdings(start, len, &mut req)?;

        self.uart.clear_rx()?;
        self.de.set_high()?;
        self.uart.write(&req)?;
        // Hold DE until the last stop bit is out, else the frame gets cut.
        self.uart.wait_tx_done(BLOCK)?;
        self.de.set_low()?;

        let mut resp = vec![0u8; 5 + 2 * len as usize];
        let n = self.uart.read(&mut resp, TickType::new_millis(TIMEOUT_MS).ticks())?;
        ensure!(n > 0, "timeout");
        let mut regs = Vec::with_capacity(len as usize);
        mreq.parse_u16(&resp[..n], &mut regs)?;
        ensure!(regs.len() == len as usize, "got {} regs, want {len}", regs.len());
        Ok(regs)
    }
}

/// Raw registers for L1 ~2 kW varying, L2 300 W, L3 off.
#[cfg(feature = "sim")]
fn sim(start: u16, len: u16, t: f32) -> Vec<u16> {
    fn put(r: &mut [u16], i: usize, v: u64, n: usize) {
        for k in 0..n {
            r[i + k] = (v >> (16 * (n - 1 - k))) as u16;
        }
    }
    let mut r = vec![0u16; len as usize];
    if start == b23::ENERGY_START {
        put(&mut r, 0, 1_234_500 + (t * 2.3 / 36.0) as u64, 4); // 0.01 kWh
        return r;
    }
    let p = [2000.0 + 1500.0 * (t / 20.0).sin(), 300.0];
    for (n, &p) in p.iter().enumerate() {
        let u = 230.0 + (t + n as f32).sin();
        put(&mut r, n * 2, (u * 10.0) as u64, 2);
        put(&mut r, 0x0C + n * 2, (p / u * 100.0) as u64, 2);
        put(&mut r, 0x16 + n * 2, (p * 100.0) as u64, 2);
        put(&mut r, 0x26 + n * 2, (p / 0.95 * 100.0) as u64, 2);
        r[0x3B + n] = 950;
    }
    put(&mut r, 0x14, ((p[0] + p[1]) * 100.0) as u64, 2);
    put(&mut r, 0x24, ((p[0] + p[1]) / 0.95 * 100.0) as u64, 2);
    r[0x2C] = 5000;
    r[0x3A] = 950;
    r
}
