//! Whisper speech recognition through whisper.cpp (`whisper-rs`).
//!
//! whisper.cpp is used rather than a pure-Rust implementation because it is
//! the fastest maintained local Whisper on Apple Silicon (Metal) and CPU, and
//! it exposes token-level timestamps, which word timing depends on. The build
//! needs a C/C++ toolchain and `cmake`; the crate is behind the `whisper`
//! feature so the rest of the workspace builds without them.

use std::path::Path;
use std::sync::OnceLock;

use sonolytica_core::{Audio, Capability, Confidence, MediaTime, TimeRange, Word};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::models::ModelSpec;
use crate::traits::{
    AsrConfig, AsrOutput, AsrProvider, AsrSegment, ProviderError, ProviderInfo, ProviderResult,
};

const WHISPER_RATE: u32 = 16_000;

pub struct WhisperAsr {
    context: WhisperContext,
    info: ProviderInfo,
}

impl WhisperAsr {
    pub fn load(spec: &'static ModelSpec, path: &Path) -> ProviderResult<Self> {
        // Route whisper.cpp/GGML logs into the `log` facade. Without a backend
        // enabled this silences the C library's stderr chatter, which would
        // otherwise corrupt a JSON document printed to stdout.
        static LOG_HOOKS: OnceLock<()> = OnceLock::new();
        LOG_HOOKS.get_or_init(whisper_rs::install_logging_hooks);

        let context = WhisperContext::new_with_params(
            path.to_str()
                .ok_or_else(|| ProviderError::Io("model path is not valid UTF-8".into()))?,
            WhisperContextParameters::default(),
        )
        .map_err(|e| ProviderError::Inference(format!("could not load Whisper model: {e}")))?;

        Ok(Self {
            context,
            info: ProviderInfo {
                name: "whisper-cpp",
                capability: Capability::Transcribe,
                device: if cfg!(feature = "metal") {
                    "metal"
                } else {
                    "cpu"
                },
                models: vec![spec.reference()],
                notes: "16 kHz mono. Word timings come from Whisper token timestamps \
                        (10 ms resolution), not forced alignment.",
            },
        })
    }
}

fn clean_token_text(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<|")
        || trimmed.starts_with("[_")
        || trimmed.starts_with("_[")
    {
        None
    } else {
        Some(trimmed)
    }
}

fn clamp_range(range: TimeRange, limit: MediaTime) -> Option<TimeRange> {
    let start = range.start.min(limit);
    let end = range.end.min(limit);
    (end > start).then_some(TimeRange::new(start, end))
}

fn segment_words(segment: &whisper_rs::WhisperSegment<'_>) -> ProviderResult<Vec<Word>> {
    let mut words: Vec<Word> = Vec::new();
    let mut current = String::new();
    let mut start_cs: Option<i64> = None;
    let mut end_cs: i64 = 0;
    let mut prob_sum = 0.0_f32;
    let mut prob_count = 0_u32;

    let flush = |words: &mut Vec<Word>,
                 current: &mut String,
                 start_cs: &mut Option<i64>,
                 end_cs: i64,
                 prob_sum: f32,
                 prob_count: u32| {
        let text = current.trim().to_string();
        if !text.is_empty() {
            if let Some(start) = *start_cs {
                let confidence = if prob_count > 0 {
                    Confidence::new((prob_sum / prob_count as f32).clamp(0.0, 1.0))
                } else {
                    None
                };
                words.push(Word {
                    text,
                    range: TimeRange::new(
                        MediaTime::from_centis(start),
                        MediaTime::from_centis(end_cs.max(start)),
                    ),
                    confidence,
                });
            }
        }
        current.clear();
        *start_cs = None;
    };

    let token_count = segment.n_tokens();
    for index in 0..token_count {
        let token = segment
            .get_token(index)
            .ok_or_else(|| ProviderError::Inference("token index out of bounds".into()))?;
        let raw = token
            .to_str_lossy()
            .map_err(|e| ProviderError::Inference(e.to_string()))?;
        let Some(text) = clean_token_text(&raw) else {
            continue;
        };
        let data = token.token_data();
        let t0 = if data.t0 >= 0 {
            data.t0
        } else {
            segment.start_timestamp()
        };
        let t1 = if data.t1 >= 0 {
            data.t1
        } else {
            segment.end_timestamp()
        };
        let starts_new_word = current.is_empty() || raw.starts_with(' ');
        if starts_new_word && !current.is_empty() {
            flush(
                &mut words,
                &mut current,
                &mut start_cs,
                end_cs,
                prob_sum,
                prob_count,
            );
            prob_sum = 0.0;
            prob_count = 0;
        }
        if start_cs.is_none() {
            start_cs = Some(t0);
        }
        current.push_str(text);
        current.push(' ');
        end_cs = t1;
        prob_sum += token.token_probability();
        prob_count += 1;
    }
    flush(
        &mut words,
        &mut current,
        &mut start_cs,
        end_cs,
        prob_sum,
        prob_count,
    );

    Ok(words)
}

