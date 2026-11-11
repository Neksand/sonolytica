//! Provider registry: what this build can actually run on this machine.
//!
//! Discovery is model-driven: a provider is registered only when its pinned
//! artifacts are present and their digests match. Missing pieces become
//! `unavailable` entries with a human-readable reason, which the engine turns
//! into document warnings instead of running a model it cannot trust.

use sonolytica_core::{Capability, ModelRef};

#[allow(unused_imports)]
use crate::models::{ModelStore, find_model};
use crate::traits::{AsrProvider, DiarizationProvider, ProviderInfo, VadProvider};

#[derive(Debug, Clone)]
pub struct Unavailable {
    pub provider: &'static str,
    pub capability: Capability,
    pub reason: String,
}

#[derive(Default)]
pub struct Registry {
    pub vads: Vec<Box<dyn VadProvider>>,
    pub asrs: Vec<Box<dyn AsrProvider>>,
    pub diarizers: Vec<Box<dyn DiarizationProvider>>,
    pub unavailable: Vec<Unavailable>,
}

impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("vad_providers", &self.vads.len())
            .field("asr_providers", &self.asrs.len())
            .field("diarization_providers", &self.diarizers.len())
            .field("unavailable", &self.unavailable)
            .finish()
    }
}

impl Registry {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build a registry from the models present in `store`.
    pub fn discover(store: ModelStore, threads: usize) -> Self {
        let _ = (threads, &store);
        let mut registry = Self::empty();

        #[cfg(feature = "silero")]
        {
            match find_model("silero-vad-v5") {
                Some(spec) => match store.require(spec) {
                    Ok(path) => match crate::silero::SileroVad::load(spec, &path, threads) {
                        Ok(provider) => registry.vads.push(Box::new(provider)),
                        Err(error) => registry.unavailable.push(Unavailable {
                            provider: "silero-vad",
                            capability: Capability::Vad,
                            reason: error.to_string(),
                        }),
                    },
                    Err(error) => registry.unavailable.push(Unavailable {
                        provider: "silero-vad",
                        capability: Capability::Vad,
                        reason: error.to_string(),
                    }),
                },
                None => registry.unavailable.push(Unavailable {
                    provider: "silero-vad",
                    capability: Capability::Vad,
                    reason: "model is not in this build's registry".into(),
                }),
            }
        }
        #[cfg(not(feature = "silero"))]
        registry.unavailable.push(Unavailable {
            provider: "silero-vad",
            capability: Capability::Vad,
            reason: "this build was compiled without the `silero` feature".into(),
        });

        #[cfg(feature = "whisper")]
        {
            for id in ["whisper-base.en", "whisper-tiny.en", "whisper-small"] {
                let Some(spec) = find_model(id) else {
                    continue;
                };
                if let Ok(path) = store.require(spec) {
                    // Loading is deferred to first use: discovery must not
                    // allocate hundreds of megabytes for models nobody asked
                    // for.
                    registry
                        .asrs
                        .push(Box::new(crate::whisper::LazyWhisperAsr::new(spec, path)));
                }
            }
            if registry.asrs.is_empty() {
                registry.unavailable.push(Unavailable {
                    provider: "whisper-cpp",
                    capability: Capability::Transcribe,
                    reason:
                        "no Whisper ggml model found; run `sonolytica models fetch whisper-base.en`"
                            .into(),
                });
            }
        }
        #[cfg(not(feature = "whisper"))]
        registry.unavailable.push(Unavailable {
            provider: "whisper-cpp",
            capability: Capability::Transcribe,
            reason: "this build was compiled without the `whisper` feature".into(),
        });

        #[cfg(feature = "diarize")]
        {
            // Sortformer is preferred: it resolves overlap and does not
            // over-segment 2-4 speaker recordings. Pyannote stays registered
            // as the fallback for files where the model is absent or fails.
            match find_model("sortformer-4spk-v2") {
                Some(spec) => match store.require(spec) {
                    Ok(path) => {
                        match crate::sortformer::SortformerDiarizer::load(spec, &path, threads) {
                            Ok(provider) => registry.diarizers.push(Box::new(provider)),
                            Err(error) => registry.unavailable.push(Unavailable {
                                provider: "sortformer-4spk-v2",
                                capability: Capability::Diarize,
                                reason: error.to_string(),
                            }),
                        }
                    }
                    Err(error) => registry.unavailable.push(Unavailable {
                        provider: "sortformer-4spk-v2",
                        capability: Capability::Diarize,
                        reason: error.to_string(),
                    }),
                },
                None => registry.unavailable.push(Unavailable {
                    provider: "sortformer-4spk-v2",
                    capability: Capability::Diarize,
                    reason: "model is not in this build's registry".into(),
                }),
            }

            match (
                find_model("pyannote-segmentation-3.0"),
                find_model("wespeaker-campplus"),
            ) {
                (Some(seg_spec), Some(emb_spec)) => {
                    let seg_path = store.require(seg_spec);
                    let emb_path = store.require(emb_spec);
                    match (seg_path, emb_path) {
                        (Ok(seg_path), Ok(emb_path)) => {
                            match crate::diarize::PyannoteDiarizer::load(
                                seg_spec, emb_spec, &seg_path, &emb_path, threads,
                            ) {
                                Ok(provider) => registry.diarizers.push(Box::new(provider)),
                                Err(error) => registry.unavailable.push(Unavailable {
                                    provider: "pyannote-3.0",
                                    capability: Capability::Diarize,
                                    reason: error.to_string(),
                                }),
                            }
                        }
                        (seg, emb) => {
                            let reason = seg.err().or(emb.err()).map(|e| e.to_string());
                            registry.unavailable.push(Unavailable {
                                provider: "pyannote-3.0",
                                capability: Capability::Diarize,
                                reason: reason.unwrap_or_else(|| "models unavailable".into()),
                            });
                        }
                    }
                }
                _ => registry.unavailable.push(Unavailable {
                    provider: "pyannote-3.0",
                    capability: Capability::Diarize,
                    reason: "diarization models are not in this build's registry".into(),
                }),
            }
        }
        #[cfg(not(feature = "diarize"))]
        registry.unavailable.push(Unavailable {
            provider: "pyannote-3.0",
            capability: Capability::Diarize,
            reason: "this build was compiled without the `diarize` feature".into(),
        });

        registry
    }

    pub fn vad(&self) -> Option<&dyn VadProvider> {
        self.vads.first().map(AsRef::as_ref)
    }

    /// Pick the ASR provider for an explicitly requested model id, otherwise
    /// the first registered one (discovery order is by preference).
    pub fn asr_for(&self, model_id: Option<&str>) -> Option<&dyn AsrProvider> {
        match model_id {
            Some(id) => self
                .asrs
                .iter()
                .find(|p| p.info().models.iter().any(|m| m.id == id))
                .map(AsRef::as_ref),
            None => self.asrs.first().map(AsRef::as_ref),
        }
    }

    pub fn diarizer(&self) -> Option<&dyn DiarizationProvider> {
        self.diarizers.first().map(AsRef::as_ref)
    }

    pub fn provider_infos(&self) -> Vec<ProviderInfo> {
        let mut out: Vec<ProviderInfo> = Vec::new();
        out.extend(self.vads.iter().map(|p| p.info()));
        out.extend(self.asrs.iter().map(|p| p.info()));
        out.extend(self.diarizers.iter().map(|p| p.info()));
        out
    }

    pub fn models_in_use(&self) -> Vec<ModelRef> {
        self.provider_infos()
            .into_iter()
            .flat_map(|info| info.models)
            .collect()
    }
}
