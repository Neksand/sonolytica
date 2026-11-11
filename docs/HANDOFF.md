# Handoff — SONOLYTICA after the media/agent cycle

Last updated: 2026-11-11. Branch `main`, working tree clean, **no remote
configured** (the user pushes).

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
| Rust | 15 904 lines, 7 crates: `providers` (4.3k), `cli` (3.6k), `engine` (2.6k), `core` (2.1k), `dsp` (1.4k), `media` (1.2k), `eval` (0.7k) |
| Binary | `sonolytica` (`sonolytica-cli`) |
| SDK | `sonolytica-engine` (`Engine::analyze`) |
| Schema | **1.4.0**, additive 1.0 → 1.4 |
| Models | 10, SHA-256 pinned, in `~/.cache/sonolytica/models` |
| Query commands | 14: brief, timeline, at, search, transcript, words, speakers, chapters, frames, music, acoustic, runs, summary, help |
| Skill | `skills/sonolytica/` (SKILL.md + 9 references + install README) |
| Tests | **135** model-free / **150** all-features |

Features: `sonolytica-cli --features full-metal` = whisper + silero + diarize +
ocr + align + metal. `cargo test --workspace` runs without models/cmake/ORT.

## 2. Capabilities and verified numbers

| Area | Result | Where |
|---|---|---|
| ASR | WER **6.05 %**, CER 2.25 % — LibriSpeech test-clean, 100 utts, `whisper-base.en` | EVAL |
| Diarization | AHC pilots: DER **11.81 % dev / 17.46 % test**; Sortformer 4spk with the 0.70 VAD gate: **11.08 % weighted / 7.76 % unweighted** on `test-00002`, **9.95 % / 7.49 %** on fresh `test-00003`; JER **9.85 % / 7.34 %** there; counts **6/6 exact** | EVAL |
| Alignment | wav2vec2 CTC, 20 ms frames, English; **median 110 ms** vs Whisper, 88 % ≤600 ms; Whisper itself is 300–950 ms early on JFK | EVAL |
| OCR | PP-OCRv5 det+rec; 12/12 lines exact on the 6-slide fixture; 7/7 CJK terminal | OCR |
| Music | 120.3 BPM for a 120 BPM click; C major triad correct; A–B–A section boundaries within 80/160 ms | EVAL |
| Video | 2 fps chromaticity signatures, cuts ±0.5 s; 6/6 scenes, 6 keyframes on the hard fixture | EVAL/BENCHMARK |
| Throughput | **16.9×** on the 240 s hard fixture with the VAD gate (17.9× without; +0.8 s); 14.1× on the 155.6 s full audio pipeline before the gate | EVAL |
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

`query brief` (2026-11-12): the hard fixture scores **16/16 at 22 410 fresh**
tokens in 15 commands (the first run, with three reference files preloaded, used
26 998); the easy fixture scores **8/8 at 9 521 fresh** versus 41 399 for the
previous skill. Details and caveats: BENCHMARK.

## 3. Things that must not regress

- `cargo test --workspace` passes **without models, cmake, or native runtimes**.
  Core crates must not depend on `ort`/`whisper`.
- No fabricated data: missing = `null`/warning/`partial`, never a guess.
- Every document passes `Document::validate`; timestamps never exceed media.
- Every model is SHA-pinned + license-documented; unpinned refused.
- Diarization prefers Sortformer when its pinned model is present; pyannote is
  the fallback. The 4-speaker cap is documented, never silently exceeded.
- Diarization runs a **dedicated VAD gate pass at threshold 0.70**; the
  configured 0.45 pass belongs to ASR and must not be tightened to gate. A gate
  failure warns `diarization_vad_gate_unavailable` and leaves turns ungated.
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
- **Diarization (default)**: Sortformer 4spk v2 ONNX, 128-bin log-mel with no
  normalisation, 80 ms activity. Overlap becomes two simultaneous turns. It
  caps at 4 speakers; crowded audio collapses into four voices. Files longer
  than 300 s are processed in overlapping windows and speaker channels are
  matched by activity co-occurrence. 469 MB on disk, ~2.8 GB peak RSS on a
  4-minute file.
- **Diarization (fallback)**: pyannote 3.0 + WeSpeaker, 1.5 s chunks,
  average-linkage AHC, threshold 0.45, `max_speakers=12`. Over-segments crowded
  audio (4 voices → 7 labels on the hard fixture); overlap is flagged but not
  split; an overlapped line can be missing from the transcript entirely.
- **Rejected count experiments** (do not retry without a new idea): single
  silhouette split, tiny-cluster pruning, NME eigengap (collapsed real files to
  1 speaker, DER 63.46 %), widest dendrogram gap (4→2 on the first file).
  Synthetic block tests pass while real embeddings fail — synthetic-only
  validation is not evidence.
- **VAD gate threshold**: at the configured 0.45 the gate removed nothing —
  Silero calls music and room noise speech (all 271 turns on `uicid` had ≥79 %
  coverage) — and it dropped one real turn on `ltgmz`. At 0.70 in a dedicated
  pass it works: `test-00003` weighted DER 11.02 → 9.95 %. Do not gate with the
  ASR threshold, and do not add energy heuristics on top.
