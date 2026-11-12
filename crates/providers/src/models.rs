//! Model registry and local model store.
//!
//! Every model is pinned by SHA-256 and carries its license and provenance.
//! Downloads are verified before they are made visible, so a partial or
//! tampered file can never be fed to a model runtime.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use sonolytica_core::{Capability, ModelRef};

use crate::traits::ProviderError;
use crate::traits::ProviderResult;

#[derive(Debug, Clone, Copy)]
pub struct ModelSpec {
    pub id: &'static str,
    pub capability: Capability,
    pub filename: &'static str,
    pub url: &'static str,
    /// Lowercase hex SHA-256 of the artifact.
    pub sha256: &'static str,
    pub license: &'static str,
    pub approx_bytes: u64,
    pub notes: &'static str,
}

impl ModelSpec {
    pub fn reference(&self) -> ModelRef {
        ModelRef {
            id: self.id.to_string(),
            version: None,
            sha256: Some(self.sha256.to_string()),
            license: Some(self.license.to_string()),
        }
    }
}

/// Models known to SONOLYTICA. This is a registry, not a download list: nothing
/// is fetched unless the user asks for it.
pub const MODELS: &[ModelSpec] = &[
    ModelSpec {
        id: "silero-vad-v5",
        capability: Capability::Vad,
        filename: "silero_vad.onnx",
        url: "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx",
        sha256: "1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3",
        license: "MIT",
        approx_bytes: 2_300_000,
        notes: "Voice activity detection, 16 kHz mono, 512-sample windows.",
    },
    ModelSpec {
        id: "whisper-tiny.en",
        capability: Capability::Transcribe,
        filename: "ggml-tiny.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin",
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        license: "MIT",
        approx_bytes: 78_000_000,
        notes: "English-only, fastest. Useful for smoke tests, not production accuracy.",
    },
    ModelSpec {
        id: "whisper-base.en",
        capability: Capability::Transcribe,
        filename: "ggml-base.en.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin",
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        license: "MIT",
        approx_bytes: 148_000_000,
        notes: "English-only baseline; good speed/accuracy trade-off on CPU.",
    },
    ModelSpec {
        id: "whisper-small",
        capability: Capability::Transcribe,
        filename: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
        license: "MIT",
        approx_bytes: 488_000_000,
        notes: "Multilingual; the default for non-English audio.",
    },
    ModelSpec {
        id: "wav2vec2-base-960h",
        capability: Capability::Align,
        filename: "wav2vec2-base-960h.onnx",
        url: "https://huggingface.co/Xenova/wav2vec2-base-960h/resolve/a19f851b3d42865797e410752b4c570c871e4825/onnx/model.onnx",
        sha256: "e46614273f03ff4b87923a965e417fa72004825522cb007c9c25633b8475490d",
        license: "Apache-2.0",
        approx_bytes: 377_887_594,
        notes: "CTC emissions for forced word alignment; 16 kHz mono, 20 ms frames, \
                32-class uppercase English vocabulary. English-only, 30 s per pass.",
    },
    ModelSpec {
        id: "sortformer-4spk-v2.1",
        capability: Capability::Diarize,
        filename: "sortformer-4spk-v2.1.onnx",
        url: "https://huggingface.co/soniqo/Sortformer-Diarization-4spk-ONNX/resolve/main/sortformer-default.onnx",
        sha256: "cc5d606a5e9f6381c736d753bac34875c436bb27210d1ab0adc4ccb952c0062a",
        license: "NVIDIA Open Model License",
        approx_bytes: 474_630_246,
        notes: "Streaming 4-speaker diarization with the native Arrival-Order Speaker Cache: \
                 128-bin log-mel in, 80 ms activity out, 340-frame chunks and a 188-frame \
                 speaker cache. ONNX export of nvidia/diar_streaming_sortformer_4spk-v2.1 \
                 with no retraining. Attribution in NOTICE.",
    },
    ModelSpec {
        id: "pyannote-segmentation-3.0",
        capability: Capability::Diarize,
        filename: "segmentation-3.0.onnx",
        url: "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/segmentation-3.0.onnx",
        sha256: "b78fc48113bb46fd247ae6a9aea737079550c647638db961df7e0e1e9f4ba62e",
        license: "MIT",
        approx_bytes: 6_000_000,
        notes: "Local speaker change detection (powerset classes) on 10 s windows.",
    },
    ModelSpec {
        id: "wespeaker-campplus",
        capability: Capability::Diarize,
        filename: "wespeaker_en_voxceleb_CAM++.onnx",
        url: "https://github.com/thewh1teagle/pyannote-rs/releases/download/v0.1.0/wespeaker_en_voxceleb_CAM%2B%2B.onnx",
        sha256: "c46fad10b5f81e1aa4a60c162714208577093655076c5450f8c469e522ec54ef",
        license: "Apache-2.0",
        approx_bytes: 28_000_000,
        notes: "Speaker embedding model; clustering is done by SONOLYTICA.",
    },
    ModelSpec {
        id: "pp-ocrv5-det",
        capability: Capability::Vision,
        filename: "pp_ocrv5_det.onnx",
        url: "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx/resolve/e6f4fa85f00e168c862bc462aebca69eef9b3d3d/inference.onnx",
        sha256: "a431985659dc921974177a95adcfbb90fd9e51989a5e04d70d0b75f597b6e61d",
        license: "Apache-2.0",
        approx_bytes: 4_826_518,
        notes: "PP-OCRv5 mobile text detection (DB), ONNX Runtime, CPU.",
    },
    ModelSpec {
        id: "pp-ocrv5-rec",
        capability: Capability::Vision,
        filename: "pp_ocrv5_rec.onnx",
        url: "https://huggingface.co/PaddlePaddle/PP-OCRv5_mobile_rec_onnx/resolve/ed152b8b495f84de93cda5709d768548a9127622/inference.onnx",
        sha256: "da72dc72ca4dc220df0dfde68c1dedc31c58d3e76a25871122e5056227d50092",
        license: "Apache-2.0",
        approx_bytes: 16_534_782,
        notes: "PP-OCRv5 mobile CTC recognition; 18383-character dictionary shipped with the crate.",
    },
    ModelSpec {
        id: "yunet-face-detection",
        capability: Capability::Vision,
        filename: "yunet_face_detection_2023mar.onnx",
        url: "https://media.githubusercontent.com/media/opencv/opencv_zoo/48b10fa505aeb51fbd127fc60a5c660040e823b7/models/face_detection_yunet/face_detection_yunet_2023mar.onnx",
        sha256: "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
        license: "MIT",
        approx_bytes: 232_589,
        notes: "YuNet face detection (OpenCV Zoo). Count and boxes only; no identity, no embeddings.",
    },
    ModelSpec {
        id: "movenet-multipose",
        capability: Capability::Vision,
        filename: "movenet-multipose.onnx",
        url: "https://huggingface.co/Xenova/movenet-multipose-lightning/resolve/main/onnx/model.onnx",
        sha256: "60f724a56f0119d22d2afcf530019db307c77d4ea598eb5bd53bb65afe93930d",
        license: "Apache-2.0",
        approx_bytes: 19_049_169,
        notes: "Body pose on extracted keyframes: up to 6 people, 17 COCO keypoints. \
                 Detection only; no tracking, identity or per-frame coverage.",
    },
    ModelSpec {
        id: "yolos-tiny",
        capability: Capability::Vision,
        filename: "yolos_tiny.onnx",
        url: "https://huggingface.co/Xenova/yolos-tiny/resolve/e2f9c7673f0fa61849efe2b56a0d7774779ebb9d/onnx/model.onnx",
        sha256: "0dbdd37573fb6b6ed836aa980ab729c81b659deec900799f37b6a09a57762e88",
        license: "Apache-2.0",
        approx_bytes: 26_227_993,
        notes: "YOLOS-tiny COCO object detection (91 classes). Boxes and scores only; no tracking or attributes.",
    },
];

