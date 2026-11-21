//! AutoMix: picks where and how long to blend into the next song, the way a
//! DJ would, from the outgoing track's cached analysis.
//!
//! * The blend ends where the music ends (trailing silence is skipped).
//! * A quiet outro is where the mix happens: the blend starts when the
//!   outro starts (up to 16 s), so the next song rises over it.
//! * With a clear beat, the length is whole bars (8 bars for faster songs,
//!   4 bars for slower ones) and the start lands on a bar line.
//! * Without a usable beat (ballads, spoken intros) it stays a short,
//!   smooth fade.
//!
//! DJ mode (`Style::Dj`) plays it like a club DJ instead and picks a move
//! for every pair of songs: a drop straight into the next song's hook, a
//! short beatmatched EQ blend, or an echo out (see [`dj_plan`]).
//!
//! Tempo matching, beat alignment of the incoming track, keylock and the
//! bass swap are done by the regular transition code (see `transition.rs`);
//! AutoMix only turns them on and chooses the timing.

use crate::analysis::{Grid, TrackAnalysis};
use crate::transition::{Curve, DjStyle, FilterSweep, XfConfig, XfShape};

/// AutoMix flavour chosen in the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Style {
    /// Smooth, timed blends (the original AutoMix).
    #[default]
    Normal,
    /// Club-style phrase mixes with an EQ bass swap / echo out.
    Dj,
}

impl Style {
    pub fn parse(s: &str) -> Self {
        match s {
            "dj" => Style::Dj,
            _ => Style::Normal,
        }
    }
}

/// Longest blend any mode asks the mixer for.
pub const MAX_BLEND_SECS: f32 = 40.0;

/// Confidence above which the outgoing beat grid is trusted for bar
/// lengths and phrase alignment (`MIN_CONFIDENCE` is the floor for any use).
const SURE_BEAT: f64 = 2.2;
const MIN_SECS: f64 = 3.0;
const MAX_SECS: f64 = 16.0;
/// Fade used when nothing is known about the outgoing track.
pub const FALLBACK_SECS: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixPoint {
    /// Track time (seconds) when the blend should start.
    pub start_s: f64,
    /// Length of the blend.
    pub secs: f32,
}

/// When and how long to blend out of a track of `duration` seconds
/// (Normal timing; DJ mode plans with [`dj_plan`] and falls back to this).
pub fn mix_point(a: Option<&TrackAnalysis>, duration: f64) -> MixPoint {
    normal_mix_point(a, duration)
}

/// Where the music really ends. Ignores implausible values (analysis of a
/// different encode, or a track that is silent for minutes).
fn music_end(a: &TrackAnalysis, duration: f64) -> f64 {
    let end = a.music_end_s as f64;
    if !end.is_finite() || end <= duration - 45.0 || end > duration + 0.5 {
        duration
    } else {
        end.min(duration)
    }
}

/// One DJ-mode transition, planned from both tracks' analysis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DjPlan {
    /// Outgoing track time (s) the transition starts on (a bar line).
    pub start_s: f64,
    /// Whole window length (s).
    pub secs: f32,
    /// EqSwap, Drop or EchoOut.
    pub style: DjStyle,
    /// Drop / EchoOut: incoming track time (s) to come in on (its drop, or
    /// where it gets going after a quiet intro).
    pub in_start_s: Option<f64>,
    /// Outgoing beat length (s), for the echo.
    pub beat_s: f64,
    /// Where the new song gets going after a quiet intro (a cut that has
    /// to fall back to an echo out comes in here).
    pub entry_s: Option<f64>,
}

/// Beat grid good enough to plan on.
fn sure_grid(g: Option<Grid>) -> Option<Grid> {
    g.filter(|g| g.bpm.is_finite() && g.bpm > 40.0 && g.bpm < 220.0)
        .filter(|g| g.confidence >= SURE_BEAT)
}