- **Overlap tie-break**: preferring higher confidence, then a closer turn start,
  then the speaker id changed nothing useful (`test-00002` DER 11.08 → 11.11 %,
  JER 10.93 → 10.95 %) and was reverted. The real fix is speaker-attributed
  ASR, not a merge heuristic.
- **Speaker-turn ranges are clamped to diarization turns** (+300 ms boundary
  tolerance) while the text stays complete. A word outside the range is still
  in `query words` and in the dialogue; only the turn's time is clamped. This
  clamping is what turns the VAD gate's trims into measured DER.
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

Model store `~/.cache/sonolytica/models` (~1.7 GB): silero_vad.onnx,
ggml-tiny.en/base.en/small, wav2vec2-base-960h.onnx, segmentation-3.0.onnx,
sortformer-4spk-v2.onnx, wespeaker CAM++, pp_ocrv5_det/rec.onnx. `models list`
shows status.

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
cargo test --workspace --all-features   # 136

# real models (macOS fixtures, ~10 s)
SONOLYTICA_E2E=1 cargo test -p sonolytica-engine --features "full,metal" --test e2e_speech
SONOLYTICA_E2E=1 cargo test -p sonolytica-engine --features "full,metal" --test e2e_alignment

# evals (datasets downloaded by hand, see scripts/eval/README.md)
python3 scripts/eval/wer_librispeech.py --root /tmp/sono-eval/LibriSpeech/test-clean --count 100
python3 scripts/eval/der_voxconverse.py --parquet /tmp/sono-eval/voxconverse/dev-00000-of-00005.parquet --count 5

# diarization on the fresh shard used by the Sortformer report
curl -L -o /tmp/sono-eval/voxconverse-test2.parquet \
  https://huggingface.co/datasets/diarizers-community/voxconverse/resolve/main/data/test-00002-of-00011.parquet
python3 scripts/eval/der_voxconverse.py \
  --parquet /tmp/sono-eval/voxconverse-test2.parquet \
  --count 0 --min-speakers 2 --max-speakers 4 \
  --workdir /tmp/sono-eval/der-test2 --json-out /tmp/sono-eval/der-test2.json

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

1. **Speaker count + overlap — first pass done.** Sortformer 4spk v2 ONNX is
   the default; on a fresh VoxConverse shard it gets 5/6 counts exact (MAE 0.33
   vs 5.0), 6.40 % vs 8.99 % DER on files ≤7 min, and splits overlap into two
   simultaneous turns. One 20-minute 4-speaker file still collapses to two
   voices; noisy single-speaker files show false alarm. The 0.70 VAD gate now
   cuts that (fresh `test-00003`: weighted DER 9.95 %, 6/6 counts exact); next
   is the model's native streaming cache for long files and an AMI subset.
   pyannote remains the fallback.
2. **Overlap text attribution — JER shipped, tie-break rejected.** `sonolytica-eval
   der` now prints JER (test-00003 **9.85 % weighted / 7.34 % unweighted**) and
   `scripts/eval/der_voxconverse.py` aggregates it. A merge tie-break changed
   nothing useful (DER 11.08 → 11.11 %) and was reverted. Overlapped words are
   still one merged ASR line; splitting them needs speaker-attributed ASR
   (multitalker), a separate cycle. A missing overlapped line is an ASR
   limitation, not a diarization one.
3. **`query brief` — done (2026-11-12).** One compact read (media, runs,
   speech, speakers, chapters, video/OCR, music/acoustic) shipped with a
   "minimum commands per question" table and a context budget in the skill.
   Measured: hard fixture **16/16 at 22 410 fresh** in 15 commands (≤25k gate),
   easy **8/8 at 9 521 fresh** versus 41 399. Do not regress those numbers; the
   easy win is the one to quote.
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
- **Context preloading is a real cost.** The first `query brief` hard session
  opened the cheatsheet and two workflow files and made 30 commands: 26 998
  fresh tokens. Reading `SKILL.md` once and opening one reference only when
  needed cut it to 22 410 in 15 commands, score unchanged. The skill now states
  that budget.
- **A deterministic tie-break is not automatically an improvement.** Preferring
  confidence and turn start made the merge order-independent but raised
  `test-00002` DER 11.08 → 11.11 % and JER 10.93 → 10.95 %; reverted and
  recorded. Keep the metric even when the experiment fails.
- **Synthetic fixtures exposed real bugs** three times: colour-blind cuts
  (red/blue), dark-slide collapse (chromaticity), ASR padding vs onset. Keep
  making fixtures uncomfortable.

## 9. For the next agent

Start with `git status`, `docs/HANDOFF.md` (this file), `docs/STATUS.md`, then
pick item 2 (overlap text attribution) or the streaming cache in item 1. Do not
re-litigate the rejected count experiments; do not add GPL; keep the model-free
build. If a change is not measured on a licensed or reproducible fixture, it is
not done.
