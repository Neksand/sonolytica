//! NVIDIA Sortformer 4-speaker diarization through ONNX Runtime.
//!
//! The model is an ONNX export of `nvidia/diar_streaming_sortformer_4spk-v2.1`
//! that takes one 30.48 s mel chunk plus the streaming state and returns
//! per-frame speaker activity at 80 ms. Unlike a sliding-window segmenter, the
//! state carries an Arrival-Order Speaker Cache (AOSC) across chunks, so a
//! speaker index means the same person for the whole recording.
//!
//! Everything here is deliberately explicit:
//!
//! 1. NeMo log-mel: pre-emphasis 0.97, 25 ms Hann window, 10 ms hop,
//!    512-point power spectrum, 128 Slaney mel filters, `ln(x + 2^-24)`, no
//!    normalisation.
//! 2. The graph takes `chunk[1, 3048, 128]`, the speaker cache
//!    `[1, 188, 512]`, the FIFO `[1, 40, 512]`, their lengths, and returns
//!    predictions for `[cache | fifo | chunk]` plus the chunk embeddings.
//! 3. Chunks advance by 340 frames (27.2 s) with one left-context frame and 40
//!    right-context frames. The host owns the AOSC: FIFO -> speaker cache,
//!    log-activity scoring, per-speaker boosting, silence slots, top-k.
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

/// Streaming geometry from the export's `config.json`. One call reports 340
/// frames (27.2 s) and needs 40 frames of future context to do it.
const SUB_FACTOR: usize = 8;
const CHUNK_FRAMES: usize = 340;
const LEFT_CONTEXT: usize = 1;
const RIGHT_CONTEXT: usize = 40;
const WINDOW_FRAMES: usize = LEFT_CONTEXT + CHUNK_FRAMES + RIGHT_CONTEXT; // 381
const MEL_INPUT_FRAMES: usize = WINDOW_FRAMES * SUB_FACTOR; // 3048
const FIFO_FRAMES: usize = 40;
const CACHE_FRAMES: usize = 188;
const UPDATE_PERIOD: usize = 188;
const EMB_DIM: usize = 512;

/// AOSC compression constants. They mirror NeMo's `SortformerModules`.
const SIL_FRAMES_PER_SPK: usize = 3;
const SIL_THRESHOLD: f32 = 0.2;
const PRED_SCORE_THRESHOLD: f32 = 0.25;
const SCORES_BOOST_LATEST: f32 = 0.05;
const STRONG_BOOST_RATE: f32 = 0.75;
const WEAK_BOOST_RATE: f32 = 1.5;
const MIN_POS_SCORES_RATE: f32 = 0.5;

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

    /// Log-mel at 10 ms per frame, time-major (`frames * N_MELS`).
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

/// AOSC state. Embeddings and predictions are flat row-major; the cache is
/// ordered by speaker block, exactly as NeMo's compression emits it.
#[derive(Default)]
struct StreamingState {
    cache: Vec<f32>, // cache_len * EMB_DIM
    cache_len: usize,
    cache_preds: Vec<f32>, // cache_len * NUM_SPEAKERS
    compressed: bool,
    fifo: Vec<f32>, // fifo_len * EMB_DIM
    fifo_len: usize,
    fifo_preds: Vec<f32>, // fifo_len * NUM_SPEAKERS
    mean_sil: Vec<f32>,   // EMB_DIM
    n_sil: u64,
}

