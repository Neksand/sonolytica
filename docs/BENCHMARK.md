# Benchmark: does the skill make an agent better at media?

Date: 2026-11-11; the `query brief` section was added 2026-11-12. Three
fixtures (a 14 s clip, Russian speech, a 4 min meeting),
18 tasks, eleven scored sessions across three models, and two contaminated
attempts that are recorded as failures rather than hidden.

The benchmark measures the **agent + skill + CLI** as a system, not a model on a
leaderboard. The question is whether an agent with the skill answers media
questions more correctly and more honestly than the same kind of agent left to
its own tools.

## Fixture

14.4 s, 1280×720, two shots and two synthetic voices.

| Time | Picture | Audio |
|---|---|---|
| 00:00–00:07 | dark red slide, `QUARTERLY REVENUE +20%` | S1: "The quick brown fox humps over the lazy dog, our preaching starts at ten dollars." (00:00.000–00:04.860) |
| 00:07–00:14.4 | dark blue slide, `NEXT STEP: HIRE TWO ENGINEERS` | S3: "I am the second speaker!" (00:05.656–00:07.656), then "We are checking that speaker the organization separates two voices." (00:07.992–00:14.400) |

Slides were rendered with PIL, assembled with FFmpeg (`concat`, `libx264`,
AAC); the audio is the two-voice `say` fixture used elsewhere. The hard cut at
7.000 s is deliberately between two *dark* scenes (red and blue): it exposed a
real bug in the shot detector, which used luma only and saw one scene. Fixed by
using an RGB histogram.

Ground truth, from the reference document:

| # | Question | Answer |
|---|---|---|
| 1 | text on the slide at 00:08 | `NEXT STEP: HIRE TWO ENGINEERS` |
| 2 | what is said between 00:07 and 00:09 | tail of "I am the second speaker!" (ends 07.656) and the head of "We are checking that speaker the organization separates two voices." (starts 07.992) |
| 3 | who spoke more | S3, 8.400 s over 2 turns; S1, 4.720 s over 1 turn |
| 4 | anything about a deadline | no |

## Sessions

Same style of prompt, same four questions, Russian answers. Sessions A/C ran on
`nemotron-3.5-super-vl-free`; B ran on `longcat-2.5-free`. Both are free models
in OpenCode; the quota ran out mid-benchmark, which is why the matrix is not
fully crossed.

| Session | Model | Skill | OCR | Commands | Result |
|---|---|---|---|---|---|
| **A** | Nemotron VL | no | no | 8 | used `ffmpeg` + a `whisper` CLI on PATH; read frames; could not diarize |
| **B** | LongCat 2.5 | v1 | no | 18 | followed the protocol; transcript garbled by a wrong model choice |
| **C** | Nemotron VL | v3 (current) | yes | 7 | full pipeline, OCR, exact citations |
| A′ | Nemotron VL | no | no | 9 | **excluded**: read documents from the working directory |
| C′ | LongCat 2.5 | v2 | – | – | **excluded**: free quota exhausted before the run |

## Scores (0–2 per question)

| Question | A (no skill) | B (skill v1) | C (skill v3) |
|---|---|---|---|
| 1. slide text | 2 — read the frame | 2 — read the frame, labelled observation | 2 — `text_overlay` 0.987 |
| 2. speech 7–9 s | 1 — "the second speaker." only, no range | 2 — quoted the document | 2 — both utterances, exact ranges |
| 3. who spoke more | 0 — no diarization, refused | 2 — internally consistent | 2 — exact, with the range caveat |
| 4. deadline | 2 — no, with evidence | 2 — no, searched variants | 2 — no, "fact about the transcript, not the world" |
| **Total** | **5/8** | **8/8** | **8/8** |
| Invented facts | 0 | 0 | 0 |

The totals hide the important difference: B scored 8/8 while quoting a document
whose English transcript had been mangled by `whisper-small` auto-detecting
Italian. It was faithful to a bad document. C scored 8/8 on a correct document,
with OCR instead of a visual guess. Correctness of the *document* is a separate
column that the skill cannot fix by itself.

## What the runs changed

1. **Shot detection was colour-blind.** The red→blue cut produced one scene and
   one keyframe. Fixed: RGB signatures (dHash + 4×4×4 histogram).
