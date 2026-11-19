//! Audio tap for the fullscreen visualizer: the output samples are copied
//! into a small lock-free ring while the visualizer is open, and
//! `spectrum` turns the latest window into log-spaced band levels.

use std::sync::atomic::{AtomicBool, AtomicI16, AtomicU32, AtomicUsize, Ordering};

/// Interleaved samples kept (enough for a 2048-frame stereo window).
const RING: usize = 8192;
pub const FFT: usize = 2048;

static ON: AtomicBool = AtomicBool::new(false);
static POS: AtomicUsize = AtomicUsize::new(0);
static CH: AtomicU32 = AtomicU32::new(2);
static SR: AtomicU32 = AtomicU32::new(44_100);
#[allow(clippy::declare_interior_mutable_const)]
const ZERO: AtomicI16 = AtomicI16::new(0);
static BUF: [AtomicI16; RING] = [ZERO; RING];

pub fn set_on(on: bool) {
    ON.store(on, Ordering::Relaxed);
}

#[inline]
pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Stream format of the samples being pushed.
pub fn set_format(sr: u32, ch: u16) {
    SR.store(sr.max(1), Ordering::Relaxed);
    CH.store(ch.max(1) as u32, Ordering::Relaxed);
}

/// Record one output sample (call only while `on()`).
#[inline]
pub fn push(v: i16) {
    let i = POS.fetch_add(1, Ordering::Relaxed);
    BUF[i % RING].store(v, Ordering::Relaxed);
}

/// The most recent `FFT` mono frames, oldest first.
fn window() -> Vec<f32> {
    let ch = CH.load(Ordering::Relaxed).clamp(1, 8) as usize;
    let end = POS.load(Ordering::Relaxed);
    // Align to a frame boundary so channels don't swap.
    let end = end - end % ch;
    let need = FFT * ch;
    let start = end.saturating_sub(need);
    let mut out = vec![0f32; FFT];
    for (k, f) in (start..end).step_by(ch).enumerate().take(FFT) {
        let mut s = 0f32;
        for c in 0..ch {
            s += BUF[(f + c) % RING].load(Ordering::Relaxed) as f32;
        }
        out[k + FFT - (end - start) / ch] = s / ch as f32 / 32768.0;
    }
    out
}

/// In-place iterative radix-2 FFT (len must be a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let (wr, wi) = (ang.cos(), ang.sin());
        for s in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1f32, 0f32);
            for k in 0..len / 2 {
                let (a, b) = (s + k, s + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

/// Band levels 0..=1 for `bands` log-spaced bands from 40 Hz to 16 kHz.
pub fn spectrum_of(samples: &[f32], sr: u32, bands: usize) -> Vec<f32> {
    let n = samples.len();
    let mut re: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, v)| v * (0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / n as f32).cos()))
        .collect();
    let mut im = vec![0f32; n];
    fft(&mut re, &mut im);
    let bin_hz = sr as f32 / n as f32;
    let (lo, hi) = (40f32, 16_000f32.min(sr as f32 / 2.0));
    (0..bands)
        .map(|b| {
            let f0 = lo * (hi / lo).powf(b as f32 / bands as f32);
            let f1 = lo * (hi / lo).powf((b + 1) as f32 / bands as f32);
            let i0 = ((f0 / bin_hz).floor() as usize).clamp(1, n / 2 - 1);
            let i1 = ((f1 / bin_hz).ceil() as usize).clamp(i0 + 1, n / 2);
            let mag = (i0..i1)
                .map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt())
                .fold(0f32, f32::max);
            // Hann-windowed full-scale sine peaks at n/4.
            let db = 20.0 * (mag / (n as f32 / 4.0)).max(1e-9).log10();
            // Gentle tilt so highs (less energy in music) still show.
            let tilt = 3.0 * (f0 / 1000.0).max(0.03).log2();
            ((db + tilt + 66.0) / 60.0).clamp(0.0, 1.0)
        })
        .collect()
}

/// Current band levels of the output.
pub fn spectrum(bands: usize) -> Vec<f32> {
    spectrum_of(&window(), SR.load(Ordering::Relaxed), bands.clamp(4, 256))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_lights_its_band() {
        let sr = 44_100;
        let s: Vec<f32> = (0..FFT)
            .map(|i| (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / sr as f32).sin() * 0.5)
            .collect();
        let b = spectrum_of(&s, sr, 32);
        let top = b
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap()
            .0;
        // 1 kHz on a 40 Hz..16 kHz log scale with 32 bands is band ~17.
        assert!((16..=18).contains(&top), "band {top} {b:?}");
        assert!(b[top] > 0.8);
        assert!(b[2] < 0.3, "low band quiet: {}", b[2]);
    }

    #[test]
    fn silence_is_zero() {
        let b = spectrum_of(&vec![0.0; FFT], 48_000, 16);
        assert!(b.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn ring_window_reads_latest_frames() {
        set_format(44_100, 2);
        for i in 0..(FFT * 2) {
            push((i % 100) as i16);
            push((i % 100) as i16);
        }
        let w = window();
        assert_eq!(w.len(), FFT);
        let last = ((FFT * 2 - 1) % 100) as f32 / 32768.0;
        assert!((w[FFT - 1] - last).abs() < 1e-6);
    }
}
