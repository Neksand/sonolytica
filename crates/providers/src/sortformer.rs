//! NVIDIA Sortformer 4-speaker diarization through ONNX Runtime.
//!
//! The model is an offline export of `nvidia/diar_streaming_sortformer_4spk-v2`
//! that takes a 128-bin log-mel spectrogram and returns per-frame speaker
//! activity at 80 ms. Unlike the pyannote path it resolves overlap: two
//! speakers can be active in the same frame, and both turns are emitted.
//!
//! Everything here is deliberately explicit:
//!
//! 1. NeMo log-mel: pre-emphasis 0.97, 25 ms Hann window, 10 ms hop, 512-point
//!    power spectrum, 128 Slaney mel filters, `ln(x + 2^-24)`, no normalisation.
//! 2. Audio is processed in at most 300 s windows with 10 s overlap. The model
//!    has full attention, so an hour-long file cannot fit in one forward pass.
//! 3. Speaker channels are matched across windows by activity co-occurrence in
//!    the overlap, then compacted to `S1..Sn` by first appearance.
//! 4. A probability above 0.5 starts a turn; runs shorter than 240 ms are
//!    dropped as threshold flicker.
//!
//! The model tracks at most four speakers. Beyond that it collapses voices;
//! `docs/MODELS.md` and the provider notes record that limit.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ort::session::Session;
use ort::value::Tensor;
use rustfft::FftPlanner;
use rustfft::num_complex::Complex;
use sonolytica_core::{
    Audio, Capability, Confidence, DiarizationConfig, MediaTime, SpeakerId, TimeRange,
};

use crate::models::ModelSpec;
use crate::traits::{
    DiarTurn, DiarizationOutput, DiarizationProvider, ProviderError, ProviderInfo, ProviderResult,
};

const RATE: usize = 16_000;
const N_FFT: usize = 512;
const WIN: usize = 400;
const HOP: usize = 160;
const N_MELS: usize = 128;
const N_BINS: usize = N_FFT / 2 + 1;
const PREEMPH: f32 = 0.97;
const LOG_GUARD: f32 = 5.960_464_5e-8; // 2^-24
const FRAME_S: f32 = 0.08;
const NUM_SPEAKERS: usize = 4;
const THRESHOLD: f32 = 0.5;
const MIN_TURN_S: f32 = 0.24;
const STITCH_GAP_S: f32 = 0.2;
/// Longest forward pass. 300 s peaks around 3 GB of ONNX Runtime memory on a
/// 4-minute file; a full meeting would exhaust RAM, so windows are mandatory.
///
/// ponytail: fixed windows plus activity matching; the model's native
/// streaming cache (AOSC) is the upgrade if long meetings become a target.
const MAX_CHUNK_S: usize = 300;
const CHUNK_OVERLAP_S: usize = 10;
const OVERLAP_FRAMES: usize = CHUNK_OVERLAP_S * 1000 / 80;

struct MelFrontend {
    window: Vec<f32>,
    filters: Vec<f32>, // N_MELS * N_BINS, row-major by mel
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
}

impl MelFrontend {
    fn new() -> Self {
        let mut window = vec![0.0_f32; WIN];
        for (i, value) in window.iter_mut().enumerate() {
            // Symmetric Hann, like NeMo's `torch.hann_window(periodic=False)`.
            *value = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (WIN - 1) as f32).cos();
        }

        let mel_points: Vec<f64> = (0..=N_MELS + 1)
            .map(|i| {
                let mel = mel_from_hz(0.0)
                    + (mel_from_hz(8_000.0) - mel_from_hz(0.0)) * i as f64 / (N_MELS + 1) as f64;
                hz_from_mel(mel)
            })
            .collect();
        let freqs: Vec<f64> = (0..N_BINS)
            .map(|i| i as f64 * 8_000.0 / (N_BINS - 1) as f64)
            .collect();

        let mut filters = vec![0.0_f32; N_MELS * N_BINS];
        for mel in 0..N_MELS {
            let left = mel_points[mel];
            let centre = mel_points[mel + 1];
            let right = mel_points[mel + 2];
            let norm = 2.0 / (right - left);
            for (bin, freq) in freqs.iter().enumerate() {
                let lower = (freq - left) / (centre - left);
                let upper = (right - freq) / (right - centre);
                let weight = lower.min(upper).max(0.0) * norm;
                filters[mel * N_BINS + bin] = weight as f32;
            }
        }

