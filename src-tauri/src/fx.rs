//! Output effects applied after the mix: a stereo reverb (Freeverb-style)
//! for the "slowed + reverb" sound. Only built when the user turns it on,
//! so the default output path is untouched.

/// Lowpass-feedback comb filter.
struct Comb {
    buf: Vec<f32>,
    i: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            i: 0,
            store: 0.0,
        }
    }
    #[inline]
    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let y = self.buf[self.i];
        self.store = y * (1.0 - damp) + self.store * damp;
        self.buf[self.i] = x + self.store * feedback;
        self.i = (self.i + 1) % self.buf.len();
        y
    }
}

struct Allpass {
    buf: Vec<f32>,
    i: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            i: 0,
        }
    }
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        self.buf[self.i] = x + b * 0.5;
        self.i = (self.i + 1) % self.buf.len();
        b - x
    }
}

// Freeverb tunings at 44.1 kHz; the right channel is offset for width.
const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;

struct Lane {
    combs: Vec<Comb>,
    passes: Vec<Allpass>,
}

impl Lane {
    fn new(sr: u32, spread: usize) -> Self {
        let k = sr.max(1) as f64 / 44_100.0;
        let sc = |n: usize| ((n + spread) as f64 * k).round() as usize;
        Self {
            combs: COMBS.iter().map(|&n| Comb::new(sc(n))).collect(),
            passes: ALLPASSES.iter().map(|&n| Allpass::new(sc(n))).collect(),
        }
    }
    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let mut out = 0.0;
        for c in self.combs.iter_mut() {
            out += c.process(x, feedback, damp);
        }
        for a in self.passes.iter_mut() {
            out = a.process(out);
        }
        out
    }
}

pub struct Reverb {
    lanes: Vec<Lane>,
    /// Wet amount 0..=1.
    pub mix: f32,
    feedback: f32,
    damp: f32,
}

impl Reverb {
    pub fn new(sr: u32, ch: usize, mix: f32) -> Self {
        Self {
            lanes: (0..ch.clamp(1, 2))
                .map(|i| Lane::new(sr, i * SPREAD))
                .collect(),
            mix: mix.clamp(0.0, 1.0),
            // Large, fairly dark room.
            feedback: 0.86,
            damp: 0.35,
        }
    }

    /// Process one interleaved frame in place.
    pub fn process(&mut self, frame: &mut [i16]) {
        if frame.is_empty() {
            return;
        }
        let input = frame.iter().map(|&v| v as f32).sum::<f32>() / frame.len() as f32 * 0.03;
        let wet = self.mix;
        // Keep the overall level about even as the wet share grows.
        let dry = 1.0 - 0.45 * wet;
        let n = self.lanes.len();
        let mut outs = [0f32; 2];
        for (i, l) in self.lanes.iter_mut().enumerate() {
            outs[i] = l.process(input, self.feedback, self.damp);
        }
        for (c, s) in frame.iter_mut().enumerate() {
            let w = outs[c.min(n - 1)];
            *s = (*s as f32 * dry + w * wet * 1.6)
                .round()
                .clamp(i16::MIN as f32, i16::MAX as f32) as i16;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impulse_rings_and_decays() {
        let mut r = Reverb::new(44_100, 2, 1.0);
        let mut f = [20_000i16, 20_000];
        r.process(&mut f);
        let mut energy_early = 0f64;
        let mut energy_late = 0f64;
        for i in 0..(44_100 * 4) {
            let mut z = [0i16, 0];
            r.process(&mut z);
            let e = (z[0] as f64).powi(2) + (z[1] as f64).powi(2);
            if i < 22_050 {
                energy_early += e;
            } else if i > 44_100 * 3 {
                energy_late += e;
            }
        }
        assert!(energy_early > 0.0, "reverb produced a tail");
        assert!(energy_late < energy_early * 0.05, "tail decays");
    }

    #[test]
    fn dry_signal_passes_with_no_wet() {
        let mut r = Reverb::new(48_000, 2, 0.0);
        let mut f = [1000i16, -1000];
        r.process(&mut f);
        assert_eq!(f, [1000, -1000]);
    }
}
