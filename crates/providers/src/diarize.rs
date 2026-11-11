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
//! 3. Single-speaker regions long enough to embed get a WeSpeaker embedding.
//! 4. Embeddings are clustered by cosine similarity with single linkage; if
//!    the result exceeds `max_speakers`, the closest clusters are merged.
//! 5. Adjacent regions of the same speaker are stitched.

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

#[derive(Debug, Clone)]
struct LocalRegion {
    start_s: f32,
    end_s: f32,
    probability: f32,
    embedding: Vec<f32>,
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
                notes: "10 s non-overlapping windows; overlap is detected and reported but \
                        not attributed to two speakers.",
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
                            embedding: Vec::new(),
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

/// Single-linkage clustering by cosine similarity, then reduce to `max_speakers`
/// by merging the most similar clusters. Deterministic for a given input.
fn cluster_embeddings(embeddings: &[Vec<f32>], threshold: f32, max_speakers: usize) -> Vec<usize> {
    let n = embeddings.len();
    if n == 0 {
        return Vec::new();
    }
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if cosine(&embeddings[i], &embeddings[j]) >= threshold {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                if a != b {
                    parent[b] = a;
                }
            }
        }
    }

    let roots: Vec<usize> = (0..n).map(|i| find(&mut parent, i)).collect();
    let mut labels: Vec<usize> = {
        let mut map: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        roots
            .iter()
            .map(|root| {
                let next = map.len();
                *map.entry(*root).or_insert(next)
            })
            .collect()
    };

    // Merge the closest clusters until the speaker budget is respected.
    // Labels are compacted every round: merging a non-maximal label would
    // otherwise leave a gap and spin forever.
    loop {
        let mut compact: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for label in &mut labels {
            let next = compact.len();
            *label = *compact.entry(*label).or_insert(next);
        }
        let count = compact.len();
        if count <= max_speakers.max(1) || count <= 1 {
            break;
        }

        let dims = embeddings[0].len();
        let mut centroids = vec![vec![0.0_f32; dims]; count];
        for (embedding, &label) in embeddings.iter().zip(labels.iter()) {
            for (c, v) in centroids[label].iter_mut().zip(embedding.iter()) {
                *c += *v;
            }
        }
        for centroid in &mut centroids {
            l2_normalize(centroid);
        }

        let mut best = (f32::MIN, 0usize, 1usize);
        for a in 0..count {
            for b in (a + 1)..count {
                let similarity = cosine(&centroids[a], &centroids[b]);
                if similarity > best.0 {
                    best = (similarity, a, b);
                }
            }
        }
        let (_, a, b) = best;
        for label in &mut labels {
            if *label == b {
                *label = a;
            }
        }
    }

    labels
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
        let (mut regions, overlap_detected) = self.local_regions(audio, min_region_s)?;
        if regions.is_empty() {
            return Ok(DiarizationOutput {
                turns: Vec::new(),
                overlap_detected,
            });
        }

        for region in &mut regions {
            let start = (region.start_s * RATE as f32) as usize;
            let end = ((region.end_s * RATE as f32) as usize).min(audio.samples.len());
            region.embedding = self.embed(&audio.samples[start..end])?;
        }

        let embeddings: Vec<Vec<f32>> = regions.iter().map(|r| r.embedding.clone()).collect();
        let labels = cluster_embeddings(&embeddings, cfg.threshold, cfg.max_speakers as usize);

        let mut turns: Vec<DiarTurn> = Vec::with_capacity(regions.len());
        for (region, label) in regions.iter().zip(labels.iter()) {
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
}