impl WhisperAsr {
    /// Decode one slice of audio and shift its timestamps onto the media
    /// timeline by `offset`.
    fn transcribe_slice(
        &self,
        samples: &[f32],
        offset: MediaTime,
        media_limit: MediaTime,
        cfg: &AsrConfig,
    ) -> ProviderResult<(Option<String>, Vec<AsrSegment>)> {
        let mut state = self.context.create_state().map_err(|e| {
            ProviderError::Inference(format!("could not create Whisper state: {e}"))
        })?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(cfg.threads as i32);
        params.set_language(cfg.language.as_deref());
        params.set_translate(cfg.translate_to_english);
        params.set_token_timestamps(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_no_context(true);
        params.set_no_speech_thold(0.6);

        state
            .full(params, samples)
            .map_err(|e| ProviderError::Inference(format!("Whisper inference failed: {e}")))?;

        let n_segments = state.full_n_segments();
        let mut segments = Vec::with_capacity(n_segments.max(0) as usize);
        // Whisper pads its input to the encoder window and its timestamp
        // tokens can therefore point past the slice. Timestamps beyond the
        // media are never emitted.
        let limit = media_limit;

        for index in 0..n_segments {
            let Some(segment) = state.get_segment(index) else {
                continue;
            };
            let text = segment
                .to_str_lossy()
                .map_err(|e| ProviderError::Inference(e.to_string()))?
                .trim()
                .to_string();
            if text.is_empty() {
                continue;
            }

            let mut prob_sum = 0.0_f32;
            let mut prob_count = 0_u32;
            for token_index in 0..segment.n_tokens() {
                if let Some(token) = segment.get_token(token_index) {
                    prob_sum += token.token_probability();
                    prob_count += 1;
                }
            }
            let confidence = if prob_count > 0 {
                Confidence::new((prob_sum / prob_count as f32).clamp(0.0, 1.0))
            } else {
                None
            };

            let mut words = Vec::new();
            for mut word in segment_words(&segment)? {
                let shifted = TimeRange::new(
                    offset.saturating_add(word.range.start),
                    offset.saturating_add(word.range.end),
                );
                if let Some(range) = clamp_range(shifted, limit) {
                    word.range = range;
                    words.push(word);
                }
            }

            let Some(range) = clamp_range(
                TimeRange::new(
                    offset.saturating_add(MediaTime::from_centis(segment.start_timestamp())),
                    offset.saturating_add(MediaTime::from_centis(segment.end_timestamp())),
                ),
                limit,
            ) else {
                continue;
            };

            segments.push(AsrSegment {
                range,
                text,
                words,
                confidence,
                no_speech_prob: Some(segment.no_speech_probability()),
            });
        }

        let lang_id = state.full_lang_id_from_state();
        let language = whisper_rs::get_lang_str(lang_id).map(str::to_string);
        Ok((language, segments))
    }
}

impl AsrProvider for WhisperAsr {
    fn info(&self) -> ProviderInfo {
        self.info.clone()
    }

