//! SONOLYTICA command line interface.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use sonolytica_core::{AnalysisBlock, AnalysisConfig, Capability, Document, MediaTime, RunStatus};
use sonolytica_engine::Engine;
use sonolytica_providers::{ModelStore, default_model_dir, models::MODELS};
use tracing_subscriber::EnvFilter;

mod query;

#[derive(Debug, Parser)]
#[command(
    name = "sonolytica",
    version,
    about = "Multimodal perception infrastructure for AI agents",
    long_about = "SONOLYTICA turns audio and video into a timestamped, typed timeline that \
                  agents can query. Analysis stages are independent: a missing model disables \
                  one capability, never the whole run."
)]
struct Cli {
    /// Model directory (defaults to $SONOLYTICA_MODEL_DIR or ~/.cache/sonolytica/models)
    #[arg(long, global = true, env = "SONOLYTICA_MODEL_DIR")]
    models_dir: Option<PathBuf>,

    /// Increase log verbosity (repeatable)
    #[arg(long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show container and stream metadata
    Inspect(InspectArgs),
    /// Analyse media and emit a SONOLYTICA document
    Analyze(AnalyzeArgs),
    /// Query an analysed document without jq
    Query(query::QueryArgs),
    /// List or fetch pinned models
    Models {
        #[command(subcommand)]
        command: ModelsCommand,
    },
    /// Environment and capability check
    Doctor,
    /// Print the document schema example
    Schema,
}

