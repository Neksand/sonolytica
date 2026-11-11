//! Analysis orchestration.
//!
//! The engine owns three things: the order stages run in, the isolation between
//! them, and the assembly of their outputs into one [`Document`]. It knows
//! nothing about specific models — only the provider traits.
//!
//! Isolation rule: a failing optional stage records a `Failed` run and a
//! warning, and the rest of the pipeline continues. Only prerequisites (probe,
//! decode) are fatal.

#![allow(clippy::must_use_candidate)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]

pub mod error;
pub mod merge;

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use sonolytica_core::{
    AcousticSummary, AnalysisBlock, AnalysisConfig, Audio, Bpm, Capability, Chord, ChordQuality,
    Confidence, Document, DocumentBuilder, KeyEstimate, Loudness, MediaTime, Mode, ModelRef,
    MusicSummary, PitchClass, RunRecord, RunStatus, SegmentKind, SpeakerId, SpeechSummary,
    TimeRange, TrackKind, VadConfig, Warning, Word, now_unix_ms,
};
use sonolytica_dsp::FrameConfig;
use sonolytica_media::{
    DecodeOptions, FrameOptions, MediaTools, decode_mono, detect_shots, probe, sample_video,
    sha256_file, validate_input,
};
use sonolytica_providers::{
    AsrConfig, AsrOutput, AsrSegment, DiarTurn, DiarizationOutput, ModelStore, ProviderError,
    ProviderInfo, Registry, SpeechRegion,
};

pub use error::EngineError;
pub use merge::{Utterance, build_speaker_turns};

const SPEECH_RATE: u32 = 16_000;
const MUSIC_RATE: u32 = 32_000;

/// The analysis engine. One instance holds immutable provider state and can be
/// reused across many analyses.
pub struct Engine {
    registry: Arc<Registry>,
    tools: MediaTools,
    /// Wav2Vec2 forced aligner, loaded on first use. `None` when the build or
    /// the model store cannot provide one; alignment then degrades to Whisper
    /// word timings with a warning.
    aligner: Option<Arc<sonolytica_providers::align::LazyWav2Vec2Aligner>>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("registry", &self.registry)
            .field("tools", &self.tools)
            .field("aligner", &self.aligner)
            .finish()
    }
}

impl Engine {
    pub fn new(registry: Registry, tools: MediaTools) -> Self {
        Self {
            registry: Arc::new(registry),
            tools,
            aligner: None,
        }
    }