    fn transcribe(&self, audio: &Audio, cfg: &AsrConfig) -> ProviderResult<AsrOutput> {
        if audio.sample_rate != WHISPER_RATE {
            return Err(ProviderError::Unsupported(format!(
                "Whisper requires {} Hz audio, got {} Hz",
                WHISPER_RATE, audio.sample_rate
            )));
        }
        if audio.is_empty() {
            return Ok(AsrOutput::default());
        }

        let media_limit = audio.duration();
        if cfg.regions.is_empty() {
            let (language, segments) =
                self.transcribe_slice(&audio.samples, MediaTime::ZERO, media_limit, cfg)?;
            return Ok(AsrOutput { language, segments });
        }

        let mut language: Option<String> = None;
        let mut segments = Vec::new();
        for region in &cfg.regions {
            let slice = audio.samples_in(*region);
            if slice.len() < WHISPER_RATE as usize / 20 {
                continue;
            }
            // Whisper drops the first word of a chunk that starts on speech.
            // A short digital-silence lead-in fixes that; timestamps stay on
            // the media timeline because the offset includes the padding.
            let pad = MediaTime::from_millis(300);
            let lead_time = region.start.min(pad);
            let lead_samples = lead_time.to_sample(WHISPER_RATE);
            let offset = region.start.saturating_sub(lead_time);
            let mut padded = Vec::with_capacity(lead_samples + slice.len());
            padded.resize(lead_samples, 0.0);
            padded.extend_from_slice(slice);

            match self.transcribe_slice(&padded, offset, media_limit, cfg) {
                Ok((region_language, mut region_segments)) => {
                    if language.is_none() {
                        language = region_language;
                    }
                    segments.append(&mut region_segments);
                }
                Err(error) => {
                    // One bad region must not discard the others; surface the
                    // error only if every region failed.
                    tracing::warn!(?region, %error, "region transcription failed");
                }
            }
        }
        if segments.is_empty() && !cfg.regions.is_empty() {
            // Fall back to the full buffer once, so a VAD that is too strict
            // cannot silently produce an empty transcript.
            let (language, segments) =
                self.transcribe_slice(&audio.samples, MediaTime::ZERO, media_limit, cfg)?;
            return Ok(AsrOutput { language, segments });
        }
        segments.sort_by_key(|s| s.range.start);
        Ok(AsrOutput { language, segments })
    }
}

/// Defers loading the model until the first transcription. Discovery can list
/// several ASR models without paying their memory cost; only the model the
/// caller actually selects is loaded.
pub struct LazyWhisperAsr {
    spec: &'static ModelSpec,
    path: std::path::PathBuf,
    inner: OnceLock<Result<WhisperAsr, String>>,
}

impl LazyWhisperAsr {
    pub fn new(spec: &'static ModelSpec, path: std::path::PathBuf) -> Self {
        Self {
            spec,
            path,
            inner: OnceLock::new(),
        }
    }

    fn get(&self) -> ProviderResult<&WhisperAsr> {
        match self.inner.get_or_init(|| {
            WhisperAsr::load(self.spec, &self.path).map_err(|error| error.to_string())
        }) {
            Ok(provider) => Ok(provider),
            Err(message) => Err(ProviderError::Inference(message.clone())),
        }
    }
}

impl AsrProvider for LazyWhisperAsr {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            name: "whisper-cpp",
            capability: Capability::Transcribe,
            device: if cfg!(feature = "metal") {
                "metal"
            } else {
                "cpu"
            },
            models: vec![self.spec.reference()],
            notes: "16 kHz mono, speech-region decoding. Word timings come from \
                    Whisper token timestamps (10 ms resolution), not forced alignment.",
        }
    }

    fn transcribe(&self, audio: &Audio, cfg: &AsrConfig) -> ProviderResult<AsrOutput> {
        self.get()?.transcribe(audio, cfg)
    }
}

#[cfg(test)]
mod tests {
    use super::clean_token_text;

    #[test]
    fn drops_control_tokens_but_keeps_words() {
        assert_eq!(clean_token_text(" Hello"), Some("Hello"));
        assert_eq!(clean_token_text("  "), None);
        assert_eq!(clean_token_text("<|endoftext|>"), None);
        assert_eq!(clean_token_text("[_BEG_]"), None);
    }
}
