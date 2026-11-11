//! Music information retrieval without models: onset, tempo, beat grid, key
//! and triadic chord estimation.
//!
//! These are classical, explainable algorithms with known limits. They are
//! reported with an explicit confidence score, and the docs state what the
//! score means: a heuristic strength, not a calibrated probability.
//!
//! * Tempo: autocorrelation of the onset envelope with a log-normal prior
//!   around 120 BPM, then dynamic-programming beat tracking (Ellis 2007).
//! * Key: Krumhansl-Kessler profiles correlated against the average chroma.
//! * Chords: cosine similarity against 24 major/minor triad templates, median
//!   filtered over time.

use crate::error::DspError;
use crate::frames::{Frame, FrameConfig, analyze_frames};
use crate::loudness::{LoudnessMeasurement, measure_loudness};

#[derive(Debug, Clone, Default)]
pub struct TempoEstimate {
    pub bpm: f32,
    /// Autocorrelation peak strength in `0..=1`. Heuristic, not calibrated.
    pub confidence: f32,
    pub beats_s: Vec<f32>,
}

#[derive(Debug, Clone, Copy)]
pub struct KeyEstimate {
    /// Pitch class index, C = 0.
    pub tonic: u8,
    pub major: bool,
    /// Margin over the runner-up profile, mapped to `0..=1`. Heuristic.
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChordQuality {
    Major,
    Minor,
}

#[derive(Debug, Clone)]
pub struct ChordEvent {
    pub start_s: f32,
    pub end_s: f32,
    /// Pitch class index, C = 0.
    pub root: u8,
    pub quality: ChordQuality,
    pub confidence: f32,
}

#[derive(Debug, Clone, Default)]
pub struct MusicFeatures {
    pub tempo: Option<TempoEstimate>,
    pub key: Option<KeyEstimate>,
    pub chords: Vec<ChordEvent>,
    pub loudness: Option<LoudnessMeasurement>,
    pub mean_centroid_hz: Option<f32>,
    pub frames: usize,
}

/// Autocorrelation tempo estimation plus DP beat tracking.
pub fn estimate_tempo(
    onset: &[f32],
    frame_rate: f32,
    min_bpm: f32,
    max_bpm: f32,
) -> Option<TempoEstimate> {
    if onset.len() < 8 || frame_rate <= 0.0 || min_bpm <= 0.0 || max_bpm <= min_bpm {
        return None;
    }

    let n = onset.len();
    let mean = onset.iter().map(|&v| f64::from(v)).sum::<f64>() / n as f64;
    let mut x: Vec<f64> = onset
        .iter()
        .map(|&v| (f64::from(v) - mean).max(0.0))
        .collect();
    let max = x.iter().copied().fold(0.0_f64, f64::max);
    if max <= f64::EPSILON {
        return None;
    }
    for v in &mut x {
        *v /= max;
    }

    let lag_min = ((frame_rate * 60.0 / max_bpm).floor() as usize).max(2);
    let lag_max = ((frame_rate * 60.0 / min_bpm).ceil() as usize).min(n / 2);
    if lag_max <= lag_min {
        return None;
    }

    let ac_zero: f64 = x.iter().map(|v| v * v).sum();
    let mut best = (0.0_f64, lag_min);
    let mut scores = Vec::with_capacity(lag_max - lag_min + 1);

    for lag in lag_min..=lag_max {
        let mut acc = 0.0_f64;
        for i in 0..n - lag {
            acc += x[i] * x[i + lag];
        }
        acc /= (n - lag) as f64;

        let bpm = 60.0 * f64::from(frame_rate) / lag as f64;
        // Prefer plausible tempi: one standard deviation is about 0.7 octave.
        let octaves = (bpm / 120.0).log2() / 0.7;
        let prior = (-0.5 * octaves * octaves).exp();
        let weighted = acc * prior;
        scores.push(weighted);
        if weighted > best.0 {
            best = (weighted, lag);
        }
    }

    let lag = best.1;
    let pos = lag - lag_min;
    let refined = {
        let at = |p: usize| scores.get(p).copied().unwrap_or(0.0);
        if pos > 0 && pos + 1 < scores.len() {
            let (a, b, c) = (at(pos - 1), at(pos), at(pos + 1));
            let denom = a - 2.0 * b + c;
            if denom.abs() > f64::EPSILON {
                let delta = 0.5 * (a - c) / denom;
                lag as f64 + delta.clamp(-0.5, 0.5)
            } else {
                lag as f64
            }
        } else {
            lag as f64
        }
    };

    let bpm = 60.0 * f64::from(frame_rate) / refined;
    let confidence = if ac_zero > f64::EPSILON {
        (best.0 / ac_zero).clamp(0.0, 1.0)
    } else {
        0.0
    };

    let beats_s = track_beats(onset, frame_rate, bpm as f32);

    Some(TempoEstimate {
        bpm: bpm as f32,
        confidence: confidence as f32,
        beats_s,
    })
}

/// Dynamic-programming beat tracker (Ellis 2007) over a normalised onset
/// envelope. `tightness` trades regularity against onset alignment.
fn track_beats(onset: &[f32], frame_rate: f32, bpm: f32) -> Vec<f32> {
    const TIGHTNESS: f32 = 8.0;
    let n = onset.len();
    if n == 0 || bpm <= 0.0 || frame_rate <= 0.0 {
        return Vec::new();
    }
    let period = frame_rate * 60.0 / bpm;
    if !period.is_finite() || period < 2.0 {
        return Vec::new();
    }

    let max_onset = onset.iter().copied().fold(0.0_f32, f32::max).max(1e-9);
    let norm: Vec<f32> = onset.iter().map(|&v| v / max_onset).collect();

    let mut score = vec![0.0_f32; n];
    let mut back = vec![0usize; n];

    for i in 0..n {
        let lo = i.saturating_sub((2.0 * period) as usize);
        let hi = i.saturating_sub((period * 0.5) as usize).max(lo);
        let mut best_score = 0.0_f32;
        let mut best_prev = i;
        for j in lo..=hi.min(i) {
            let delta = (i - j) as f32 / period;
            let candidate = score[j] - TIGHTNESS * delta.ln().powi(2);
            if candidate > best_score {
                best_score = candidate;
                best_prev = j;
            }
        }
        score[i] = norm[i] + best_score;
        back[i] = best_prev;
    }

    let mut idx = (0..n)
        .max_by(|&a, &b| score[a].total_cmp(&score[b]))
        .unwrap_or(0);
    let mut beats = vec![idx];
    while back[idx] != idx && beats.len() < n {
        idx = back[idx];
        beats.push(idx);
    }
    beats.reverse();
    beats.dedup();

    beats
        .into_iter()
        .map(|frame| frame as f32 / frame_rate)
        .collect()
}

const KRUMHANSL_MAJOR: [f32; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const KRUMHANSL_MINOR: [f32; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

fn pearson(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let n = 12.0_f32;
    let mean_a = a.iter().sum::<f32>() / n;
    let mean_b = b.iter().sum::<f32>() / n;
    let mut cov = 0.0;
    let mut var_a = 0.0;
    let mut var_b = 0.0;
    for i in 0..12 {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }
    let denom = (var_a * var_b).sqrt();
    if denom <= 1e-9 { 0.0 } else { cov / denom }
}

/// Estimate key from averaged chroma.
pub fn estimate_key(chroma_sum: &[f32; 12]) -> Option<KeyEstimate> {
    let total: f32 = chroma_sum.iter().sum();
    if total <= 1e-6 {
        return None;
    }
    let mut normalized = [0.0_f32; 12];
    for i in 0..12 {
        normalized[i] = chroma_sum[i] / total;
    }

    let mut ranked: Vec<(f32, u8, bool)> = Vec::with_capacity(24);
    for tonic in 0..12_usize {
        for (profile, major) in [(&KRUMHANSL_MAJOR, true), (&KRUMHANSL_MINOR, false)] {
            let mut rotated = [0.0_f32; 12];
            for i in 0..12 {
                rotated[i] = profile[(i + 12 - tonic) % 12];
            }
            ranked.push((pearson(&normalized, &rotated), tonic as u8, major));
        }
    }
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0));

    let best = ranked.first()?;
    let second = ranked.get(1).map_or(0.0, |r| r.0);
    if best.0 <= 0.0 {
        return None;
    }
    let confidence = ((best.0 - second) / 0.15).clamp(0.0, 1.0);
    Some(KeyEstimate {
        tonic: best.1,
        major: best.2,
        confidence,
    })
}

fn chord_template(root: usize, quality: ChordQuality) -> [f32; 12] {
    let mut t = [0.0_f32; 12];
    t[root] = 1.0;
    let third = if quality == ChordQuality::Major { 4 } else { 3 };
    t[(root + third) % 12] = 1.0;
    t[(root + 7) % 12] = 1.0;
    t
}

fn cosine(a: &[f32; 12], b: &[f32; 12]) -> f32 {
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for i in 0..12 {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = (na * nb).sqrt();
    if denom <= 1e-9 { 0.0 } else { dot / denom }
}

/// Estimate a chord sequence from frame chroma. `min_duration_s` removes
/// short-lived flicker that would otherwise dominate the output.
pub fn estimate_chords(
    frames: &[&Frame],
    min_duration_s: f32,
    silence_dbfs: f32,
) -> Vec<ChordEvent> {
    if frames.is_empty() {
        return Vec::new();
    }
    // label: -1 = no chord, otherwise root * 2 + (minor as usize)
    let mut labels: Vec<i8> = Vec::with_capacity(frames.len());
    let mut scores: Vec<f32> = Vec::with_capacity(frames.len());

    for frame in frames {
        if frame.rms_dbfs <= silence_dbfs {
            labels.push(-1);
            scores.push(0.0);
            continue;
        }
        let mut best = (-1_i8, 0.0_f32);
        for root in 0..12_usize {
            for (quality, minor) in [(ChordQuality::Major, false), (ChordQuality::Minor, true)] {
                let t = chord_template(root, quality);
                let score = cosine(&frame.chroma, &t);
                if score > best.1 {
                    best = ((root * 2 + usize::from(minor)) as i8, score);
                }
            }
        }
        if best.1 < 0.5 {
            labels.push(-1);
            scores.push(best.1);
        } else {
            labels.push(best.0);
            scores.push(best.1);
        }
    }

    let labels = median_filter(&labels, 9);
    let frame_dt = if frames.len() > 1 {
        (frames[frames.len() - 1].time_s - frames[0].time_s) / (frames.len() - 1) as f32
    } else {
        0.02
    };

    let mut events: Vec<ChordEvent> = Vec::new();
    let mut run_start = 0usize;
    for i in 1..=labels.len() {
        let boundary = i == labels.len() || labels[i] != labels[run_start];
        if boundary {
            let label = labels[run_start];
            let start_s = frames[run_start].time_s - frame_dt / 2.0;
            let end_s = frames[i - 1].time_s + frame_dt / 2.0;
            if label >= 0 && end_s - start_s >= min_duration_s {
                let root = (label / 2) as usize;
                let quality = if label % 2 == 0 {
                    ChordQuality::Major
                } else {
                    ChordQuality::Minor
                };
                let mean_score = scores[run_start..i].iter().sum::<f32>() / (i - run_start) as f32;
                events.push(ChordEvent {
                    start_s: start_s.max(0.0),
                    end_s,
                    root: root as u8,
                    quality,
                    confidence: mean_score.clamp(0.0, 1.0),
                });
            }
            run_start = i;
        }
    }
    events
}

fn median_filter(labels: &[i8], window: usize) -> Vec<i8> {
    let mut out = labels.to_vec();
    if window < 3 || labels.len() < window {
        return out;
    }
    let half = window / 2;
    let mut scratch = Vec::with_capacity(window);
    for i in 0..labels.len() {
        let lo = i.saturating_sub(half);
        let hi = (i + half + 1).min(labels.len());
        scratch.clear();
        scratch.extend_from_slice(&labels[lo..hi]);
        scratch.sort_unstable();
        out[i] = scratch[scratch.len() / 2];
    }
    out
}

/// One pass over the audio: frames, tempo, key, chords and loudness.
pub fn analyze_music(
    samples: &[f32],
    sample_rate: u32,
    cfg: FrameConfig,
) -> Result<MusicFeatures, DspError> {
    if samples.is_empty() {
        return Ok(MusicFeatures::default());
    }
    let frame_rate = cfg.frame_rate(sample_rate);

    let mut onsets = Vec::new();
    let mut chroma_sum = [0.0_f32; 12];
    let mut frames: Vec<Frame> = Vec::new();
    let mut centroid_sum = 0.0_f64;

    for frame in analyze_frames(samples, sample_rate, cfg)? {
        onsets.push(frame.flux);
        if frame.rms_dbfs > -60.0 {
            for (i, c) in frame.chroma.iter().enumerate() {
                chroma_sum[i] += c;
            }
        }
        centroid_sum += f64::from(frame.centroid_hz);
        frames.push(frame);
    }

    let tempo = estimate_tempo(&onsets, frame_rate, 40.0, 240.0);
    let key = estimate_key(&chroma_sum);
    let chord_frames: Vec<&Frame> = frames.iter().collect();
    let chords = estimate_chords(&chord_frames, 0.3, -55.0);
    let loudness = measure_loudness(samples, sample_rate).ok();
    let mean_centroid_hz = if frames.is_empty() {
        None
    } else {
        Some((centroid_sum / frames.len() as f64) as f32)
    };

    Ok(MusicFeatures {
        tempo,
        key,
        chords,
        loudness,
        mean_centroid_hz,
        frames: frames.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click_track(bpm: f32, secs: f32, rate: u32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        let mut out = vec![0.0_f32; n];
        let period = (rate as f32 * 60.0 / bpm) as usize;
        let mut pos = 0;
        while pos < n {
            // Short decaying burst so spectral flux sees a clear onset.
            for i in 0..(rate as usize / 200) {
                if pos + i < n {
                    let env = (-(i as f32) / 40.0).exp();
                    out[pos + i] = 0.9 * env;
                }
            }
            pos += period;
        }
        out
    }

    fn chord(root_hz: f32, secs: f32, rate: u32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        let third = 2.0_f32.powf(4.0 / 12.0);
        let fifth = 2.0_f32.powf(7.0 / 12.0);
        (0..n)
            .map(|i| {
                let t = i as f32 / rate as f32;
                let s = (2.0 * std::f32::consts::PI * root_hz * t).sin()
                    + (2.0 * std::f32::consts::PI * root_hz * third * t).sin()
                    + (2.0 * std::f32::consts::PI * root_hz * fifth * t).sin();
                s * 0.2
            })
            .collect()
    }

    #[test]
    fn tempo_of_a_click_track_is_recovered() {
        let samples = click_track(120.0, 12.0, 32_000);
        let cfg = FrameConfig {
            fft_size: 1024,
            hop_size: 256,
            ..FrameConfig::default()
        };
        let onsets: Vec<f32> = analyze_frames(&samples, 32_000, cfg)
            .unwrap()
            .map(|f| f.flux)
            .collect();
        let tempo = estimate_tempo(&onsets, cfg.frame_rate(32_000), 40.0, 240.0)
            .expect("tempo from a clear click track");
        assert!(
            (tempo.bpm - 120.0).abs() < 4.0,
            "expected ~120 BPM, got {}",
            tempo.bpm
        );
        assert!(tempo.beats_s.len() >= 8, "should find most beats");
    }

    #[test]
    fn detects_c_major() {
        let samples = chord(261.63, 4.0, 32_000);
        let features = analyze_music(&samples, 32_000, FrameConfig::default()).unwrap();
        let key = features.key.expect("a clear triad must yield a key");
        assert_eq!(key.tonic, 0, "C should be the tonic");
        assert!(key.major, "C major, not C minor");
    }

    #[test]
    fn follows_a_chord_change() {
        let mut samples = chord(261.63, 3.0, 32_000); // C major
        samples.extend(chord(392.00, 3.0, 32_000)); // G major
        let features = analyze_music(&samples, 32_000, FrameConfig::default()).unwrap();
        let roots: Vec<u8> = features.chords.iter().map(|c| c.root).collect();
        assert!(roots.contains(&0), "expected a C chord, got {roots:?}");
        assert!(roots.contains(&7), "expected a G chord, got {roots:?}");
        let c_pos = roots.iter().position(|&r| r == 0).unwrap();
        let g_pos = roots.iter().position(|&r| r == 7).unwrap();
        assert!(c_pos <= g_pos, "C should precede G: {roots:?}");
    }

    #[test]
    fn silence_has_no_key_and_no_chords() {
        let features =
            analyze_music(&vec![0.0_f32; 32_000], 32_000, FrameConfig::default()).unwrap();
        assert!(features.key.is_none());
        assert!(features.chords.is_empty());
        assert!(features.tempo.is_none());
    }
}
