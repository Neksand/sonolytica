//! Local speaker diarization: pyannote segmentation (powerset classes) plus
//! WeSpeaker embeddings, clustered by cosine similarity.
//!
//! The upstream Python pipeline does agglomerative clustering over the whole
//! recording with a tuned pipeline. This implementation is deliberately
//! simpler and documented as such:
//!
//! 1. 10 s windows through the segmentation model give frame-level powerset
//!    classes: silence, three local speakers, or two-speaker overlap.
//! 2. Overlap frames are counted and reported, but not split into two
//!    speakers, because doing so without a proper overlap model would invent
//!    boundaries.
//! 3. Single-speaker regions long enough to embed are split into 1.5 s chunks
//!    with a 0.75 s hop. More windows per speaker give the clustering real
//!    statistics instead of one vector per 10 s class run.
//! 4. Chunk embeddings are clustered by average-linkage agglomerative
//!    clustering on cosine similarity; merges continue while the best pair is
//!    above `threshold`, or regardless of similarity while the cluster count
//!    exceeds `max_speakers`.
//! 5. A region's speaker is the majority label of its chunks, and adjacent
//!    regions of the same speaker are stitched.

use std::path::Path;
use std::sync::Mutex;

use ort::session::Session;
use ort::value::Tensor;
use sonolytica_core::{
    Audio, Capability, Confidence, DiarizationConfig, MediaTime, SpeakerId, TimeRange,
};

use crate::models::ModelSpec;
use crate::traits::{
    DiarTurn, DiarizationOutput, DiarizationProvider, ProviderError, ProviderInfo, ProviderResult,
    cosine, l2_normalize,
};

const RATE: u32 = 16_000;
const WINDOW_SAMPLES: usize = 160_000; // 10 s
const FRAME_SAMPLES: usize = 270; // model stride at 16 kHz
const FRAME_START: usize = 721; // first frame offset reported by pyannote
const OVERLAP_CLASS_MIN: usize = 4;
const NUM_CLASSES: usize = 7;

/// Embedding windows inside one speech region. Short chunks mean more vectors,
/// but too short and the speaker embedding stops being reliable.
const EMBED_WINDOW_S: f32 = 1.5;
const EMBED_HOP_S: f32 = 0.75;
const EMBED_MIN_S: f32 = 1.0;

#[derive(Debug, Clone)]
struct LocalRegion {
    start_s: f32,
    end_s: f32,
    probability: f32,
}

pub struct PyannoteDiarizer {
    segmentation: Mutex<Session>,
    embedding: Mutex<Session>,
    info: ProviderInfo,
}

impl PyannoteDiarizer {
    pub fn load(
        segmentation_spec: &'static ModelSpec,
        embedding_spec: &'static ModelSpec,
        segmentation_path: &Path,
        embedding_path: &Path,
        threads: usize,
    ) -> ProviderResult<Self> {
        Ok(Self {
            segmentation: Mutex::new(crate::onnx::build_session(segmentation_path, threads)?),
            embedding: Mutex::new(crate::onnx::build_session(embedding_path, threads)?),
            info: ProviderInfo {
                name: "pyannote-3.0+wespeaker",
                capability: Capability::Diarize,
                device: "cpu",
                models: vec![segmentation_spec.reference(), embedding_spec.reference()],
                notes: "10 s non-overlapping segmentation windows; 1.5 s embeddings clustered \
                        by average linkage. Overlap is detected and reported but not attributed \
                        to two speakers.",
            },
        })
    }

