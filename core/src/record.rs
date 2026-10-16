//! Energy record of one power-on run: integrated P·dt plus a fixed-size power graph.

use serde::Serialize;

pub const BUCKETS: usize = 240;
/// A run only replaces the last record if it is at least this long and large,
/// so plugging in just to read the last record keeps it.
pub const MIN_SECS: f64 = 60.0;
pub const MIN_WH: f64 = 1.0;

const FORMAT: u8 = 1;
const HEADER: usize = 1 + 8 + 8 + 4 + 4 + 2;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Record {
    pub secs: f64,
    pub wh: f64,
    pub peak_w: f32,
    /// Seconds per graph point; doubles each time the graph fills up.
    pub bucket_s: f32,
    /// Average W per bucket.
    pub graph: Vec<f32>,
    #[serde(skip)]
    acc_ws: f64,
    #[serde(skip)]
    acc_s: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Summary {
    pub secs: f64,
    pub wh: f64,
    pub peak_w: f32,
    pub avg_w: Option<f32>,
}

impl Default for Record {
    fn default() -> Self {
        Self { secs: 0.0, wh: 0.0, peak_w: 0.0, bucket_s: 1.0, graph: Vec::with_capacity(BUCKETS), acc_ws: 0.0, acc_s: 0.0 }
    }
}

impl Record {
    /// Integrate one sample. `None` (meter offline) pauses the record.
    pub fn tick(&mut self, p_w: Option<f32>, dt_s: f32) {
        let Some(p) = p_w else { return };
        self.secs += dt_s as f64;
        self.wh += p as f64 * dt_s as f64 / 3600.0;
        self.peak_w = self.peak_w.max(p);
        self.acc_ws += p as f64 * dt_s as f64;
        self.acc_s += dt_s;
        if self.acc_s >= self.bucket_s {
            self.graph.push((self.acc_ws / self.acc_s as f64) as f32);
            self.acc_ws = 0.0;
            self.acc_s = 0.0;
            if self.graph.len() == BUCKETS {
                self.graph = self.graph.chunks(2).map(|c| (c[0] + c[1]) / 2.0).collect();
                self.bucket_s *= 2.0;
            }
        }
    }

    pub fn summary(&self) -> Summary {
        let avg_w = (self.secs > 0.0).then(|| (self.wh * 3600.0 / self.secs) as f32);
        Summary { secs: self.secs, wh: self.wh, peak_w: self.peak_w, avg_w }
    }

    pub fn counts(&self) -> bool {
        self.secs >= MIN_SECS && self.wh >= MIN_WH
    }

    /// Little-endian blob for NVS. The open bucket is not stored (≤ one bucket lost).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(HEADER + 4 * self.graph.len());
        b.push(FORMAT);
        b.extend(self.secs.to_le_bytes());
        b.extend(self.wh.to_le_bytes());
        b.extend(self.peak_w.to_le_bytes());
        b.extend(self.bucket_s.to_le_bytes());
        b.extend((self.graph.len() as u16).to_le_bytes());
        self.graph.iter().for_each(|v| b.extend(v.to_le_bytes()));
        b
    }

    /// `None` on unknown format or corrupt length.
    pub fn from_bytes(b: &[u8]) -> Option<Self> {
        if b.len() < HEADER || b[0] != FORMAT {
            return None;
        }
        let f64_at = |i: usize| f64::from_le_bytes(b[i..i + 8].try_into().unwrap());
        let f32_at = |i: usize| f32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let n = u16::from_le_bytes([b[25], b[26]]) as usize;
        if n >= BUCKETS || b.len() != HEADER + 4 * n {
            return None;
        }
        let r = Self {
            secs: f64_at(1),
            wh: f64_at(9),
            peak_w: f32_at(17),
            bucket_s: f32_at(21),
            graph: (0..n).map(|k| f32_at(HEADER + 4 * k)).collect(),
            ..Default::default()
        };
        (r.secs.is_finite() && r.wh.is_finite() && r.bucket_s >= 1.0).then_some(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integrates_energy_and_peak() {
        let mut r = Record::default();
        for _ in 0..3600 {
            r.tick(Some(1000.0), 1.0);
        }
        r.tick(Some(1500.0), 0.25);
        assert!((r.wh - 1000.0 - 1500.0 * 0.25 / 3600.0).abs() < 1e-6);
        assert_eq!(r.peak_w, 1500.0);
        assert!((r.summary().avg_w.unwrap() - 1000.0).abs() < 0.2);
    }

    #[test]
    fn offline_pauses() {
        let mut r = Record::default();
        r.tick(None, 10.0);
        assert_eq!((r.secs, r.wh, r.graph.len()), (0.0, 0.0, 0));
        assert_eq!(r.summary().avg_w, None);
    }

    #[test]
    fn graph_halves_when_full() {
        let mut r = Record::default();
        for k in 0..BUCKETS {
            r.tick(Some(k as f32), 1.0);
        }
        assert_eq!((r.graph.len(), r.bucket_s), (BUCKETS / 2, 2.0));
        assert_eq!(&r.graph[..2], &[0.5, 2.5]);
        for _ in 0..4 {
            r.tick(Some(7.0), 1.0); // 2 s buckets now
        }
        assert_eq!(r.graph.len(), BUCKETS / 2 + 2);
    }

    #[test]
    fn promotion_rule() {
        let mut r = Record::default();
        r.tick(Some(100.0), 59.0);
        assert!(!r.counts());
        r.tick(Some(100.0), 1.0);
        assert!(r.counts()); // 60 s, 1.67 Wh
        let mut idle = Record::default();
        idle.tick(Some(0.5), 3600.0);
        assert!(!idle.counts()); // 0.5 Wh
    }

    #[test]
    fn bytes_roundtrip_and_reject() {
        let mut r = Record::default();
        for k in 0..500 {
            r.tick(Some(k as f32), 1.0);
        }
        let b = r.to_bytes();
        let back = Record::from_bytes(&b).unwrap();
        assert_eq!((back.secs, back.wh, back.peak_w, back.bucket_s), (r.secs, r.wh, r.peak_w, r.bucket_s));
        assert_eq!(back.graph, r.graph);

        assert!(Record::from_bytes(&b[..b.len() - 1]).is_none());
        assert!(Record::from_bytes(&[]).is_none());
        let mut bad = b.clone();
        bad[0] = 99;
        assert!(Record::from_bytes(&bad).is_none());
    }
}