        let fft = FftPlanner::<f32>::new().plan_fft_forward(N_FFT);
        Self {
            window,
            filters,
            fft,
        }
    }

    /// Log-mel for one chunk, time-major (`frames * N_MELS`).
    fn compute(&self, samples: &[f32]) -> Vec<f32> {
        let frames = samples.len() / HOP;
        if frames == 0 {
            return Vec::new();
        }
        let mut padded = vec![0.0_f32; samples.len() + N_FFT];
        padded[N_FFT / 2..N_FFT / 2 + samples.len()].copy_from_slice(samples);
        for index in 1..samples.len() {
            padded[N_FFT / 2 + index] = samples[index] - PREEMPH * samples[index - 1];
        }
        padded[N_FFT / 2] = samples[0];

        let mut spectrum = vec![Complex::new(0.0_f32, 0.0); N_FFT];
        let mut mel = Vec::with_capacity(frames * N_MELS);
        let offset = (N_FFT - WIN) / 2;

        for frame in 0..frames {
            let start = frame * HOP + offset;
            for (i, value) in spectrum.iter_mut().enumerate() {
                *value = if i < WIN {
                    Complex::new(padded[start + i] * self.window[i], 0.0)
                } else {
                    Complex::new(0.0, 0.0)
                };
            }
            self.fft.process(&mut spectrum);

            let mut power = [0.0_f32; N_BINS];
            for (bin, value) in spectrum.iter().take(N_BINS).enumerate() {
                power[bin] = value.re * value.re + value.im * value.im;
            }
            for mel_index in 0..N_MELS {
                let weights = &self.filters[mel_index * N_BINS..(mel_index + 1) * N_BINS];
                let energy: f32 = weights
                    .iter()
                    .zip(power.iter())
                    .map(|(weight, value)| weight * value)
                    .sum();
                mel.push((energy + LOG_GUARD).ln());
            }
        }
        mel
    }
}

fn mel_from_hz(hz: f64) -> f64 {
    if hz < 1_000.0 {
        hz / (200.0 / 3.0)
    } else {
        15.0 + (hz / 1_000.0).ln() / (6.4_f64.ln() / 27.0)
    }
}

fn hz_from_mel(mel: f64) -> f64 {
    if mel < 15.0 {
        mel * (200.0 / 3.0)
    } else {
        1_000.0 * ((mel - 15.0) * (6.4_f64.ln() / 27.0)).exp()
    }
}

pub struct SortformerDiarizer {
    session: Mutex<Option<Session>>,
    path: PathBuf,
    threads: usize,
    mel: MelFrontend,
    info: ProviderInfo,
}

impl SortformerDiarizer {
    pub fn load(spec: &'static ModelSpec, path: &Path, threads: usize) -> ProviderResult<Self> {
        Ok(Self {
            // Loading a 469 MB graph at discovery would allocate half a
            // gigabyte for `inspect` and music-only runs. First diarization
            // pays for it; a load failure is reported then and the engine
            // falls back to pyannote.
            session: Mutex::new(None),
            path: path.to_path_buf(),
            threads,
            mel: MelFrontend::new(),
            info: ProviderInfo {
                name: "sortformer-4spk-v2",
                capability: Capability::Diarize,
                device: "cpu",
                models: vec![spec.reference()],
                notes: "Up to 4 speakers. Overlapping speech is returned as two simultaneous \
                        turns instead of a flag. Audio is processed in 300 s windows with 10 s \
                        overlap; 5+ speaker recordings are collapsed into four voices.",
            },
        })
    }