impl StreamingState {
    fn new() -> Self {
        Self {
            mean_sil: vec![0.0; EMB_DIM],
            ..Self::default()
        }
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
            // Loading a 475 MB graph at discovery would allocate half a
            // gigabyte for `inspect` and music-only runs. First diarization
            // pays for it; a load failure is reported then and the engine
            // falls back to pyannote.
            session: Mutex::new(None),
            path: path.to_path_buf(),
            threads,
            mel: MelFrontend::new(),
            info: ProviderInfo {
                name: "sortformer-4spk-v2.1",
                capability: Capability::Diarize,
                device: "cpu",
                models: vec![spec.reference()],
                notes: "Up to 4 speakers. Overlapping speech is returned as two simultaneous \
                        turns instead of a flag. Native AOSC streaming (340-frame chunks, \
                        40-frame FIFO, 188-frame speaker cache) keeps speaker labels stable \
                        for the whole recording; 5+ speaker recordings are still collapsed \
                        into four voices.",
            },
        })
    }

    /// One streaming step. `chunk_mel` is `MEL_INPUT_FRAMES * N_MELS`, zero
    /// padded past `valid_mel`; `reported` is how many 80 ms frames this call
    /// contributes to the timeline.
    fn step(
        &self,
        session: &mut Session,
        chunk_mel: &[f32],
        valid_mel: usize,
        reported: usize,
        state: &mut StreamingState,
    ) -> ProviderResult<Vec<[f32; NUM_SPEAKERS]>> {
        let slen = state.cache_len;
        let flen = state.fifo_len;

        let mut cache = vec![0.0_f32; CACHE_FRAMES * EMB_DIM];
        cache[..slen * EMB_DIM].copy_from_slice(&state.cache[..slen * EMB_DIM]);
        let mut fifo = vec![0.0_f32; FIFO_FRAMES * EMB_DIM];
        fifo[..flen * EMB_DIM].copy_from_slice(&state.fifo[..flen * EMB_DIM]);

        let chunk = Tensor::from_array((
            vec![1_i64, MEL_INPUT_FRAMES as i64, N_MELS as i64],
            chunk_mel.to_vec(),
        ))
        .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let chunk_lengths = Tensor::from_array((vec![1_i64], vec![valid_mel as i64]))
            .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let spkcache =
            Tensor::from_array((vec![1_i64, CACHE_FRAMES as i64, EMB_DIM as i64], cache))
                .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let spkcache_lengths = Tensor::from_array((vec![1_i64], vec![slen as i64]))
            .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let fifo_input =
            Tensor::from_array((vec![1_i64, FIFO_FRAMES as i64, EMB_DIM as i64], fifo))
                .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let fifo_lengths = Tensor::from_array((vec![1_i64], vec![flen as i64]))
            .map_err(|e| ProviderError::Inference(e.to_string()))?;

        let outputs = session
            .run(ort::inputs![
                "chunk" => chunk,
                "chunk_lengths" => chunk_lengths,
                "spkcache" => spkcache,
                "spkcache_lengths" => spkcache_lengths,
                "fifo" => fifo_input,
                "fifo_lengths" => fifo_lengths,
            ])
            .map_err(|e| ProviderError::Inference(format!("sortformer run failed: {e}")))?;

        let (pred_shape, preds) = outputs
            .get("spkcache_fifo_chunk_preds")
            .ok_or_else(|| ProviderError::Inference("sortformer predictions missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let pred_dims: Vec<usize> = pred_shape.iter().map(|d| *d as usize).collect();
        let expected_preds = CACHE_FRAMES + FIFO_FRAMES + WINDOW_FRAMES;
        if pred_dims.len() != 3
            || pred_dims[0] != 1
            || pred_dims[1] != expected_preds
            || pred_dims[2] != NUM_SPEAKERS
        {
            return Err(ProviderError::Inference(format!(
                "unexpected sortformer prediction shape {pred_dims:?}"
            )));
        }

        let (emb_shape, embs) = outputs
            .get("chunk_pre_encode_embs")
            .ok_or_else(|| ProviderError::Inference("sortformer embeddings missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let emb_dims: Vec<usize> = emb_shape.iter().map(|d| *d as usize).collect();
        if emb_dims.len() != 3 || emb_dims[0] != 1 || emb_dims[2] != EMB_DIM {
            return Err(ProviderError::Inference(format!(
                "unexpected sortformer embedding shape {emb_dims:?}"
            )));
        }
        let (_, emb_lengths) = outputs
            .get("chunk_pre_encode_lengths")
            .ok_or_else(|| ProviderError::Inference("sortformer lengths missing".into()))?
            .try_extract_tensor::<i64>()
            .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let emb_valid = emb_lengths.first().copied().unwrap_or(0).max(0) as usize;
        if LEFT_CONTEXT + reported > emb_valid {
            return Err(ProviderError::Inference(format!(
                "sortformer returned {emb_valid} chunk frames, need {}",
                LEFT_CONTEXT + reported
            )));
        }

        // The output is left-packed: [cache | fifo | chunk]. The chunk region
        // includes one left-context frame, so its reported part starts after it.
        let chunk_start = slen + flen + LEFT_CONTEXT;
        let mut chunk_probs = Vec::with_capacity(reported);
        for frame in 0..reported {
            let base = (chunk_start + frame) * NUM_SPEAKERS;
            chunk_probs.push([
                preds[base],
                preds[base + 1],
                preds[base + 2],
                preds[base + 3],
            ]);
        }

        // Fresh predictions for the old FIFO come from this same forward pass.
        let mut combined_preds = Vec::with_capacity((flen + reported) * NUM_SPEAKERS);
        combined_preds.extend_from_slice(&preds[slen * NUM_SPEAKERS..(slen + flen) * NUM_SPEAKERS]);
        for probability in &chunk_probs {
            combined_preds.extend_from_slice(probability);
        }
        let mut combined_embs = Vec::with_capacity((flen + reported) * EMB_DIM);
        combined_embs.extend_from_slice(&state.fifo[..flen * EMB_DIM]);
        combined_embs
            .extend_from_slice(&embs[LEFT_CONTEXT * EMB_DIM..(LEFT_CONTEXT + reported) * EMB_DIM]);

        if flen + reported > FIFO_FRAMES {
            let overflow = flen + reported - FIFO_FRAMES;
            let pop = (flen + reported).min(UPDATE_PERIOD.max(overflow));

            // Running silence profile from the frames leaving the FIFO.
            for frame in 0..pop {
                let base = frame * NUM_SPEAKERS;
                let sum: f32 = combined_preds[base..base + NUM_SPEAKERS].iter().sum();
                if sum < SIL_THRESHOLD {
                    let n = state.n_sil as f32;
                    let emb = &combined_embs[frame * EMB_DIM..(frame + 1) * EMB_DIM];
                    for (mean, value) in state.mean_sil.iter_mut().zip(emb.iter()) {
                        *mean = (*mean * n + *value) / (n + 1.0);
                    }
                    state.n_sil += 1;
                }
            }

            state.fifo.clear();
            state
                .fifo
                .extend_from_slice(&combined_embs[pop * EMB_DIM..]);
            state.fifo_len = flen + reported - pop;
            state.fifo_preds.clear();
            state
                .fifo_preds
                .extend_from_slice(&combined_preds[pop * NUM_SPEAKERS..]);

            let old_preds = if state.compressed {
                &state.cache_preds[..slen * NUM_SPEAKERS]
            } else {
                &preds[..slen * NUM_SPEAKERS]
            };
            let mut candidate_embs = Vec::with_capacity((slen + pop) * EMB_DIM);
            candidate_embs.extend_from_slice(&state.cache[..slen * EMB_DIM]);
            candidate_embs.extend_from_slice(&combined_embs[..pop * EMB_DIM]);
            let mut candidate_preds = Vec::with_capacity((slen + pop) * NUM_SPEAKERS);
            candidate_preds.extend_from_slice(old_preds);
            candidate_preds.extend_from_slice(&combined_preds[..pop * NUM_SPEAKERS]);

            if slen + pop > CACHE_FRAMES {
                let (cache, cache_preds) = compress_cache(
                    &candidate_embs,
                    &candidate_preds,
                    slen + pop,
                    &state.mean_sil,
                );
                state.cache = cache;
                state.cache_preds = cache_preds;
                state.cache_len = CACHE_FRAMES;
                state.compressed = true;
            } else {
                state.cache = candidate_embs;
                state.cache_preds = candidate_preds;
                state.cache_len = slen + pop;
            }
        } else {
            state.fifo = combined_embs;
            state.fifo_len = flen + reported;
            state.fifo_preds = combined_preds;
        }

        Ok(chunk_probs)
    }
}

/// Compress `n > CACHE_FRAMES` cache candidates down to `CACHE_FRAMES`.
///
/// NeMo's AOSC scoring: `log(p/(1-p)) + sum_j log(1-p_j) - log(0.5)`, disabled
/// outside speech, boosted twice so every speaker keeps a minimum share, with
/// `SIL_FRAMES_PER_SPK * NUM_SPEAKERS` silence slots that always survive.
fn compress_cache(embs: &[f32], preds: &[f32], n: usize, mean_sil: &[f32]) -> (Vec<f32>, Vec<f32>) {
    let padded = n + SIL_FRAMES_PER_SPK;
    let per_spk = CACHE_FRAMES / NUM_SPEAKERS - SIL_FRAMES_PER_SPK;
    let strong = (per_spk as f32 * STRONG_BOOST_RATE) as usize;
    let weak = (per_spk as f32 * WEAK_BOOST_RATE) as usize;
    let min_pos = (per_spk as f32 * MIN_POS_SCORES_RATE) as usize;

    let mut scores = vec![0.0_f32; n * NUM_SPEAKERS];
    let mut pos_count = [0usize; NUM_SPEAKERS];
    for frame in 0..n {
        let base = frame * NUM_SPEAKERS;
        let mut log_1_sum = 0.0_f32;
        for spk in 0..NUM_SPEAKERS {
            log_1_sum += (1.0 - preds[base + spk]).max(PRED_SCORE_THRESHOLD).ln();
        }
        for spk in 0..NUM_SPEAKERS {
            let raw = preds[base + spk];
            let score = if raw > THRESHOLD {
                raw.max(PRED_SCORE_THRESHOLD).ln() - (1.0 - raw).max(PRED_SCORE_THRESHOLD).ln()
                    + log_1_sum
                    - 0.5_f32.ln()
            } else {
                f32::NEG_INFINITY
            };
            scores[base + spk] = score;
            if score > 0.0 {
                pos_count[spk] += 1;
            }
        }
    }
    for frame in 0..n {
        let base = frame * NUM_SPEAKERS;
        for spk in 0..NUM_SPEAKERS {
            if preds[base + spk] > THRESHOLD
                && scores[base + spk] <= 0.0
                && pos_count[spk] >= min_pos
            {
                scores[base + spk] = f32::NEG_INFINITY;
            }
        }
    }

    // Newer frames win ties.
    for frame in CACHE_FRAMES..n {
        for spk in 0..NUM_SPEAKERS {
            scores[frame * NUM_SPEAKERS + spk] += SCORES_BOOST_LATEST;
        }
    }

    for (k, scale) in [(strong, 2.0_f32), (weak, 1.0_f32)] {
        for spk in 0..NUM_SPEAKERS {
            let mut ranked: Vec<(f32, usize)> = (0..n)
                .map(|frame| (scores[frame * NUM_SPEAKERS + spk], frame))
                .collect();
            ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
            for &(score, frame) in ranked.iter().take(k) {
                if score != f32::NEG_INFINITY {
                    scores[frame * NUM_SPEAKERS + spk] -= scale * 0.5_f32.ln();
                }
            }
        }
    }

    // Speaker-major flat indices, with silence pads at +inf so they always
    // survive. Decoding must use the padded frame count; using `n` here would
    // turn the pads into frames 0..2 and silently disable most of the cache.
    let mut flat: Vec<(f32, usize)> = Vec::with_capacity(padded * NUM_SPEAKERS);
    for spk in 0..NUM_SPEAKERS {
        for frame in 0..padded {
            let score = if frame < n {
                scores[frame * NUM_SPEAKERS + spk]
            } else {
                f32::INFINITY
            };
            flat.push((score, spk * padded + frame));
        }
    }
    flat.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut selected: Vec<(f32, usize)> = flat.into_iter().take(CACHE_FRAMES).collect();
    selected.sort_by_key(|(_, index)| *index);

    let mut cache = Vec::with_capacity(CACHE_FRAMES * EMB_DIM);
    let mut cache_preds = vec![0.0_f32; CACHE_FRAMES * NUM_SPEAKERS];
    for (slot, (score, index)) in selected.iter().enumerate() {
        let frame = index % padded;
        let disabled = frame >= n || *score == f32::NEG_INFINITY;
        if disabled {
            cache.extend_from_slice(mean_sil);
        } else {
            cache.extend_from_slice(&embs[frame * EMB_DIM..(frame + 1) * EMB_DIM]);
            let src = frame * NUM_SPEAKERS;
            let dst = slot * NUM_SPEAKERS;
            cache_preds[dst..dst + NUM_SPEAKERS].copy_from_slice(&preds[src..src + NUM_SPEAKERS]);
        }
    }
    (cache, cache_preds)
}

/// Threshold per-frame activity into turns, then merge same-speaker gaps up to
/// 160 ms (the GTCR stitching used by the old window path is now unnecessary:
/// the AOSC keeps one channel per person for the whole file).
fn turns_from_probs(probs: &[[f32; NUM_SPEAKERS]], overlap: &mut bool) -> Vec<DiarTurn> {
    let mut turns = Vec::new();
    for frame in probs {
        if frame.iter().filter(|value| **value > THRESHOLD).count() > 1 {
            *overlap = true;
        }
    }
    for channel in 0..NUM_SPEAKERS {
        let mut run: Option<(usize, f32)> = None; // start, probability sum
        for (index, frame) in probs.iter().enumerate() {
            let active = frame[channel] > THRESHOLD;
            match &mut run {
                Some((_, sum)) if active => *sum += frame[channel],
                Some((start, sum)) => {
                    push_turn(&mut turns, *start, index, channel, *sum);
                    run = active.then_some((index, frame[channel]));
                }
                None if active => run = Some((index, frame[channel])),
                None => {}
            }
        }
        if let Some((start, sum)) = run {
            push_turn(&mut turns, start, probs.len(), channel, sum);
        }
    }
    turns
}

fn push_turn(turns: &mut Vec<DiarTurn>, start: usize, end: usize, channel: usize, sum: f32) {
    let length_s = (end - start) as f32 * FRAME_S;
    if length_s < MIN_TURN_S {
        return;
    }
    let (start, end) = (start as i64, end as i64);
    let begin = MediaTime::from_millis(start * 80);
    let finish = MediaTime::from_millis(end * 80);
    turns.push(DiarTurn {
        range: TimeRange::new(begin, finish),
        speaker: SpeakerId::new(format!("S{}", channel + 1)),
        confidence: Confidence::new((sum / (end - start) as f32).clamp(0.0, 1.0)),
    });
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

        let mel = self.mel.compute(&audio.samples);
        let total_mel = mel.len() / N_MELS;
        let total_frames = total_mel / SUB_FACTOR;
        if total_frames == 0 {
            return Ok(DiarizationOutput::default());
        }

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

        let mut state = StreamingState::new();
        let mut chunk_mel = vec![0.0_f32; MEL_INPUT_FRAMES * N_MELS];
        let mut probs: Vec<[f32; NUM_SPEAKERS]> = Vec::with_capacity(total_frames);
        let mut start = 0_usize;
        while start < total_frames {
            chunk_mel.fill(0.0);
            let reported = CHUNK_FRAMES.min(total_frames - start);
            let valid_mel = if start == 0 {
                // The first window has no history: one zero left-context frame.
                let valid = MEL_INPUT_FRAMES.min((total_frames + 1) * SUB_FACTOR);
                let audio_mel = (valid - SUB_FACTOR).min(total_mel);
                chunk_mel[SUB_FACTOR * N_MELS..(SUB_FACTOR + audio_mel) * N_MELS]
                    .copy_from_slice(&mel[..audio_mel * N_MELS]);
                valid
            } else {
                let first_mel = (start - LEFT_CONTEXT) * SUB_FACTOR;
                let valid = MEL_INPUT_FRAMES.min(total_mel - first_mel);
                chunk_mel[..valid * N_MELS]
                    .copy_from_slice(&mel[first_mel * N_MELS..(first_mel + valid) * N_MELS]);
                valid
            };

            let chunk_probs = self.step(session, &chunk_mel, valid_mel, reported, &mut state)?;
            probs.extend_from_slice(&chunk_probs);
            start += CHUNK_FRAMES;
        }

        let mut overlap_detected = false;
        let turns = turns_from_probs(&probs, &mut overlap_detected);
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

    /// Regression for the AOSC silence-pad decode: the flat top-k index must be
    /// reduced modulo the *padded* frame count. Decoding modulo `n` selected
    /// frames 0..2 as if they were silence and disabled most of the cache.
    #[test]
    fn compression_keeps_speech_frames_and_exactly_twelve_silence_slots() {
        let n = 300;
        let mut embs = vec![0.0_f32; n * EMB_DIM];
        let mut preds = vec![0.0_f32; n * NUM_SPEAKERS];
        for frame in 0..n {
            // Channel 0 speaks first, channel 1 speaks last; 0.9 is confident.
            let channel = if frame < 150 { 0 } else { 1 };
            preds[frame * NUM_SPEAKERS + channel] = 0.9;
            for d in 0..EMB_DIM {
                embs[frame * EMB_DIM + d] = (frame as f32 + 1.0) * 10.0 + d as f32 * 0.001;
            }
        }
        let mean_sil = vec![0.0_f32; EMB_DIM];
        let (cache, cache_preds) = compress_cache(&embs, &preds, n, &mean_sil);
        assert_eq!(cache.len(), CACHE_FRAMES * EMB_DIM);
        assert_eq!(cache_preds.len(), CACHE_FRAMES * NUM_SPEAKERS);

        let silent_rows = cache_preds
            .chunks_exact(NUM_SPEAKERS)
            .filter(|row| row.iter().all(|value| *value == 0.0))
            .count();
        assert_eq!(
            silent_rows,
            SIL_FRAMES_PER_SPK * NUM_SPEAKERS,
            "the cache must keep exactly the reserved silence slots"
        );

        // Every surviving speech row is one of the real embeddings, never the
        // zero silence profile.
        let mut speech_rows = 0;
        for row in cache.chunks_exact(EMB_DIM) {
            if row.iter().all(|value| *value == 0.0) {
                continue;
            }
            speech_rows += 1;
            let frame = (row[0] / 10.0).round() as usize;
            assert!((1..=n).contains(&frame), "unexpected embedding row {frame}");
        }
        assert_eq!(
            speech_rows,
            CACHE_FRAMES - SIL_FRAMES_PER_SPK * NUM_SPEAKERS
        );
        // Both speakers survive the top-k boost.
        let channel_zero = cache_preds
            .chunks_exact(NUM_SPEAKERS)
            .filter(|row| row[0] > 0.5)
            .count();
        let channel_one = cache_preds
            .chunks_exact(NUM_SPEAKERS)
            .filter(|row| row[1] > 0.5)
            .count();
        assert!(
            channel_zero > 0 && channel_one > 0,
            "{channel_zero}/{channel_one}"
        );
    }

    #[test]
    fn turns_split_on_channel_and_overlap_is_flagged() {
        let probs: Vec<[f32; 4]> = (0..50)
            .map(|frame| {
                if (20..30).contains(&frame) {
                    [0.9, 0.8, 0.0, 0.0]
                } else if frame < 20 {
                    [0.9, 0.0, 0.0, 0.0]
                } else {
                    [0.0, 0.9, 0.0, 0.0]
                }
            })
            .collect();
        let mut overlap = false;
        let turns = turns_from_probs(&probs, &mut overlap);
        assert!(overlap);
        assert_eq!(turns.len(), 2, "{turns:?}");
        assert_eq!(turns[0].speaker.as_str(), "S1");
        assert_eq!(turns[1].speaker.as_str(), "S2");
    }

    #[test]
    fn short_runs_are_dropped_and_gaps_are_stitched() {
        let mut probs = vec![[0.0_f32; 4]; 40];
        for frame in probs.iter_mut().take(10) {
            frame[0] = 0.9;
        }
        // 2-frame hole, then a 10-frame run: stitched.
        for frame in probs.iter_mut().take(22).skip(12) {
            frame[0] = 0.9;
        }
        // 2-frame blip: dropped (below 240 ms).
        probs[30][1] = 0.9;
        probs[31][1] = 0.9;
        let mut overlap = false;
        let turns = stitch(turns_from_probs(&probs, &mut overlap));
        assert_eq!(turns.len(), 1, "{turns:?}");
        assert!(!overlap);
        assert_eq!(turns[0].range, TimeRange::from_secs(0.0, 22.0 * 0.08));
    }
}
