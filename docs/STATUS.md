# Project status — 2026-11-12

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
| Rust | 17 161 lines across 7 crates (`providers` 4 347, `cli` 4 826, `engine` 2 631, `core` 2 080, `dsp` 1 360, `media` 1 226, `eval` 691) |
| Query commands | 15 (`brief`, `timeline`, `at`, `search`, `transcript`, `words`, `speakers`, `chapters`, `frames`, `music`, `acoustic`, `runs`, `summary`, `verify`, `help`) |
| CLI | `analyze`, `query`, `report` (HTML), `install-skill` (4 harnesses), `models fetch --set core\|full`, `doctor`, `inspect`, `schema` |
| Models | 10, all SHA-256 pinned: Silero VAD, whisper tiny/base/small, wav2vec2 aligner, Sortformer 4spk, pyannote segmentation, WeSpeaker, PP-OCRv5 det/rec |
| Schema | 1.4.0 (`Document::validate`, additive 1.0 → 1.4) |
| Agent skill | `SKILL.md` (200 lines) + 5 references + 5 workflows + install guide |
| Docs | 15 files incl. 5 ADRs; `EVAL.md`, `BENCHMARK.md`, `DOGFOOD.md`, `OCR.md` |

## Engineering health

| Check | Result |
|---|---|
| `cargo test --workspace` | 143 passed, 0 failed |
| with `full,metal` | 158 passed, 0 failed |
| `cargo clippy -D warnings` | clean (default and full) |
| `cargo fmt --check` | clean |
| Model-free build | tests pass with no models, no cmake, no ONNX runtime |
| Docs-as-tests | every command in the skill is checked against the binary |

## Measured quality

| Area | Metric | Value | Conditions |
|---|---|---|---|
| ASR | WER | **6.05 %** | LibriSpeech test-clean, 100 utterances, 2048 words, `whisper-base.en` |
| ASR | CER | 2.25 % | same run; 23 deletions (1.1 %), 0 empty hypotheses |
| Diarization | DER | **11.81 %** dev / **17.46 %** test (AHC pilots); Sortformer **11.08 %** weighted / **7.76 %** unweighted on `test-00002`, **9.95 %** / **7.49 %** on the fresh `test-00003` | VoxConverse, collar 0.25 s, 0.70 VAD gate |
| Diarization | JER | **10.93 %** weighted / **7.67 %** unweighted on `test-00002`; **9.85 %** / **7.34 %** on `test-00003` | frame Jaccard, same collar and mapping as DER |
| Diarization | speaker count | AHC MAE **5.0** → Sortformer **0.33** (5/6 exact) on the fresh 2-4 speaker subset; **0.00** (6/6) on `test-00003` | 6 files each, 2140 s / 2401 s |
| Diarization | false alarm | **4.27 → 2.47 %** (`gwloo`) and **13.41 → 6.53 %** (`uicid`); **15.61 → 5.71 %** (`xtdcl`) on fresh data | 1-spk and 2-spk real recordings |
| Diarization | failure mode | one 20-min 4-speaker file collapses to 2 voices (warning `diarization_speaker_cap` above 10 min); speaker-turn ranges are clamped to VAD-gated turns (raw `query words` stay complete) | Sortformer caps at 4 |
| OCR | lines | 2/2, 6/6, 7/7 CJK | synthetic slides and a terminal screenshot, PP-OCRv5, 0.987 confidence on the benchmark slide |
| Video | cut accuracy | ±0.5 s | 3-shot synthetic video, 2 fps signatures; colour-cut bug found and fixed |
| Alignment | word drift vs Whisper | **median 110 ms** | wav2vec2 CTC, English, two-voice fixture; 88 % within 600 ms |
| Music | tempo / key / sections | 120.3 vs 120 BPM; C major correct; A–B–A boundaries within 160 ms | synthetic clicks, triads and a 30 s C–G–C track |
| Throughput | realtime factor | **16.9×** on the 240 s hard fixture (17.9× without the VAD gate pass); 14.1× was the 155.6 s pipeline before the gate | full audio pipeline with Sortformer |
| Russian | transcript | exact | 12 s, `whisper-small --language ru`, 0 word errors on a clean fixture |
| Agent | answers | **16/20 vs 20/20** easy; **7/16 vs 16/16** hard | no skill vs skill; the hard fixture is a 4 min, 4-voice meeting with no ASR/OCR tools for the baseline |
| Agent, `query brief` v2 | fresh tokens | easy **7 517** at 8/8; hard **10 393** at 16/16; long **11 308** at 16/16 | DeepSeek V4.1 Flash, clean dirs, fixed prompts; see BENCHMARK |
| Agent, 20 min real meeting | score / cost | old skill **16/16 / 17 633**, new **16/16 / 11 308** | VoxConverse `fvhrk`, 4 speakers (document reports 2 + cap warning) |

WER and DER are **pilots**, not leaderboard results: small subsets, one machine,
and the VoxConverse threshold was calibrated on the same dev files. The numbers
exist so changes can be judged by deltas, not to claim state of the art.

## What agents gain

`docs/BENCHMARK.md` compares the same model with and without the skill. On the hard 4-minute meeting the multimodal baseline scored 7/16 and the text-only baseline 4/16; both models with the skill scored 16/16, and the multimodal skill run used fewer fresh tokens than its no-skill baseline (41 139 vs 91 952).

- video fixture: 5/8 without, 8/8 with; the skill used **43 % fewer fresh
  tokens** (35 013 vs 61 280) because it queried instead of experimenting;
- with `query brief` v2 and the overlap map (2026-11-12) the easy fixture fell
  to **7 517 fresh** (9 521 before), the hard fixture to **10 393** (22 410) at
  16/16, and the 20-minute meeting to **11 308** (17 633) at 16/16;
- that is cheaper **and** better: −21 % easy, −54 % hard, −36 % long, with one
  extra point on the long fixture after overlaps entered `brief`;
- Russian fixture: 11/12 without, 12/12 with; here the skill cost **more**
  (34 558 vs 23 131 fresh tokens) because a 12 s voice memo does not need a
  five-stage document;
- both: zero invented facts, citations with exact ranges, explicit refusals.

The pattern: the skill pays for itself when the media needs diarization, video
frames or OCR. `query brief` removes most of the fixed reading cost, but a very
short clean recording is still cheaper with a bare `whisper` call. The skill
makes the agent **honest and citable**, not more perceptive.

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
   gets six of six counts right on freshly seen `test-00003`, and the 0.70 VAD
   gate cuts false alarm on noisy recordings (weighted DER 11.02 → 9.95 %).
   A 20-minute four-speaker file still collapses to two voices, but the document
   now carries a `diarization_speaker_cap` warning above 10 minutes. The next
   attempt is the model's native streaming cache.
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
  long meetings; speaker-turn ranges are clipped to the diarized turn, while
  raw words remain complete.
- **Slides / screencasts: strong.** Cuts, keyframes and exact on-screen text.
- **General video understanding: partial.** Navigation and text, no semantics.
- **Music: measured but shallow.** Tempo, key and technical sections are
  correct on synthetic signals with known answers; no notes, bass or instruments.
- **As an agent interface: the strongest part.** The protocol, schema and
  benchmark are the differentiator; perception is deliberately local and
  replaceable.

Next, in order of payoff: an overlap-aware query (`brief` maps cuts and OCR but
not overlapping turns, which cost the new core a point on the long fixture);
AOSC streaming for long files (license check first), multitalker ASR for overlap,
`--effort` modes, music notes/stems, caching, MCP, non-English alignment,
objects/faces.
