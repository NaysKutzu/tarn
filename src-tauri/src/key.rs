//! Musical key detection for harmonic mixing: a pitch-class profile
//! (chroma) from Goertzel filters on decimated mono audio, matched against
//! the Krumhansl–Kessler key profiles. Keys are reported in Camelot
//! notation (8B = C major, 8A = A minor) for DJ-style compatibility.

use serde::{Deserialize, Serialize};

const NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];
const MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];
/// Analysis block at the decimated rate (~0.37 s at 11 kHz).
const BLOCK: usize = 4096;
/// Notes analysed: C2 (MIDI 36) up to B6 (MIDI 95).
const LOW_MIDI: i32 = 36;
const HIGH_MIDI: i32 = 95;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    /// Pitch class of the tonic, 0 = C.
    pub tonic: u8,
    pub minor: bool,
    /// Correlation margin over the runner-up key (higher = surer).
    pub confidence: f32,
}

impl Key {
    /// Camelot wheel position: number 1..=12 and letter (A = minor).
    pub fn camelot(&self) -> (u8, char) {
        // The wheel steps by fifths; minor keys share their relative major's number.
        let major_tonic = if self.minor {
            (self.tonic + 3) % 12
        } else {
            self.tonic
        };
        let n = ((major_tonic as u32 * 7) % 12 + 7) % 12 + 1;
        (n as u8, if self.minor { 'A' } else { 'B' })
    }

    pub fn camelot_str(&self) -> String {
        let (n, l) = self.camelot();
        format!("{n}{l}")
    }

    pub fn name(&self) -> String {
        format!(
            "{}{}",
            NAMES[self.tonic as usize % 12],
            if self.minor { "m" } else { "" }
        )
    }

    /// Steps between two keys on the Camelot wheel: 0 = same key,
    /// 1 = neighbour (±1 number, or relative major/minor), more = clash.
    pub fn distance(&self, other: &Key) -> u8 {
        let (a, la) = self.camelot();
        let (b, lb) = other.camelot();
        let d = (a as i32 - b as i32).rem_euclid(12);
        let d = d.min(12 - d) as u8;
        d + u8::from(la != lb)
    }
}

pub struct KeyDetector {
    dec: usize,
    acc: f32,
    acc_n: usize,
    buf: Vec<f32>,
    window: Vec<f32>,
    /// (pitch class, Goertzel coefficient) per analysed note.
    notes: Vec<(usize, f32)>,
    chroma: [f64; 12],
    blocks: usize,
}

impl KeyDetector {
    pub fn new(sr: u32) -> Self {
        let dec = ((sr as f64 / 11_025.0).round() as usize).max(1);
        let fs = sr as f64 / dec as f64;
        let notes = (LOW_MIDI..=HIGH_MIDI)
            .map(|m| {
                let f = 440.0 * 2f64.powf((m - 69) as f64 / 12.0);
                (
                    (m % 12) as usize,
                    (2.0 * (2.0 * std::f64::consts::PI * f / fs).cos()) as f32,
                )
            })
            .collect();
        let window = (0..BLOCK)
            .map(|i| {
                let x = std::f32::consts::PI * 2.0 * i as f32 / BLOCK as f32;
                0.5 - 0.5 * x.cos()
            })
            .collect();
        Self {
            dec,
            acc: 0.0,
            acc_n: 0,
            buf: Vec::with_capacity(BLOCK),
            window,
            notes,
            chroma: [0.0; 12],
            blocks: 0,
        }
    }

    /// Feed one interleaved frame.
    pub fn push_frame(&mut self, frame: &[i16]) {
        if frame.is_empty() {
            return;
        }
        let mono = frame.iter().map(|&v| v as f32).sum::<f32>() / frame.len() as f32;
        self.acc += mono;
        self.acc_n += 1;
        if self.acc_n < self.dec {
            return;
        }
        self.buf.push(self.acc / self.acc_n as f32 / 32768.0);
        self.acc = 0.0;
        self.acc_n = 0;
        if self.buf.len() == BLOCK {
            self.process_block();
            self.buf.clear();
        }
    }

    fn process_block(&mut self) {
        let energy: f32 = self.buf.iter().map(|v| v * v).sum::<f32>() / BLOCK as f32;
        if energy < 1e-6 {
            return; // silence
        }
        let x: Vec<f32> = self
            .buf
            .iter()
            .zip(&self.window)
            .map(|(a, w)| a * w)
            .collect();
        let mut block = [0f64; 12];
        for &(pc, coeff) in &self.notes {
            let (mut s1, mut s2) = (0f32, 0f32);
            for &v in &x {
                let s = v + coeff * s1 - s2;
                s2 = s1;
                s1 = s;
            }
            let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
            block[pc] += (power.max(0.0) as f64).sqrt();
        }
        // Normalise per block so loud passages don't dominate the profile.
        let sum: f64 = block.iter().sum();
        if sum > 0.0 {
            for (c, b) in self.chroma.iter_mut().zip(block) {
                *c += b / sum;
            }
            self.blocks += 1;
        }
    }

