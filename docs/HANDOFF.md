# Handoff — SONOLYTICA after the AOSC streaming cycle

Last updated: 2026-11-12. Branch `main`, working tree clean, **no remote
configured** (the user pushes). Since `1960e26`: reproducible benchmark + token
scripts, `brief` v2 (scene cuts, OCR lines, overlap spans), `query verify`,
HTML `report`, `install-skill` for four harnesses, release workflow, dense
frames, YuNet faces, YOLOS objects, a 200-line skill core, AOSC streaming
diarization for long files (4/4 speakers on `fvhrk`, DER 6.40 %), an analysis
cache, `query ask` evidence packs, MoveNet body pose, and Florence-2 keyframe
captions behind a multi-file model bundle.

Read this first, then `docs/STATUS.md` (numbers), `docs/EVAL.md` (methods),
`docs/BENCHMARK.md` (agent and head-to-head tests), `docs/MODELS.md` (licenses,
caps), `docs/OCR.md`, `docs/SCHEMA.md` (1.6.0), `docs/MCP.md`. Forward plans live outside the
repo: `/Users/neksand/.opencode/plan/sonolytica-skill-cost-2026-11-12.md`
(closed), `sonolytica-ideal-night-2026-11-12.md` (night work), and
`sonolytica-beat-all-2026-11-12.md` (category + launch).

---

## 1. What this is

One media file becomes one typed, timestamped JSON document: speech with word
timings and speaker turns, music/acoustic aggregates, video shots, keyframes,
OCR text, and provenance on every segment. Agents read it via
`sonolytica query` and cite it. Local only, no Python at runtime, no GPL.

| | |
|---|---|
| Rust | 22 462 lines, 7 crates: `providers` (6 167), `cli` (8 213), `engine` (2 638), `core` (2 167), `dsp` (1 360), `media` (1 226), `eval` (691) |
| Binary | `sonolytica` (`sonolytica-cli`) |
| SDK | `sonolytica-engine` (`Engine::analyze`) |
| Schema | **1.6.0**, additive 1.0 → 1.6 |
| Models | 14, SHA-256 pinned, in `~/.cache/sonolytica/models` |
| Commands | 17 query subcommands (brief … verify, ask, captions, poses) + `help` + `report`, `install-skill`, `analyze`, `models`, `cache`, `setup`, `mcp`, `completions`, `doctor`, `inspect`, `schema` |
| Skill | `skills/sonolytica/` (SKILL.md 200 lines + 5 references + 5 workflows) |
| Tests | **173** model-free / **188** all-features |

Features: `sonolytica-cli --features full-metal` = whisper + silero + diarize +
ocr + vision + align + metal. `cargo test --workspace` runs without
models/cmake/ORT.

## 2. Capabilities and verified numbers