    /// Frame-level powerset classes for one 10 s window.
    fn window_classes(&self, window: &[f32]) -> ProviderResult<(Vec<(usize, f32)>, bool)> {
        let mut padded = Vec::with_capacity(WINDOW_SAMPLES);
        padded.extend_from_slice(window);
        padded.resize(WINDOW_SAMPLES, 0.0);

        let input = Tensor::from_array((vec![1_i64, 1, WINDOW_SAMPLES as i64], padded))
            .map_err(|e| ProviderError::Inference(e.to_string()))?;

        let mut guard = self
            .segmentation
            .lock()
            .map_err(|_| ProviderError::Inference("segmentation session poisoned".into()))?;
        let outputs = guard
            .run(ort::inputs!["input" => input])
            .map_err(|e| ProviderError::Inference(format!("segmentation run failed: {e}")))?;
        let (shape, data) = outputs
            .get("output")
            .ok_or_else(|| ProviderError::Inference("segmentation output missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| ProviderError::Inference(e.to_string()))?;

        let dims: Vec<usize> = shape.iter().map(|d| *d as usize).collect();
        if dims.len() != 3 || dims[2] != NUM_CLASSES {
            return Err(ProviderError::Inference(format!(
                "unexpected segmentation output shape {dims:?}"
            )));
        }
        let frames = dims[1];

        let mut classes = Vec::with_capacity(frames);
        let mut overlap = false;
        for frame in 0..frames {
            let row = &data[frame * NUM_CLASSES..(frame + 1) * NUM_CLASSES];
            let (best, &probability) = row
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .unwrap_or((0, &0.0));
            if best >= OVERLAP_CLASS_MIN {
                overlap = true;
            }
            classes.push((best, probability));
        }
        Ok((classes, overlap))
    }

    fn embed(&self, samples: &[f32]) -> ProviderResult<Vec<f32>> {
        if samples.len() < RATE as usize / 4 {
            return Err(ProviderError::Unsupported(
                "region too short to embed".into(),
            ));
        }
        let fbank = knf_rs::compute_fbank(samples)
            .map_err(|e| ProviderError::Inference(format!("fbank failed: {e}")))?;
        let rows = fbank.nrows();
        let cols = fbank.ncols();
        if rows == 0 || cols == 0 {
            return Err(ProviderError::Inference("empty fbank matrix".into()));
        }
        let data: Vec<f32> = fbank.iter().copied().collect();
        let tensor = Tensor::from_array((vec![1_i64, rows as i64, cols as i64], data))
            .map_err(|e| ProviderError::Inference(e.to_string()))?;

        let mut guard = self
            .embedding
            .lock()
            .map_err(|_| ProviderError::Inference("embedding session poisoned".into()))?;
        let outputs = guard
            .run(ort::inputs!["feats" => tensor])
            .map_err(|e| ProviderError::Inference(format!("embedding run failed: {e}")))?;
        let (_, embedding) = outputs
            .get("embs")
            .ok_or_else(|| ProviderError::Inference("embedding output missing".into()))?
            .try_extract_tensor::<f32>()
            .map_err(|e| ProviderError::Inference(e.to_string()))?;

        let mut embedding = embedding.to_vec();
        l2_normalize(&mut embedding);
        Ok(embedding)
    }

    fn local_regions(
        &self,
        audio: &Audio,
        min_region_s: f32,
    ) -> ProviderResult<(Vec<LocalRegion>, bool)> {
        let mut regions = Vec::new();
        let mut overlap_detected = false;
        // The segmentation model always consumes full 10 s windows; never let
        // its padded tail turn into a timestamp past the real audio.
        let limit_s = audio.samples.len() as f32 / RATE as f32;

        for (window_index, window) in audio.samples.chunks(WINDOW_SAMPLES).enumerate() {
            let window_start_s = (window_index * WINDOW_SAMPLES) as f32 / RATE as f32;
            let (classes, overlap) = self.window_classes(window)?;
            overlap_detected |= overlap;

            let mut run: Option<(usize, usize, f32)> = None; // class, first frame, prob sum
            let mut frames_count = 0usize;

            let emit = |run: &mut Option<(usize, usize, f32)>,
                        end_frame: usize,
                        regions: &mut Vec<LocalRegion>| {
                if let Some((class, start_frame, prob_sum)) = run.take() {
                    if class == 0 {
                        return;
                    }
                    let count = end_frame - start_frame;
                    let start_sample = FRAME_START + start_frame * FRAME_SAMPLES;
                    let end_sample = FRAME_START + end_frame * FRAME_SAMPLES;
                    let start_s = (window_start_s + start_sample as f32 / RATE as f32).min(limit_s);
                    let end_s = (window_start_s + end_sample as f32 / RATE as f32).min(limit_s);
                    if end_s - start_s >= min_region_s {
                        regions.push(LocalRegion {
                            start_s,
                            end_s,
                            probability: prob_sum / count as f32,
                        });
                    }
                }
            };

            for (frame, &(class, probability)) in classes.iter().enumerate() {
                frames_count = frame + 1;
                match &mut run {
                    Some((current, _, sum)) if *current == class => {
                        *sum += probability;
                    }
                    _ => {
                        emit(&mut run, frame, &mut regions);
                        if class != 0 {
                            run = Some((class, frame, probability));
                        }
                    }
                }
            }
            emit(&mut run, frames_count, &mut regions);
        }

        Ok((regions, overlap_detected))
    }
}

/// Chunk boundaries for embedding one speech region: `EMBED_WINDOW_S` windows
/// with `EMBED_HOP_S` hop, never shorter than `EMBED_MIN_S`, never past the
/// region end. A region shorter than the minimum is one chunk.
fn chunk_bounds(start_s: f32, end_s: f32) -> Vec<(f32, f32)> {
    let duration = end_s - start_s;
    if duration <= EMBED_MIN_S {
        return vec![(start_s, end_s)];
    }

    let mut bounds = Vec::new();
    let mut offset = 0.0_f32;
    while offset < duration {
        let begin = start_s + offset;
        let end = (begin + EMBED_WINDOW_S).min(end_s);
        if end - begin >= EMBED_MIN_S {
            bounds.push((begin, end));
        } else if let Some(last) = bounds.last_mut() {
            // Absorb a sliver at the end instead of embedding 0.2 s of audio.
            last.1 = end_s;
        }
        offset += EMBED_HOP_S;
    }
    bounds
}

/// Majority label of one region's chunks; ties go to the earliest cluster.
fn majority_label(labels: &[usize]) -> usize {
    let mut counts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for label in labels {
        *counts.entry(*label).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        .map_or(0, |(label, _)| label)
}

/// Average-linkage agglomerative clustering on cosine similarity.
///
/// Merges while the best pair is at least `threshold`, or regardless of
/// similarity while the cluster count exceeds `max_speakers`. Deterministic for
/// a given input.
fn cluster_embeddings(embeddings: &[Vec<f32>], threshold: f32, max_speakers: usize) -> Vec<usize> {
    let n = embeddings.len();
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![0];
    }

    // Average-linkage agglomerative clustering. The pairwise matrix is built
    // once and updated with the Lance-Williams rule, which is cheaper and much
    // less prone to chaining than recomputing centroids or single linkage.
    //
    // ponytail: O(n^2) memory, O(n^3) worst case in the max search. Fine for a
    // few thousand 1.5 s chunks; move to a k-NN graph or VB-HMM if multi-hour
    // recordings become the norm.
    let mut similarity = vec![vec![0.0_f32; n]; n];
    for i in 0..n {
        for j in (i + 1)..n {
            let value = cosine(&embeddings[i], &embeddings[j]);
            similarity[i][j] = value;
            similarity[j][i] = value;
        }
    }

    let mut active = vec![true; n];
    let mut size = vec![1.0_f32; n];
    let mut label: Vec<usize> = (0..n).collect();
    let mut count = n;

    while count > 1 {
        let mut best = (f32::NEG_INFINITY, 0usize, 1usize);
        for i in 0..n {
            if !active[i] {
                continue;
            }
            for j in (i + 1)..n {
                if active[j] && similarity[i][j] > best.0 {
                    best = (similarity[i][j], i, j);
                }
            }
        }
        let (pair_similarity, a, b) = best;
        if pair_similarity < threshold && count <= max_speakers.max(1) {
            break;
        }

        let (size_a, size_b) = (size[a], size[b]);
        for k in 0..n {
            if !active[k] || k == a || k == b {
                continue;
            }
            let merged =
                (size_a * similarity[a][k] + size_b * similarity[b][k]) / (size_a + size_b);
            similarity[a][k] = merged;
            similarity[k][a] = merged;
        }
        size[a] += size[b];
        active[b] = false;
        for value in &mut label {
            if *value == b {
                *value = a;
            }
        }
        count -= 1;
    }

    // Compact in order of first appearance: the earliest region becomes S1.
    let mut remap: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    label
        .iter()
        .map(|root| {
            let next = remap.len();
            *remap.entry(*root).or_insert(next)
        })
        .collect()
}

impl DiarizationProvider for PyannoteDiarizer {
    fn info(&self) -> ProviderInfo {
        self.info.clone()
    }

    fn diarize(&self, audio: &Audio, cfg: &DiarizationConfig) -> ProviderResult<DiarizationOutput> {
        if audio.sample_rate != RATE {
            return Err(ProviderError::Unsupported(format!(
                "diarization requires {RATE} Hz audio, got {} Hz",
                audio.sample_rate
            )));
        }
        if audio.is_empty() {
            return Ok(DiarizationOutput::default());
        }

        let min_region_s = cfg.min_region_ms as f32 / 1_000.0;
        let (regions, overlap_detected) = self.local_regions(audio, min_region_s)?;
        if regions.is_empty() {
            return Ok(DiarizationOutput {
                turns: Vec::new(),
                overlap_detected,
            });
        }

        // Several short embeddings per region beat one long one: a 10 s class
        // run can straddle a turn, and a single vector then represents two
        // people. The region keeps the majority label of its chunks.
        let mut region_embeddings: Vec<Vec<Vec<f32>>> = Vec::with_capacity(regions.len());
        for region in &regions {
            let mut chunks = Vec::new();
            for (begin, end) in chunk_bounds(region.start_s, region.end_s) {
                let start = (begin * RATE as f32) as usize;
                let end = ((end * RATE as f32) as usize).min(audio.samples.len());
                if end > start {
                    chunks.push(self.embed(&audio.samples[start..end])?);
                }
            }
            region_embeddings.push(chunks);
        }

        let mut flat: Vec<Vec<f32>> = Vec::new();
        for chunks in &region_embeddings {
            flat.extend(chunks.iter().cloned());
        }
        let labels = cluster_embeddings(&flat, cfg.threshold, cfg.max_speakers as usize);

        let mut region_labels = Vec::with_capacity(regions.len());
        let mut cursor = 0;
        for chunks in &region_embeddings {
            let slice = &labels[cursor..cursor + chunks.len()];
            cursor += chunks.len();
            region_labels.push(majority_label(slice));
        }

        let mut turns: Vec<DiarTurn> = Vec::with_capacity(regions.len());
        for (region, label) in regions.iter().zip(region_labels.iter()) {
            let turn = DiarTurn {
                range: TimeRange::new(
                    MediaTime::from_secs_f64(f64::from(region.start_s)),
                    MediaTime::from_secs_f64(f64::from(region.end_s)),
                ),
                speaker: SpeakerId::new(format!("S{}", label + 1)),
                confidence: Confidence::new(region.probability.clamp(0.0, 1.0)),
            };
            match turns.last_mut() {
                Some(previous) if previous.speaker == turn.speaker => {
                    // Stitch regions across window boundaries and short gaps.
                    let gap = turn.range.start.saturating_sub(previous.range.end);
                    if gap <= MediaTime::from_millis(200) {
                        previous.range = previous.range.union(&turn.range);
                        previous.confidence = match (previous.confidence, turn.confidence) {
                            (Some(a), Some(b)) => Confidence::new(a.get().max(b.get())),
                            (a, b) => a.or(b),
                        };
                        continue;
                    }
                    turns.push(turn);
                }
                _ => turns.push(turn),
            }
        }

        Ok(DiarizationOutput {
            turns,
            overlap_detected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(v: &[f32]) -> Vec<f32> {
        let mut v = v.to_vec();
        l2_normalize(&mut v);
        v
    }

    #[test]
    fn clusters_similar_embeddings() {
        let embeddings = vec![
            unit(&[1.0, 0.0, 0.0]),
            unit(&[1.0, 0.1, 0.0]),
            unit(&[0.0, 1.0, 0.0]),
            unit(&[0.1, 1.0, 0.0]),
        ];
        let labels = cluster_embeddings(&embeddings, 0.8, 8);
        assert_eq!(labels[0], labels[1]);
        assert_eq!(labels[2], labels[3]);
        assert_ne!(labels[0], labels[2]);
        assert_eq!(labels[0], 0, "earliest region is S1");
    }

    #[test]
    fn respects_speaker_budget() {
        let embeddings = vec![unit(&[1.0, 0.0]), unit(&[0.0, 1.0]), unit(&[-1.0, 0.0])];
        let labels = cluster_embeddings(&embeddings, 0.99, 2);
        assert!(labels.iter().max().copied().unwrap_or(0) < 2);
    }

    #[test]
    fn average_linkage_does_not_chain() {
        // 0°, 45°, 90°. Single linkage at 0.6 would merge all three through the
        // middle point; average linkage must keep the two ends apart.
        let embeddings = vec![unit(&[1.0, 0.0]), unit(&[1.0, 1.0]), unit(&[0.0, 1.0])];
        let labels = cluster_embeddings(&embeddings, 0.6, 8);
        assert_ne!(labels[0], labels[2], "opposite ends must not chain");
    }

    #[test]
    fn chunks_cover_a_region_without_slivers() {
        let bounds = chunk_bounds(0.0, 10.0);
        assert!(bounds.len() > 1);
        assert_eq!(bounds.first().unwrap().0, 0.0);
        assert_eq!(bounds.last().unwrap().1, 10.0);
        for (start, end) in &bounds {
            assert!(end > start);
            assert!(*end - *start >= EMBED_MIN_S, "sliver {start}..{end}");
        }
        // A short region is one chunk; a tiny one is not sliced at all.
        assert_eq!(chunk_bounds(5.0, 6.0), vec![(5.0, 6.0)]);
        assert_eq!(chunk_bounds(5.0, 5.4), vec![(5.0, 5.4)]);
    }

    #[test]
    fn majority_label_prefers_the_earliest_on_a_tie() {
        assert_eq!(majority_label(&[2, 2, 1]), 2);
        assert_eq!(majority_label(&[1, 0]), 0);
        assert_eq!(majority_label(&[]), 0);
    }
}