    /// Best matching key, or None with too little tonal material.
    pub fn finish(&self) -> Option<Key> {
        if self.blocks < 8 {
            return None;
        }
        detect(&self.chroma)
    }
}

fn correlate(chroma: &[f64; 12], profile: &[f64; 12], tonic: usize) -> f64 {
    let n = 12.0;
    let mx = chroma.iter().sum::<f64>() / n;
    let my = profile.iter().sum::<f64>() / n;
    let (mut num, mut dx, mut dy) = (0.0, 0.0, 0.0);
    for i in 0..12 {
        let x = chroma[(i + tonic) % 12] - mx;
        let y = profile[i] - my;
        num += x * y;
        dx += x * x;
        dy += y * y;
    }
    if dx <= 0.0 || dy <= 0.0 {
        0.0
    } else {
        num / (dx * dy).sqrt()
    }
}

/// Krumhansl–Schmuckler: the key whose profile correlates best.
pub fn detect(chroma: &[f64; 12]) -> Option<Key> {
    let mut scores: Vec<(f64, u8, bool)> = Vec::with_capacity(24);
    for t in 0..12 {
        scores.push((correlate(chroma, &MAJOR, t), t as u8, false));
        scores.push((correlate(chroma, &MINOR, t), t as u8, true));
    }
    scores.sort_by(|a, b| b.0.total_cmp(&a.0));
    let (best, tonic, minor) = scores[0];
    if best <= 0.2 {
        return None;
    }
    Some(Key {
        tonic,
        minor,
        confidence: (best - scores[1].0) as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render chords (MIDI note lists) as summed sines, 1.5 s each.
    fn render(chords: &[&[i32]], sr: u32) -> KeyDetector {
        let mut d = KeyDetector::new(sr);
        let per = (sr as f32 * 1.5) as usize;
        for (ci, chord) in chords.iter().cycle().take(chords.len() * 3).enumerate() {
            for i in 0..per {
                let t = (ci * per + i) as f32 / sr as f32;
                let v: f32 = chord
                    .iter()
                    .map(|&m| {
                        let f = 440.0 * 2f32.powf((m - 69) as f32 / 12.0);
                        (2.0 * std::f32::consts::PI * f * t).sin()
                    })
                    .sum::<f32>()
                    / chord.len() as f32;
                let s = (v * 12_000.0) as i16;
                d.push_frame(&[s, s]);
            }
        }
        d
    }

    #[test]
    fn detects_c_major_progression() {
        // C - F - G - C, with a bass note.
        let k = render(
            &[
                &[48, 60, 64, 67],
                &[41, 60, 65, 69],
                &[43, 59, 62, 67],
                &[48, 60, 64, 67],
            ],
            44_100,
        )
        .finish()
        .unwrap();
        assert_eq!((k.tonic, k.minor), (0, false), "{}", k.name());
        assert_eq!(k.camelot_str(), "8B");
    }

    #[test]
    fn detects_a_minor_progression() {
        // Am - Dm - E - Am.
        let k = render(
            &[
                &[45, 57, 60, 64],
                &[50, 57, 62, 65],
                &[40, 56, 59, 64],
                &[45, 57, 60, 64],
            ],
            48_000,
        )
        .finish()
        .unwrap();
        assert_eq!((k.tonic, k.minor), (9, true), "{}", k.name());
        assert_eq!(k.camelot_str(), "8A");
    }

    #[test]
    fn camelot_wheel() {
        let k = |t: u8, m: bool| Key {
            tonic: t,
            minor: m,
            confidence: 1.0,
        };
        assert_eq!(k(7, false).camelot_str(), "9B"); // G
        assert_eq!(k(11, false).camelot_str(), "1B"); // B
        assert_eq!(k(4, true).camelot_str(), "9A"); // Em
        assert_eq!(k(8, true).camelot_str(), "1A"); // G#m
        assert_eq!(k(0, false).distance(&k(9, true)), 1); // relative minor
        assert_eq!(k(0, false).distance(&k(7, false)), 1); // fifth up
        assert_eq!(k(0, false).distance(&k(11, false)), 5);
        assert_eq!(k(0, false).distance(&k(0, false)), 0);
    }

    #[test]
    fn silence_has_no_key() {
        let mut d = KeyDetector::new(44_100);
        for _ in 0..44_100 * 5 {
            d.push_frame(&[0, 0]);
        }
        assert!(d.finish().is_none());
    }
}
