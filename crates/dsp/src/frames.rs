//! Single-pass spectral framing.
//!
//! Long recordings are the normal case, so frames are yielded one at a time
//! instead of materialising a `frames x bins` spectrogram: an hour at 32 kHz
//! with 2048-point FFTs would otherwise cost about a gigabyte.

use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

use crate::error::DspError;
use crate::{amplitude_to_dbfs, hann_window};

#[derive(Debug, Clone, Copy)]
pub struct FrameConfig {
    pub fft_size: usize,
    pub hop_size: usize,
    /// Lowest frequency used for chroma aggregation.
    pub chroma_min_hz: f32,
    /// Highest frequency used for chroma aggregation.
    pub chroma_max_hz: f32,
    /// Fraction of energy below which the spectral rolloff is measured.
    pub rolloff_fraction: f32,
}

impl Default for FrameConfig {
    fn default() -> Self {
        Self {
            fft_size: 2048,
            hop_size: 512,
            chroma_min_hz: 55.0,
            chroma_max_hz: 5_000.0,
            rolloff_fraction: 0.85,
        }
    }
}

impl FrameConfig {
    pub fn validate(&self) -> Result<(), DspError> {
        if self.fft_size < 64 || !self.fft_size.is_power_of_two() {
            return Err(DspError::InvalidConfig(
                "fft_size must be a power of two >= 64".into(),
            ));
        }
        if self.hop_size == 0 || self.hop_size > self.fft_size {
            return Err(DspError::InvalidConfig(
                "hop_size must be in 1..=fft_size".into(),
            ));
        }
        Ok(())
    }

    pub fn frame_rate(&self, sample_rate: u32) -> f32 {
        sample_rate as f32 / self.hop_size as f32
    }
}

/// Per-frame spectral and temporal descriptors.
#[derive(Debug, Clone)]
pub struct Frame {
    pub index: u64,
    /// Frame centre on the buffer timeline, in seconds.
    pub time_s: f32,
    pub rms_dbfs: f32,
    /// Sum of squared magnitudes, useful as an energy gate.
    pub energy: f32,
    pub zero_crossing_rate: f32,
    pub centroid_hz: f32,
    pub rolloff_hz: f32,
    /// 1.0 for noise, near 0.0 for a pure tone.
    pub flatness: f32,
    /// Half-wave rectified spectral flux against the previous frame.
    pub flux: f32,
    /// Normalised chroma, loudest pitch class = 1.0.
    pub chroma: [f32; 12],
}

/// Iterator over frames of a buffer. Allocates a fixed set of buffers and
/// reuses them per frame.
pub struct FrameStream<'a> {
    samples: &'a [f32],
    sample_rate: u32,
    cfg: FrameConfig,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    time_buf: Vec<f32>,
    fft_buf: Vec<Complex<f32>>,
    prev_mag: Vec<f32>,
    mag: Vec<f32>,
    frame: u64,
    n_frames: u64,
}

impl std::fmt::Debug for FrameStream<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameStream")
            .field("sample_rate", &self.sample_rate)
            .field("fft_size", &self.cfg.fft_size)
            .field("hop_size", &self.cfg.hop_size)
            .field("n_frames", &self.n_frames)
            .finish()
    }
}

impl<'a> FrameStream<'a> {
    pub fn new(samples: &'a [f32], sample_rate: u32, cfg: FrameConfig) -> Result<Self, DspError> {
        cfg.validate()?;
        if sample_rate == 0 {
            return Err(DspError::InvalidSampleRate);
        }
        let bins = cfg.fft_size / 2 + 1;
        let n_frames = if samples.is_empty() {
            0
        } else {
            samples.len().div_ceil(cfg.hop_size) as u64
        };
        let fft = FftPlanner::new().plan_fft_forward(cfg.fft_size);
        Ok(Self {
            samples,
            sample_rate,
            cfg,
            fft,
            window: hann_window(cfg.fft_size),
            time_buf: vec![0.0; cfg.fft_size],
            fft_buf: vec![Complex::new(0.0, 0.0); cfg.fft_size],
            prev_mag: vec![0.0; bins],
            mag: vec![0.0; bins],
            frame: 0,
            n_frames,
        })
    }