    /// Discover providers from the local model store.
    pub fn discover(store: ModelStore) -> Self {
        let threads = std::thread::available_parallelism()
            .map_or(4, std::num::NonZeroUsize::get)
            .min(8);
        // Presence only: digest verification and the 360 MB session load are
        // deferred until alignment is actually requested.
        let aligner = if sonolytica_providers::align::available(&store) {
            Some(Arc::new(
                sonolytica_providers::align::LazyWav2Vec2Aligner::new(store.clone(), threads),
            ))
        } else {
            None
        };
        Self {
            registry: Arc::new(Registry::discover(store, threads)),
            tools: MediaTools::detect(),
            aligner,
        }
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Analyse one media file. The returned document has passed every
    /// structural invariant, or an error is returned instead.
    pub async fn analyze(
        &self,
        path: &Path,
        config: &AnalysisConfig,
    ) -> Result<Document, EngineError> {
        let mut config = config.clone();
        let unsupported: Vec<Capability> = config
            .modules
            .iter()
            .copied()
            .filter(|c| matches!(c, Capability::Vision))
            .collect();
        config.normalize();

        let input = validate_input(path, &config.limits)?;
        let mut media = probe(&self.tools, &input.path, &config.limits).await?;
        media.path = input.path.display().to_string();
        media.size_bytes = Some(input.size_bytes);

        if config.hash_media {
            let hash_path = input.path.clone();
            media.sha256 = Some(
                blocking(move || sha256_file(&hash_path))
                    .await?
                    .map_err(EngineError::Media)?,
            );
        }

        let inspection_only = config
            .modules
            .iter()
            .all(|c| matches!(c, Capability::Inspect));
        let video_analysis = config.wants(Capability::Scene) && media.has_video();
        if !media.has_audio() && !inspection_only && !video_analysis {
            return Err(EngineError::NoAudio);
        }

        let mut builder = DocumentBuilder::new(media, config.clone());

        for capability in unsupported {
            builder.warn(Warning::new(
                "capability_not_implemented",
                format!("`{capability}` is not implemented in this release"),
            ));
        }

        for unavailable in &self.registry.unavailable {
            if config.wants(unavailable.capability) {
                builder.warn(Warning::new(
                    "provider_unavailable",
                    format!(
                        "{} ({}) is unavailable: {}",
                        unavailable.provider, unavailable.capability, unavailable.reason
                    ),
                ));
            }
        }

        if inspection_only {
            return builder.finish().map_err(|v| {
                EngineError::InvalidDocument(
                    v.iter()
                        .map(|i| format!("{}: {}", i.code, i.message))
                        .collect::<Vec<_>>()
                        .join("; "),
                )
            });
        }

        // --- scenes ---------------------------------------------------------
        // Video needs no model: FFmpeg decodes a small grayscale grid, and a
        // spike in the frame signature is a cut. This runs before audio so a
        // video-only file can still produce a document.
        if config.wants(Capability::Scene) && builder.document().media.has_video() {
            let options = FrameOptions {
                max_duration_secs: config.limits.max_duration_secs,
                timeout_secs: config.limits.timeout_secs,
                ..FrameOptions::default()
            };
            let started = (now_unix_ms(), Instant::now());
            match sample_video(&self.tools, &input.path, &options).await {
                Ok(frames) if !frames.is_empty() => {
                    let shots = detect_shots(&frames);
                    let run = builder.add_run(run_record(
                        Capability::Scene,
                        "sonolytica-media",
                        None,
                        "cpu",
                        &config,
                        started,
                        RunStatus::Complete,
                    ));
                    let track = builder.add_track(TrackKind::Scene, "shots", run);
                    let end = builder
                        .document()
                        .media
                        .duration
                        .or_else(|| {
                            frames.last().map(|frame| {
                                MediaTime::from_secs_f64(
                                    f64::from(frame.time_s) + f64::from(1.0 / options.fps),
                                )
                            })
                        })
                        .unwrap_or(MediaTime::ZERO);

                    let mut boundaries = vec![MediaTime::ZERO];
                    for shot in &shots {
                        let instant = MediaTime::from_secs_f64(f64::from(shot.start_s));
                        let instant = instant.min(end);
                        if instant > MediaTime::ZERO && instant < end {
                            boundaries.push(instant);
                        }
                    }
                    boundaries.dedup();
                    for (index, start) in boundaries.iter().enumerate() {
                        let next = boundaries.get(index + 1).copied().unwrap_or(end);
                        if next <= *start {
                            continue;
                        }
                        builder.add_segment(
                            track,
                            TimeRange::new(*start, next),
                            SegmentKind::Scene {
                                index: u32::try_from(index).unwrap_or(u32::MAX),
                                cut: *start > MediaTime::ZERO,
                            },
                            None,
                            run,
                        );
                    }
                }
                Ok(_) => {
                    let run = builder.add_run(run_record(
                        Capability::Scene,
                        "sonolytica-media",
                        None,
                        "cpu",
                        &config,
                        started,
                        RunStatus::Failed {
                            reason: "the video stream produced no decodable frames".into(),
                        },
                    ));
                    builder.warn(
                        Warning::new(
                            "scene_failed",
                            "scene detection decoded no frames from the video stream",
                        )
                        .with_run(run),
                    );
                }
                Err(error) => {
                    let run = builder.add_run(run_record(
                        Capability::Scene,
                        "sonolytica-media",
                        None,
                        "cpu",
                        &config,
                        started,
                        RunStatus::Failed {
                            reason: error.to_string(),
                        },
                    ));
                    builder.warn(
                        Warning::new("scene_failed", format!("scene detection failed: {error}"))
                            .with_run(run),
                    );
                }
            }
        }

        let needs_speech = config.wants(Capability::Vad)
            || config.wants(Capability::Transcribe)
            || config.wants(Capability::Diarize);
        let mut speech_audio: Option<Audio> = None;
        if needs_speech {
            speech_audio = Some(
                decode_mono(
                    &self.tools,
                    &input.path,
                    &DecodeOptions {
                        sample_rate: SPEECH_RATE,
                        max_duration_secs: config.limits.max_duration_secs,
                        timeout_secs: config.limits.timeout_secs,
                    },
                )
                .await?,
            );
        }

        // --- VAD -----------------------------------------------------------
        // VAD runs whenever it was requested, and also as a prerequisite for
        // ASR when a detector is available: decoding speech regions instead of
        // the whole file avoids Whisper's long-form early end-of-text failure
        // and skips silence.
        let mut vad_regions: Vec<SpeechRegion> = Vec::new();
        if self.registry.vad().is_some()
            && (config.wants(Capability::Vad) || config.wants(Capability::Transcribe))
        {
            if let Some(audio) = speech_audio.take() {
                let registry = Arc::clone(&self.registry);
                let vad_config: VadConfig = config.vad.clone();
                let started = (now_unix_ms(), Instant::now());
                let (audio, outcome) = blocking(move || {
                    let result = match registry.vad() {
                        Some(provider) => provider.detect(&audio, &vad_config),
                        None => Err(ProviderError::Unsupported(
                            "no VAD provider available".into(),
                        )),
                    };
                    (audio, result)
                })
                .await?;
                speech_audio = Some(audio);

                let status = match &outcome {
                    Ok(_) => RunStatus::Complete,
                    Err(error) => RunStatus::Failed {
                        reason: error.to_string(),
                    },
                };
                let run = builder.add_run(run_record(
                    Capability::Vad,
                    "silero-vad",
                    self.registry
                        .vad()
                        .and_then(|p| p.info().models.first().cloned()),
                    "cpu",
                    &config,
                    started,
                    status,
                ));
                match outcome {
                    Ok(regions) => {
                        tracing::info!(regions = regions.len(), "vad complete");
                        vad_regions = regions;
                    }
                    Err(error) => {
                        builder.warn(Warning::new("vad_failed", error.to_string()).with_run(run));
                    }
                }
            }
        }

        // --- ASR -----------------------------------------------------------
        let mut asr_output: Option<sonolytica_providers::AsrOutput> = None;
        if config.wants(Capability::Transcribe)
            && self.registry.asr_for(config.asr_model.as_deref()).is_some()
        {
            if let Some(audio) = speech_audio.take() {
                let registry = Arc::clone(&self.registry);
                let model_preference = config.asr_model.clone();
                let asr_config = AsrConfig {
                    language: config.language.clone(),
                    translate_to_english: config.translate_to_english,
                    threads: 4,
                    regions: if vad_regions.is_empty() {
                        Vec::new()
                    } else {
                        merge_speech_regions(&vad_regions, builder.document().duration())
                    },
                };
                let provider_info = registry
                    .asr_for(model_preference.as_deref())
                    .map(|p| p.info());
                let started = (now_unix_ms(), Instant::now());
                let (audio, outcome) = blocking(move || {
                    let result = match registry.asr_for(model_preference.as_deref()) {
                        Some(provider) => provider.transcribe(&audio, &asr_config),
                        None => Err(ProviderError::Unsupported(
                            "no ASR provider available".into(),
                        )),
                    };
                    (audio, result)
                })
                .await?;
                speech_audio = Some(audio);

                let model = provider_info
                    .as_ref()
                    .and_then(|info| info.models.first().cloned());
                let status = match &outcome {
                    Ok(output) if output.segments.is_empty() => RunStatus::Partial {
                        reason: "no speech recognised".into(),
                    },
                    Ok(_) => RunStatus::Complete,
                    Err(error) => RunStatus::Failed {
                        reason: error.to_string(),
                    },
                };
                let run = builder.add_run(run_record(
                    Capability::Transcribe,
                    provider_info
                        .as_ref()
                        .map_or("whisper-cpp", |info| info.name),
                    model.clone(),
                    provider_info.as_ref().map_or("cpu", |info| info.device),
                    &config,
                    started,
                    status,
                ));

                match outcome {
                    Ok(output) => asr_output = Some(output),
                    Err(error) => {
                        builder.warn(Warning::new("asr_failed", error.to_string()).with_run(run));
                    }
                }
            }
        }

        // --- Forced word alignment -----------------------------------------
        // Runs after ASR and before the speech track is assembled: the track
        // records whatever words survive here. A missing aligner is a warning,
        // never a failed analysis.
        if config.align_words {
            if let Some(output) = asr_output.as_mut() {
                if let Some(aligner) = self.aligner.clone() {
                    if let Some(audio) = speech_audio.as_ref() {
                        align_output(&mut builder, aligner, audio, output, &config).await?;
                    }
                } else {
                    builder.warn(Warning::new(
                        "align_unavailable",
                        "forced alignment was requested but the `align` feature or the \
                         wav2vec2-base-960h model is missing; keeping Whisper token timestamps",
                    ));
                }
            }
        }

        // --- Diarization ---------------------------------------------------
        let mut diarization: Option<DiarizationOutput> = None;
        if config.wants(Capability::Diarize) && self.registry.diarizer().is_some() {
            if let Some(audio) = speech_audio.take() {
                let registry = Arc::clone(&self.registry);
                let diar_config = config.diarization.clone();
                let started = (now_unix_ms(), Instant::now());
                // Diarization is the last consumer of the speech-rate buffer,
                // so the owned audio can be dropped inside the task. Providers
                // are tried in registry order: Sortformer first, then the
                // pyannote fallback if a model fails at inference time.
                let (outcome, provider_info) = blocking(move || {
                    let mut last: Option<(ProviderError, ProviderInfo)> = None;
                    for provider in &registry.diarizers {
                        let info = provider.info();
                        match provider.diarize(&audio, &diar_config) {
                            Ok(output) => return (Ok(output), Some(info)),
                            Err(error) => {
                                tracing::warn!(
                                    provider = info.name,
                                    error = %error,
                                    "diarization provider failed; trying the next one"
                                );
                                last = Some((error, info));
                            }
                        }
                    }
                    match last {
                        Some((error, info)) => (Err(error), Some(info)),
                        None => (
                            Err(ProviderError::Unsupported(
                                "no diarization provider available".into(),
                            )),
                            None,
                        ),
                    }
                })
                .await?;

                let status = match &outcome {
                    Ok(output) if output.turns.is_empty() => RunStatus::Partial {
                        reason: "no speaker turns found".into(),
                    },
                    Ok(_) => RunStatus::Complete,
                    Err(error) => RunStatus::Failed {
                        reason: error.to_string(),
                    },
                };
                let run = builder.add_run(run_record(
                    Capability::Diarize,
                    provider_info.as_ref().map_or("pyannote-3.0", |i| i.name),
                    provider_info
                        .as_ref()
                        .and_then(|i| i.models.first().cloned()),
                    provider_info.as_ref().map_or("cpu", |i| i.device),
                    &config,
                    started,
                    status,
                ));
                match outcome {
                    Ok(output) => diarization = Some(output),
                    Err(error) => {
                        builder.warn(
                            Warning::new("diarization_failed", error.to_string()).with_run(run),
                        );
                    }
                }
            }
        }

        // --- Speech track --------------------------------------------------
        let mut asr_segment_ids: Vec<sonolytica_core::SegmentId> = Vec::new();
        let mut word_count = 0_u64;
        if let Some(output) = &asr_output {
            let run = last_successful_run(&builder, Capability::Transcribe)
                .unwrap_or_else(|| builder.run_count().saturating_sub(1));
            let track = builder.add_track(TrackKind::Speech, "asr", run);
            for segment in &output.segments {
                word_count += segment.words.len() as u64;
                let id = builder.add_segment(
                    track,
                    segment.range,
                    sonolytica_core::SegmentKind::Speech {
                        text: segment.text.clone(),
                        words: segment.words.clone(),
                        language: output.language.clone(),
                        no_speech_prob: segment.no_speech_prob,
                    },
                    segment.confidence,
                    run,
                );
                asr_segment_ids.push(id);
            }
        }

        // --- Speaker turns -------------------------------------------------
        let asr_segments: &[AsrSegment] = asr_output
            .as_ref()
            .map_or(&[][..], |output| output.segments.as_slice());
        let diar_turns: &[DiarTurn] = diarization
            .as_ref()
            .map_or(&[][..], |output| output.turns.as_slice());

        let media_end = builder.document().duration();
        let utterances: Vec<Utterance> = build_speaker_turns(asr_segments, diar_turns)
            .into_iter()
            .filter_map(|mut utterance| {
                if let Some(range) = clamp_to_media(utterance.range, media_end) {
                    utterance.range = range;
                    Some(utterance)
                } else {
                    None
                }
            })
            .collect();
        let overlap_flags = mark_overlaps(&utterances);

        if !utterances.is_empty() {
            let run = last_successful_run(&builder, Capability::Diarize)
                .unwrap_or_else(|| builder.run_count().saturating_sub(1));
            let track = builder.add_track(TrackKind::SpeakerTurn, "speaker_turns", run);
            let mut turn_ids = Vec::with_capacity(utterances.len());
            for (index, utterance) in utterances.iter().enumerate() {
                let parent = utterance
                    .parent_asr
                    .and_then(|i| asr_segment_ids.get(i).copied());
                let id = builder.add_segment(
                    track,
                    utterance.range,
                    sonolytica_core::SegmentKind::SpeakerTurn {
                        speaker: utterance.speaker.clone(),
                        text: utterance.text.clone(),
                        overlap: overlap_flags[index].0,
                    },
                    utterance.confidence,
                    run,
                );
                builder.set_parent(id, parent);
                turn_ids.push(id);
            }
            for (index, (overlap, interrupts_previous)) in overlap_flags.iter().enumerate() {
                if *overlap && index > 0 {
                    builder.add_relation(
                        turn_ids[index - 1],
                        turn_ids[index],
                        sonolytica_core::RelationKind::Overlaps,
                        None,
                    );
                }
                if *interrupts_previous && index > 0 {
                    builder.add_relation(
                        turn_ids[index - 1],
                        turn_ids[index],
                        sonolytica_core::RelationKind::Interrupts,
                        None,
                    );
                }
            }
        }

        // --- Speech summary ------------------------------------------------
        if asr_output.is_some() || diarization.is_some() || !vad_regions.is_empty() {
            let total = builder.document().duration().max(MediaTime::from_micros(1));
            let speech_duration = if vad_regions.is_empty() {
                union_duration(asr_segments.iter().map(|s| s.range).collect())
            } else {
                union_duration(vad_regions.iter().map(|r| r.range).collect())
            };
            let speakers: std::collections::BTreeSet<SpeakerId> =
                utterances.iter().map(|u| u.speaker.clone()).collect();
            let ratio = (speech_duration.micros() as f32 / total.micros() as f32).clamp(0.0, 1.0);
            builder.add_analysis(AnalysisBlock::Speech(SpeechSummary {
                language: asr_output.as_ref().and_then(|o| o.language.clone()),
                speaker_count: u32::try_from(speakers.len()).unwrap_or(0),
                speech_duration,
                speech_ratio: ratio,
                word_count,
            }));
        }

        // --- Music and acoustic analysis -----------------------------------
        let needs_music = config.wants(Capability::Music);
        let needs_acoustic = config.wants(Capability::Acoustic);
        if needs_music || needs_acoustic {
            let audio = decode_mono(
                &self.tools,
                &input.path,
                &DecodeOptions {
                    sample_rate: MUSIC_RATE,
                    max_duration_secs: config.limits.max_duration_secs,
                    timeout_secs: config.limits.timeout_secs,
                },
            )
            .await?;

            let frame_config = FrameConfig {
                fft_size: config.dsp.fft_size,
                hop_size: config.dsp.hop_size,
                ..FrameConfig::default()
            };

            if needs_music {
                let samples = audio.samples.clone();
                let started = (now_unix_ms(), Instant::now());
                let cfg = frame_config;
                let outcome =
                    blocking(move || sonolytica_dsp::analyze_music(&samples, MUSIC_RATE, cfg))
                        .await?;
                let status = match &outcome {
                    Ok(_) => RunStatus::Complete,
                    Err(error) => RunStatus::Failed {
                        reason: error.to_string(),
                    },
                };
                let run = builder.add_run(run_record(
                    Capability::Music,
                    "sonolytica-dsp",
                    None,
                    "cpu",
                    &config,
                    started,
                    status,
                ));
                match outcome {
                    Ok(features) => {
                        builder.add_analysis(AnalysisBlock::Music(map_music(&features)));
                    }
                    Err(error) => {
                        builder.warn(Warning::new("music_failed", error.to_string()).with_run(run));
                    }
                }
            }

            if needs_acoustic {
                let samples = audio.samples.clone();
                let silence_dbfs = config.dsp.silence_dbfs;
                let min_silence =
                    f32::from(u16::try_from(config.dsp.min_silence_ms).unwrap_or(500)) / 1_000.0;
                let started = (now_unix_ms(), Instant::now());
                let outcome = blocking(move || {
                    sonolytica_dsp::describe_acoustics(
                        &samples,
                        MUSIC_RATE,
                        silence_dbfs,
                        min_silence,
                    )
                })
                .await?;
                let status = match &outcome {
                    Ok(_) => RunStatus::Complete,
                    Err(error) => RunStatus::Failed {
                        reason: error.to_string(),
                    },
                };
                let run = builder.add_run(run_record(
                    Capability::Acoustic,
                    "sonolytica-dsp",
                    None,
                    "cpu",
                    &config,
                    started,
                    status,
                ));
                match outcome {
                    Ok(features) => {
                        builder.add_analysis(AnalysisBlock::Acoustic(map_acoustic(&features)));
                    }
                    Err(error) => {
                        builder
                            .warn(Warning::new("acoustic_failed", error.to_string()).with_run(run));
                    }
                }
            }
        }

        builder.finish().map_err(|violations| {
            EngineError::InvalidDocument(
                violations
                    .iter()
                    .map(|v| format!("{}: {}", v.code, v.message))
                    .collect::<Vec<_>>()
                    .join("; "),
            )
        })
    }
}

/// Run blocking work without stalling the async runtime.
async fn blocking<T, F>(work: F) -> Result<T, EngineError>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| EngineError::Join(e.to_string()))
}