/// DJ mode: picks the move for this pair of songs and when to do it.
///
/// * The move lands on the end of the outgoing song's last phrase (the bar
///   where its music ends, or where a quiet outro starts), never half a
///   minute early.
/// * **Drop** when the next song has a quiet intro and a clear jump into its
///   hook: the old song builds (rising filter, noise riser, echo), is cut on
///   the bar, and the new one slams in straight on its drop.
/// * **EqSwap** when the tempos can be matched and there is no drop to jump
///   to: a short (8 bars, 4 on slow songs) beatmatched EQ blend.
/// * **EchoOut** otherwise: one bar of echo build, cut, new song from its
///   first beat under the echo tail.
///
/// None without a trustworthy outgoing beat (Normal timing is used then).
pub fn dj_plan(
    out: &TrackAnalysis,
    inc: Option<&TrackAnalysis>,
    duration: f64,
    max_pct: f64,
    prev: Option<DjStyle>,
) -> Option<DjPlan> {
    if duration <= 30.0 {
        return None;
    }
    let g = sure_grid(out.tail)?;
    let beat = g.period_s();
    let bar = 4.0 * beat;
    if bar <= 0.5 {
        return None;
    }
    let end = music_end(out, duration);
    let snap_down = |t: f64| g.phase_s + ((t - g.phase_s) / bar).floor() * bar;
    // End of the last phrase: the last bar line before the music ends, or
    // the start of a quiet outro (dead air on a dance floor).
    let mut phrase_end = snap_down(end);
    if let Some(outro) = outro_start(&out.wave, duration, end) {
        let s = snap_down(outro + 0.25 * bar);
        if s < phrase_end && s > duration * 0.5 {
            phrase_end = s;
        }
    }
    let floor = duration * 0.5;
    let in_grid = inc.and_then(|a| {
        a.head
            .filter(|g| g.bpm.is_finite() && g.bpm > 40.0 && g.bpm < 220.0)
            .filter(|g| g.confidence >= crate::transition::MIN_CONFIDENCE)
    });
    let matched = in_grid
        .is_some_and(|ig| crate::transition::match_rate(g.bpm, ig.bpm, max_pct).is_some());
    let drop = inc.zip(in_grid).and_then(|(a, ig)| find_drop(a, ig));
    let entry = inc.zip(in_grid).and_then(|(a, ig)| find_entry(a, ig));

    let plan = |style: DjStyle, build: f64, secs: f64, in_start_s: Option<f64>| {
        let start = phrase_end - build;
        (start > floor).then_some(DjPlan {
            start_s: start,
            secs: secs as f32,
            style,
            in_start_s,
            beat_s: beat,
            entry_s: entry,
        })
    };
    // Keep the set varied: right after a drop, blend if the beats allow it.
    let drop = drop.filter(|_| !(matched && prev == Some(DjStyle::Drop)));
    if let Some(d) = drop {
        // Two bars of build (one on slow songs); the cut sits at
        // `DROP_CUT` of the window, the rest is the echo tail.
        let bars = if 2.0 * bar <= 6.0 { 2.0 } else { 1.0 };
        let build = bars * bar;
        if let Some(p) = plan(DjStyle::Drop, build, build / DjStyle::DROP_CUT as f64, Some(d)) {
            return Some(p);
        }
    }
    if matched {
        let mut bars = if g.bpm >= 110.0 { 8.0 } else { 4.0 };
        while bars > 2.0 && bars * bar > 16.0 {
            bars /= 2.0;
        }
        let secs = bars * bar;
        // Quiet intro: start the new song early enough that it gets going
        // right on the bass swap, instead of blending into near silence.
        let in_start = entry.zip(in_grid).and_then(|(e, ig)| {
            let (_, eff) = crate::transition::match_rate(g.bpm, ig.bpm, max_pct)?;
            let t = e - (bars / 2.0) * 4.0 * 60.0 / eff;
            (t > inc.map(|a| a.lead_silence_s as f64).unwrap_or(0.0) + 0.5).then_some(t)
        });
        if let Some(p) = plan(DjStyle::EqSwap, secs, secs, in_start) {
            return Some(p);
        }
    }
    let build = if bar < 1.5 { 2.0 * bar } else { bar };
    // No silence after the cut: skip a quiet intro of the new song.
    plan(DjStyle::EchoOut, build, build / DjStyle::ECHO_CUT as f64, entry)
}