pub fn find_model(id: &str) -> Option<&'static ModelSpec> {
    MODELS.iter().find(|m| m.id == id)
}

pub fn models_for(capability: Capability) -> impl Iterator<Item = &'static ModelSpec> {
    MODELS.iter().filter(move |m| m.capability == capability)
}

/// Default location: `$SONOLYTICA_MODEL_DIR` or `$HOME/.cache/sonolytica/models`.
pub fn default_model_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("SONOLYTICA_MODEL_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join(".cache").join("sonolytica").join("models")
}

#[derive(Debug, Clone)]
pub struct ModelStore {
    dir: PathBuf,
}

impl Default for ModelStore {
    fn default() -> Self {
        Self::new(default_model_dir())
    }
}

impl ModelStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    pub fn path(&self, spec: &ModelSpec) -> PathBuf {
        self.dir.join(spec.filename)
    }

    pub fn is_present(&self, spec: &ModelSpec) -> bool {
        self.path(spec).is_file()
    }

    /// Verify the artifact digest against the pinned value.
    pub fn verify(&self, spec: &ModelSpec) -> ProviderResult<bool> {
        let path = self.path(spec);
        if !path.is_file() {
            return Ok(false);
        }
        if spec.sha256.is_empty() {
            return Err(ProviderError::Inference(format!(
                "model `{}` has no pinned digest in this build; refusing to trust it",
                spec.id
            )));
        }
        let actual = sha256_of(&path)?;
        Ok(actual == spec.sha256)
    }

    /// Path to a verified model, or a `ModelMissing` error naming the fix.
    pub fn require(&self, spec: &ModelSpec) -> ProviderResult<PathBuf> {
        let path = self.path(spec);
        if !path.is_file() {
            return Err(ProviderError::ModelMissing {
                id: spec.id.to_string(),
                hint: format!("run `sonolytica models fetch {}`", spec.id),
            });
        }
        if !self.verify(spec)? {
            return Err(ProviderError::ModelMissing {
                id: spec.id.to_string(),
                hint: format!(
                    "{} failed digest verification; delete it and run `sonolytica models fetch {}`",
                    path.display(),
                    spec.id
                ),
            });
        }
        Ok(path)
    }

    /// Download, verify, and atomically install a model.
    pub fn fetch(&self, spec: &ModelSpec) -> ProviderResult<PathBuf> {
        if spec.sha256.is_empty() {
            return Err(ProviderError::Inference(format!(
                "model `{}` has no pinned digest; cannot verify a download",
                spec.id
            )));
        }
        let final_path = self.path(spec);
        if final_path.is_file() && self.verify(spec).unwrap_or(false) {
            return Ok(final_path);
        }

        std::fs::create_dir_all(&self.dir).map_err(|e| ProviderError::Io(e.to_string()))?;
        let partial = self.dir.join(format!("{}.partial", spec.filename));

        tracing::info!(model = spec.id, url = spec.url, "downloading model");
        let response = ureq::get(spec.url)
            .header("User-Agent", "sonolytica/0.1")
            .call()
            .map_err(|e| ProviderError::Io(format!("download failed: {e}")))?;

        let mut reader = response.into_body().into_reader();
        let mut file = std::fs::File::create(&partial)
            .map_err(|e| ProviderError::Io(format!("cannot write {}: {e}", partial.display())))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 256 * 1024];
        let mut written = 0_u64;
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| ProviderError::Io(format!("download interrupted: {e}")))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            std::io::Write::write_all(&mut file, &buf[..n])
                .map_err(|e| ProviderError::Io(e.to_string()))?;
            written += n as u64;
        }
        drop(file);

        let actual = format!("{:x}", hasher.finalize());
        if actual != spec.sha256 {
            let _ = std::fs::remove_file(&partial);
            return Err(ProviderError::Inference(format!(
                "digest mismatch for {}: expected {}, got {} (download discarded)",
                spec.id, spec.sha256, actual
            )));
        }

        std::fs::rename(&partial, &final_path).map_err(|e| ProviderError::Io(e.to_string()))?;
        tracing::info!(model = spec.id, bytes = written, "model installed");
        Ok(final_path)
    }

    /// `(spec, present, verified)` for every known model.
    pub fn status(&self) -> Vec<(&'static ModelSpec, bool, bool)> {
        MODELS
            .iter()
            .map(|spec| {
                let present = self.path(spec).is_file();
                let verified = present && self.verify(spec).unwrap_or(false);
                (spec, present, verified)
            })
            .collect()
    }
}

fn sha256_of(path: &Path) -> ProviderResult<String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| ProviderError::Io(format!("{path:?}: {e}")))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| ProviderError::Io(e.to_string()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_ids_are_unique_and_non_empty() {
        let mut seen = std::collections::BTreeSet::new();
        for spec in MODELS {
            assert!(seen.insert(spec.id), "duplicate model id {}", spec.id);
            assert!(spec.filename.ends_with(".bin") || spec.filename.ends_with(".onnx"));
        }
    }

    #[test]
    fn missing_model_reports_the_fix() {
        let store = ModelStore::new("/nonexistent/sonolytica/models");
        let spec = &MODELS[0];
        match store.require(spec) {
            Err(ProviderError::ModelMissing { hint, .. }) => {
                assert!(hint.contains("models fetch"));
            }
            other => panic!("expected ModelMissing, got {other:?}"),
        }
    }

    #[test]
    fn unpinned_model_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let store = ModelStore::new(dir.path());
        let spec = ModelSpec {
            sha256: "",
            ..MODELS[0]
        };
        std::fs::write(store.path(&spec), b"x").unwrap();
        assert!(store.verify(&spec).is_err());
    }
}
