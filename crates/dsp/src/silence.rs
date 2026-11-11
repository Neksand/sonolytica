//! Silence and broad acoustic characterisation.
//!
//! Silence is measured on short-term RMS relative to an absolute dBFS
//! threshold. That is a deliberate, explainable rule: an agent asking "where
//! are the gaps" gets the same answer on every machine, and a quiet recording
//! is reported as quiet rather than being normalised into false activity.

use crate::error::DspError;
use crate::{amplitude_to_dbfs, loudness::measure_loudness};

const FRAME_MS: u32 = 20;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SilenceRegion {
    pub start_s: f32,
    pub end_s: f32,
}

#[derive(Debug, Clone, Default)]
pub struct AcousticFeatures {
    pub silence: Vec<SilenceRegion>,
    pub mean_rms_dbfs: Option<f32>,
    pub peak_dbfs: Option<f32>,
    pub loudness_range_lu: Option<f32>,
    pub integrated_lufs: Option<f32>,
}

/// Detect silence regions lasting at least `min_silence_s`.
pub fn detect_silence(
    samples: &[f32],
    sample_rate: u32,
    threshold_dbfs: f32,
    min_silence_s: f32,
) -> Result<Vec<SilenceRegion>, DspError> {
    if sample_rate == 0 {
        return Err(DspError::InvalidSampleRate);
    }
    if samples.is_empty() {
        return Ok(Vec::new());
    }

    let frame_len = (sample_rate * FRAME_MS / 1_000).max(1) as usize;
    let mut regions = Vec::new();
    let mut run_start: Option<usize> = None;

    for (idx, chunk) in samples.chunks(frame_len).enumerate() {
        let sum_sq: f64 = chunk.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
        let rms = (sum_sq / chunk.len() as f64).sqrt() as f32;
        let silent = amplitude_to_dbfs(rms) < threshold_dbfs;
        if silent {
            run_start.get_or_insert(idx);
        } else if let Some(start) = run_start.take() {
            let start_s = start as f32 * FRAME_MS as f32 / 1_000.0;
            let end_s = idx as f32 * FRAME_MS as f32 / 1_000.0;
            if end_s - start_s >= min_silence_s {
                regions.push(SilenceRegion { start_s, end_s });
            }
        }
    }
    if let Some(start) = run_start {
        let start_s = start as f32 * FRAME_MS as f32 / 1_000.0;
        let end_s = samples.len() as f32 / sample_rate as f32;
        if end_s - start_s >= min_silence_s {
            regions.push(SilenceRegion { start_s, end_s });
        }
    }

    Ok(regions)
}

/// Silence plus level statistics for the acoustic analysis block.
pub fn describe_acoustics(
    samples: &[f32],
    sample_rate: u32,
    silence_dbfs: f32,
    min_silence_s: f32,
) -> Result<AcousticFeatures, DspError> {
    if samples.is_empty() {
        return Ok(AcousticFeatures::default());
    }
    let silence = detect_silence(samples, sample_rate, silence_dbfs, min_silence_s)?;

    let mut sum_sq = 0.0_f64;
    let mut peak = 0.0_f32;
    for &s in samples {
        sum_sq += f64::from(s) * f64::from(s);
        peak = peak.max(s.abs());
    }
    let mean_rms = (sum_sq / samples.len() as f64).sqrt() as f32;
    let loud = measure_loudness(samples, sample_rate)?;

    Ok(AcousticFeatures {
        silence,
        mean_rms_dbfs: Some(amplitude_to_dbfs(mean_rms)),
        peak_dbfs: Some(amplitude_to_dbfs(peak)),
        loudness_range_lu: loud.loudness_range_lu,
        integrated_lufs: loud.integrated_lufs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(secs: f32, rate: u32, amp: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin() * amp)
            .collect()
    }

    #[test]
    fn finds_gaps_around_a_tone() {
        let mut samples = vec![0.0_f32; 16_000];
        samples.extend(tone(1.0, 16_000, 0.5));
        samples.extend(vec![0.0_f32; 16_000]);

        let regions = detect_silence(&samples, 16_000, -45.0, 0.5).unwrap();
        assert_eq!(regions.len(), 2, "expected a gap before and after");
        assert!((regions[0].start_s - 0.0).abs() < 0.05);
        assert!((regions[0].end_s - 1.0).abs() < 0.05);
        assert!((regions[1].start_s - 2.0).abs() < 0.05);
        assert!((regions[1].end_s - 3.0).abs() < 0.05);
    }

    #[test]
    fn short_gaps_are_ignored() {
        let mut samples = tone(0.2, 16_000, 0.5);
        samples.extend(vec![0.0_f32; 1_600]);
        samples.extend(tone(0.2, 16_000, 0.5));
        let regions = detect_silence(&samples, 16_000, -45.0, 0.5).unwrap();
        assert!(regions.is_empty());
    }

    #[test]
    fn reports_levels() {
        let features = describe_acoustics(&tone(2.0, 16_000, 0.5), 16_000, -45.0, 0.5).unwrap();
        let mean = features.mean_rms_dbfs.unwrap();
        assert!((mean + 9.03).abs() < 1.0, "half-scale sine is ~-9 dBFS");
    }
}