    fn fill(&mut self) -> Option<Frame> {
        if self.frame >= self.n_frames {
            return None;
        }
        let idx = self.frame;
        self.frame += 1;

        let start = idx as usize * self.cfg.hop_size;
        let available = self.samples.len().saturating_sub(start);
        let take = available.min(self.cfg.fft_size);
        if take == 0 {
            return None;
        }

        self.time_buf[..take].copy_from_slice(&self.samples[start..start + take]);
        self.time_buf[take..].fill(0.0);

        // Temporal descriptors use the unpadded frame.
        let mut sum_sq = 0.0_f64;
        let mut crossings = 0usize;
        for i in 0..take {
            let s = self.time_buf[i];
            sum_sq += f64::from(s) * f64::from(s);
            if i > 0 && (self.time_buf[i - 1] >= 0.0) != (s >= 0.0) {
                crossings += 1;
            }
        }
        let rms = if take > 0 {
            (sum_sq / take as f64).sqrt() as f32
        } else {
            0.0
        };
        let zcr = if take > 1 {
            crossings as f32 / (take - 1) as f32
        } else {
            0.0
        };

        for i in 0..self.cfg.fft_size {
            self.fft_buf[i] = Complex::new(self.time_buf[i] * self.window[i], 0.0);
        }
        self.fft.process(&mut self.fft_buf);

        let mut total = 0.0_f32;
        let mut weighted = 0.0_f64;
        let mut energy = 0.0_f64;
        let mut log_sum = 0.0_f64;
        let mut flux = 0.0_f32;
        let mut max_mag = 0.0_f32;

        for (k, slot) in self.mag.iter_mut().enumerate() {
            let m = self.fft_buf[k].norm();
            *slot = m;
            total += m;
            weighted += f64::from(k as f32 * m);
            energy += f64::from(m) * f64::from(m);
            log_sum += f64::from((m + 1e-12).ln());
            max_mag = max_mag.max(m);

            let positive = m - self.prev_mag[k];
            if positive > 0.0 {
                flux += positive;
            }
        }

        // Chroma is built from spectral peaks, not every bin. Summing raw
        // magnitudes lets Hann sidelobes of strong partials leak into every
        // pitch class; picking local maxima keeps the chroma vector sparse and
        // makes key/chord templates meaningful.
        let mut chroma = [0.0_f32; 12];
        let chroma_floor = max_mag * 0.01;
        let mut chroma_total = 0.0_f32;
        for k in 2..self.mag.len().saturating_sub(1) {
            let m = self.mag[k];
            if m < chroma_floor || m < self.mag[k - 1] || m < self.mag[k + 1] {
                continue;
            }
            // Parabolic interpolation of the peak position.
            let (a, b, c) = (self.mag[k - 1], m, self.mag[k + 1]);
            let denom = a - 2.0 * b + c;
            let offset = if denom.abs() > f32::EPSILON {
                (0.5 * (a - c) / denom).clamp(-0.5, 0.5)
            } else {
                0.0
            };
            let freq = bin_to_hz(k, self.cfg.fft_size, self.sample_rate)
                + offset * self.sample_rate as f32 / self.cfg.fft_size as f32;
            if !(self.cfg.chroma_min_hz..=self.cfg.chroma_max_hz).contains(&freq) {
                continue;
            }
            let pitch = 69.0 + 12.0 * (freq / 440.0).log2();
            let class = (pitch.round() as i64).rem_euclid(12) as usize;
            let weight = m * m;
            chroma[class] += weight;
            chroma_total += weight;
        }
        if chroma_total > 0.0 {
            for c in &mut chroma {
                *c /= chroma_total;
            }
        }

        let bins = self.mag.len() as f32;
        let centroid_bin = if total > 0.0 {
            (weighted / f64::from(total)) as f32
        } else {
            0.0
        };
        let centroid_hz = centroid_bin * self.sample_rate as f32 / self.cfg.fft_size as f32;

        let threshold = total * self.cfg.rolloff_fraction;
        let mut cumulative = 0.0_f32;
        let mut rolloff_bin = self.mag.len() - 1;
        for (k, &m) in self.mag.iter().enumerate() {
            cumulative += m;
            if cumulative >= threshold {
                rolloff_bin = k;
                break;
            }
        }

        let arithmetic = f64::from(total / bins);
        let flatness = if arithmetic > 0.0 {
            ((log_sum / f64::from(bins)).exp() / arithmetic) as f32
        } else {
            0.0
        };

        let flux = flux / bins;

        self.prev_mag.copy_from_slice(&self.mag);

        let centre = start + self.cfg.fft_size / 2;
        Some(Frame {
            index: idx,
            time_s: centre as f32 / self.sample_rate as f32,
            rms_dbfs: amplitude_to_dbfs(rms),
            energy: energy as f32,
            zero_crossing_rate: zcr,
            centroid_hz,
            rolloff_hz: bin_to_hz(rolloff_bin, self.cfg.fft_size, self.sample_rate),
            flatness,
            flux,
            chroma,
        })
    }
}

impl Iterator for FrameStream<'_> {
    type Item = Frame;

    fn next(&mut self) -> Option<Self::Item> {
        self.fill()
    }
}

pub fn analyze_frames(
    samples: &[f32],
    sample_rate: u32,
    cfg: FrameConfig,
) -> Result<FrameStream<'_>, DspError> {
    FrameStream::new(samples, sample_rate, cfg)
}

fn bin_to_hz(bin: usize, fft_size: usize, sample_rate: u32) -> f32 {
    bin as f32 * sample_rate as f32 / fft_size as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, secs: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    #[test]
    fn tone_lands_in_expected_bin() {
        let samples = sine(440.0, 16_000, 1.0);
        let frames: Vec<Frame> = analyze_frames(&samples, 16_000, FrameConfig::default())
            .unwrap()
            .collect();
        assert!(!frames.is_empty());
        let middle = &frames[frames.len() / 2];
        assert!(
            (middle.centroid_hz - 440.0).abs() < 40.0,
            "centroid {} should be near 440",
            middle.centroid_hz
        );
        // A4 is pitch class 9 (A).
        let loudest = (0..12).max_by(|a, b| middle.chroma[*a].total_cmp(&middle.chroma[*b]));
        assert_eq!(loudest, Some(9));
    }

    #[test]
    fn silence_is_recognised_as_such() {
        let samples = vec![0.0_f32; 16_000];
        let frames: Vec<Frame> = analyze_frames(&samples, 16_000, FrameConfig::default())
            .unwrap()
            .collect();
        assert!(!frames.is_empty());
        assert!(frames.iter().all(|f| f.rms_dbfs == -120.0));
        assert!(frames.iter().all(|f| f.centroid_hz == 0.0));
    }

    #[test]
    fn rejects_bad_configuration() {
        let samples = vec![0.0_f32; 100];
        assert!(
            analyze_frames(
                &samples,
                16_000,
                FrameConfig {
                    fft_size: 1000,
                    ..FrameConfig::default()
                }
            )
            .is_err()
        );
    }
}
