# Roadmap

Milestones are ordered by technical dependency, not by marketing appeal. Each
phase has an acceptance gate that must be demonstrated, not asserted.

## Current cycle (2026-11)

Delivered: the typed `query` interface (no `jq`), `--profile
structure|standard|ultra`, derived chapters, and the agent skill rewritten as a
citation protocol with references and workflows. The skill-cost cycle is closed
(reproducible benchmark, `brief` v2, 200-line core): hard 16/16 at 14 145 fresh
tokens, easy 8/8 at 8 626, long 20 min 15/16 at 17 741. Open: overlap mapping
(the long run lost one point on a range), easy fixed cost.

Ordered next, each gated on a measured number:

1. **Baselines** — WER on LibriSpeech test-clean, DER on AMI/VoxConverse;
   recorded in `docs/EVAL.md` with dataset, model, parameters and date.
2. **Speech quality** — forced alignment ✅ (wav2vec2 CTC, English, `--align`);
   next: VAD-boundary word loss, overlap-aware diarization, non-English alignment.
3. **Video** — shot boundaries ✅, one keyframe per scene with `artifacts[]` ✅
   (`query frames`), on-screen text with PP-OCRv5 ✅ (`--ocr`,
   `text_overlay`). Next: object/face models.
4. **Music depth** — section segmentation, note transcription, stems/bass.
5. **MCP** — only for a shell-less client.

The skill is the interface; it can never be stronger than the document, so
capability work comes before skill work on any new modality.

## Phase 0 — Foundation ✅

Workspace, domain model, provenance, configuration, invariant validator,
model-free tests, CI, ADRs.

**Gate:** `cargo test --workspace` on a machine without FFmpeg models or a C
toolchain. ✅

## Phase 1 — Real audio pipeline without models ✅

FFmpeg probe/decode, bounded chunking at two rates, SHA-256 provenance, STFT
frames, loudness/RMS/LRA, silence, tempo/beat grid, chroma, key, triads.

**Gate:** a generated file produces a valid document, DSP measurements are
unit-tested against physics (0 dBFS sine ≈ −3 LUFS, 120 BPM clicks ≈ 120 BPM,
C-major triad → C major). ✅

## Phase 2 — Speech ✅

Silero VAD, Whisper with token timestamps, local diarization
(pyannote segmentation + WeSpeaker + cosine clustering), deterministic
merge into speaker turns, overlap/interruption edges, speech summary, model
registry with digest verification.

**Gate:** a real two-voice fixture produces a speaker-attributed, word-timed
transcript through the full pipeline, with no failed stages. ✅
(`SONOLYTICA_E2E=1 cargo test -p sonolytica-engine --features "full,metal"
--test e2e_speech`)

**Alignment:** `--align` replaces Whisper token times with wav2vec2 CTC forced
boundaries (English, 20 ms frames); measured median drift 110 ms. Region-boundary
word loss is a text problem and remains open.

## Phase 3 — Music and acoustic depth (partially ✅)

Done: tempo + DP beat tracker, key profiles, triad chords, loudness, silence,
spectral summaries.

Next:
- Section segmentation (self-similarity over chroma/energy) — must be evaluated
  before it is trusted; no heuristic output without a confidence.
- Sound event classification through an ONNX classifier (PANNs/AST/YAMNet).
- Instrument and vocal detection (source separation is a model decision, not a
  DSP one: Demucs via isolated bridge if ONNX parity is unacceptable).
- Polyphonic transcription: explicitly research, not a promised feature.

**Gate for each:** a licensed evaluation set, a documented metric, and a
reported number with conditions — or no claim at all.

## Phase 4 — Video (partially ✅)

Done: frame sampler over the existing FFmpeg pipe (64×36 gray at 2 fps),
shot-boundary detection (perceptual hash + luma histogram, adaptive threshold),
scene segments on the same timeline, video-only input.

Next:
- Scene↔speech alignment edges; keyframe selection and deduplication, with
  `artifacts` referenced from the document.
- OCR for on-screen text.
- Optional vision provider (ONNX embedding/classifier) behind a feature.
- Speaker-to-face association only when diarization and face embeddings agree;
  otherwise report "unresolved", never a guess.

**Gate:** a video with known cuts yields cut times within one frame, and
audio-video timestamps agree on the container timebase.

## Phase 5 — Agent ecosystem

- `sonolytica mcp` ✅ (2026-11-13): tools-only stdio JSON-RPC, same binary and
  query implementation. Tools: `analyze_media`, `query_brief`, `query_ask`,
  `query_timeline`, `query_search`, `query_transcript`, `query_words`,
  `query_speakers`, `query_captions`, `query_poses`, `query_runs`,
  `query_verify`, `list_models`, `doctor`. See `docs/MCP.md`.
  Remaining: official `rmcp` SDK, streamable HTTP, resources/prompts.
- Resources: the document itself, plus per-capability slices (not yet).
- Transcript index (SQLite FTS5) for large corpora; documents stay the source
  of truth.
- Portable agent skill updated with tool schemas and citation rules.
- Optional HTTP API (`axum`) only after MCP proves the shape.

**Gate:** an agent can answer "who said X, when, and what else happened then?"
using only MCP tool calls, and every claim it makes cites a segment id.

## Phase 6 — Research and scale

- Benchmark harness runs on licensed datasets: LibriSpeech test-clean
  (CC BY 4.0) for WER; AMI or VoxConverse (CC BY 4.0) for DER; SMC/ballroom for
  beat tracking (research licences; manual download, not CI).
- Streaming/incremental analysis with checkpointed documents.
- Content-addressed cache: key = media SHA-256 + config hash + model digest.
- Quantized model profiles and an `ort`-free (tract/candle) build for
  environments that cannot ship native binaries.
- Optional isolated Python bridge (JSON-lines over stdio) for research models
  with no Rust/ONNX path. Never in the core path.

## Explicit non-goals for now

- A hosted service or multi-tenant control plane.
- Training or fine-tuning models.
- A GUI.
- Emotion/intent as if they were facts about a person; any future vocal-affect
  output must be labelled as expression, not internal state.
