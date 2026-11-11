# Handoff — SONOLYTICA after the media/agent cycle

Last updated: 2026-11-11. Branch `main`, working tree clean, **no remote
configured** (the user pushes). Latest code commit: `e2f5592`; the handoff commit (`docs: handoff for a fresh session`) follows it.

Read this first, then `docs/STATUS.md` (numbers), `docs/EVAL.md` (methods),
`docs/BENCHMARK.md` (agent tests), `docs/OCR.md` (OCR), `docs/SCHEMA.md`
(schema 1.4.0). The forward plan lives outside the repo at
`/Users/neksand/.opencode/plan/sonolytica-next-cycle-2026-11-11.md`.

---

## 1. What this is

One media file becomes one typed, timestamped JSON document: speech with word
timings and speaker turns, music/acoustic aggregates, video shots, keyframes,
OCR text, and provenance on every segment. Agents read it via
`sonolytica query` and cite it. Local only, no Python at runtime, no GPL.

| | |
|---|---|
| Rust | 14 398 lines, 7 crates: `providers` (3.7k), `cli` (3.1k), `engine` (2.3k), `core` (2.1k), `dsp` (1.4k), `media` (1.2k), `eval` (0.6k) |
| Binary | `sonolytica` (`sonolytica-cli`) |
| SDK | `sonolytica-engine` (`Engine::analyze`) |
| Schema | **1.4.0**, additive 1.0 → 1.4 |
| Models | 9, SHA-256 pinned, in `~/.cache/sonolytica/models` |
| Query commands | 13: timeline, at, search, transcript, words, speakers, chapters, frames, music, acoustic, runs, summary, help |
| Skill | `skills/sonolytica/` (SKILL.md + 9 references + install README) |
| Tests | **121** model-free / **131** all-features |

Features: `sonolytica-cli --features full-metal` = whisper + silero + diarize +
ocr + align + metal. `cargo test --workspace` runs without models/cmake/ORT.

## 2. Capabilities and verified numbers

| Area | Result | Where |
|---|---|---|
| ASR | WER **6.05 %**, CER 2.25 % — LibriSpeech test-clean, 100 utts, `whisper-base.en` | EVAL |
| Diarization | DER **11.81 % dev / 17.46 % test** (VoxConverse pilots, 5 files each); speaker count is the weak spot | EVAL |
| Alignment | wav2vec2 CTC, 20 ms frames, English; **median 110 ms** vs Whisper, 88 % ≤600 ms; Whisper itself is 300–950 ms early on JFK | EVAL |
| OCR | PP-OCRv5 det+rec; 12/12 lines exact on the 6-slide fixture; 7/7 CJK terminal | OCR |
| Music | 120.3 BPM for a 120 BPM click; C major triad correct; A–B–A section boundaries within 80/160 ms | EVAL |
| Video | 2 fps chromaticity signatures, cuts ±0.5 s; 6/6 scenes, 6 keyframes on the hard fixture | EVAL/BENCHMARK |
| Throughput | **21.64× realtime** on 121.7 s, full pipeline, all stages complete | EVAL |
| Russian | exact 12 s transcript with `whisper-small --language ru` | DOGFOOD |

### Agent benchmarks (details and prompts in BENCHMARK)

Easy 14 s video (4 questions, max 8):

| Model | Skill | Score | Fresh tokens |
|---|---|---|---|
| Nemotron 3.5 Super VL | no | 5/8 | 61 280 |
| Nemotron 3.5 Super VL | yes | 8/8 | 35 013 |
| LongCat 2.5 | yes | 8/8 | 55 997 |
| DeepSeek V4.1 Flash | no | 8/8 | 9 752 |
| DeepSeek V4.1 Flash | yes | 8/8* | 41 399 |

Russian 12 s (6 tasks, max 12): Nemotron no 11/12 (23 131), skill 12/12
(34 558).

Hard 4-minute meeting, 4 voices, 6 slides, 1 overlap; baselines had **no**
whisper/tesseract/sonolytica (8 tasks, max 16):

| Model | Skill | Score | Fresh tokens |
|---|---|---|---|
| Nemotron VL | no | **7/16** | 91 952 |
| Nemotron VL | yes | **16/16** | 41 139 |
| DeepSeek | no | **4/16** | 7 403 |
| DeepSeek | yes | **16/16** | 15 813 |

