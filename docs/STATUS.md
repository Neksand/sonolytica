# Project status — 2026-11-11

A snapshot for deciding what SONOLYTICA is actually good for. Every number
below comes from a run on this machine and a file in this repository; links go
to the method.

## What it is

One media file becomes one typed, timestamped JSON document: speech with word
timings and speaker turns, music and acoustic aggregates, video shots,
keyframes and OCR text, all on one timeline with provenance and a validator.
Agents read it through `sonolytica query` and cite it. No cloud, no Python at
runtime, no GPL.

## Size

| | |
|---|---|
| Rust | 15 045 lines across 7 crates (`providers` 4 330, `cli` 3 089, `engine` 2 315, `core` 2 080, `dsp` 1 360, `media` 1 226, `eval` 645) |
| Query commands | 13 (`timeline`, `at`, `search`, `transcript`, `words`, `speakers`, `chapters`, `frames`, `music`, `acoustic`, `runs`, `summary`, `help`) |
| Models | 10, all SHA-256 pinned: Silero VAD, whisper tiny/base/small, wav2vec2 aligner, Sortformer 4spk, pyannote segmentation, WeSpeaker, PP-OCRv5 det/rec |
| Schema | 1.4.0 (`Document::validate`, additive 1.0 → 1.4) |
| Agent skill | `SKILL.md` (290 lines) + 10 reference files + install guide |
| Docs | 15 files incl. 5 ADRs; `EVAL.md`, `BENCHMARK.md`, `DOGFOOD.md`, `OCR.md` |

## Engineering health

| Check | Result |
|---|---|
| `cargo test --workspace` | 121 passed, 21 suites, 0 failed |
| with `full,metal` | 136 passed, 0 failed |
| `cargo clippy -D warnings` | clean (default and full) |
| `cargo fmt --check` | clean |
| Model-free build | tests pass with no models, no cmake, no ONNX runtime |
| Docs-as-tests | every command in the skill is checked against the binary |

## Measured quality

| Area | Metric | Value | Conditions |
|---|---|---|---|
| ASR | WER | **6.05 %** | LibriSpeech test-clean, 100 utterances, 2048 words, `whisper-base.en` |
| ASR | CER | 2.25 % | same run; 23 deletions (1.1 %), 0 empty hypotheses |
| Diarization | DER | **11.81 %** dev / **17.46 %** test (AHC pilots); **11.43 %** weighted / **8.19 %** unweighted with Sortformer on a fresh shard | VoxConverse, collar 0.25 s |
| Diarization | speaker count | AHC MAE **5.0** → Sortformer **0.33** (5/6 exact) on the fresh 2-4 speaker subset | 6 files, 2140 s |
| Diarization | failure mode | one 20-min 4-speaker file collapses to 2 voices; noisy single-speaker files show false alarm | Sortformer caps at 4 |
| OCR | lines | 2/2, 6/6, 7/7 CJK | synthetic slides and a terminal screenshot, PP-OCRv5, 0.987 confidence on the benchmark slide |
| Video | cut accuracy | ±0.5 s | 3-shot synthetic video, 2 fps signatures; colour-cut bug found and fixed |
| Alignment | word drift vs Whisper | **median 110 ms** | wav2vec2 CTC, English, two-voice fixture; 88 % within 600 ms |
| Music | tempo / key / sections | 120.3 vs 120 BPM; C major correct; A–B–A boundaries within 160 ms | synthetic clicks, triads and a 30 s C–G–C track |
| Throughput | realtime factor | **14.1×** with Sortformer / **21.64×** AHC | 155.6 s and 121.7 s full pipeline, 11.0 s and 8.28 s wall |
| Russian | transcript | exact | 12 s, `whisper-small --language ru`, 0 word errors on a clean fixture |
| Agent | answers | **16/20 vs 20/20** easy; **7/16 vs 16/16** hard | no skill vs skill; the hard fixture is a 4 min, 4-voice meeting with no ASR/OCR tools for the baseline |

