//! ABB B23 register decode. Ref: docs/registers.md.

use serde::Serialize;

pub const INST_START: u16 = 0x5B00;
pub const INST_LEN: u16 = 62; // 5B00..=5B3D
pub const ENERGY_START: u16 = 0x5000;
pub const ENERGY_LEN: u16 = 8; // 5000..=5007

/// Below this L-N voltage a phase counts as not connected.
pub const PHASE_MIN_V: f32 = 50.0;

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize)]
pub struct Phase {
    pub active: bool,
    pub u: Option<f32>,
    pub i: Option<f32>,
    pub p: Option<f32>,
    pub q: Option<f32>,
    pub s: Option<f32>,
    pub pf: Option<f32>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize)]
pub struct Inst {
    pub l: [Phase; 3],
    pub p: Option<f32>,
    pub q: Option<f32>,
    pub s: Option<f32>,
    pub f: Option<f32>,
    pub pf: Option<f32>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize)]
pub struct Energy {
    pub import_kwh: Option<f64>,
    pub export_kwh: Option<f64>,
}

fn u16_at(r: &[u16], i: usize) -> Option<u16> {
    Some(r[i]).filter(|&v| v != u16::MAX)
}

fn i16_at(r: &[u16], i: usize) -> Option<i16> {
    Some(r[i] as i16).filter(|&v| v != i16::MAX)
}

fn u32_at(r: &[u16], i: usize) -> Option<u32> {
    Some((r[i] as u32) << 16 | r[i + 1] as u32).filter(|&v| v != u32::MAX)
}

fn i32_at(r: &[u16], i: usize) -> Option<i32> {
    u32_at(r, i).map(|v| v as i32).filter(|&v| v != i32::MAX)
}

fn u64_at(r: &[u16], i: usize) -> Option<u64> {
    let v = r[i..i + 4].iter().fold(0u64, |a, &w| a << 16 | w as u64);
    Some(v).filter(|&v| v != u64::MAX)
}

/// Decode `INST_LEN` registers read from `INST_START`.
pub fn decode_inst(r: &[u16]) -> Option<Inst> {
    if r.len() != INST_LEN as usize {
        return None;
    }
    let u32s = |i, k: f32| u32_at(r, i).map(|v| v as f32 * k);
    let i32s = |i, k: f32| i32_at(r, i).map(|v| v as f32 * k);
    let pf = |i| i16_at(r, i).map(|v| v as f32 * 0.001);
    let phase = |n: usize| {
        let u = u32s(n * 2, 0.1);
        Phase {
            active: u.is_some_and(|u| u >= PHASE_MIN_V),
            u,
            i: u32s(0x0C + n * 2, 0.01),
            p: i32s(0x16 + n * 2, 0.01),
            q: i32s(0x1E + n * 2, 0.01),
            s: i32s(0x26 + n * 2, 0.01),
            pf: pf(0x3B + n),
        }
    };
    Some(Inst {
        l: [phase(0), phase(1), phase(2)],
        p: i32s(0x14, 0.01),
        q: i32s(0x1C, 0.01),
        s: i32s(0x24, 0.01),
        f: u16_at(r, 0x2C).map(|v| v as f32 * 0.01),
        pf: pf(0x3A),
    })
}

/// Decode `ENERGY_LEN` registers read from `ENERGY_START`.
pub fn decode_energy(r: &[u16]) -> Option<Energy> {
    if r.len() != ENERGY_LEN as usize {
        return None;
    }
    let kwh = |i| u64_at(r, i).map(|v| v as f64 * 0.01);
    Some(Energy { import_kwh: kwh(0), export_kwh: kwh(4) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put32(r: &mut [u16], i: usize, v: u32) {
        r[i] = (v >> 16) as u16;
        r[i + 1] = v as u16;
    }

    fn approx(a: Option<f32>, b: f32) {
        let a = a.expect("value");
        assert!((a - b).abs() < 1e-3, "{a} != {b}");
    }

    #[test]
    fn single_phase_with_export() {
        let mut r = [0u16; INST_LEN as usize];
        put32(&mut r, 0x00, 2301); // 230.1 V
        put32(&mut r, 0x0C, 1234); // 12.34 A
        put32(&mut r, 0x14, -150_000i32 as u32); // -1500 W total (export)
        put32(&mut r, 0x16, -150_000i32 as u32);
        r[0x2C] = 4998; // 49.98 Hz
        r[0x3B] = -950i16 as u16; // PF -0.95

        let d = decode_inst(&r).unwrap();
        assert!(d.l[0].active && !d.l[1].active && !d.l[2].active);
        approx(d.l[0].u, 230.1);
        approx(d.l[0].i, 12.34);
        approx(d.l[0].p, -1500.0);
        approx(d.p, -1500.0);
        approx(d.f, 49.98);
        approx(d.l[0].pf, -0.95);
    }

    #[test]
    fn invalid_markers_are_none() {
        let mut r = [0u16; INST_LEN as usize];
        put32(&mut r, 0x00, u32::MAX);
        put32(&mut r, 0x14, i32::MAX as u32);
        r[0x2C] = u16::MAX;
        r[0x3A] = i16::MAX as u16;

        let d = decode_inst(&r).unwrap();
        assert_eq!((d.l[0].u, d.l[0].active), (None, false));
        assert_eq!((d.p, d.f, d.pf), (None, None, None));
    }

    #[test]
    fn energy() {
        // 0x0000_0001_0000_0015 * 0.01 kWh; export invalid
        let r = [0, 1, 0, 0x15, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF];
        let e = decode_energy(&r).unwrap();
        assert_eq!(e.import_kwh, Some(0x1_0000_0015u64 as f64 * 0.01));
        assert_eq!(e.export_kwh, None);
    }

    #[test]
    fn wrong_length_rejected() {
        assert!(decode_inst(&[0; 10]).is_none());
        assert!(decode_energy(&[0; 10]).is_none());
    }
}