| Area | Result | Where |
|---|---|---|
| ASR | WER **6.05 %**, CER 2.25 % — LibriSpeech test-clean, 100 utts, `whisper-base.en` | EVAL |
| Diarization | AHC pilots: DER **11.81 % dev / 17.46 % test**; Sortformer 4spk with the 0.70 VAD gate: **11.08 % weighted / 7.76 % unweighted** on `test-00002`, **9.95 % / 7.49 %** on fresh `test-00003`; JER **9.85 % / 7.34 %** there; counts **6/6 exact**. AOSC streaming v2.1 on the 20-minute `fvhrk`: **4/4 speakers, DER 6.40 %** (was 2/4, 15.99 %); no regression on 1-4 speaker files | EVAL |
| Alignment | wav2vec2 CTC, 20 ms frames, English; **median 110 ms** vs Whisper, 88 % ≤600 ms; Whisper itself is 300–950 ms early on JFK | EVAL |
| OCR | PP-OCRv5 det+rec; 12/12 lines exact on the 6-slide fixture; 7/7 CJK terminal | OCR |
| Music | 120.3 BPM for a 120 BPM click; C major triad correct; A–B–A section boundaries within 80/160 ms | EVAL |
| Video | 2 fps chromaticity signatures, cuts ±0.5 s; 6/6 scenes, 6 keyframes on the hard fixture; `--frame-stride N` adds dense frames (scene first, `--max-frames` caps) | EVAL/BENCHMARK |
| Faces | YuNet on keyframes: 1/1 on a portrait (0.946), 3/3 on a synthetic 3-face frame, 0 false faces on 6 slide keyframes | local test |
| Objects | YOLOS-tiny on keyframes: `person` 0.999 + `tie` 0.991 on a portrait, 0 false objects on 6 slide keyframes | local test |
| Pose | MoveNet Multipose on a portrait: 1 person, 11 COCO keypoints above 0.3; on a face-only frame it invents 3 person-shaped boxes (0.41–0.62), so it is an observation, not a body-presence oracle | local test |
| Cost | cache hit **0.01 s** vs 7.64 s on the 14.4 s fixture; `query ask` 978 B text vs 3 912 B for three equivalent queries | local test |
| Captions | Florence-2-base-ft (MIT, 550 MB): portrait caption answered 6/8 visual-boundary questions, 0 unsupported; 16/16 caption-aware; slide captions can invent colours | local test |
| Agent interface | `sonolytica mcp`: tools-only stdio JSON-RPC, 14 tools, same binary; `setup` fetches core models + installs the skill; completions for bash/zsh/fish | local test |
| Distribution | release tarballs + checksummed `install.sh`; Dockerfile and GHCR workflow; `setup` one command | local build |


| Free agents | easy 8/8 / 18 224, hard 16/16 / 19 396, long 16/16* / 31 570, visual 16/16 refusal / 17 652 | clean dirs, `$0`, 2026-11-13; * Q4 corrected, old overlap spans were merge artifacts |

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

`query brief` v2 + overlap map + command budget (2026-11-12, reproducible
prompts). Medians across runs: hard **16/16 / 14 773 fresh** (range 13 517–27 235,
n=3), easy **8/8 / 8 818** (7 517–21 021, n=5), long 20-minute meeting
**16/16 / 13 443** (11 308–15 578, n=2). The range is real: every command
re-sends context. Adding "no `--help`/`doctor` once `brief` shows the stages"
took easy from 21 021 to **9 150** on the next run, same 8/8.

Head-to-head (same prompts): Gemini via Antigravity **14/16 alone, 16/16 with
the skill, 6.6× faster**; TwelveLabs Pegasus 1.5 **10/16 hard, 3/8 easy,
10/16 long**, and it got the 20-minute speaker count right (4) where the old
Sortformer document reported 2 — the case AOSC streaming later fixed. A blind
judge (Codex, anonymised) kept the ranking:
Gemini 10, skill 16, TwelveLabs 7, DeepSeek+skill 15, with **19 valid citations
for skill runs vs 7 and 3**. Raw outputs: `docs/bench/2026-11-12/`.

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
- A Sortformer hypothesis that fills all four slots carries
  `diarization_speaker_cap`; an agent must say "at least N" there, never an
  exact count. Long files no longer trigger it by duration alone.
- Skill cost/quality baseline (medians): hard **16/16 / 14 773 fresh** (13 517–
  27 235, n=3), easy **8/8 / 8 818** (7 517–21 021, n=5), long **14/16* / 13 443**
  (11 308–15 578, n=2). * The old Q4 overlap spans were merge artifacts; the
  AOSC document reports none and its free run is 16/16. Cost is dominated by agent behaviour: one `--help` or
  `doctor` run re-sends thousands of tokens. Quote the median and the range, not
  a single run. Re-run the paired benchmark before claiming an improvement; do
  not preload reference files.
- **Faces are detection-only**: `--faces` stores class `face`, a box and a
  score. No embeddings, names, identity or tracking. Do not add recognition
  without an explicit product decision (privacy) and a license that allows it.
- **Objects are YOLOS-tiny**: 91 COCO classes, **softmax over 92 ids with id 91
  as background**, threshold 0.5. Sigmoid decoding returns background-only
  detections; do not "fix" it back.
- **Vision models look at sampled keyframes only.** "Not detected" is not "not
  present"; `--frame-stride N` is the tool for long static shots.
