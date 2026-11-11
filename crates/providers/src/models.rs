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
        sha256: "",
        license: "MIT",
        approx_bytes: 488_000_000,
        notes: "Multilingual; the default for non-English audio.",
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