#[derive(Debug, Args)]
struct InspectArgs {
    /// Input media file
    file: PathBuf,
    /// Emit JSON instead of a human summary
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AnalyzeArgs {
    /// Input media file
    file: PathBuf,
    /// Preset: structure (no transcript), standard, ultra (all + no silent degradation).
    /// Default: standard.
    #[arg(long, value_enum, conflicts_with = "modules")]
    profile: Option<Profile>,
    /// Write the document here instead of stdout ("-" means stdout)
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Comma separated capabilities: vad,transcribe,diarize,music,acoustic
    #[arg(long, value_delimiter = ',')]
    modules: Option<Vec<String>>,
    /// Preferred language (ISO 639-1)
    #[arg(long)]
    language: Option<String>,
    /// Explicit ASR model id (see `sonolytica models list`)
    #[arg(long)]
    model: Option<String>,
    /// Translate speech to English
    #[arg(long)]
    translate: bool,
    /// Skip hashing the input (faster, weaker provenance)
    #[arg(long)]
    no_hash: bool,
    /// Print a human summary to stderr as well as JSON
    #[arg(long)]
    summary: bool,
    /// Exit with status 2 if any stage failed or a requested capability did not run
    #[arg(long)]
    strict: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Profile {
    /// Speech regions, speakers, music and acoustic structure — no transcript
    Structure,
    /// Everything the audio pipeline can do, with a transcript
    Standard,
    /// Standard plus a hard guarantee: exit 2 if any requested capability failed
    /// or did not run at all
    Ultra,
}

impl Profile {
    fn modules(self) -> &'static [Capability] {
        match self {
            Self::Structure => &[
                Capability::Vad,
                Capability::Diarize,
                Capability::Music,
                Capability::Acoustic,
                Capability::Scene,
            ],
            Self::Standard | Self::Ultra => &STANDARD_MODULES,
        }
    }
}

/// Everything the local pipeline can do on a media file. `Scene` is a no-op on
/// audio-only input: the stage checks for a video stream and produces nothing.
const STANDARD_MODULES: [Capability; 6] = [
    Capability::Vad,
    Capability::Transcribe,
    Capability::Diarize,
    Capability::Music,
    Capability::Acoustic,
    Capability::Scene,
];

#[derive(Debug, Subcommand)]
enum ModelsCommand {
    /// List known models and their local status
    List,
    /// Download and verify models
    Fetch {
        /// Model ids; omit with --all to fetch every model
        ids: Vec<String>,
        #[arg(long)]
        all: bool,
    },
}

fn init_tracing(verbose: u8) {
    let default = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("sonolytica={default},ort=error,{default}")));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing(cli.verbose);
    match run(cli).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode> {
    let store = ModelStore::new(cli.models_dir.unwrap_or_else(default_model_dir));
    match cli.command {
        Command::Inspect(args) => {
            let config = AnalysisConfig {
                modules: vec![Capability::Inspect],
                ..AnalysisConfig::default()
            };
            let engine = Engine::discover(store);
            let document = engine
                .analyze(&args.file, &config)
                .await
                .with_context(|| format!("could not inspect {}", args.file.display()))?;
            if args.json {
                println!("{}", serde_json::to_string_pretty(&document.media)?);
            } else {
                println!("{}", describe_media(&document));
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Analyze(args) => {
            let config = build_config(&args)?;
            let engine = Engine::discover(store.clone());
            let document = engine
                .analyze(&args.file, &config)
                .await
                .with_context(|| format!("analysis of {} failed", args.file.display()))?;

            match args.out.as_deref() {
                Some(path) if path != Path::new("-") => {
                    std::fs::write(path, document.to_json_pretty()?)
                        .with_context(|| format!("could not write {}", path.display()))?;
                }
                _ => println!("{}", document.to_json_pretty()?),
            }
            if args.summary {
                eprintln!("{}", summarize(&document));
            }
            if (args.strict || args.profile == Some(Profile::Ultra))
                && is_degraded(&document, &config)
            {
                return Ok(ExitCode::from(2));
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Query(args) => {
            query::run(args.command)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Models { command } => {
            match command {
                ModelsCommand::List => {
                    println!("model directory: {}", store.dir().display());
                    println!("{:<28} {:>10}  {:<14} STATUS", "ID", "SIZE", "LICENSE");
                    for (spec, present, verified) in store.status() {
                        let status = match (present, verified) {
                            (_, true) => "verified",
                            (true, false) => "PRESENT, DIGEST MISMATCH",
                            (false, _) => "not downloaded",
                        };
                        println!(
                            "{:<28} {:>10}  {:<14} {}",
                            spec.id,
                            format!("{:.0} MB", spec.approx_bytes as f64 / 1e6),
                            spec.license,
                            status
                        );
                    }
                }
                ModelsCommand::Fetch { ids, all } => {
                    let specs: Vec<&_> = if all {
                        MODELS.iter().collect()
                    } else {
                        if ids.is_empty() {
                            bail!("name one or more model ids, or pass --all");
                        }
                        ids.iter()
                            .map(|id| {
                                sonolytica_providers::find_model(id).ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "unknown model `{id}`; run `sonolytica models list`"
                                    )
                                })
                            })
                            .collect::<Result<Vec<_>>>()?
                    };
                    for spec in specs {
                        let path = store.fetch(spec).with_context(|| {
                            format!("could not fetch {} ({})", spec.id, spec.license)
                        })?;
                        println!("{} -> {}", spec.id, path.display());
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor => {
            let tools = sonolytica_media::MediaTools::detect();
            println!("sonolytica {}", env!("CARGO_PKG_VERSION"));
            match tools.ffmpeg_version().await {
                Ok(line) => println!("ffmpeg: {line}"),
                Err(error) => println!("ffmpeg: NOT AVAILABLE ({error})"),
            }
            println!("model directory: {}", store.dir().display());
            let engine = Engine::discover(store.clone());
            let registry = engine.registry();
            let infos = registry.provider_infos();
            if infos.is_empty() {
                println!("providers: none available");
            } else {
                println!("providers:");
                for info in infos {
                    println!(
                        "  {:<22} {:<12} {}  [{}]",
                        info.name,
                        info.capability.to_string(),
                        info.device,
                        info.models
                            .iter()
                            .map(|m| m.id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
            if !registry.unavailable.is_empty() {
                println!("unavailable:");
                for entry in &registry.unavailable {
                    println!(
                        "  {:<22} {:<12} {}",
                        entry.provider,
                        entry.capability.to_string(),
                        entry.reason
                    );
                }
            }
            let build = if cfg!(feature = "full-metal") {
                "full-metal"
            } else if cfg!(feature = "full") {
                "full"
            } else {
                "minimal (DSP only)"
            };
            println!("build: {build}");
            Ok(ExitCode::SUCCESS)
        }
        Command::Schema => {
            println!("{}", schema_example()?);
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn build_config(args: &AnalyzeArgs) -> Result<AnalysisConfig> {
    let mut config = AnalysisConfig {
        language: args.language.clone(),
        translate_to_english: args.translate,
        asr_model: args.model.clone(),
        hash_media: !args.no_hash,
        ..AnalysisConfig::default()
    };
    if let Some(modules) = &args.modules {
        let mut parsed = Vec::with_capacity(modules.len());
        for module in modules {
            let capability = Capability::parse(module)
                .ok_or_else(|| anyhow::anyhow!("unknown module `{module}`"))?;
            parsed.push(capability);
        }
        config.modules = parsed;
    } else {
        let profile = args.profile.unwrap_or(Profile::Standard);
        config.modules = profile.modules().to_vec();
    }
    config.normalize();
    Ok(config)
}

/// True when the document is weaker than what was asked for: a stage failed, or
/// a requested capability produced no run at all (a missing model is a warning,
/// so `is_complete()` alone would call that a success).
///
/// Capabilities that cannot apply to this media are not degradations: `scene`
/// on an audio file is expected to produce nothing.
fn is_degraded(document: &Document, config: &AnalysisConfig) -> bool {
    if !document.is_complete() {
        return true;
    }
    let has_video = document.media.has_video();
    config
        .modules
        .iter()
        .filter(|capability| **capability != Capability::Inspect)
        .filter(|capability| **capability != Capability::Scene || has_video)
        .any(|capability| {
            !document
                .runs
                .iter()
                .any(|run| run.capability == *capability)
        })
}

fn describe_media(document: &Document) -> String {
    let media = &document.media;
    let mut out = String::new();
    out.push_str(&format!(
        "{}\n  duration   {}\n  container  {}\n  size       {}\n",
        media.path,
        media.duration.unwrap_or(MediaTime::ZERO),
        media.container.as_deref().unwrap_or("unknown"),
        media.size_bytes.map_or_else(
            || "unknown".to_string(),
            |b| format!("{:.1} MB", b as f64 / 1e6)
        )
    ));
    if let Some(hash) = &media.sha256 {
        out.push_str(&format!("  sha256     {hash}\n"));
    }
    out.push_str("  streams\n");
    for stream in &media.streams {
        out.push_str(&format!(
            "    #{} {:<9} {:<10} {}",
            stream.index,
            format!("{:?}", stream.kind).to_lowercase(),
            stream.codec.as_deref().unwrap_or("-"),
            stream
                .sample_rate
                .map_or_else(String::new, |r| format!("{r} Hz "))
        ));
        if let (Some(w), Some(h)) = (stream.width, stream.height) {
            out.push_str(&format!("{w}x{h}"));
        }
        out.push('\n');
    }
    out
}

fn summarize(document: &Document) -> String {
    let mut out = String::new();
    out.push_str(&describe_media(document));
    out.push_str(&format!(
        "  segments   {}  tracks {}  relations {}\n",
        document.segments.len(),
        document.tracks.len(),
        document.relations.len()
    ));

    for analysis in &document.analyses {
        match analysis {
            AnalysisBlock::Speech(speech) => {
                out.push_str(&format!(
                    "speech       {} words · {} speaker(s) · {} ({:.0}% of media)",
                    speech.word_count,
                    speech.speaker_count,
                    speech.speech_duration,
                    speech.speech_ratio * 100.0
                ));
                if let Some(lang) = &speech.language {
                    out.push_str(&format!(" · language {lang}"));
                }
                out.push('\n');
            }
            AnalysisBlock::Music(music) => {
                out.push_str("music        ");
                if let Some(bpm) = &music.bpm {
                    out.push_str(&format!("{:.1} BPM (score {}) ", bpm.value, bpm.confidence));
                }
                if let Some(key) = &music.key {
                    out.push_str(&format!(
                        "{} {} ",
                        key.tonic.name(),
                        if key.mode == sonolytica_core::Mode::Major {
                            "major"
                        } else {
                            "minor"
                        }
                    ));
                }
                if let Some(loudness) = &music.loudness {
                    out.push_str(&format!("{:.1} LUFS ", loudness.integrated_lufs));
                }
                out.push_str(&format!(
                    "· {} chords · {} beats",
                    music.chords.len(),
                    music.beats.len()
                ));
                out.push('\n');
            }
            AnalysisBlock::Acoustic(acoustic) => {
                out.push_str(&format!(
                    "acoustic     {} silence region(s)",
                    acoustic.silence.len()
                ));
                if let Some(peak) = acoustic.peak_dbfs {
                    out.push_str(&format!(" · peak {peak:.1} dBFS"));
                }
                if let Some(range) = acoustic.dynamic_range_lu {
                    out.push_str(&format!(" · LRA {range:.1} LU"));
                }
                out.push('\n');
            }
        }
    }

    out.push_str("runs         ");
    let runs: Vec<String> = document
        .runs
        .iter()
        .map(|run| {
            let status = match &run.status {
                RunStatus::Complete => "complete",
                RunStatus::Partial { .. } => "partial",
                RunStatus::Failed { .. } => "FAILED",
            };
            format!("{}={status} ({} ms)", run.capability, run.duration_ms)
        })
        .collect();
    out.push_str(&runs.join(", "));
    out.push('\n');

    for warning in &document.warnings {
        out.push_str(&format!(
            "warning      [{}] {}\n",
            warning.code, warning.message
        ));
    }
    out
}

fn schema_example() -> Result<String> {
    let media = sonolytica_core::MediaInfo {
        path: "/media/interview.m4a".into(),
        sha256: Some("…".into()),
        size_bytes: Some(12_345_678),
        container: Some("mp4".into()),
        duration: Some(MediaTime::from_millis(93_500)),
        bit_rate: Some(128_000),
        streams: vec![sonolytica_core::StreamInfo {
            index: 0,
            kind: sonolytica_core::StreamKind::Audio,
            codec: Some("aac".into()),
            sample_rate: Some(44_100),
            channels: Some(2),
            width: None,
            height: None,
            frame_rate: None,
            language: Some("en".into()),
            duration: Some(MediaTime::from_millis(93_500)),
            bit_rate: Some(128_000),
        }],
    };
    let document = Document::new(media, AnalysisConfig::default());
    Ok(document.to_json_pretty()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonolytica_core::MediaInfo;

    #[test]
    fn profiles_expand_to_the_expected_capabilities() {
        assert!(
            !Profile::Structure
                .modules()
                .contains(&Capability::Transcribe)
        );
        assert!(
            Profile::Structure.modules().contains(&Capability::Diarize),
            "structure still separates speakers"
        );
        assert!(
            Profile::Standard
                .modules()
                .contains(&Capability::Transcribe)
        );
        assert!(
            Profile::Standard.modules().contains(&Capability::Scene),
            "video scenes are part of the standard preset"
        );
        assert!(Profile::Ultra.modules().contains(&Capability::Transcribe));
    }

    #[test]
    fn scene_on_an_audio_only_file_is_not_a_degradation() {
        let document = Document::new(MediaInfo::default(), AnalysisConfig::default());
        let requested = AnalysisConfig {
            modules: vec![Capability::Scene],
            ..AnalysisConfig::default()
        };
        assert!(!is_degraded(&document, &requested));
    }

    #[test]
    fn profile_and_modules_conflict() {
        let error = Cli::try_parse_from([
            "sonolytica",
            "analyze",
            "a.wav",
            "--profile",
            "structure",
            "--modules",
            "vad",
        ])
        .unwrap_err();
        assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
    }

    #[test]
    fn a_capability_without_a_run_is_degraded() {
        let document = Document::new(MediaInfo::default(), AnalysisConfig::default());
        let requested = AnalysisConfig {
            modules: vec![Capability::Vad],
            ..AnalysisConfig::default()
        };
        assert!(is_degraded(&document, &requested));

        let nothing_requested = AnalysisConfig {
            modules: Vec::new(),
            ..AnalysisConfig::default()
        };
        assert!(!is_degraded(&document, &nothing_requested));
    }
}