- **Captions are model-generated observations, not sight.** `--caption` is
  opt-in, one per keyframe, can invent a colour or a detail; never treat it as
  OCR, as speech or as ground truth. Quote it as "caption at <time>".
- **Multi-file models are pinned file by file plus a composite manifest
  digest.** Never pin only the decoder and fetch a different tokenizer later;
  `ModelStore::require` returns the verified bundle directory.
- **MCP mirrors the CLI, it does not reimplement it.** `sonolytica mcp` runs
  the same binary for every tool call. A second query implementation would
  drift; keep the server as a transport adapter.
- **`setup` is idempotent.** It verifies and skips existing models, installs
  the skill only when asked, and never deletes media or documents.
- **Pose is detection-only and keyframe-bound.** 17 COCO keypoints per person,
  no tracking, no identity. A tight face crop can produce a plausible body
  below it; treat pose as an observation at one instant, never as proof that a
  person was present or absent between samples.
- **The cache is a proven record, not a new run.** A hit copies the document
  and its artifacts, rewrites the relative paths and `media.path`, and adds
  `analysis_cached`; timings and `created_unix_ms` stay from the first run.
  Never present it as a fresh analysis. Degraded or partial annotations are
  not cached.
- **`query ask` is retrieval, not an answer.** It ranks existing segments,
  names absent terms and returns ranges; it never synthesises prose. The agent
  must still quote from the returned evidence and verify with `query verify`.
- **Captions are model-generated observations.** Florence-2 text is not OCR,
  not speech and not ground truth; it is bound to one keyframe, can invent a
  colour or a detail, and must be cited as a caption at that instant. OCR stays
  the authority for exact on-screen text. `--caption` is opt-in.
- **A multi-file model is a bundle.** The registry pins every file separately
  and a composite manifest digest; `ModelStore::require` returns the directory.
  Never pin only the decoder and silently fetch a different tokenizer.
- **Dense frames are opt-in**: scene keyframes first, stride fills the rest,
  `--max-frames` still caps. `brief` collapses consecutive identical OCR lines
  so a dense run does not turn the map into a contact sheet.
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
- **Diarization (default)**: Sortformer 4spk v2.1 streaming ONNX, 128-bin
  log-mel with no normalisation, 80 ms activity, native AOSC state: 340-frame
  chunks (1 left, 40 right), 40-frame FIFO, 188-frame speaker cache compressed
  by log-activity scores with per-speaker boosts and 12 silence slots. Overlap
  becomes two simultaneous turns; labels are stable for the whole file, no
  per-window re-matching. It caps at 4 speakers; crowded audio collapses into
  four voices. 475 MB on disk, **2.07 GB peak RSS and 45 s for a 20-minute
  file** (26.0x realtime with whisper-base.en). The silence-pad decode must use
  `n + 3` as the modulus; using `n` silently disables most of the cache.
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
- **`query brief` is a map, not evidence**: 1.7–3.1 KB, up to 5 chapters and 8
  speakers with true totals. Exact quotes and ranges come from `search`,
  `words` or `transcript`. Do not grow it with prose; grow it with small
  machine-readable facts (scene cuts, capped OCR lines) only if a benchmark
  shows fewer commands and no score loss.
- **Reference preloading is the main fixed skill cost**: reading the cheatsheet
  and two workflows cost 4 588 fresh tokens and bought nothing. The skill says
  read `SKILL.md` once, open one reference only when needed.
- **Keyframes**: one per scene by default; `--frame-stride N` adds dense
  samples, scene frames first and `--max-frames` caps the total. OCR, faces and
  objects run on those keyframes and add **point ranges** (the frame instant,
  not the shot). Artifacts use paths relative to the document; `query frames`
  resolves them.
- **YuNet preprocessing is exact**: fixed 640×640 input, **BGR, 0..255 floats**,
  no mean subtraction, score `sqrt(cls*obj)`, NMS 0.3. Feeding 0..1 yields zero
  detections with no error — that cost one debug cycle.