`*` one false "visually confirmed" claim, fixed in the skill: text-only models
must not claim sight.

## 3. Things that must not regress

- `cargo test --workspace` passes **without models, cmake, or native runtimes**.
  Core crates must not depend on `ort`/`whisper`.
- No fabricated data: missing = `null`/warning/`partial`, never a guess.
- Every document passes `Document::validate`; timestamps never exceed media.
- Every model is SHA-pinned + license-documented; unpinned refused.
- `ort` pinned `=2.0.0-rc.15`; one environment per process; **shared session
  builder uses `AutoDevicePolicy::PreferCPU`** (CoreML silently mis-executes
  dynamic-shape graphs: PP-OCRv5 detection collapsed, recognition blank; VAD
  and diarization also report `device: "cpu"`, so CPU is the honest choice).
- LTO stays off: thin LTO breaks linking ORT CoreML objects.
- Provider traits synchronous; async boundary in the engine.
- No GPL (`deny.toml`).

## 4. Design decisions and quirks (learned the hard way)

- **ASR segment start includes padding**: ~300 ms digital lead-in plus VAD pad.
  `speech.range.start` is not the first audible syllable. For exact onset use
  `words[].range` or `--align`. The hard fixture proved it: doc says 07.992,
  the actual speech starts ~08.96, and DSP silence says 08.06–08.96.
- **Language**: pass `--language`; English → `whisper-base.en`, others →
  `whisper-small --language xx`. Auto-detect mislabelled English as Italian
  twice. Never infer language from the user's prompt.
- **Diarization**: 1.5 s chunks, average-linkage AHC, threshold 0.45,
  `max_speakers=12`. Over-segments crowded audio (4 voices → 7 labels on the
  hard fixture); overlap is flagged but not split; an overlapped line can be
  missing from the transcript entirely.
- **Rejected count experiments** (do not retry without a new idea): single
  silhouette split, tiny-cluster pruning, NME eigengap (collapsed real files to
  1 speaker, DER 63.46 %), widest dendrogram gap (4→2 on the first file).
  Synthetic block tests pass while real embeddings fail — synthetic-only
  validation is not evidence.
- **Keyframes**: one per scene by default; OCR runs on those keyframes and adds
  `text_overlay` with a **point range** (the frame instant, not the shot).
  Artifacts use paths relative to the document; `query frames` resolves them.
- **Shot detection**: 2 fps, 64×36 RGB, **chromaticity** histogram (pixel
  normalised by its brightest channel). Raw RGB collapses all dark scenes into
  bin 0; that bug cost a whole 4-minute fixture.
- **Music sections** are technical boundaries from chroma self-similarity, no
  verse/chorus labels; novelty is normalised 0–1.
- **Schema**: only additive changes, minor bumps, old documents stay readable.
  `1.1.0` artifacts, `1.2.0` text_overlay/text track, `1.3.0` align,
  `1.4.0` music sections.

## 5. Environment (this machine)

macOS 26.6.2, M5 Pro, 24 GB, Rust 1.99, FFmpeg 9.0.2. **FFmpeg has no
`drawtext`** (no libfreetype), so the OCR e2e skips here; the CLI path was
verified on PIL-rendered slides and real keyframes. `whisper` CLI and
`tesseract` **are installed** on PATH — the first benchmarks' baselines used
them; the hard benchmark forbids them by instruction.

Model store `~/.cache/sonolytica/models` (~1.2 GB): silero_vad.onnx,
ggml-tiny.en/base.en/small, wav2vec2-base-960h.onnx, segmentation-3.0.onnx,
wespeaker CAM++, pp_ocrv5_det/rec.onnx. `models list` shows status.

Fixtures under `/private/tmp` (not committed; regenerate):
`sono-eval` (LibriSpeech, VoxConverse parquet), `bench-hard` (meeting.mp4 +
ref.json + artifacts), `sono-bench` (14 s video), `sono-query-e2e`,
`sono-stats` (abc/aba wavs). They survive a session clear but not a reboot.

## 6. How to verify and reproduce