WER and DER are **pilots**, not leaderboard results: small subsets, one machine,
and the VoxConverse threshold was calibrated on the same dev files. The numbers
exist so changes can be judged by deltas, not to claim state of the art.

## What agents gain

`docs/BENCHMARK.md` compares the same model with and without the skill. On the hard 4-minute meeting the multimodal baseline scored 7/16 and the text-only baseline 4/16; both models with the skill scored 16/16, and the multimodal skill run used fewer fresh tokens than its no-skill baseline (41 139 vs 91 952).

- video fixture: 5/8 without, 8/8 with; the skill used **43 % fewer fresh
  tokens** (35 013 vs 61 280) because it queried instead of experimenting;
- Russian fixture: 11/12 without, 12/12 with; here the skill cost **more**
  (34 558 vs 23 131 fresh tokens) because a 12 s voice memo does not need a
  five-stage document;
- both: zero invented facts, citations with exact ranges, explicit refusals.

The pattern: the skill pays for itself when the media needs diarization, video
frames or OCR, and is overhead for a short clean recording. It makes the agent
**honest and citable**, not more perceptive.

## Where it stands against the usual alternatives

| | Native multimodal APIs | TwelveLabs / Gemini video | SONOLYTICA |
|---|---|---|---|
| Open-ended visual semantics | strong | strong (Pegasus / 1 fps) | **none** (no object/face model) |
| Exact word quotes + timestamps | weak / unverifiable | summary-level | **word-level, from the document** |
| Speaker turns and overlap flag | inconsistent | yes (paid) | yes, overlap split into simultaneous turns (Sortformer); pyannote fallback only flags |
| Provenance and "absence is null" | no | no | **enforced by schema + validator** |
| Local / private | no | no | **yes** |
| Cost | per token | $1.75/hour of video | free after models |
| Video navigation | 1 fps | agentic, $-metered | 2 fps signatures + keyframes, **10–50× fewer images** |
| Music depth | no | no | sections + tempo/key/chords; no notes/stems |

## Risks

1. **Speaker count is fixed for 2-4 speakers, not for long files.** Sortformer
   gets five of six counts right on a fresh shard (MAE 0.33 vs 5.0) and splits
   overlap; a 20-minute four-speaker file still collapses to two voices, and
   noisy single-speaker audio produces false alarms. A VAD gate did not help —
   Silero classifies the same noise as speech (see EVAL). The next attempt is
   the model's native streaming cache.
2. **One-fixture benchmark.** Ten questions, synthetic media. Real interviews,
   meetings, music and long videos are untested.
3. **Music is shallow**: tempo, key, triads, sections and loudness. No notes,
   bass or instruments.
4. **Alignment is English-only**: word timings are aligned with wav2vec2 CTC
   for English; other languages keep Whisper token timestamps.
5. **No objects, faces or vision model**: keyframes and OCR are the whole
   visual layer.
6. **Video keyframes are one per scene**: text or action inside a long static
   shot is missed.

## Verdict

- **Speech-heavy structured media (interviews, lectures, meetings with up to
  four speakers): strong.** Exact quotes, speaker turns, chapters, keyframes,
  OCR, citations, honest gaps — with a known caveat on 5+ speakers and very
  long meetings.
- **Slides / screencasts: strong.** Cuts, keyframes and exact on-screen text.
- **General video understanding: partial.** Navigation and text, no semantics.
- **Music: measured but shallow.** Tempo, key and technical sections are
  correct on synthetic signals with known answers; no notes, bass or instruments.
- **As an agent interface: the strongest part.** The protocol, schema and
  benchmark are the differentiator; perception is deliberately local and
  replaceable.

Next, in order of payoff: overlap text attribution, streaming cache for long
files, music notes/stems, non-English alignment, object/face models, MCP.