- **Florence-2 is four graphs, no KV cache.** 768×768 ImageNet-normalised
  CHW → vision `image_features [1,577,768]`; prompt encoded **without** special
  tokens; encoder input is image features followed by text embeddings; decoder
  starts at `</s>` (2) and re-feeds the whole prefix each step; EOS is 2.
  `decoder_model_merged` dead-ends and `decoder_with_past` is static-16-token;
  use `decoder_model_int8`. The tokenizer has no `<CAPTION>` token — encode the
  English rewrite.
- **MoveNet output is keypoints-first, not box-first**: each of the 6 rows is
  `17 × (y, x, score)` followed by `[ymin, xmin, ymax, xmax, person_score]`.
  The input is `int32` RGB, 256×256 letterboxed; a `uint8` feed is rejected.
  The output is split as keypoints first only because the ONNX export reordered
  the TF Hub tensor; read the raw row before trusting a port.
- **Cache artifact paths are relative to the document name.** Store under a
  canonical `document.json.artifacts/` prefix and rewrite on load; a literal
  `String::strip_prefix` keeps the leading `/`, so use a prefix with the
  separator or `Path::strip_prefix`. The cache unit test pins the rewrite.
- **YOLOS decoding is not DETR-sigmoid**: the export uses **softmax over 92
  ids**, id 91 is background, boxes are normalised `(cx, cy, w, h)`, shortest
  edge 512. The first implementation used sigmoid and saw only background at
  0.92 confidence; verify a new detector's distribution in Python before
  trusting the postprocess.
- **Report paths are hostile by default**: keyframes are copied into
  `<report>.artifacts/frames/` with an index prefix, absolute paths and `..` are
  refused. A document is untrusted input.
- **The skill is embedded in the binary**: `install-skill` writes it to Claude
  Code, Codex, OpenCode and Cursor; `--dry-run`/`--uninstall` must never delete
  anything in dry-run mode.
- **Shot detection**: 2 fps, 64×36 RGB, **chromaticity** histogram (pixel
  normalised by its brightest channel). Raw RGB collapses all dark scenes into
  bin 0; that bug cost a whole 4-minute fixture.
- **Music sections** are technical boundaries from chroma self-similarity, no
  verse/chorus labels; novelty is normalised 0–1.
- **Schema**: only additive changes, minor bumps, old documents stay readable.
  `1.1.0` artifacts, `1.2.0` text_overlay/text track, `1.3.0` align,
  `1.4.0` music sections, `1.5.0` pose, `1.6.0` captions.

## 5. Environment (this machine)

macOS 26.6.2, M5 Pro, 24 GB, Rust 1.99, FFmpeg 9.0.2. **FFmpeg has no
`drawtext`** (no libfreetype), so the OCR e2e skips here; the CLI path was
verified on PIL-rendered slides and real keyframes. `whisper` CLI and
`tesseract` **are installed** on PATH — the first benchmarks' baselines used
them; the hard benchmark forbids them by instruction.

Model store `~/.cache/sonolytica/models` (~2.6 GB, 14 models): silero_vad.onnx,
ggml-tiny.en/base.en/small, wav2vec2-base-960h.onnx, segmentation-3.0.onnx,
sortformer-4spk-v2.1.onnx, wespeaker CAM++, pp_ocrv5_det/rec.onnx,
yunet_face_detection_2023mar.onnx, yolos_tiny.onnx. `models list` shows status.

Fixtures under `/private/tmp` (not committed; regenerate):
`sono-eval` (LibriSpeech, VoxConverse parquet + `der-test2-*`, `der-test3-*`
workdirs and JSON scores), `bench-hard` (meeting.mp4 + ref.json + artifacts),
`sono-bench` (14 s video), `sono-query-e2e`, `sono-stats` (abc/aba wavs).
Benchmark scratch dirs: `bench-brief`, `bench-easy`, `bench-agent-*`,
`bench-gemini-*`; baseline binary: `sonolytica-baseline`; head-to-head raw
outputs are committed under `docs/bench/2026-11-12/`. They survive a session
clear but not a reboot.

## 6. How to verify and reproduce

