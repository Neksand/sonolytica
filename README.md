# SONOLYTICA

**Multimodal perception infrastructure for AI agents.** SONOLYTICA turns audio
and video into a typed, timestamped timeline that an agent can query — speech
with word timings and speaker turns, music structure, acoustic events, and the
relationships between them.

It is a Rust workspace, not a wrapper around one model. Models are optional
providers behind stable traits; the engine, the temporal model and the DSP
pipeline keep working when a model is missing or fails.

```
media ──► probe/decode ──► VAD ──► ASR ──┐
                         └► diarization ─┴─► merge ──► Document (typed timeline)
                         └► DSP (music, loudness, silence) ─┘
```

## Status

This is the first development cycle. What is real today:

- **Media ingestion** through FFmpeg subprocesses: metadata, mono PCM decode,
  SHA-256 provenance, size/duration/timeout limits.
- **A typed document model**: hierarchical tracks, segments with parent/child
  links, typed cross-modal relations, confidence, provenance and schema
  versioning, with an invariant validator.
- **DSP analysis, no models required**: STFT features, EBU R128 loudness, true
  peak, LRA, silence detection, onset/tempo/beat tracking, chroma, key and
  triad chord estimation, chroma self-similarity sections, plus loudness range.
- **Speech**: Silero VAD, Whisper (whisper.cpp) with word timestamps, forced
  word alignment (wav2vec2 CTC, English), local diarization (Sortformer 4spk,
  or pyannote segmentation + WeSpeaker embeddings as a fallback), speaker-
  attributed dialogue, overlap as simultaneous turns.
- **Failure isolation**: a missing model produces a warning and a `Partial`
  document, never a fabricated result.
- **CLI** with `inspect`, `analyze`, `query`, `models`, `doctor`, `schema`.
  `query` answers timeline, search, speaker, music, acoustic, scene and
  run-status questions without `jq`.
- **Video shots and keyframes, no vision model**: FFmpeg samples 2 fps RGB
  signatures, an adaptive hash/histogram spike marks a cut, `scene` segments
  land on the same timeline as speech, and `analyze --out` writes keyframe JPEGs
  the agent can open with its own vision.
- **On-screen text**: `analyze --ocr` runs PP-OCRv5 (ONNX, Apache-2.0) on the
  keyframes and emits `text_overlay` segments with exact text, box and
  confidence.
- **Evaluation harness**: WER/CER, diarization error rate, per-run timings.

Not implemented yet (roadmap): object and face detection, non-English
alignment, MCP server, HTTP server, Python bridge, sound-event classification,
music notes and stems.

No accuracy numbers are claimed in this repository. The eval harness exists so
numbers can be produced honestly against licensed datasets. A measured snapshot
of the whole project is in [`docs/STATUS.md`](docs/STATUS.md).

## Install

Requirements:

- Rust 1.99 (pinned in `rust-toolchain.toml`)
- FFmpeg 7+ on `PATH` (`SONOLYTICA_FFMPEG` / `SONOLYTICA_FFPROBE` override)
- macOS 13.4+ when using the ONNX Runtime prebuilts (CoreML objects)
- For the speech pipeline: `cmake` and a C/C++ toolchain (whisper.cpp)

```bash
# DSP-only build: no native model toolchain, no models
cargo build --release -p sonolytica-cli

# Full local pipeline (macOS with Metal)
cargo build --release -p sonolytica-cli --features full-metal

# Full local pipeline (CPU, Linux/Windows)
cargo build --release -p sonolytica-cli --features full
```

## Quickstart

```bash
# 1. Check the environment and what this build can actually run
sonolytica doctor

# 2. Fetch pinned models (verified by SHA-256 before use)
sonolytica models fetch \
  silero-vad-v5 whisper-base.en sortformer-4spk-v2 \
  pyannote-segmentation-3.0 wespeaker-campplus

# 3. Analyse (--profile structure | standard | ultra)
sonolytica analyze interview.mp3 --out interview.json --profile standard --summary

# 4. Ask the document questions — typed output, no jq required
sonolytica query summary interview.json
sonolytica query timeline interview.json --from 00:00:10 --to 00:00:20
sonolytica query transcript interview.json        # full text, untruncated
sonolytica query search interview.json pricing
sonolytica query speakers interview.json
sonolytica query timeline interview.json --type scene   # video shots, if any
sonolytica query frames interview.json --max 8          # keyframe JPEG paths
sonolytica query timeline interview.json --type text_overlay   # OCR lines
sonolytica query runs interview.json          # which stages failed, and why
```

For video with on-screen text, add `--ocr` to `analyze`.

