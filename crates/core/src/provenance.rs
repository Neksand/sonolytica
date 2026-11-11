//! Provenance: who produced what, with which model, under which parameters.

use serde::{Deserialize, Serialize};

use crate::config::Capability;

pub type RunId = u32;

/// Identity of a model artifact. `sha256` is the only field that actually
/// pins behaviour; ids and versions are for humans and tooling.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRef {
    pub id: String,
    pub version: Option<String>,
    pub sha256: Option<String>,
    pub license: Option<String>,
}

impl ModelRef {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            version: None,
            sha256: None,
            license: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RunStatus {
    Complete,
    /// The run produced usable output but not everything requested.
    Partial {
        reason: String,
    },
    Failed {
        reason: String,
    },
}

impl RunStatus {
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Complete | Self::Partial { .. })
    }
}

/// One execution of one provider for one capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: RunId,
    pub capability: Capability,
    /// Provider name, e.g. `whisper-cpp` or `sonolytica-dsp`.
    pub provider: String,
    pub model: Option<ModelRef>,
    pub params_sha256: String,
    /// Execution device as reported by the provider, e.g. `cpu`, `metal`.
    pub device: String,
    pub started_at_unix_ms: u64,
    pub duration_ms: u64,
    pub status: RunStatus,
}

/// A non-fatal problem. Warnings are part of the output contract: an agent
/// must be able to see that, for example, diarization was skipped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Warning {
    pub code: String,
    pub message: String,
    pub run: Option<RunId>,
}

impl Warning {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            run: None,
        }
    }

    pub fn with_run(mut self, run: RunId) -> Self {
        self.run = Some(run);
        self
    }
}
