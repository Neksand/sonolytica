# SONOLYTICA

**Media becomes a document an agent can cite.** Local, typed, timestamped:
speech with word timings and speaker turns, shots and keyframes, on-screen text,
music and acoustics — one timeline, provenance on every fact, no cloud.

```console
$ sonolytica analyze meeting.mp4 --out meeting.json --ocr --summary
$ sonolytica query brief meeting.json
$ sonolytica query search meeting.json pricing
00:00:43.608–00:00:47.608  speech         S2  The pricing page goes live on Friday!  [0.680]  | pricing 00:00:44.058–00:00:44.928
```

Agents answer from `meeting.json` with exact quotes and ranges, or say the fact
is absent. They never guess, and every claim is checkable.

[![ci](https://github.com/neksand/sonolytica/actions/workflows/ci.yml/badge.svg)](https://github.com/neksand/sonolytica/actions/workflows/ci.yml)
[![license](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![release](https://img.shields.io/github/v/release/neksand/sonolytica?display_name=tag)](https://github.com/neksand/sonolytica/releases)

## Why

- **Agents are blind without a document.** A text-only model can run `ffmpeg`
  and `whisper`; it cannot tell you who spoke at 02:14, what was on screen at
  01:20, or whether a budget cut was ever mentioned. SONOLYTICA builds that
  document once and answers from it.
- **Citations, not vibes.** Every range is a document fact. Missing data is
  `null`, a warning or `partial` — never a plausible sentence.
- **Local and cheap.** No API, no upload, no per-minute bill. Whisper, Silero,
  Sortformer and PP-OCRv5 run on your machine, pinned by SHA-256.

## Install

```bash
# macOS (arm64) / Linux (x86_64) release binary
curl -fsSL https://raw.githubusercontent.com/neksand/sonolytica/main/install.sh | sh

# or build from source
cargo build --release -p sonolytica-cli --features full-metal   # macOS, Metal
cargo build --release -p sonolytica-cli --features full         # CPU only
```

Requirements: FFmpeg 7+ on `PATH`; `cmake` for the speech pipeline. Models are
downloaded on demand and verified by digest.

```bash
sonolytica doctor                 # what this machine can actually run
sonolytica models fetch --set core  # ~1.5 GB: VAD, Whisper, Sortformer, aligner
sonolytica install-skill          # Claude Code, Codex, OpenCode, Cursor
```

`install-skill` embeds the skill in the binary: no repository checkout, no
copy-paste, re-run to upgrade. `--project` installs into the current directory,
`--dry-run` prints paths, `--uninstall` removes it.

## 60-second tour

```bash
sonolytica analyze talk.mp4 --out talk.json --profile standard --ocr --summary
sonolytica query brief talk.json            # media, runs, speakers, chapters, cuts, OCR
sonolytica query search talk.json budget    # exact ranges, never rounded
sonolytica query timeline talk.json --from 00:01:20 --to 00:01:30
sonolytica query verify talk.json --claims answer.json  # quotes + ranges, mechanically
sonolytica report talk.json --out talk.html # human page: chapters, quotes, frames
```

`report` writes one HTML file plus its keyframes — open it, share it, print it.
Every quote carries its range; warnings are shown, not hidden.

## What is in the document

| Layer | Facts | Provider |
|---|---|---|
| Speech | words, ranges, confidence, language | Whisper (whisper.cpp) |
| Speakers | turns, overlap spans, speaking time | Sortformer 4spk, pyannote fallback |
| Alignment | word boundaries, ~110 ms median drift | wav2vec2 CTC (English) |
| Video | shots, cuts, keyframes on the timeline | FFmpeg signatures (2 fps) |
| On-screen text | exact OCR lines with boxes | PP-OCRv5 |
| Music | tempo, key, triads, sections, loudness | local DSP |
| Acoustics | silence, LRA, true peak | local DSP |

Every fact has a `run` (provider, model digest, status, device) and the
document passes `Document::validate` before it is written.

## What it does not do (honest)

- **No semantics.** No object, face or action model in this release; keyframes
  and OCR are the whole visual layer. A local VLM is opt-in roadmap work, not a
  shipped feature.
- **Diarization caps at four speakers.** Above 10 minutes a
  `diarization_speaker_cap` warning says the count is approximate. Long-meeting
  streaming is the next quality milestone.
- **Overlaps are labelled, not split.** Two simultaneous turns exist; a merged
  ASR line is not separated word by word.
- **Music is structural.** Tempo, key, triads and technical sections; no notes,
  stems, instruments or taste.
- **English alignment.** Other languages keep Whisper token timestamps.

## Measured, not claimed

`docs/BENCHMARK.md` has the full method and the failures. Current medians with
DeepSeek V4.1 Flash, clean directories, fixed prompts:

| Fixture | Score | Fresh tokens | Before the skill cycle |
|---|---|---|---|
| 14 s video, 4 questions | **8/8** | **7 517** | 9 521 |
| 4 min meeting, 8 questions | **16/16** | **10 393** | 22 410 |
| 20 min real meeting, 8 questions | **16/16** | **11 308** | 17 633 |

Reproduce any row with `scripts/bench/prepare_agent_fixture.sh` and
`scripts/bench/session_tokens.sh`. If a number moves, it comes with the date,
model, fixture and command log — or it is not written down.

Head-to-head on the same hard fixture: **Gemini (Antigravity) 14/16 alone,
16/16 with the skill and 6.6× faster; TwelveLabs Pegasus 10/16**; raw outputs
and the exact misses in [`docs/BENCHMARK.md`](docs/BENCHMARK.md).

## Why not X?

| | Gemini / TwelveLabs | WhisperX + pyannote scripts | SONOLYTICA |
|---|---|---|---|
| Runs locally, no upload | no | yes | **yes** |
| Word-level quotes with ranges | summary-level | yes | **yes, with a validator** |
| Speaker turns + overlap map | partial | yes | **yes** |
| Typed schema for agents | no | no | **yes** |
| HTML report for humans | partial | no | **yes** |
| Open-ended video semantics | **strong** | no | no (frames + OCR) |
| Cost | $/hour of video | free + glue | **free** |

We win on evidence and locality, not on seeing. That is the design.

## For developers

```bash
cargo test --workspace                 # no models, no cmake, no ONNX runtime
cargo test --workspace --all-features  # every provider compiled in
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

- `crates/core` — document model, schema, validator, timeline math.
- `crates/engine` — pipeline stages and the async boundary.
- `crates/providers` — model registry, Whisper, Silero, diarization, OCR.
- `crates/cli` — `analyze`, `query`, `report`, `install-skill`, `models`.

Design: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), ADRs in
[`docs/adr/`](docs/adr/). Current numbers: [`docs/STATUS.md`](docs/STATUS.md).
Eval methods: [`docs/EVAL.md`](docs/EVAL.md).

## License

Dual-licensed MIT OR Apache-2.0. Model artifacts keep their own permissive
licenses (no GPL) and are never vendored; FFmpeg is a subprocess, not a link.
See [`docs/MODELS.md`](docs/MODELS.md).
