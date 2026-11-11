//! Audio buffers in the analysis domain.

use crate::time::{MediaTime, TimeRange};

/// Mono PCM audio, normalized to `[-1.0, 1.0]` by the decoder.
///
/// Analysis always operates on mono: models take mono input and cross-channel
/// stereo analysis is a separate (future) capability, not an implicit average.
#[derive(Clone, Debug)]
pub struct Audio {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

impl Audio {
    pub fn new(sample_rate: u32, samples: Vec<f32>) -> Self {
        Self {
            sample_rate,
            samples,
        }
    }

    pub fn silent(sample_rate: u32, duration: MediaTime) -> Self {
        let n = duration.to_sample(sample_rate);
        Self {
            sample_rate,
            samples: vec![0.0; n],
        }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn duration(&self) -> MediaTime {
        MediaTime::from_sample(self.samples.len(), self.sample_rate)
    }

    /// Samples covered by `range`, clamped to the buffer.
    pub fn samples_in(&self, range: TimeRange) -> &[f32] {
        let start = range.start.to_sample(self.sample_rate).min(self.len());
        let end = range.end.to_sample(self.sample_rate).min(self.len());
        if end <= start {
            &[]
        } else {
            &self.samples[start..end]
        }
    }

    pub fn peak(&self) -> f32 {
        self.samples.iter().fold(0.0_f32, |acc, s| acc.max(s.abs()))
    }

    pub fn rms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = self
            .samples
            .iter()
            .map(|&s| f64::from(s) * f64::from(s))
            .sum();
        (sum / self.samples.len() as f64).sqrt() as f32
    }

    /// Reject buffers that cannot be trusted downstream. A NaN or infinity in
    /// a decoded sample means the decode itself is suspect; better to fail the
    /// run than to emit features derived from it.
    pub fn validate(&self) -> Result<(), String> {
        if self.sample_rate == 0 {
            return Err("sample rate must be greater than zero".to_string());
        }
        if let Some(pos) = self.samples.iter().position(|s| !s.is_finite()) {
            return Err(format!("non-finite sample at index {pos}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_in_range_is_clamped() {
        let audio = Audio::new(1_000, vec![0.5; 1_000]);
        let range = TimeRange::from_secs(-1.0, 0.5);
        assert_eq!(audio.samples_in(range).len(), 500);
        assert_eq!(audio.samples_in(TimeRange::from_secs(2.0, 3.0)).len(), 0);
    }

    #[test]
    fn rejects_non_finite_samples() {
        let audio = Audio::new(16_000, vec![0.0, f32::NAN]);
        assert!(audio.validate().is_err());
    }
}