```bash
cd /Users/neksand/orca/projects/SONOLYTICA
git status                       # must be clean
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace           # 173, no models
cargo test --workspace --all-features   # 188

# real models (macOS fixtures, ~10 s)
SONOLYTICA_E2E=1 cargo test -p sonolytica-engine --features "full,metal" --test e2e_speech
SONOLYTICA_E2E=1 cargo test -p sonolytica-engine --features "full,metal" --test e2e_alignment

# evals (datasets downloaded by hand, see scripts/eval/README.md)
python3 scripts/eval/wer_librispeech.py \
  --root /private/tmp/sono-eval/librispeech/LibriSpeech/test-clean --count 100
python3 scripts/eval/der_voxconverse.py \
  --parquet /private/tmp/sono-eval/voxconverse/dev-00000-of-00005.parquet --count 5

# fresh diarization shards already downloaded on this machine
python3 scripts/eval/der_voxconverse.py \
  --parquet /private/tmp/sono-eval/voxconverse-test2.parquet \
  --count 0 --min-speakers 2 --max-speakers 4 \
  --workdir /private/tmp/sono-eval/der-test2-final \
  --json-out /private/tmp/sono-eval/der-test2-final.json
# MCP and onboarding
printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' | ./target/debug/sonolytica mcp
./target/debug/sonolytica setup --no-models --no-skill
./target/debug/sonolytica completions bash | head
cargo test -p sonolytica-cli --test mcp
# cache, evidence pack, pose, captions
./target/debug/sonolytica analyze /private/tmp/face.mp4 --out /tmp/cap.json --max-frames 1 --caption
./target/debug/sonolytica query captions /tmp/cap.json --json
./target/debug/sonolytica cache
./target/debug/sonolytica query ask /private/tmp/bench-brief2/doc.json "budget" --json
./target/debug/sonolytica query poses /private/tmp/pose-test/face.json --json
# AOSC gate: the only 4-speaker file in that shard
python3 scripts/eval/der_voxconverse.py \
  --parquet /private/tmp/sono-eval/voxconverse-test2.parquet \
  --count 0 --min-speakers 4 --max-speakers 4 \
  --workdir /private/tmp/sono-eval/der-aosc-test2 \
  --json-out /private/tmp/sono-eval/der-aosc-test2.json
python3 scripts/eval/der_voxconverse.py \
  --parquet /private/tmp/sono-eval/voxconverse-test3.parquet \
  --count 0 --min-speakers 2 --max-speakers 4 \
  --workdir /private/tmp/sono-eval/der-test3-env \
  --json-out /private/tmp/sono-eval/der-test3-env.json

# vision smoke (models fetched): one face, three faces, zero false positives
./target/debug/sonolytica analyze /private/tmp/face.mp4 --out /tmp/face.json \
  --profile structure --faces --objects
./target/debug/sonolytica query timeline /tmp/face.json --type object

# HTML report and skill install
./target/debug/sonolytica report /private/tmp/bench-brief2/doc.json --out /tmp/report.html
./target/debug/sonolytica install-skill --dry-run --target opencode

# query brief smoke: compact JSON 2-5 KB, key sections present
cargo build -p sonolytica-cli --features full-metal
./target/debug/sonolytica query brief /private/tmp/bench-brief2/doc.json --json | jq -c . | wc -c
./target/debug/sonolytica query brief /private/tmp/bench-brief2/doc.json | head -30

# agent benchmark: clean fixture, exact prompt, tokens from OpenCode's store
scripts/bench/prepare_agent_fixture.sh --name hard \
  --media /private/tmp/bench-hard/meeting.mp4
scripts/bench/session_tokens.sh ses_…      # or --last
# long fixture (20 min, VoxConverse fvhrk): scripts/bench/README.md

# JER is printed by the eval binary; the script aggregates weighted/unweighted
./target/debug/sonolytica-eval der --rttm REF.rttm --document DOC.json --file-id NAME

# hard benchmark fixture
python3 scripts/bench/make_meeting_fixture.py --out /private/tmp/bench-hard
```