/// Where the incoming song gets going when it opens quietly (ambient
/// intro, a lone instrument): the first moment its level reaches the body
/// of the song, on a bar (or beat) line. None when it starts strong.
fn find_entry(a: &TrackAnalysis, grid: Grid) -> Option<f64> {
    let n = a.wave.len();
    let dur = a.duration_s as f64;
    if n < 60 || dur < 30.0 {
        return None;
    }
    let bin_s = dur / n as f64;
    let mut body: Vec<u8> = a.wave[n / 10..n * 85 / 100].to_vec();
    body.sort_unstable();
    let median = body[body.len() / 2] as f64;
    if median < 30.0 {
        return None;
    }
    let lead = a.lead_silence_s as f64;
    let from = (lead / bin_s).floor() as usize;
    let to = ((lead + 30.0) / bin_s).min(n as f64 - 3.0) as usize;
    let i = (from..to).find(|&i| {
        let m = (a.wave[i] as f64 + a.wave[i + 1] as f64 + a.wave[i + 2] as f64) / 3.0;
        m >= 0.6 * median
    })?;
    let t = i as f64 * bin_s;
    if t < lead + 2.0 {
        return None;
    }
    // On the nearest bar when that is close, else on the beat before.
    let beat = grid.period_s();
    let bar = 4.0 * beat;
    let near_bar = grid.phase_s + ((t - grid.phase_s) / bar).round() * bar;
    let snapped = if (near_bar - t).abs() <= 0.6 {
        near_bar
    } else {
        grid.phase_s + ((t - grid.phase_s) / beat).floor() * beat
    };
    (snapped > lead + 1.0).then_some(snapped)
}

/// The incoming song's drop / hook: where its level jumps clearly from a
/// quieter intro into the body of the song, 8-50 s in,
/// snapped to a phrase (4 bars from its first beat) or at least a bar.
fn find_drop(a: &TrackAnalysis, grid: Grid) -> Option<f64> {
    let n = a.wave.len();
    let dur = a.duration_s as f64;
    if n < 60 || dur < 60.0 {
        return None;
    }
    let bin_s = dur / n as f64;
    let w = ((4.0 / bin_s).ceil() as usize).max(2);
    let mean = |lo: usize, hi: usize| {
        let s = &a.wave[lo.min(n)..hi.min(n)];
        s.iter().map(|&v| v as f64).sum::<f64>() / s.len().max(1) as f64
    };
    let mut body: Vec<u8> = a.wave[n / 10..n * 85 / 100].to_vec();
    body.sort_unstable();
    let median = body[body.len() / 2] as f64;
    if median < 30.0 {
        return None;
    }
    let first = ((a.lead_silence_s as f64 + 8.0) / bin_s).ceil() as usize;
    let last = ((50.0f64.min(dur * 0.35)) / bin_s) as usize;
    let mut best: Option<(usize, f64)> = None;
    for i in first.max(w)..last.min(n - w) {
        let pre = mean(i - w, i);
        let post = mean(i, i + w);
        // A real step: quiet right before it, loud right after.
        let just_before = mean(i - 2, i);
        if post >= 0.9 * median
            && pre <= 0.6 * post
            && just_before <= 0.65 * post
            && post - pre >= 0.35 * median
        {
            let jump = post - pre;
            if best.is_none_or(|(_, b)| jump > b + 1e-9) {
                best = Some((i, jump));
            }
        }
    }
    let (i, _) = best?;
    let t = i as f64 * bin_s;
    let bar = 4.0 * grid.period_s();
    let phrase = 4.0 * bar;
    let near = |unit: f64| grid.phase_s + ((t - grid.phase_s) / unit).round() * unit;
    let p = near(phrase);
    let snapped = if (p - t).abs() <= bin_s + 0.6 * bar { p } else { near(bar) };
    (snapped > a.lead_silence_s as f64 + 2.0).then_some(snapped)
}

