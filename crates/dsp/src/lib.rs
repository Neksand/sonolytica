//! Signal processing for SONOLYTICA.
//!
//! Deliberately model-free: everything here is deterministic DSP that runs in
//! bounded memory over long recordings. Model-driven understanding lives in
//! `sonolytica-providers`.
//!
//! All functions work on mono `f32` samples at a caller-declared rate and
//! return seconds relative to the start of the buffer. Mapping back to the
//! media timeline is the engine's job.

#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
// Index arithmetic is the clearest expression of these DSP loops.
#![allow(clippy::needless_range_loop)]

pub mod error;
pub mod frames;
pub mod loudness;
pub mod music;
pub mod silence;

pub use error::DspError;
pub use frames::{Frame, FrameConfig, FrameStream, analyze_frames};
pub use loudness::{LoudnessMeasurement, measure_loudness};
pub use music::{
    ChordEvent, KeyEstimate, MusicFeatures, TempoEstimate, analyze_music, estimate_chords,
    estimate_key, estimate_tempo,
};
pub use silence::{AcousticFeatures, SilenceRegion, describe_acoustics, detect_silence};

/// Window function: periodic Hann, the standard for overlap-add STFT.
pub fn hann_window(size: usize) -> Vec<f32> {
    if size == 0 {
        return Vec::new();
    }
    (0..size)
        .map(|i| {
            let phase = 2.0 * std::f32::consts::PI * i as f32 / size as f32;
            0.5 - 0.5 * phase.cos()
        })
        .collect()
}

/// `20 * log10(value)` with a floor at -120 dBFS instead of -inf.
pub fn amplitude_to_dbfs(amplitude: f32) -> f32 {
    if amplitude <= 0.0 {
        -120.0
    } else {
        (20.0 * amplitude.log10()).max(-120.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_window_has_expected_shape() {
        let w = hann_window(8);
        assert_eq!(w.len(), 8);
        assert!(w[0].abs() < 1e-6);
        assert!((w[4] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn dbfs_floors_instead_of_infinity() {
        assert_eq!(amplitude_to_dbfs(0.0), -120.0);
        assert!((amplitude_to_dbfs(1.0)).abs() < 1e-6);
        assert!((amplitude_to_dbfs(0.5) + 6.0206).abs() < 1e-3);
    }
}
