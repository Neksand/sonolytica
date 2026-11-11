//! EBU R128 loudness via the `ebur128` crate (a faithful libebur128 port).

use ebur128::{EbuR128, Mode};

use crate::DspError;

#[derive(Debug, Clone, Copy)]
pub struct LoudnessMeasurement {
    /// Integrated loudness (gated, ITU-R BS.1770-4), LUFS.
    pub integrated_lufs: Option<f32>,
    /// Loudness range, LU.
    pub loudness_range_lu: Option<f32>,
    /// True peak (4x oversampled), dBTP.
    pub true_peak_dbtp: Option<f32>,
}

/// Measure integrated loudness, LRA and true peak for mono audio.
///
/// Returns `None` for measurements that cannot be computed (for example LRA on
/// very short material) instead of fabricating a number.
pub fn measure_loudness(
    samples: &[f32],
    sample_rate: u32,
) -> Result<LoudnessMeasurement, DspError> {
    if sample_rate == 0 {
        return Err(DspError::InvalidSampleRate);
    }
    if samples.is_empty() {
        return Ok(LoudnessMeasurement {
            integrated_lufs: None,
            loudness_range_lu: None,
            true_peak_dbtp: None,
        });
    }

    let mut meter = EbuR128::new(1, sample_rate, Mode::I | Mode::LRA | Mode::TRUE_PEAK)
        .map_err(|e| DspError::InvalidConfig(format!("ebur128 init failed: {e}")))?;

    meter
        .add_frames_f32(samples)
        .map_err(|e| DspError::InvalidConfig(format!("ebur128 ingest failed: {e}")))?;

    let integrated = meter.loudness_global().ok().map(|v| v as f32);
    let lra = meter.loudness_range().ok().map(|v| v as f32);
    let peak = meter.true_peak(0).ok().map(|v| v as f32);

    Ok(LoudnessMeasurement {
        integrated_lufs: integrated.filter(|v| v.is_finite()),
        loudness_range_lu: lra.filter(|v| v.is_finite()),
        true_peak_dbtp: peak.filter(|v| v.is_finite()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, rate: u32, secs: f32, amplitude: f32) -> Vec<f32> {
        let n = (rate as f32 * secs) as usize;
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / rate as f32).sin() * amplitude)
            .collect()
    }

    #[test]
    fn full_scale_sine_is_about_minus_three_lufs() {
        // A 0 dBFS 1 kHz sine measures -3.01 LUFS under BS.1770.
        let samples = sine(1_000.0, 48_000, 5.0, 1.0);
        let m = measure_loudness(&samples, 48_000).unwrap();
        let lufs = m.integrated_lufs.expect("measurable");
        assert!(
            (lufs + 3.01).abs() < 0.5,
            "expected ~-3.01 LUFS, got {lufs}"
        );
    }

    #[test]
    fn quieter_sine_is_quieter() {
        let loud = measure_loudness(&sine(1_000.0, 48_000, 5.0, 1.0), 48_000).unwrap();
        let quiet = measure_loudness(&sine(1_000.0, 48_000, 5.0, 0.1), 48_000).unwrap();
        assert!(
            quiet.integrated_lufs.unwrap() < loud.integrated_lufs.unwrap() - 15.0,
            "20 dB amplitude drop must show up in LUFS"
        );
    }

    #[test]
    fn empty_audio_yields_no_numbers() {
        let m = measure_loudness(&[], 48_000).unwrap();
        assert!(m.integrated_lufs.is_none());
    }
}