Agent benchmark protocol (see the plan for the next cycle): make a clean
directory with only the media, a copy of `skills/sonolytica/` and the full-metal
binary. Forbid `whisper`, `tesseract`, `ffmpeg`, Python and the repository.
Ask the questions in Russian, require ranges, start with `query brief`. Record
the model, date, score, every command and fresh tokens from `session_v2`.

Note: `cargo test` compiles the bin **without** features and can overwrite
`target/debug/sonolytica`. Rebuild with `--features full-metal` before using
the CLI; eval scripts have a preflight that refuses a DSP-only build.

Token accounting: OpenCode stores per-session totals in
`~/.local/share/opencode/opencode.db`, table `session_v2`
(`tokens_input`, `tokens_output`, `tokens_reasoning`, `tokens_cache_read`,
`cost`). "Fresh" = input + output.

## 7. Known gaps and next steps (priority)

### Skill cycle + vision (2026-11-12): delivered

Full plans: `/Users/neksand/.opencode/plan/sonolytica-skill-cost-2026-11-12.md`
(closed), `sonolytica-ideal-night-2026-11-12.md`. Medians over every run
(DeepSeek V4.1 Flash, clean directories, fixed prompts):

| Fixture | Before | Now (median) | Range | Gate |
|---|---|---|---|---|
| hard 4 min | 16/16, 22 410 | **16/16, 14 773** | 13 517–27 235 (n=3) | ≤18 000: met at the median |
| easy 14.4 s | 8/8, 9 521 | **8/8, 8 818** | 7 517–21 021 (n=5) | ≤8 000: missed |
| long 20 min | 14/16*, 17 633 | **14/16*, 13 443** | 11 308–15 578 (n=2) | met (cost); * Q4 reference corrected |

The spread is agent behaviour, not the document: every command re-sends the
context. The budget rule ("no `--help`/`doctor` once `brief` shows the stages")
took easy 21 021 → 9 150 on the next run with the same 8/8.

Delivered in this cycle:

- `brief` v2: scene cuts, one OCR line per keyframe, **overlap spans**; dense
  OCR lines collapse so the map stays 2–4 KB compact;
- `query verify`: mechanical quote + range check, exit 1 on a failed claim;
- `sonolytica report`: self-contained HTML + copied keyframes, faces/objects
  per frame;
- `sonolytica install-skill`: skill embedded in the binary, installed into
  Claude Code, Codex, OpenCode and Cursor;
- `models fetch --set core|full`; 13 pinned models;
- release workflow (macOS arm64/x86_64, Linux x86_64/arm64) + checksummed
  `install.sh`; product README, issue forms;
- **vision**: `--frame-stride` dense frames, `--faces` (YuNet, count/boxes only),
  `--objects` (YOLOS-tiny, 91 COCO classes), both opt-in and keyframe-bound;
- head-to-head vs Gemini (Antigravity) and TwelveLabs Pegasus, plus a blind
  judge; raw outputs in `docs/bench/2026-11-12/`.

Open, in priority order:

1. **Speaker-attributed ASR for real overlap.** The long fixture is **16/16**
   after correcting Q4: the RTTM window has only `spk02`, so the old overlap
   spans were merge artifacts. The capability gap is real on genuinely
   overlapping speech (AMI), not on `fvhrk`. Research: Parakeet TDT 0.6B v3
   multitalker (CC-BY-4.0, ~2.6 GB, `parakeet-rs` with the same `ort`),
   7–10 days; the deterministic per-turn baseline is 2–3 days but cannot split
   simultaneous words. The hard fixture proves the mask is the blocker: with
   a reference S4 mask Parakeet recovered the missing 130–131.4 line, but AOSC
   S4 activity there is ≤0.011. Add an overlap-aware diarizer (old offline v2,
   Nemotron-3, or separation) before integrating Parakeet; protocol in
   [`docs/bench/overlap.md`](bench/overlap.md).