fn normal_mix_point(a: Option<&TrackAnalysis>, duration: f64) -> MixPoint {
    let fallback = |end: f64| {
        let secs = (FALLBACK_SECS as f64).min(end * 0.3).max(0.5);
        MixPoint { start_s: (end - secs).max(0.0), secs: secs as f32 }
    };
    let Some(a) = a else {
        return fallback(duration);
    };
    if duration <= 1.0 {
        return fallback(duration);
    }
    let end = music_end(a, duration);

    let grid = a.tail.filter(|g| g.bpm.is_finite() && g.bpm > 40.0 && g.bpm < 220.0);
    let sure = grid.filter(|g| g.confidence >= SURE_BEAT);

    // Default length from the beat.
    let mut secs = match (sure, grid) {
        (Some(g), _) => {
            let beats = if g.bpm >= 110.0 { 32.0 } else { 16.0 };
            (beats * g.period_s()).clamp(6.0, MAX_SECS)
        }
        (None, Some(_)) => 8.0,
        (None, None) => FALLBACK_SECS as f64,
    };
    let mut start = end - secs;

    // Mix over a quiet outro when there is one.
    if let Some(outro) = outro_start(&a.wave, duration, end) {
        let len = end - outro;
        if len >= 4.0 {
            start = outro.max(end - MAX_SECS);
            secs = end - start;
        }
    }

    // Start on a bar line (rounding to the nearest one, never past the end).
    if let Some(g) = sure {
        let bar = 4.0 * g.period_s();
        if bar > 0.5 {
            let k = ((start - g.phase_s) / bar).round();
            let snapped = g.phase_s + k * bar;
            if end - snapped >= MIN_SECS && end - snapped <= MAX_SECS + bar {
                start = snapped;
                secs = end - start;
            }
        }
    }

    // Never more than a third of a short song.
    let cap = (duration * 0.34).max(MIN_SECS);
    if secs > cap {
        secs = cap;
        start = end - secs;
    }
    secs = secs.clamp(MIN_SECS.min(end), MAX_SECS + 4.0);
    MixPoint { start_s: start.max(0.0), secs: secs as f32 }
}

/// Start of a quiet ending (fade-out / outro) from the waveform overview:
/// the earliest point in the last 40 s after which the level stays well
/// under the body of the song. None when the song ends at full level.
fn outro_start(wave: &[u8], duration: f64, end: f64) -> Option<f64> {
    let n = wave.len();
    if n < 40 || duration <= 30.0 {
        return None;
    }
    let bin_s = duration / n as f64;
    let at = |t: f64| ((t / bin_s).floor() as usize).min(n - 1);
    // Typical loudness of the body (middle of the song).
    let mut body: Vec<u8> = wave[n / 10..n * 85 / 100].to_vec();
    body.sort_unstable();
    let median = body[body.len() / 2] as f64;
    if median < 30.0 {
        return None;
    }
    let quiet = median * 0.55;
    let last = at((end - 0.5).max(0.0));
    let first = at((end - 40.0).max(0.0));
    if (wave[last] as f64) >= quiet {
        return None;
    }
    let mut i = last;
    while i > first && (wave[i - 1] as f64) < quiet {
        i -= 1;
    }
    let t = i as f64 * bin_s;
    (end - t >= 2.0).then_some(t)
}