2. **The skill did not tell the agent how to choose the ASR language.** B used
   `whisper-small` with auto-detect (Italian); C′ used `whisper-small
   --language ru` because the *question* was Russian. Both produce confident
   garbage. The skill now says: judge the media language, not the prompt's;
   English → `whisper-base.en`, other → `whisper-small --language xx`; check
   `language` and the first lines before trusting a transcript.
3. **Agents read any JSON they can find.** A′ answered perfectly by reading
   previous runs' documents from the working directory. The benchmark now runs
   each session in a directory containing only the media file. This is also a
   product lesson: keep analysis outputs out of the media directory.
4. **A free quota is not a benchmark plan.** LongCat sessions hit
   `Free usage limit reached` mid-run. The matrix above is honest about which
   cells are missing.

## Tokens

Counted from OpenCode's own session records (`session_v2`), so the numbers are
consistent across sessions. `input` is uncached prompt, `cache read` is the
cached prefix re-read (much cheaper), `output` includes the answer, `reasoning`
is the model's thinking. "Fresh" = `input + output`.

| Session | Model | Skill | input | cache read | output | reasoning | total | fresh |
|---|---|---|---|---|---|---|---|---|
| A | Nemotron VL | no | 58 717 | 169 104 | 2 563 | 5 524 | 235 908 | 61 280 |
| C | Nemotron VL | v3 + OCR | 32 905 | 182 112 | 2 108 | 6 612 | 223 737 | 35 013 |
| B | LongCat 2.5 | v1 | 52 878 | 81 408 | 3 119 | 10 603 | 148 008 | 55 997 |
| A′ (contaminated) | Nemotron VL | no | 110 418 | 13 008 | 1 309 | 4 345 | 129 080 | 111 727 |
| LongCat baseline (quota) | LongCat 2.5 | no | 171 541 | 3 893 888 | 13 566 | 44 145 | 4 123 140 | 185 107 |

Reading it:

- The skill did **not** cost more: C used 43 % fewer fresh tokens than A
  (35 013 vs 61 280) and 5 % fewer total tokens, while scoring 8/8 instead of
  5/8. Reading the skill once paid for itself by removing exploratory ffmpeg and
  whisper calls.
- Cache reads dominate every session; they are the cheap part of the bill. The
  number that matters for a paid model is the fresh column.
- The LongCat baseline burned 4.1 M tokens (3.9 M of it cache) and hit the free
  quota before finishing. That is the cost of "invent your own pipeline".
- These are single sessions, not averages.

## Second fixture: Russian, one speaker

12 s of Russian speech (macOS `say -v Milena`), clean, one voice. Transcript
from the reference run: three sentences about pricing, delivery and a quarterly
result; the only number is "20 %".

Six tasks: quote 00:03–00:07, list amounts, say whether discounts are mentioned,
identify the language and how, find «подписку» with a timestamp, count speakers.

| Question | A (whisper CLI, no skill) | C (skill, whisper-small ru) |
|---|---|---|
| 1. quote 03–07 | 2 — exact | 2 — exact |
| 2. amounts | 2 — 20 % | 2 — 20 % with word range |
| 3. discounts | 2 — none | 2 — none, searched a stem |
| 4. language | 2 — `ru` from whisper output | 2 — from media/document, not the prompt |
| 5. «подписку» | 2 — 04.80–05.34 | 2 — 04.880–05.490 |
| 6. speakers | 1 — "probably one", unverified | 2 — S1, 3 turns, 10.890 s |
| **Total** | **11/12** | **12/12** |

This is the case where the skill is **more expensive**: A2 used 23 131 fresh
tokens (87 685 total) with three whisper commands; C2 used 34 558 fresh
(233 790 total) because it built a full five-stage document and read the skill.
The extra 11.4k fresh tokens bought a verified speaker count, word-level timing
and provenance; for a 12 s voice memo that is arguably overkill. On the video
fixture the same skill was *cheaper* than exploring with ffmpeg.

Combined across both fixtures: **A 16/20 (80 %), C 20/20 (100 %)**, zero
invented facts in both.

## Cross-model: native multimodal vs text-only

Same video fixture, four questions. "Native media" means the model itself can
see frames; DeepSeek V4.1 Flash cannot.