2. **Free-form VQA tier** (SmolVLM2-500M, Apache-2.0): the Florence-2
   caption tier is in; the next gap is arbitrary questions ("is he holding a
   knife?") that a one-shot caption does not answer. Opt-in, RAM/license gate.
3. **Music depth**: basic-pitch notes and technical highlight windows.
4. **Distribution and launch**: external brew tap (needs a separate repo),
   a second Gemini run when the quota resets, launch assets; Docker and
   `setup` are done.
5. **Easy fixed cost**: median 8 818 vs 8 000; the remaining cost is the
   harness prompt, the skill read and the agent's own loop.
6. **MCP hardening**: streamable HTTP and the official `rmcp` SDK; the stdio
   tools server is done.

AOSC streaming shipped (2026-11-12): license reviewed, `fvhrk` 4/4 speakers at
DER 6.40 % / JER 6.29 % (gate ≥3 / ≤11.8 %), no regression on the 1-4 speaker
files. Cost tooling also shipped: analysis cache (0.01 s hit vs 7.64 s),
`query ask` evidence packs, and MoveNet pose (Apache-2.0, `--pose`). Full
numbers in `docs/EVAL.md` and `docs/BENCHMARK.md`.

### Capability roadmap after the skill cycle (by payoff)

Do these only after the skill-cost cycle, unless the user explicitly reprioritises.
Each needs its own measurement and one local commit; when a gate fails, revert
and record it in `docs/EVAL.md`. Delivered since this list: AOSC streaming,
the analysis cache, `query ask` evidence packs and MoveNet pose.

1. **AOSC streaming for long files — delivered 2026-11-12.** The 300 s windows
   and their 10-minute warning are gone; the v2.1 streaming export with the
   native speaker cache gets `fvhrk` 4/4 at DER 6.40 % (gate ≥3 / ≤11.8 %).
   License: NVIDIA Open Model License, attribution in `NOTICE`. No regression
   on the 1-4 speaker files (see `docs/EVAL.md`).
2. **Multitalker ASR for overlap.** Sortformer returns two simultaneous turns,
   but Whisper still emits one merged line; a missing overlapped line is an ASR
   limitation. Add a speaker-attributed pass and measure JER plus word
   attribution. Do not invent a word split the ASR never produced. Separate
   cycle; the rejected merge tie-break is not the solution.
3. **Modes `--effort low|normal|max` + `--vision agent|local`.** RAM table in
   the skill: low ≈40 MB (no ASR), normal +200–500 MB, max +400 MB (OCR/align),
   local VLM +2–4 GB opt-in. Store the chosen mode in `config.profile/effort`;
   auto 5/5 on the fixtures; quick ≤1 min on a 2-hour video. Big refactor — do
   not mix it with diarization.
4. **Music notes/stems.** basic-pitch (Apache-2.0, ~20 MB) for notes; Demucs
   (MIT) for stems/bass only if the ONNX export works and F1 ≥0.6. No claim
   before that number exists.
5. **Caching — delivered 2026-11-12.** Key = media SHA-256 + `AnalysisConfig`
   hash + annotation flags + schema/generator/cache version. A hit copies the
   document and artifacts, rewrites relative paths and `media.path`, and adds
   `analysis_cached`; 0.01 s vs 7.64 s on the 14.4 s fixture. Degraded or
   partial annotations are not cached. `sonolytica cache [--clear]` manages it.
   Still open: a cache-size budget and eviction policy.
6. **MCP — delivered 2026-11-13 (stdio, tools-only).** 14 tools; the server
   runs the same binary for every call, so there is no duplicate query logic.
   See `docs/MCP.md`. Remaining: official `rmcp`, streamable HTTP,
   resources/prompts. `setup` + completions also shipped for onboarding.
7. **Non-English alignment.** MMS/XLSR CTC with a FLEURS WER table
   (ru/es/de/fr/zh). Later; English stays wav2vec2 for now.

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
- **A truncated range is a wrong range.** The slim skill's only scoring loss on
  the 20-minute fixture was an overlap span cut four seconds short: content
  right, timestamp wrong. Score it 1, and fix the routing rather than the
  answer.
- **The benchmark harness must be code.** Hand-built directories and remembered
  prompts made the first `brief` numbers incomparable; one script that copies
  media + skill + binary and one verbatim prompt per fixture made repeat runs
  spread 4.6–9.3 %, inside the ±10 % gate.
- **Slimming prose moves routing knowledge, not just words.** Moving the
  "minimum commands" table to the cheatsheet saved lines; on the long fixture
  the agent then explored the raw JSON with `jq` and spent extra commands.
- **A detector's postprocess is a hypothesis until checked.** YuNet expects BGR
  0..255 (0..1 gives zero detections); YOLOS-tiny needs softmax with id 91 as
  background (sigmoid returned background at 0.92). Both were caught only by
  printing the raw distribution from Python ONNX Runtime. Do that before
  shipping any new model.
- **Fresh-token cost is agent behaviour, not document size.** The same skill and
  fixture produced 7 517 and 21 021; the difference was `--help`, `doctor` and
  two extra commands, each re-sending the whole context. A command budget is a
  product feature.
- **Vision is keyframe-bound.** Faces, objects and pose are sampled at
  keyframes; "not detected" is not "absent". Dense frames and the report are the
  honest way to widen coverage.
- **A body model on a face crop invents a body.** MoveNet returned three
  person-shaped boxes with 0.41–0.62 scores on a face-only synthetic frame. Pose
  is an observation at one instant, not a body-presence oracle; say what the
  keypoints are, not who was there.
- **`query ask` is retrieval.** Ranking existing segments and naming absent
  terms is cheap and citable; synthesising an answer in the CLI would be a
  second, unverified narrator. Keep it deterministic.
- **Cache keys must include post-analysis flags.** OCR/faces/objects/pose run
  after the engine, so `AnalysisConfig` alone is not enough; the annotation
  fingerprint and schema version are part of the key.
- **A cached document is a record, not a new run.** Keep its `created_unix_ms`
  and run timings, rewrite the media path, and mark it `analysis_cached`.
- **A caption is not sight.** Florence-2 described a portrait correctly
  (tie, arms, flags, smile) and called a red slide background black on the same
  run. Use captions for semantic navigation, OCR for exact text, and always say
  which layer made the claim.
- **AOSC fixed the count and exposed a bad reference.** The old `fvhrk`
  document had eight S1 turns made of words that also sit inside S2 sentences
  (`students`, `from`, …); the RTTM for that window has only `spk02`. The new
  streaming document has no overlap, so the long free run is **16/16** after
  correcting Q4, not a regression. Do not tune to the old artifact; a real
  overlap number needs an annotated AMI fixture and speaker-attributed ASR.

## 9. For the next agent

Start with `git status`, this file, `docs/STATUS.md`, then §7. The skill and
vision cycles are closed: **hard 16/16 / 14 773 median, easy 8/8 / 8 818, long
14/16* / 13 443**; * old Q4 overlap spans were merge artifacts; the AOSC free
run is 16/16. Gemini alone 14/16 vs 16/16 with the skill; TwelveLabs 10/16.
The AOSC streaming cycle is closed on the DER axis: `fvhrk` 4/4 speakers,
DER 6.40 % (was 2/4, 15.99 %), no regression on 1-4 speaker files. The free
agent matrix is easy 8/8, hard 16/16, visual 16/16 refusal, visual+caption
16/16, long 16/16 after correcting the overlap reference — the old Q4 spans
were merge artifacts, not a count loss.
Cost tooling is in: cache, `query ask`, and pose. The first caption tier is in
(Florence-2), and the first MCP server + one-command `setup` + completions are
in. The next objectives are **speaker-attributed ASR**, a **free-form VQA tier**
(SmolVLM2), streamable HTTP/rmcp, music notes, and distribution/launch.

Do not re-litigate the rejected experiments (VAD gate at 0.45, merge tie-break,
count heuristics), do not preload reference files, and quote medians + ranges
rather than a single run. Keep the model-free build, clippy, fmt and
docs-as-tests green. `cargo test` rebuilds the CLI **without** features and can
overwrite `target/debug/sonolytica` — rebuild with `--features full-metal`
before any model-backed run. If a change is not measured on a licensed or
reproducible fixture, it is not done.