Times accept `HH:MM:SS.mmm`, `MM:SS`, `12.5s`, `950ms` or raw microseconds.
Every subcommand takes `--json` for a typed object on stdout.

`query chapters` derives chapters from silences, speaker changes and scene cuts —
no language model, no invented titles.

### Profiles

| Profile | What runs | Use when |
|---|---|---|
| `structure` | VAD, diarization, music/acoustic, shot boundaries — no transcript | orientation, first pass, music-only |
| `standard` | everything, with a transcript | default; any content question |
| `ultra` | standard + exit 2 if a requested capability failed or never ran | automation that must not silently degrade |

`--profile` and `--modules` are mutually exclusive. `ultra` adds forced word
alignment (English) and turns a skipped requested capability into exit 2; note
transcription is still a separate milestone.

`analyze` emits one versioned JSON document (`schema_version: 1.0.0`). A
compact excerpt:

```json
{
  "schema_version": "1.0.0",
  "media": { "duration": 15212938, "sha256": "89e1…", "streams": [ … ] },
  "config": { "modules": ["acoustic", "diarize", "music", "transcribe", "vad"] },
  "runs": [
    { "capability": "vad",        "provider": "silero-vad", "status": "complete", "duration_ms": 46 },
    { "capability": "transcribe", "provider": "whisper-cpp", "status": "complete",
      "model": { "id": "whisper-base.en", "sha256": "a037…", "license": "MIT" } }
  ],
  "tracks":   [ { "kind": "speech" }, { "kind": "speaker_turn" } ],
  "segments": [
    { "id": 0, "range": { "start": 0, "end": 6000000 },
      "kind": { "type": "speech", "text": "…", "words": [ … ] } },
    { "id": 1, "parent": 0, "range": { "start": 0, "end": 6000000 },
      "kind": { "type": "speaker_turn", "speaker": "S1", "text": "…", "overlap": false } }
  ],
  "relations": [ { "from": 1, "to": 2, "kind": "overlaps" } ],
  "warnings": []
}
```

Time is integer microseconds from the start of the container, ranges are
half-open (`[start, end)`). See [`docs/SCHEMA.md`](docs/SCHEMA.md).

## Why this shape

- **Capability registry, not a model zoo.** The engine asks "who can diarize?"
  and records who answered. Adding a model does not touch the engine.
- **A document, not a transcript.** Word timings, speaker turns, scenes,
  chords and events share one timeline with parent links and typed relations.
- **Models are optional and verifiable.** Every artifact is pinned by digest
  and license. Absence is a warning, not an exception.
- **No invented data.** Missing values are `null`, not `0`, `"unknown"`, or a
  plausible-sounding sentence. Confidence is explicit and documented as a
  heuristic where that is what it is.

Design records: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md),
[`docs/adr/`](docs/adr/), [`docs/ROADMAP.md`](docs/ROADMAP.md).

## Evaluation

```bash
# Word error rate against a reference transcript
sonolytica-eval wer --reference ref.txt --hypothesis document.json

# Diarization error rate against an RTTM reference
sonolytica-eval der --rttm meeting.rttm --document document.json --file-id meeting

# Per-stage timings and realtime factor
sonolytica-eval report --document document.json
```

Metrics are implemented in `crates/eval`: Levenshtein WER/CER and frame-based
DER with a collar. The DER speaker mapping is greedy, and that ceiling is
documented in the source.

## Models and licensing

SONOLYTICA is dual-licensed MIT OR Apache-2.0. Model artifacts have their own
licenses (all permissive, no GPL) and are **not** vendored. See
[`docs/MODELS.md`](docs/MODELS.md). FFmpeg is invoked as a subprocess, not
linked.

## Development

```bash
cargo fmt --all
cargo clippy --workspace --all-targets --features full -- -D warnings
cargo test --workspace                                # fast, no models
SONOLYTICA_E2E=1 cargo test -p sonolytica-engine --features "full,metal" \
  --test e2e_speech -- --nocapture                    # real models (macOS fixtures)
```

The model-free test suite must keep passing on a machine with no models at
all; that is an architectural requirement, not a convenience.

## Agent skill

[`skills/sonolytica/SKILL.md`](skills/sonolytica/SKILL.md) documents how an
agent should invoke the CLI, read the document and cite timestamps. Install it
with [`skills/sonolytica/README.md`](skills/sonolytica/README.md) (`cp -R
skills/sonolytica ~/.agents/skills/`). The skill's effect is measured in
[`docs/BENCHMARK.md`](docs/BENCHMARK.md). Resuming work after a context reset:
read [`docs/HANDOFF.md`](docs/HANDOFF.md) first. An MCP server is the next
interface deliverable.