    /// Per-frame speaker probabilities for one window: `[frames][4]`, 80 ms each.
    fn window(&self, samples: &[f32]) -> ProviderResult<Vec<[f32; NUM_SPEAKERS]>> {
        let mel = self.mel.compute(samples);
        let frames = mel.len() / N_MELS;
        if frames == 0 {
            return Ok(Vec::new());
        }

        // ONNX expects [1, 128, T] bin-major; `mel` is time-major.
        let mut transposed = vec![0.0_f32; frames * N_MELS];
        for frame in 0..frames {
            for bin in 0..N_MELS {
                transposed[bin * frames + frame] = mel[frame * N_MELS + bin];
            }
        }
        let input = Tensor::from_array((vec![1_i64, N_MELS as i64, frames as i64], transposed))
            .map_err(|e| ProviderError::Inference(e.to_string()))?;

        let mut guard = self
            .session
            .lock()
            .map_err(|_| ProviderError::Inference("sortformer session poisoned".into()))?;
        if guard.is_none() {
            *guard = Some(crate::onnx::build_session(&self.path, self.threads)?);
        }
        let session = guard
            .as_mut()
            .ok_or_else(|| ProviderError::Inference("sortformer session missing".into()))?;
        let outputs = session
            .run(ort::inputs!["processed_signal" => input])
            .map_err(|e| ProviderError::Inference(format!("sortformer run failed: {e}")))?;
        let (shape, data) = outputs
            .get("speaker_logits")
            .ok_or_else(|| ProviderError::Inference("sortformer output missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| ProviderError::Inference(e.to_string()))?;

        let dims: Vec<usize> = shape.iter().map(|d| *d as usize).collect();
        if dims.len() != 3 || dims[2] != NUM_SPEAKERS {
            return Err(ProviderError::Inference(format!(
                "unexpected sortformer output shape {dims:?}"
            )));
        }
        let frames = dims[1];
        let mut probs = Vec::with_capacity(frames);
        for frame in 0..frames {
            let row = &data[frame * NUM_SPEAKERS..(frame + 1) * NUM_SPEAKERS];
            let mut values = [0.0_f32; NUM_SPEAKERS];
            values.copy_from_slice(row);
            probs.push(values);
        }
        Ok(probs)
    }
}

/// Map one window's four channels onto already known global speaker labels.
///
/// `previous` holds the previous window's last frames and their global labels;
/// `current` holds this window's first frames. A channel is matched by
/// co-occurrence in the overlap, then unmatched channels take the first free
/// label (the model cannot invent a fifth speaker).
fn match_labels(
    previous: &[[f32; NUM_SPEAKERS]],
    previous_labels: &[Option<usize>],
    current: &[[f32; NUM_SPEAKERS]],
    next_label: usize,
) -> (Vec<Option<usize>>, usize) {
    let frames = previous.len().min(current.len());
    let mut cooc = [[0.0_f64; NUM_SPEAKERS]; NUM_SPEAKERS];
    for frame in 0..frames {
        for (prev_channel, prev_values) in previous[frame].iter().enumerate() {
            if previous_labels[prev_channel].is_none() {
                continue;
            }
            for (cur_channel, cur_value) in current[frame].iter().enumerate() {
                cooc[prev_channel][cur_channel] += f64::from(*prev_values) * f64::from(*cur_value);
            }
        }
    }

    // Four channels: score every permutation instead of pulling in an
    // assignment solver for a 4x4 matrix.
    let mut best = [0usize, 1, 2, 3];
    let mut best_score = f64::NEG_INFINITY;
    for a in 0..NUM_SPEAKERS {
        for b in 0..NUM_SPEAKERS {
            if b == a {
                continue;
            }
            for c in 0..NUM_SPEAKERS {
                if c == a || c == b {
                    continue;
                }
                let d = 6 - a - b - c;
                let permutation = [a, b, c, d];
                let score: f64 = (0..NUM_SPEAKERS)
                    .map(|channel| cooc[permutation[channel]][channel])
                    .sum();
                if score > best_score {
                    best_score = score;
                    best = permutation;
                }
            }
        }
    }

    let mut labels = [None; NUM_SPEAKERS];
    let mut used = [false; NUM_SPEAKERS];
    let mut next = next_label;
    for channel in 0..NUM_SPEAKERS {
        let source = best[channel];
        if cooc[source][channel] <= 0.0 {
            continue;
        }
        if let Some(label) = previous_labels[source] {
            if label < NUM_SPEAKERS && !used[label] {
                labels[channel] = Some(label);
                used[label] = true;
            }
        }
    }
    for channel in 0..NUM_SPEAKERS {
        if labels[channel].is_some() {
            continue;
        }
        let active = current.iter().any(|frame| frame[channel] > THRESHOLD);
        if !active {
            continue;
        }
        if next < NUM_SPEAKERS {
            labels[channel] = Some(next);
            used[next] = true;
            next += 1;
        } else if let Some((source, _)) = (0..NUM_SPEAKERS)
            .filter_map(|source| {
                previous_labels[source].map(|_label| (source, cooc[source][channel]))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
        {
            labels[channel] = previous_labels[source];
        }
    }
    (labels.to_vec(), next.max(next_label))
}

fn first_activations(probs: &[[f32; NUM_SPEAKERS]]) -> Vec<Option<usize>> {
    let mut order: Vec<(usize, usize)> = Vec::new(); // (first frame, channel)
    for channel in 0..NUM_SPEAKERS {
        if let Some(frame) = probs.iter().position(|row| row[channel] > THRESHOLD) {
            order.push((frame, channel));
        }
    }
    order.sort_unstable();
    let mut labels = vec![None; NUM_SPEAKERS];
    for (label, (_, channel)) in order.into_iter().enumerate() {
        labels[channel] = Some(label);
    }
    labels
}

fn turns_from_window(
    probs: &[[f32; NUM_SPEAKERS]],
    labels: &[Option<usize>],
    window_start_s: f32,
    duration_s: f32,
) -> (Vec<DiarTurn>, bool) {
    let mut turns = Vec::new();
    let mut overlap = false;

    for frame in probs {
        if frame.iter().filter(|value| **value > THRESHOLD).count() > 1 {
            overlap = true;
        }
    }

    for channel in 0..NUM_SPEAKERS {
        let Some(label) = labels[channel] else {
            continue;
        };
        let mut run: Option<(usize, f32, usize)> = None; // start, prob sum, frames
        for (index, frame) in probs.iter().enumerate() {
            let active = frame[channel] > THRESHOLD;
            match &mut run {
                Some((_, sum, count)) if active => {
                    *sum += frame[channel];
                    *count += 1;
                }
                Some((start, sum, count)) => {
                    let length_s = *count as f32 * FRAME_S;
                    if length_s >= MIN_TURN_S {
                        let begin = window_start_s + *start as f32 * FRAME_S;
                        let end = (begin + length_s).min(duration_s);
                        turns.push(DiarTurn {
                            range: TimeRange::new(
                                MediaTime::from_secs_f64(f64::from(begin)),
                                MediaTime::from_secs_f64(f64::from(end)),
                            ),
                            speaker: SpeakerId::new(format!("S{}", label + 1)),
                            confidence: Confidence::new((*sum / *count as f32).clamp(0.0, 1.0)),
                        });
                    }
                    run = active.then_some((index, frame[channel], 1));
                }
                None if active => run = Some((index, frame[channel], 1)),
                None => {}
            }
        }
        if let Some((start, sum, count)) = run {
            let length_s = count as f32 * FRAME_S;
            if length_s >= MIN_TURN_S {
                let begin = window_start_s + start as f32 * FRAME_S;
                let end = (begin + length_s).min(duration_s);
                turns.push(DiarTurn {
                    range: TimeRange::new(
                        MediaTime::from_secs_f64(f64::from(begin)),
                        MediaTime::from_secs_f64(f64::from(end)),
                    ),
                    speaker: SpeakerId::new(format!("S{}", label + 1)),
                    confidence: Confidence::new((sum / count as f32).clamp(0.0, 1.0)),
                });
            }
        }
    }
    (turns, overlap)
}

fn stitch(mut turns: Vec<DiarTurn>) -> Vec<DiarTurn> {
    turns.sort_by_key(|turn| (turn.range.start, turn.range.end));
    let mut merged: Vec<DiarTurn> = Vec::with_capacity(turns.len());
    for turn in turns {
        if let Some(previous) = merged.last_mut() {
            if previous.speaker == turn.speaker {
                let gap = turn.range.start.saturating_sub(previous.range.end);
                if gap <= MediaTime::from_secs_f64(f64::from(STITCH_GAP_S)) {
                    previous.range = previous.range.union(&turn.range);
                    previous.confidence = match (previous.confidence, turn.confidence) {
                        (Some(a), Some(b)) => Confidence::new(a.get().max(b.get())),
                        (a, b) => a.or(b),
                    };
                    continue;
                }
            }
        }
        merged.push(turn);
    }
    merged
}

impl DiarizationProvider for SortformerDiarizer {
    fn info(&self) -> ProviderInfo {
        self.info.clone()
    }

    fn diarize(
        &self,
        audio: &Audio,
        _cfg: &DiarizationConfig,
    ) -> ProviderResult<DiarizationOutput> {
        if audio.sample_rate != RATE as u32 {
            return Err(ProviderError::Unsupported(format!(
                "sortformer requires {} Hz audio, got {} Hz",
                RATE, audio.sample_rate
            )));
        }
        if audio.is_empty() {
            return Ok(DiarizationOutput::default());
        }

        let total_samples = audio.samples.len();
        let total_s = total_samples as f32 / RATE as f32;
        let window_samples = MAX_CHUNK_S * RATE;
        let hop_samples = (MAX_CHUNK_S - CHUNK_OVERLAP_S) * RATE;
        let mut starts: Vec<usize> = Vec::new();
        let mut start = 0;
        while start < total_samples {
            starts.push(start);
            if start + window_samples >= total_samples {
                break;
            }
            start += hop_samples;
        }

        let mut turns = Vec::new();
        let mut overlap_detected = false;
        let mut previous_tail: Vec<[f32; NUM_SPEAKERS]> = Vec::new();
        let mut previous_labels: Vec<Option<usize>> = vec![None; NUM_SPEAKERS];
        let mut next_label = 0usize;

        for &begin in &starts {
            let end = (begin + window_samples).min(total_samples);
            let window_start_s = begin as f32 / RATE as f32;
            let duration_s = (end - begin) as f32 / RATE as f32;
            let probs = self.window(&audio.samples[begin..end])?;
            if probs.is_empty() {
                continue;
            }

            let labels = if previous_tail.is_empty() {
                first_activations(&probs)
            } else {
                let tail = &previous_tail[previous_tail.len().saturating_sub(OVERLAP_FRAMES)..];
                let head = &probs[..probs.len().min(OVERLAP_FRAMES)];
                let (labels, next) = match_labels(tail, &previous_labels, head, next_label);
                next_label = next;
                labels
            };

            let (mut window_turns, overlap) = turns_from_window(
                &probs,
                &labels,
                window_start_s,
                (window_start_s + duration_s).min(total_s),
            );
            overlap_detected |= overlap;
            turns.append(&mut window_turns);

            previous_tail = probs;
            previous_labels = labels;
            next_label = next_label.max(
                previous_labels
                    .iter()
                    .filter_map(|label| *label)
                    .max()
                    .map_or(0, |label| label + 1),
            );
        }

        Ok(DiarizationOutput {
            turns: stitch(turns),
            overlap_detected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mel_filterbank_has_the_expected_peaks() {
        let frontend = MelFrontend::new();
        assert_eq!(frontend.filters.len(), N_MELS * N_BINS);
        for mel in 0..N_MELS {
            let row = &frontend.filters[mel * N_BINS..(mel + 1) * N_BINS];
            let sum: f32 = row.iter().sum();
            assert!(sum > 0.0, "mel {mel} is empty");
            assert!(row.iter().all(|value| value.is_finite() && *value >= 0.0));
        }
    }

    #[test]
    fn mel_frames_are_finite_and_ordered() {
        let frontend = MelFrontend::new();
        let samples: Vec<f32> = (0..RATE)
            .map(|i| (std::f32::consts::TAU * 440.0 * i as f32 / RATE as f32).sin() * 0.1)
            .collect();
        let mel = frontend.compute(&samples);
        assert_eq!(mel.len(), (samples.len() / HOP) * N_MELS);
        assert!(mel.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn labels_follow_first_appearance() {
        let probs = vec![
            [0.0, 0.9, 0.0, 0.0],
            [0.0, 0.9, 0.0, 0.0],
            [0.8, 0.0, 0.0, 0.0],
        ];
        assert_eq!(
            first_activations(&probs),
            vec![Some(1), Some(0), None, None]
        );
    }

    #[test]
    fn overlap_produces_two_turns() {
        let probs: Vec<[f32; 4]> = (0..25)
            .map(|frame| {
                if (10..20).contains(&frame) {
                    [0.9, 0.8, 0.0, 0.0]
                } else {
                    [0.9, 0.0, 0.0, 0.0]
                }
            })
            .collect();
        let labels = first_activations(&probs);
        let (turns, overlap) = turns_from_window(&probs, &labels, 0.0, 2.0);
        assert!(overlap);
        assert_eq!(turns.len(), 2, "{turns:?}");
        assert!(turns[0].range.overlap(&turns[1].range) > MediaTime::ZERO);
    }

    #[test]
    fn labels_are_matched_across_windows() {
        let previous = vec![[0.0, 0.9, 0.0, 0.0]; 5];
        let current = vec![[0.9, 0.0, 0.0, 0.0]; 5];
        let (labels, next) = match_labels(&previous, &[Some(0), Some(1), None, None], &current, 2);
        assert_eq!(labels[0], Some(1));
        assert_eq!(next, 2);
    }
}