```bash
cd /Users/neksand/orca/projects/SONOLYTICA
git status                       # must be clean
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace           # 121, no models
cargo test --workspace --all-features   # 131

# real models (macOS fixtures, ~10 s)
SONOLYTICA_E2E=1 cargo test -p sonolytica-engine --features "full,metal" --test e2e_speech
SONOLYTICA_E2E=1 cargo test -p sonolytica-engine --features "full,metal" --test e2e_alignment

# evals (datasets downloaded by hand, see scripts/eval/README.md)
python3 scripts/eval/wer_librispeech.py --root /tmp/sono-eval/LibriSpeech/test-clean --count 100
python3 scripts/eval/der_voxconverse.py --parquet /tmp/sono-eval/voxconverse/dev-00000-of-00005.parquet --count 5

# hard benchmark fixture
python3 scripts/bench/make_meeting_fixture.py --out /tmp/bench-hard
```

Note: `cargo test` compiles the bin **without** features and can overwrite
`target/debug/sonolytica`. Rebuild with `--features full-metal` before using
the CLI; eval scripts have a preflight that refuses a DSP-only build.

Token accounting: OpenCode stores per-session totals in
`~/.local/share/opencode/opencode.db`, table `session_v2`
(`tokens_input`, `tokens_output`, `tokens_reasoning`, `tokens_cache_read`,
`cost`). "Fresh" = input + output.

## 7. Known gaps and next steps (priority)

1. **Speaker count + overlap — the top gap.** 4 voices became 7 labels; one
   overlapped line vanished. Next: Sortformer 4spk ONNX (CC-BY-4.0) for ≤4
   speakers, powerset-class mapping for overlap when unavailable; evaluate on
   a **fresh** VoxConverse/AMI subset (both current shards are burned).
2. **Overlap splitting** (two simultaneous turns) with a measured DER/JER gain.
3. **`query brief`** — one compact read (runs, chapters, speakers, highlights,
   counts) instead of 5–8 queries; add a "minimum commands per question" table
   to the skill. Target: cut DeepSeek skill sessions from ~41k to ~20k fresh
   tokens.
4. **Modes** `--effort low|normal|max` + `--vision agent|local`, with the RAM
   table in the skill: low ≈40 MB (no ASR), normal +200–500 MB, max +400 MB
   (OCR/align), local VLM +2–4 GB opt-in. Store the chosen mode in config.
5. **Music notes/stems**: basic-pitch (Apache-2.0, ~20 MB) for notes, Demucs
   (MIT) for stems/bass only if the ONNX export works and F1 ≥0.6.
6. **Non-English alignment**: MMS/XLSR CTC; FLEURS WER table (ru/es/de/fr/zh).
7. **Caching**: key = media SHA-256 + config hash + model digests; re-runs ≤5 %
   of the first.
8. **MCP** (`rmcp`): only for a shell-less client; tools mirror `query`.

## 8. Lessons from this cycle

- **Benchmark contamination is real**: agents read JSON artifacts from the
  working directory and answered from previous runs. Every session must run in
  a directory containing only the media file (now the rule).
- **A rich shell is a strong baseline**: with `whisper large-v3` and
  `tesseract` on PATH, a text-only model scored 8/8 on the 14 s fixture. The
  hard benchmark removed those tools; that is the honest test.
- **Free model quotas vanish** (LongCat hit `Free usage limit reached`
  mid-run). Record model + quota in the report; do not plan a matrix around a
  free tier.
- **Docs-as-tests** (`crates/cli/tests/skill_docs.rs`) checks every command in
  the skill against `--help`; keep it green when renaming anything.
- **Do not tune on the same split you report.** VoxConverse dev was used for
  the threshold, test for one check, then test was seen again. The next number
  needs unseen data.
- **Synthetic fixtures exposed real bugs** three times: colour-blind cuts
  (red/blue), dark-slide collapse (chromaticity), ASR padding vs onset. Keep
  making fixtures uncomfortable.

## 9. For the next agent

Start with `git status`, `docs/HANDOFF.md` (this file), `docs/STATUS.md`, then
pick item 1 in §7. Do not re-litigate the rejected count experiments; do not
add GPL; keep the model-free build. If a change is not measured on a licensed
or reproducible fixture, it is not done.
