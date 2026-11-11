//! Shared ONNX Runtime environment.
//!
//! ONNX Runtime allows exactly one environment per process, and `ort` enforces
//! that. Both the VAD and the diarization models must therefore share a single
//! lazily initialised environment; creating one per provider fails with
//! "only one environment is allowed per process".

use std::path::Path;
use std::sync::OnceLock;

use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;

use crate::traits::{ProviderError, ProviderResult};

static ENV: OnceLock<Result<ort::environment::Environment, String>> = OnceLock::new();

pub(crate) fn environment() -> ProviderResult<&'static ort::environment::Environment> {
    match ENV.get_or_init(|| {
        ort::init()
            .with_name("sonolytica")
            .build()
            .map_err(|e| e.to_string())
    }) {
        Ok(env) => Ok(env),
        Err(message) => Err(ProviderError::Inference(format!(
            "could not initialise ONNX Runtime: {message}"
        ))),
    }
}

pub(crate) fn build_session(path: &Path, threads: usize) -> ProviderResult<Session> {
    let env = environment()?;
    Session::builder(env)
        .map_err(|e| ProviderError::Inference(e.to_string()))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| ProviderError::Inference(e.to_string()))?
        .with_intra_threads(threads.max(1))
        .map_err(|e| ProviderError::Inference(e.to_string()))?
        .commit_from_file(path)
        .map_err(|e| ProviderError::Inference(format!("could not load {}: {e}", path.display())))
}