| Model | Native media | Skill | Q1 | Q2 | Q3 | Q4 | Score | Fresh tokens | Total |
|---|---|---|---|---|---|---|---|---|---|
| Nemotron 3.5 Super VL | yes | no | 2 | 1 | 0 | 2 | **5/8** | 61 280 | 235 908 |
| Nemotron 3.5 Super VL | yes | yes | 2 | 2 | 2 | 2 | **8/8** | 35 013 | 223 737 |
| DeepSeek V4.1 Flash | no | no | 2 | 2 | 2 | 2 | **8/8** | 9 752 | 249 097 |
| DeepSeek V4.1 Flash | no | yes | 2 | 2 | 2 | 2 | **8/8*** | 41 399 | 327 621 |

`*` one unverifiable claim: the text-only model said the OCR text was
"confirmed visually", which it cannot do. Fixed in the skill afterwards.

Reading it, honestly:

- **The skill's biggest effect is on the multimodal model**: 5/8 → 8/8, and
  43 % fewer fresh tokens, because it stopped exploring and queried the
  document.
- **A text-only model without the skill scored 8/8 on this fixture** because
  the machine has `whisper large-v3` and `tesseract` on PATH: it extracted
  audio, transcribed, OCR'd the frame and estimated voices with f0 + LTAS, in
  11 commands. A rich shell is a strong baseline.
- **With the skill, the text-only model paid 4× the fresh tokens** (41 399 vs
  9 752) for the same answers — the full seven-stage document plus the skill is
  overhead on a 14 s file. It gained a reproducible document, exact ranges and
  provenance the baseline cannot offer.
- **Q2 exposed a real timestamp subtlety**: the document says the second phrase
  starts at 00:07.992, but that is the ASR region with its 300 ms digital
  lead-in; the DSP silence is 08.060–08.960, and the baseline's silence
  detection was closer to the audible onset. `--align` fixes this; the skill
  now warns that a `speech` segment start includes padding.
- The 7–9 s answer from the no-skill baseline was *more accurate* than the
  padded segment time. That is why alignment exists, and why the skill says to
  use `words[].range` or `--align` for "when exactly".

## Hard fixture: 4-minute meeting

Full method, generator and ground truth: `scripts/bench/README.md`. The fixture
is 4 minutes, 4 voices, 6 slides, a 1.4 s overlap and a music intro. Baseline
sessions had `ffmpeg` and Python but **no** `whisper`, `tesseract` or
`sonolytica` — the realistic "agent without a media stack" case.

Eight tasks: Friday's event, slide text at 00:01:30, find "pricing", budget
change, budget cut (absent), slide changes, screen change 1:10–1:30, final-copy
quote. 0–2 per task.

| Session | Model | Native media | Skill | Score | Fresh tokens | Total |
|---|---|---|---|---|---|---|
| A | Nemotron 3.5 Super VL | yes | no | **7/16** | 91 952 | 328 238 |
| B | Nemotron 3.5 Super VL | yes | yes | **16/16** | 41 139 | 141 335 |
| C | DeepSeek V4.1 Flash | no | no | **4/16** | 7 403 | 139 982 |
| D | DeepSeek V4.1 Flash | no | yes | **16/16** | 15 813 | 253 094 |

Reading it:

- **Without the skill, neither model can answer the speech questions.** The
  multimodal baseline read the slides (Q2, Q4) but could not quote anyone; the
  text-only baseline could only detect pixel changes (Q6, Q7, both exact).
- **Frame sampling without the document is imprecise.** The multimodal baseline
  placed the pricing slide at ~01:03 (actual cut 00:40) and the legal slide at
  ~01:29 (actual 01:20); it sampled arbitrary seconds instead of cut times.
- **Both skill sessions scored 16/16** with exact OCR text and word ranges. They
  also volunteered the right caveats: 7 diarization labels for 4 voices, ASR
  errors ("preaching page"), and OCR taken at 00:01:21, not 00:01:30.
- **The skill was cheaper than the no-skill multimodal run** (41 139 vs 91 952
  fresh tokens) and 2.1× the text-only baseline (15 813 vs 7 403) for four
  times the score. Cost per correct answer favours the skill.