fn run_record(
    capability: Capability,
    provider: &str,
    model: Option<ModelRef>,
    device: &str,
    config: &AnalysisConfig,
    started: (u64, Instant),
    status: RunStatus,
) -> RunRecord {
    RunRecord {
        id: 0,
        capability,
        provider: provider.to_string(),
        model,
        params_sha256: config.params_sha256(),
        device: device.to_string(),
        started_at_unix_ms: started.0,
        duration_ms: u64::try_from(started.1.elapsed().as_millis()).unwrap_or(u64::MAX),
        status,
    }
}

/// Replace Whisper word timings with CTC-forced alignment, one run per ASR
/// segment.
///
/// A segment is the unit of provenance here: a transcript region that could
/// not be aligned is visible as its own failed/partial record instead of a
/// silent downgrade of the whole file. Segments longer than the model's 30 s
/// pass are split: the prefix is aligned and the Whisper words beyond the cap
/// are kept, with a `partial` run and an explicit warning.
async fn align_output(
    builder: &mut DocumentBuilder,
    aligner: Arc<sonolytica_providers::align::LazyWav2Vec2Aligner>,
    audio: &Audio,
    output: &mut AsrOutput,
    config: &AnalysisConfig,
) -> Result<(), EngineError> {
    let english = output
        .language
        .as_deref()
        .is_none_or(|language| language.to_ascii_lowercase().starts_with("en"));
    if !english {
        builder.warn(Warning::new(
            "align_unavailable",
            format!(
                "forced alignment is English-only (wav2vec2-base-960h); transcript language is {}",
                output.language.as_deref().unwrap_or("unknown")
            ),
        ));
        return Ok(());
    }

    // One load attempt: a missing digest or an unloadable graph is a single
    // document warning, not one failed run per region.
    let loader = Arc::clone(&aligner);
    if let Err(error) = blocking(move || loader.ensure_loaded()).await? {
        builder.warn(Warning::new(
            "align_unavailable",
            format!("forced alignment is unavailable: {error}"),
        ));
        return Ok(());
    }

    let model = sonolytica_providers::find_model(sonolytica_providers::align::MODEL_ID)
        .map(|spec| spec.reference());
    let limit = MediaTime::from_millis(sonolytica_providers::align::MAX_ALIGN_MILLIS);
    let media_end = audio.duration();

    for segment in &mut output.segments {
        if segment.text.trim().is_empty() {
            continue;
        }
        let truncated = segment.range.duration() > limit;
        let cutoff = segment.range.start.saturating_add(limit);
        let (text, slice_end) = if truncated {
            // Split on Whisper's own word times rather than guessing how many
            // words fit: the split point is not a boundary we invent.
            let prefix = segment
                .words
                .iter()
                .filter(|word| word.range.start < cutoff)
                .map(|word| word.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            if prefix.trim().is_empty() {
                builder.warn(Warning::new(
                    "align_truncated",
                    format!(
                        "region {} exceeds the {} s alignment pass and has no Whisper words to \
                         split it; keeping Whisper timings",
                        segment.range,
                        sonolytica_providers::align::MAX_ALIGN_MILLIS / 1_000
                    ),
                ));
                continue;
            }
            (prefix, cutoff)
        } else {
            (segment.text.clone(), segment.range.end)
        };

        let range = TimeRange::new(segment.range.start, slice_end.min(media_end));
        let samples = audio.samples_in(range).to_vec();
        if samples.len() < sonolytica_providers::align::MIN_ALIGN_SAMPLES {
            continue;
        }

        let started = (now_unix_ms(), Instant::now());
        let worker = Arc::clone(&aligner);
        let outcome = blocking(move || worker.align(&samples, &text)).await?;

        let mut replacement: Option<Vec<Word>> = None;
        let mut warning: Option<(&'static str, String)> = None;
        let status = match outcome {
            Err(error) => {
                warning = Some((
                    "align_failed",
                    format!(
                        "forced alignment failed for {}: {error}; keeping Whisper timings",
                        segment.range
                    ),
                ));
                RunStatus::Failed {
                    reason: error.to_string(),
                }
            }
            Ok(words) if words.is_empty() => {
                warning = Some((
                    "align_failed",
                    format!(
                        "forced alignment produced no confident words for {}; keeping Whisper timings",
                        segment.range
                    ),
                ));
                RunStatus::Partial {
                    reason: "no confident aligned words".into(),
                }
            }
            Ok(words) => {
                let mut candidate: Vec<Word> = words
                    .into_iter()
                    .map(|mut word| {
                        word.range = TimeRange::new(
                            segment.range.start.saturating_add(word.range.start),
                            segment.range.start.saturating_add(word.range.end),
                        );
                        word
                    })
                    .collect();
                if truncated {
                    candidate.extend(
                        segment
                            .words
                            .iter()
                            .filter(|word| word.range.start >= cutoff)
                            .cloned(),
                    );
                }
                if valid_word_sequence(&candidate, segment.range) {
                    replacement = Some(candidate);
                    if truncated {
                        warning = Some((
                            "align_truncated",
                            format!(
                                "region {} exceeds the {} s alignment pass; aligned the first \
                                 part and kept Whisper timings for the rest",
                                segment.range,
                                sonolytica_providers::align::MAX_ALIGN_MILLIS / 1_000
                            ),
                        ));
                        RunStatus::Partial {
                            reason: "aligned the first 30 s; the rest keeps Whisper timings".into(),
                        }
                    } else {
                        RunStatus::Complete
                    }
                } else {
                    warning = Some((
                        "align_invalid",
                        format!(
                            "forced alignment for {} was not monotonic inside the region; \
                             keeping Whisper timings",
                            segment.range
                        ),
                    ));
                    RunStatus::Partial {
                        reason: "aligned word ranges were rejected".into(),
                    }
                }
            }
        };

        let run = builder.add_run(run_record(
            Capability::Align,
            "wav2vec2-align",
            model.clone(),
            "cpu",
            config,
            started,
            status,
        ));
        if let Some((code, message)) = warning {
            builder.warn(Warning::new(code, message).with_run(run));
        }
        if let Some(words) = replacement {
            segment.words = words;
        }
    }
    Ok(())
}

/// Aligned words must be non-empty, non-overlapping, ordered, and inside the
/// segment they describe. Anything else keeps the Whisper words.
fn valid_word_sequence(words: &[Word], bounds: TimeRange) -> bool {
    if words.is_empty() {
        return false;
    }
    let mut previous_end = bounds.start;
    for word in words {
        if word.range.is_empty() || word.range.start < previous_end || word.range.end > bounds.end {
            return false;
        }
        previous_end = word.range.end;
    }
    true
}

fn last_successful_run(builder: &DocumentBuilder, capability: Capability) -> Option<u32> {
    builder
        .document()
        .runs
        .iter()
        .rev()
        .find(|run| run.capability == capability && run.status.is_success())
        .map(|run| run.id)
}

/// Whether each utterance overlaps the previous one, and whether it interrupts
/// it. Overlap is a temporal fact; interruption is a documented heuristic.
fn mark_overlaps(utterances: &[Utterance]) -> Vec<(bool, bool)> {
    let mut flags = vec![(false, false); utterances.len()];
    for i in 1..utterances.len() {
        let previous = &utterances[i - 1];
        let current = &utterances[i];
        if !previous.range.intersects(&current.range) {
            continue;
        }
        if previous.speaker == current.speaker {
            continue;
        }
        flags[i].0 = true;
        flags[i - 1].0 = true;

        // Interruption: the new turn starts before the previous ends and
        // continues after it. Merely touching is not an interruption.
        let overlap = previous.range.overlap(&current.range);
        if current.range.start < previous.range.end
            && current.range.end > previous.range.end
            && overlap >= MediaTime::from_millis(150)
            && previous.range.duration() >= MediaTime::from_millis(300)
        {
            flags[i].1 = true;
        }
    }
    flags
}

/// Prepare speech regions for ASR.
///
/// Regions closer than 250 ms are one utterance. Each region is then padded by
/// 300 ms of context, but never across the midpoint of a gap and never across
/// a neighbouring region: overlapping calls would duplicate words at the seam,
/// and blindly merging nearby regions is what makes Whisper fall back to
/// whole-file decoding (and drop turns).
fn merge_speech_regions(regions: &[SpeechRegion], limit: MediaTime) -> Vec<TimeRange> {
    let merge_gap = MediaTime::from_millis(250);
    let asr_pad = MediaTime::from_millis(600);
    let guard = MediaTime::from_millis(50);

    let mut base: Vec<TimeRange> = regions
        .iter()
        .map(|r| TimeRange::new(r.range.start, r.range.end.min(limit)))
        .filter(|r| !r.is_empty())
        .collect();
    base.sort_by_key(|r| r.start);

    let mut merged: Vec<TimeRange> = Vec::with_capacity(base.len());
    for range in base {
        match merged.last_mut() {
            Some(current) if range.start <= current.end.saturating_add(merge_gap) => {
                current.end = current.end.max(range.end);
            }
            _ => merged.push(range),
        }
    }

    let count = merged.len();
    let mut out = Vec::with_capacity(count);
    for (index, range) in merged.iter().enumerate() {
        let mut start = range.start.saturating_sub(asr_pad);
        let mut end = range.end.saturating_add(asr_pad).min(limit);
        if index > 0 {
            start = start.max(merged[index - 1].end.saturating_add(guard));
        }
        if index + 1 < count {
            end = end.min(merged[index + 1].start.saturating_sub(guard));
        }
        if end > start {
            out.push(TimeRange::new(start, end));
        }
    }
    out
}

fn union_duration(ranges: Vec<TimeRange>) -> MediaTime {
    if ranges.is_empty() {
        return MediaTime::ZERO;
    }
    let mut sorted = ranges;
    sorted.sort_by_key(|r| r.start);
    let mut total = MediaTime::ZERO;
    let mut current = sorted[0];
    for range in sorted.into_iter().skip(1) {
        if range.start <= current.end {
            current.end = current.end.max(range.end);
        } else {
            total = total.saturating_add(current.duration());
            current = range;
        }
    }
    total.saturating_add(current.duration())
}

fn clamp_to_media(range: TimeRange, limit: MediaTime) -> Option<TimeRange> {
    let start = range.start.min(limit);
    let end = range.end.min(limit);
    (end > start).then_some(TimeRange::new(start, end))
}

fn map_pitch(class: u8) -> PitchClass {
    PitchClass::from_index(usize::from(class))
}

fn map_music(features: &sonolytica_dsp::MusicFeatures) -> MusicSummary {
    let bpm = features.tempo.as_ref().and_then(|tempo| {
        tempo.bpm.is_finite().then_some(Bpm {
            value: tempo.bpm,
            confidence: Confidence::clamped(tempo.confidence),
        })
    });
    let beats = features
        .tempo
        .as_ref()
        .map(|tempo| {
            tempo
                .beats_s
                .iter()
                .filter(|b| b.is_finite() && **b >= 0.0)
                .map(|b| MediaTime::from_secs_f64(f64::from(*b)))
                .collect()
        })
        .unwrap_or_default();
    let key = features.key.map(|key| KeyEstimate {
        tonic: map_pitch(key.tonic),
        mode: if key.major { Mode::Major } else { Mode::Minor },
        confidence: Confidence::clamped(key.confidence),
    });
    let chords: Vec<Chord> = features
        .chords
        .iter()
        .filter(|chord| chord.end_s > chord.start_s)
        .map(|chord| Chord {
            range: TimeRange::from_secs(f64::from(chord.start_s), f64::from(chord.end_s)),
            root: map_pitch(chord.root),
            quality: match chord.quality {
                sonolytica_dsp::music::ChordQuality::Major => ChordQuality::Major,
                sonolytica_dsp::music::ChordQuality::Minor => ChordQuality::Minor,
            },
            confidence: Confidence::clamped(chord.confidence),
        })
        .collect();
    let loudness = features.loudness.and_then(|loud| {
        loud.integrated_lufs
            .filter(|v| v.is_finite())
            .map(|lufs| Loudness {
                integrated_lufs: lufs,
                loudness_range_lu: loud.loudness_range_lu,
                true_peak_dbtp: loud.true_peak_dbtp,
            })
    });

    let sections = features
        .sections
        .iter()
        .filter(|section| section.end_s > section.start_s)
        .map(|section| sonolytica_core::Section {
            range: TimeRange::new(
                MediaTime::from_secs_f64(f64::from(section.start_s)),
                MediaTime::from_secs_f64(f64::from(section.end_s)),
            ),
            novelty: section.novelty,
        })
        .collect();
    MusicSummary {
        bpm,
        beats,
        key,
        chords,
        sections,
        loudness,
    }
}

fn map_acoustic(features: &sonolytica_dsp::AcousticFeatures) -> AcousticSummary {
    AcousticSummary {
        silence: features
            .silence
            .iter()
            .filter(|region| region.end_s > region.start_s)
            .map(|region| TimeRange::from_secs(f64::from(region.start_s), f64::from(region.end_s)))
            .collect(),
        dynamic_range_lu: features.loudness_range_lu,
        mean_rms_dbfs: features.mean_rms_dbfs,
        peak_dbfs: features.peak_dbfs,
        events: Vec::new(),
    }
}

/// Convenience entry point using the default model store and tools.
pub async fn analyze(path: &Path, config: &AnalysisConfig) -> Result<Document, EngineError> {
    Engine::discover(ModelStore::default())
        .analyze(path, config)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn union_of_ranges_counts_overlap_once() {
        let ranges = vec![
            TimeRange::from_secs(0.0, 2.0),
            TimeRange::from_secs(1.0, 3.0),
            TimeRange::from_secs(5.0, 6.0),
        ];
        assert_eq!(union_duration(ranges), MediaTime::from_millis(4_000));
        assert_eq!(union_duration(Vec::new()), MediaTime::ZERO);
    }

    #[test]
    fn overlap_and_interruption_are_detected() {
        let utterances = vec![
            Utterance {
                range: TimeRange::from_secs(0.0, 4.0),
                speaker: SpeakerId::new("S1"),
                text: "a".into(),
                parent_asr: None,
                confidence: None,
            },
            Utterance {
                range: TimeRange::from_secs(3.0, 5.0),
                speaker: SpeakerId::new("S2"),
                text: "b".into(),
                parent_asr: None,
                confidence: None,
            },
        ];
        let flags = mark_overlaps(&utterances);
        assert!(flags[1].0, "overlap must be flagged");
        assert!(
            flags[1].1,
            "a turn that outlasts the previous one interrupts"
        );
    }

    #[test]
    fn aligned_words_must_be_ordered_and_inside_the_segment() {
        let bounds = TimeRange::from_secs(1.0, 2.0);
        let word = |start: f64, end: f64| Word {
            text: "w".into(),
            range: TimeRange::from_secs(start, end),
            confidence: None,
        };

        assert!(valid_word_sequence(
            &[word(1.0, 1.5), word(1.5, 1.9)],
            bounds
        ));
        assert!(!valid_word_sequence(&[], bounds), "empty result");
        assert!(
            !valid_word_sequence(&[word(0.9, 1.2)], bounds),
            "starts before the segment"
        );
        assert!(
            !valid_word_sequence(&[word(1.0, 1.5), word(1.4, 1.6)], bounds),
            "overlaps the previous word"
        );
        assert!(
            !valid_word_sequence(&[word(1.8, 2.1)], bounds),
            "ends after the segment"
        );
    }

    #[test]
    fn same_speaker_never_overlaps_itself() {
        let utterances = vec![
            Utterance {
                range: TimeRange::from_secs(0.0, 2.0),
                speaker: SpeakerId::new("S1"),
                text: "a".into(),
                parent_asr: None,
                confidence: None,
            },
            Utterance {
                range: TimeRange::from_secs(1.0, 3.0),
                speaker: SpeakerId::new("S1"),
                text: "b".into(),
                parent_asr: None,
                confidence: None,
            },
        ];
        let flags = mark_overlaps(&utterances);
        assert!(!flags[1].0);
        assert!(!flags[1].1);
    }
}