/// Transition settings AutoMix plays with: tempo matched with keylock,
/// beats aligned, silence trimmed, smooth curves and a bass swap. DJ mode
/// adds the EQ mix / echo out, a wider pitch range and a slow glide back.
pub fn config(base: &XfConfig, style: Style) -> XfConfig {
    let dj = style == Style::Dj;
    XfConfig {
        shape: XfShape {
            secs: base.shape.secs,
            curve_out: Curve::Smooth,
            curve_in: Curve::Smooth,
            out_len: 1.0,
            in_len: 1.0,
            filter: FilterSweep::Sweep,
            dj: if dj { DjStyle::Auto } else { DjStyle::Off },
        },
        smart: true,
        bpm_match: true,
        bpm_max_pct: base.bpm_max_pct.max(if dj { 10.0 } else { 8.0 }),
        beat_align: true,
        bpm_return_secs: if dj { 24.0 } else { 12.0 },
        keylock: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::Grid;

    fn track(duration: f32, end: f32, tail: Option<Grid>, wave: Vec<u8>) -> TrackAnalysis {
        TrackAnalysis {
            v: 2,
            lufs: -9.0,
            peak: 0.9,
            duration_s: duration,
            lead_silence_s: 0.0,
            music_end_s: end,
            head: None,
            tail,
            at: 0,
            wave,
            key: None,
        }
    }

    #[test]
    fn no_analysis_is_a_short_fade_at_the_end() {
        let p = mix_point(None, 200.0);
        assert!((p.start_s + p.secs as f64 - 200.0).abs() < 1e-6);
        assert!(p.secs <= FALLBACK_SECS);
    }

    #[test]
    fn dance_track_mixes_eight_bars_on_a_bar_line() {
        let g = Grid { bpm: 128.0, phase_s: 0.2, confidence: 4.0 };
        let a = track(240.0, 238.0, Some(g), vec![200; 240]);
        let p = mix_point(Some(&a), 240.0);
        let bar = 4.0 * 60.0 / 128.0;
        // Ends at the music end, on a bar line, roughly 8 bars long.
        assert!((p.start_s + p.secs as f64 - 238.0).abs() < 0.01);
        let k = (p.start_s - 0.2) / bar;
        assert!((k - k.round()).abs() < 1e-6, "start {} not on a bar", p.start_s);
        assert!(p.secs >= 12.0 && p.secs <= 18.0, "{}", p.secs);
    }

    #[test]
    fn quiet_outro_is_where_the_mix_happens() {
        // Loud body, last ~12 s fade.
        let mut wave = vec![220u8; 240];
        for w in wave.iter_mut().skip(228) {
            *w = 60;
        }
        let a = track(240.0, 240.0, None, wave);
        let p = mix_point(Some(&a), 240.0);
        assert!((p.start_s - 228.0).abs() < 1.5, "start {}", p.start_s);
        assert!((p.secs - 12.0).abs() < 1.5);
    }

    #[test]
    fn trailing_silence_is_skipped() {
        let a = track(200.0, 185.0, None, vec![200; 240]);
        let p = mix_point(Some(&a), 200.0);
        assert!((p.start_s + p.secs as f64 - 185.0).abs() < 0.01);
    }

    #[test]
    fn short_song_is_never_mostly_blend() {
        let g = Grid { bpm: 90.0, phase_s: 0.0, confidence: 4.0 };
        let a = track(30.0, 30.0, Some(g), vec![200; 240]);
        let p = mix_point(Some(&a), 30.0);
        assert!(p.secs as f64 <= 30.0 * 0.34 + 0.01);
    }

    fn grid(bpm: f64) -> Grid {
        Grid { bpm, phase_s: 0.0, confidence: 4.0 }
    }

    /// A song with a quiet intro that jumps to full level at `drop` s.
    fn with_intro(drop: f32, bpm: f64) -> TrackAnalysis {
        let wave = (0..240).map(|i| if (i as f32) < drop { 70 } else { 220 }).collect();
        TrackAnalysis { head: Some(grid(bpm)), ..track(240.0, 240.0, Some(grid(bpm)), wave) }
    }

    #[test]
    fn dj_drops_into_the_hook_of_a_song_with_a_quiet_intro() {
        let out = track(240.0, 238.0, Some(grid(128.0)), vec![200; 240]);
        let inc = with_intro(31.0, 126.0);
        let p = dj_plan(&out, Some(&inc), 240.0, 10.0, None).unwrap();
        assert_eq!(p.style, DjStyle::Drop);
        let bar = 4.0 * 60.0 / 126.0;
        let d = p.in_start_s.unwrap();
        assert!((d - 31.0).abs() < 1.5 * bar, "drop at {d}");
        // Build ends on the last bar before the music end.
        let out_bar = 4.0 * 60.0 / 128.0;
        let cut = p.start_s + p.secs as f64 * DjStyle::DROP_CUT as f64;
        assert!(238.0 - cut >= 0.0 && 238.0 - cut < out_bar + 1e-6, "cut at {cut}");
    }

    #[test]
    fn dj_blends_after_a_drop_when_the_beats_match() {
        let out = track(240.0, 238.0, Some(grid(128.0)), vec![200; 240]);
        let inc = with_intro(31.0, 126.0);
        let p = dj_plan(&out, Some(&inc), 240.0, 10.0, Some(DjStyle::Drop)).unwrap();
        assert_eq!(p.style, DjStyle::EqSwap);
    }

    #[test]
    fn dj_blends_eight_bars_when_tempos_match_and_there_is_no_drop() {
        let out = track(240.0, 238.0, Some(grid(128.0)), vec![200; 240]);
        let inc = TrackAnalysis { head: Some(grid(126.0)), ..track(240.0, 240.0, None, vec![200; 240]) };
        let p = dj_plan(&out, Some(&inc), 240.0, 10.0, None).unwrap();
        assert_eq!(p.style, DjStyle::EqSwap);
        let bar = 4.0 * 60.0 / 128.0;
        assert!((p.secs as f64 / bar - 8.0).abs() < 1e-3);
        let k = p.start_s / bar;
        assert!((k - k.round()).abs() < 1e-6);
        assert!(238.0 - (p.start_s + p.secs as f64) < bar);
    }

    #[test]
    fn dj_echoes_out_at_the_end_when_tempos_clash() {
        let out = track(240.0, 238.0, Some(grid(128.0)), vec![200; 240]);
        let inc = TrackAnalysis { head: Some(grid(90.0)), ..track(240.0, 240.0, None, vec![200; 240]) };
        let p = dj_plan(&out, Some(&inc), 240.0, 10.0, None).unwrap();
        assert_eq!(p.style, DjStyle::EchoOut);
        let cut = p.start_s + p.secs as f64 * DjStyle::ECHO_CUT as f64;
        assert!(238.0 - cut < 2.0, "cut at {cut}, far from the end");
    }

    #[test]
    fn dj_echo_out_skips_a_quiet_intro() {
        let out = track(240.0, 238.0, Some(grid(128.0)), vec![200; 240]);
        let mut wave = vec![200u8; 240];
        for w in wave.iter_mut().take(6) {
            *w = 10;
        }
        let inc = TrackAnalysis { head: Some(grid(90.0)), ..track(240.0, 240.0, None, wave) };
        let p = dj_plan(&out, Some(&inc), 240.0, 10.0, None).unwrap();
        assert_eq!(p.style, DjStyle::EchoOut);
        let at = p.in_start_s.unwrap();
        assert!(at > 3.0 && at <= 6.0, "in at {at}");
    }

    #[test]
    fn dj_mixes_out_where_a_quiet_outro_starts() {
        let mut wave = vec![220u8; 240];
        for w in wave.iter_mut().skip(210) {
            *w = 50;
        }
        let out = track(240.0, 240.0, Some(grid(124.0)), wave);
        let p = dj_plan(&out, None, 240.0, 10.0, None).unwrap();
        let cut = p.start_s + p.secs as f64 * DjStyle::ECHO_CUT as f64;
        assert!((cut - 210.0).abs() < 2.0, "cut at {cut}");
    }

    #[test]
    fn dj_without_a_beat_has_no_plan() {
        let out = track(200.0, 185.0, None, vec![200; 240]);
        assert!(dj_plan(&out, None, 200.0, 10.0, None).is_none());
    }
}

#[cfg(test)]
mod real_tracks {
    use super::*;
    use std::collections::HashMap;

    /// `TARN_ANALYSIS=~/.local/share/tarn/analysis.json cargo test --lib
    /// automix::real_tracks -- --ignored --nocapture` prints the move DJ mode
    /// picks for every pair of analysed songs.
    #[test]
    #[ignore]
    fn plans_for_analysed_songs() {
        let path = std::env::var("TARN_ANALYSIS").expect("TARN_ANALYSIS");
        let db: HashMap<String, TrackAnalysis> =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let ids: Vec<&String> = db.keys().collect();
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut prev = None;
        for (i, o) in ids.iter().enumerate() {
            let out = &db[*o];
            let inc_id = ids[(i + 1) % ids.len()];
            let inc = &db[inc_id];
            let p = dj_plan(out, Some(inc), out.duration_s as f64, 10.0, prev);
            prev = p.map(|p| p.style);
            let drop = find_drop(inc, inc.head.unwrap_or(Grid { bpm: 120.0, phase_s: 0.0, confidence: 0.0 }));
            let name = p.map(|p| format!("{:?}", p.style)).unwrap_or("Normal".into());
            *counts.entry(name.clone()).or_default() += 1;
            println!(
                "{o} {:>5.1}bpm end {:>5.1}/{:>5.1} -> {inc_id} {:>5.1}bpm drop {:?}: {name} {:?}",
                out.tail.map(|g| g.bpm).unwrap_or(0.0),
                out.music_end_s,
                out.duration_s,
                inc.head.map(|g| g.bpm).unwrap_or(0.0),
                drop.map(|d| (d * 10.0).round() / 10.0),
                p.map(|p| ((p.start_s * 10.0).round() / 10.0, p.secs)),
            );
        }
        println!("{counts:?}");
    }
}