- **Product gaps exposed, not hidden:** diarization made 7 labels from 4 voices,
  and the overlapped line ("The budget also covers the partner webinar") is
  missing from the transcript entirely. No task scores those; they are recorded
  as the next work.

## `query brief`: one call instead of five (2026-11-12)

`query brief` returns media facts, run status, speech totals, speakers, up to
five chapters and modality counts in one 2–3 KB read. The skill now starts
there and tells the agent not to preload reference files.

Method: same hard and easy fixtures, DeepSeek V4.1 Flash (OpenCode subagent),
clean directory containing only the media, a copy of `skills/sonolytica/` and
the CLI; `whisper`, `tesseract`, `ffmpeg` and Python were forbidden. The prompt
asked for answers with ranges and to start with `query brief`.

| Fixture | Session | Commands | Score | Fresh tokens | Comparison |
|---|---|---|---|---|---|
| easy 14.4 s | brief, context budget | 9 | **8/8** | **9 521** | skill v3: 41 399 |
| hard 4 min | brief, preloaded 3 refs | 30 | **16/16** | 26 998 | — |
| hard 4 min | brief, context budget | 15 | **16/16** | **22 410** | skill v3: 15 813 |

Reading it:

- **Easy fixture: 4.3× fewer fresh tokens than the previous skill session**
  (9 521 vs 41 399) with the same 8/8. Most of the old cost was exploration;
  `brief` plus four targeted queries replaced it.
- **Hard fixture: 16/16 and under the 25k gate**, but not cheaper than the
  earlier 15 813-token session. That session was already lean; this run is a
  different harness and produced longer answers. Do not claim a hard-fixture
  token win.
- **Preloading references is expensive.** The first hard run opened the
  cheatsheet and two workflow files and made 30 commands: 26 998 fresh. The
  context-budget rule in the skill (read `SKILL.md` once, open one reference
  only when needed, two absence searches max) cut it to 22 410 fresh and 15
  commands, score unchanged.
- **No invented facts, no false sight.** The text-only model explicitly said it
  could not see frames and cited OCR as document text. On the easy fixture it
  reported the second speaker's time as 6.660 s instead of the old reference's
  8.400 s: the VAD gate and alignment now trim to audible speech (the old
  00:07.992 start includes ASR padding). The "who spoke more" answer is
  correct; the duration follows the improved document.

Caveat: two hard runs and one easy run, single sessions, one model. The
comparison with earlier sessions is within the same OpenCode token store but
not the same harness version.

## Limits

- Three fixtures, 18 questions, eleven sessions. This is a smoke benchmark, not
  a statistical claim; all fixtures are synthetic.
- Sessions use different models: Nemotron VL and DeepSeek on the video fixture,
  Nemotron VL on Russian, LongCat once. Only comparisons within one model are
  controlled.
- The no-skill baselines had `whisper` and `tesseract` on PATH. That makes them
  strong; in a bare shell they would score lower.
- Scoring is manual, by comparing each answer with the reference document.
- The baseline had a `whisper` CLI available; in a shell without it, A would
  score lower. That is a fair part of "what an agent can do with a shell".
- The fixtures are synthetic: no motion, no real accents, no background noise.
  The hard fixture stresses diarization (and exposes its over-segmentation) but
  the overlapped words are missing from the transcript, so overlap splitting is
  not measured.

## Reproduce

```bash
# Build and copy the binary so the sessions cannot rebuild the repo
cargo build --release -p sonolytica-cli --features full-metal
cp target/release/sonolytica /private/tmp/sonolytica-bench

# Fixture: two PIL-rendered slides + the two-voice wav, concatenated with ffmpeg
# (see the table above for the exact texts and times)

mkdir -p /tmp/bench-a /tmp/bench-b
cp bench.mp4 /tmp/bench-a/; cp bench.mp4 /tmp/bench-b/
# Session A: ask the four questions, forbid the repo and the sonolytica binary.
# Session B: point the agent at skills/sonolytica/SKILL.md and the CLI.
```

The full commands of each session are in its report; the key ones for C were:

```bash
sonolytica analyze bench.mp4 --out doc.json --profile standard --ocr
sonolytica query timeline doc.json --type text_overlay
sonolytica query timeline doc.json --from 00:00:07 --to 00:00:09
sonolytica query speakers doc.json
sonolytica query search doc.json deadline
```
