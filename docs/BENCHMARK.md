# Benchmark: does the skill make an agent better at media?

Date: 2026-11-11; the `query brief` sections and the reproducible cycle were
added 2026-11-12. Four fixtures (a 14 s clip, Russian speech, a 4 min meeting, a
20 min real meeting), 26 tasks, fifteen scored sessions across three models, and
two contaminated attempts that are recorded as failures rather than hidden.

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

## Reproducible cycle: `brief` v2 and a slim core (2026-11-12)

This cycle made the measurement itself repeatable, then changed the skill.

Method — `scripts/bench/prepare_agent_fixture.sh` builds a directory with only
the media, a copy of `skills/sonolytica/`, the full-metal binary and
`PROMPT.md`; `scripts/bench/prompts/*.md` are the exact questions, identical for
every configuration; `scripts/bench/session_tokens.sh <session-id>` prints
`fresh = input + output` from `session_v2`. Sessions: DeepSeek V4.1 Flash
(OpenCode `general` subagent), no `whisper`, `tesseract`, `ffmpeg` or Python;
`jq` was allowed and used. Every answer was scored against the reference
document (0/2 per task), and every quote was checked character-for-character.

| Fixture | Skill | Commands | Score | Fresh (run 1 / run 2) | Median |
|---|---|---|---|---|---|
| hard 4 min | v3, `brief` v1 | — | 16/16 | — | 22 410 |
| hard 4 min | v4, `brief` v2 | 13 | **16/16** | **14 773 / 13 517** | **14 145** |
| easy 14.4 s | v3, `brief` v1 | — | 8/8 | — | 9 521 |
| easy 14.4 s | v4, `brief` v2 | 9 / 10 | **8/8** | **8 434 / 8 818** | **8 626** |
| long 20 min | v3 (341-line core) | 15 | **16/16** | 17 633 | 17 633 |
| long 20 min | v4 (200-line core) | 22 | **15/16** | 17 741 | 17 741 |

Reading it:

- **Hard: cost gate met.** 14 145 fresh against the 18 000 gate and 22 410
  baseline, same 16/16, no invented facts. Two runs spread 9.3 %, inside ±10 %.
- **Easy: cost gate missed by 7.8 %.** 8 626 against 8 000, still 9.4 % below
  the 9 521 baseline and 8/8 both runs (4.6 % spread). The remaining cost is the
  fixed read: harness prompt plus a 200-line `SKILL.md`; the agent's own
  commands are cheap.
- **Long: quality did not improve.** The old core scored 16/16; the new one
  scored 15/16: it answered the overlap question with the span
  00:19:18.470–00:19:27.120, while overlapping turns run to 00:19:31.790. That is
  the rubric's "correct content, wrong range" = 1. Cost was 0.6 % higher
  (22 commands spent on `jq` exploration of the raw JSON). A truncated range is
  a wrong range.
- **`brief` v2** adds `video.scene_cuts` and one OCR line per keyframe. On the
  4-minute OCR document the compact brief is **3 003 B** (27 text lines), inside
  the 4 KB gate; on the older over-segmented `ref.json` (7 speakers, 16 chapters,
  `ultra`) it is 4 691 B — above the gate, and it was ~3.8 KB before this
  change. The gate is met on the document the benchmark actually produces.
- **Slimming the core cost no quality on the 4-minute fixture** and one point on
  the long one. The overlap rule survived the move to `references/modalities.md`
  but the routing did not: the old core had a "minimum commands" table that the
  new core delegates to the cheatsheet.

### Round 2: overlap map in `brief` (same day)

`brief` now carries `speech.overlaps`: merged spans of turns flagged
`overlap: true`, in time order, capped at 20. `brief --json` grew to 2.8 KB on
the 20-minute document; the text output grew by one line per span. Same method,
same model, one run per fixture:

| Fixture | Score | Commands | Fresh | Round 1 |
|---|---|---|---|---|
| easy 14.4 s | **8/8** | 7 | **7 517** | 8 434 / 8 818 |
| hard 4 min | **16/16** | 9 | **10 393** | 14 773 / 13 517 |
| long 20 min | **16/16** | 8 | **11 308** | 17 741 |

The long session is the direct evidence for the change: it reported all three
overlap spans (00:19:18.470–00:19:19.599, 00:19:19.910–00:19:26.559,
00:19:26.650–00:19:31.780), scored 16/16, and made 8 commands instead of 22. The
hard and easy sessions also got cheaper, but part of that is run variance: the
skill gained one line and the fixed prompt is unchanged. Treat the direction as
solid and the exact percentages as single runs.

Caveat: one long session per skill version, manual scoring, one model. The
20-minute fixture is VoxConverse `fvhrk` (CC BY 4.0): a real 4-speaker debate
(RTTM), 4 annotated speakers, one participant speaking 18.5 s, no overlap
annotation. Those sessions used the old window-based document, which reported
2 speaker labels with a `diarization_speaker_cap` warning — the long-file
failure, not an agent error. The AOSC build now reports **4/4 at 6.40 % DER**
(`docs/EVAL.md`); the agent sessions have not been rerun on it.

## Head-to-head: Gemini (Antigravity) and TwelveLabs Pegasus (2026-11-12)

Same fixtures, same eight (or four) questions, same 0/2 rubric. Three systems:

- **Gemini alone** — `agy` (Google Antigravity CLI, Gemini backend) in a clean
  directory with only the media and the questions. Tools were allowed:
  `ffmpeg`, Python and the machine's local `whisper`. This is a strong agent
  baseline, not a raw API call.
- **Gemini + SONOLYTICA** — the same CLI, the same questions, plus
  `skills-sonolytica/`, the full-metal binary and the instruction to start at
  `query brief`.
- **TwelveLabs Pegasus 1.5** — the raw synchronous `/v1.3/analyze` API, one
  prompt per fixture, no tools, no document. Base64 upload; the long audio
  fixture was wrapped in a black 640×360 video because the API rejects
  audio-only media.

Scores are manual, against the same reference document; a correct claim with a
wrong or missing range scores 1, not 2.

| Fixture | Gemini alone | Gemini + skill | TwelveLabs Pegasus | SONOLYTICA + DeepSeek |
|---|---|---|---|---|
| hard 4 min (max 16) | **14** | **16** | **10** | **16** |
| easy 14.4 s (max 8) | **6** | **8** | **3** | **8** |
| long 20 min (max 16) | not run¹ | **16** | **10** | **16** |

Wall time and API usage (single runs):

| Run | Time | Tokens |
|---|---|---|
| Gemini alone, hard | 7 m 43 s | not exposed by `agy` |
| Gemini + skill, hard | **1 m 10 s** | not exposed |
| Gemini alone, easy | 2 m 39 s | not exposed |
| Gemini + skill, easy | **0 m 50 s** | not exposed |
| Gemini + skill, long | **2 m 07 s** | not exposed |
| TwelveLabs, hard | 0 m 20 s | 71 592 in / 400 out |
| TwelveLabs, easy | 0 m 13 s | 5 687 in / 84 out |
| TwelveLabs, long | 0 m 55 s | 54 563 in / 421 out |
| SONOLYTICA hard / easy / long | — | **10 393 / 7 517 / 11 308 fresh** |

¹ The long baseline ran for six minutes, then Antigravity returned
`RESOURCE_EXHAUSTED` ("AI credits balance is too low"). Not scored, not hidden.

Reading it:

- **Gemini is a strong baseline with a shell.** It extracted frames, ran local
  Whisper and answered 14/16 on the hard fixture. Its weak spot is precision:
  "pricing" got the slide time (00:00:40) instead of the word (00:00:44.058),
  the budget answer invented "last quarter" where the recording says "last
  week", and several answers gave no range at all.
- **The skill improved Gemini on its own turf**: 14 → 16 on hard, 6 → 8 on
  easy, and 6.6× faster on hard (7 m 43 s → 1 m 10 s) because it queried a
  document instead of rebuilding a pipeline. It also fixed the OCR omission
  only after the skill started telling the agent to pass `--ocr` at analyze
  time; the first skill run scored 12/16 for that reason — a real skill bug
  found by this test and fixed.
- **TwelveLabs is fast and semantic, not citable.** It answered in 13–55 s with
  no ranges: on hard it scored 10/16, on easy 3/8 (it missed speech that exists
  between 00:07 and 00:09 and halved the dominant speaker's time), on long
  10/16 with an invented 20:32 duration and summarised "chapters" that are not
  in the media document.
- **The long fixture exposed our real gap.** TwelveLabs reported **4 speakers**
  on the 20-minute debate; the then-current Sortformer document reported 2
  labels with a `diarization_speaker_cap` warning. The skill forced the honest
  "at least 2, may be more", but the underlying count was worse. AOSC streaming
  fixed it after the run: 4/4 at 6.40 % DER, not a prompt change.
- **No free lunch on quotas.** Antigravity exhausted its credits mid-baseline;
  TwelveLabs' free plan is metered in minutes. Local SONOLYTICA has neither
  failure mode.

### Blind cross-check

The four hard-fixture answers were anonymised (A–D) and scored by an
independent judge — Codex CLI, a different model family, given only the
reference key and the rubric, with no source names. It replaced my manual
scores with its own:

| Answer | Manual | Blind | Valid citations | Invented |
|---|---|---|---|---|
| Gemini alone | 14 | **10** | 7 | 8 |
| Gemini + skill | 16 | **16** | **19** | 2¹ |
| TwelveLabs Pegasus | 10 | **7** | 3 | 6 |
| DeepSeek + skill | 16 | **15** | **19** | 0 |

The ranking is identical — the two skill runs are first and second, the raw API
last — but the judge is much stricter about ranges. It scored Gemini's
`00:00:39` for "pricing" as 0 (the word is at 00:00:44.058), counted "last
quarter" as invented (the recording says "last week"), and took a point from
the DeepSeek run because its first slide label was 00:00:01 instead of the
scene start 00:00:00. That strictness is the product thesis: a plausible answer
with a wrong range is not a citation.

¹ The judge marked two "invented" facts for the skill answer while giving every
question 2/2; treat that counter as noisy and the citation count as the useful
signal.

Raw outputs, including every wrong timestamp and the judge JSON, are in
[`bench/2026-11-12/`](bench/2026-11-12/). Keys were never written to the
repository. TwelveLabs at temperature 0 reproduced its first answer exactly on a
second run; Gemini reruns were blocked by the Antigravity quota, so its cells
remain single runs.

Caveat: one run per cell, one Gemini model, manual scoring; the Gemini baseline
is an agent with tools, not the raw multimodal API. Treat this as a smoke
head-to-head, not a leaderboard.

## Round 3: vision-era skill, full suite (same day)

After faces/objects and the command-budget rule, three fixtures plus one rerun:

| Fixture | Score | Commands | Fresh | Note |
|---|---|---|---|---|
| hard 4 min | 16/16 | 17 | 27 235 | outlier: read transcript, scenes, chapters and five absence variants |
| easy 14.4 s | 7/8 | 11 | 21 021 | missed the tail of the first phrase; read `analyze --help` |
| easy, rerun | **8/8** | 11 | **9 150** | budget rule added: no `--help`/`doctor` once brief shows the stages |
| long 20 min | 16/16 | 12 | 15 578 | quote split across two segments, both ranges cited |

Medians over every comparable run:

| Fixture | Runs | Median fresh | Range |
|---|---|---|---|
| hard 4 min | 3 | **14 773** | 13 517–27 235 |
| easy 14.4 s | 5 | **8 818** | 7 517–21 021 |
| long 20 min | 2 | **13 443** | 11 308–15 578 |

Reading it honestly:

- **Fresh cost is dominated by agent behaviour, not by the document.** Every
  command re-sends the whole context; a single `analyze --help` is thousands of
  input tokens. The same skill and fixture produced 7.5k and 21k.
- **The command-budget rule works**: the rerun after adding "no `--help`,
  `doctor`, `models list` or `runs` once `brief` shows the stages" fell from
  21 021 to **9 150** for the same 8/8.
- **The hard gate holds at the median** (14 773 ≤ 18 000) but not on a bad run
  (27 235). Do not quote a single hard number; quote the median and the range.
- **The easy gate is still missed at the median** (8 818 vs 8 000) even though
  the best run was 7 517. The remaining cost is the harness prompt, the skill
  read and the model's own reasoning loop.

## Visual boundary: refusal, then captions (2026-11-12/13)

Six-second public-domain portrait, eight questions whose answers are visual:
tie colour, gesture, flag count, expression, flag text, posture, background,
clothing. The document can contain a `person` box, a `tie` object, a face and
pose keypoints, so a careless agent can be tempted into a visual guess.

Two modes, same prompt:

| Mode | What the document has | Result |
|---|---|---|
| no `--caption` | person/tie/face/pose, no colour or action | DeepSeek free **16/16 refusal**, GLM free **16/16 refusal**, 0 visual guesses |
| `--caption` | + Florence-2 model-generated description | DeepSeek free **16/16 caption-aware**, 6/8 questions answered from the caption, 0 unsupported guesses, 31 961 fresh |

The caption run answered tie colour, arms crossed, flags, smiling, suit and
background from the caption; flag text and sitting/standing were still absent
and the agent said so. The no-caption run refused all eight. The rubric:
without a caption, "the document does not contain this" is 2 and a visual guess
is 0; with a caption, a correct caption fact cited **as a model observation**
is 2, and a visual fact that is not in the document is still 0.

Caveat: captions can be wrong. On the two slide keyframes the same model called
a red background "black" and repeated itself; use OCR for exact on-screen text
and never quote a caption as speech. Full rubric and prompt:
`docs/bench/visual-boundary.md`, `scripts/bench/prompts/visual-boundary.md`.
The fixture is one portrait; a caption benchmark over varied scenes is next.

## Cost tooling: cache and `query ask` (2026-11-12)

These are product measurements, not agent sessions; they show the fixed cost an
agent sees. Bytes and wall time are not fresh tokens.

- **Cache.** The 14.4 s easy fixture analysed in 7.64 s; the same bytes,
  parameters and annotation flags hit the cache in **0.01 s** (0.13 % of the
  first run), copied the keyframes to the new output name, and added
  `analysis_cached`. A cache miss with `--no-cache` is unchanged.
- **Evidence pack.** On the 20-minute `fvhrk` document, `query ask "budget and
  pricing"` returns 978 B of text (2 893 B JSON) and reports `pricing` as
  absent. The equivalent three commands — `search budget`, `search pricing`,
  `speakers` — total 3 912 B of JSON. One command instead of three, and the
  absence is explicit.
- **Pose.** One person with 11 keypoints adds 4.6 KB to a 6 s document
  (4 685 → 9 268 B). It is opt-in and keyframe-bound; dense frames multiply it.

Caveat: the agent benchmark has not been rerun with `ask`/cache. The command
count and output size improve, but only a paired session can claim the token
win. The skill now tells the agent to use `ask` after `brief` and treats
`analysis_cached` as valid data.

A pilot subagent session on the easy fixture scored **8/8** with 8 `sonolytica`
commands, but its OpenCode session directory was the repository rather than the
clean fixture. Its **11 935 fresh** tokens are therefore excluded from the
table; the run is a behaviour check, not a comparable cost number.

## Free-model runs: clean directories (2026-11-13)

The previous cost rows used paid DeepSeek V4.1 Flash. This matrix uses the free
variant `opencode/deepseek-v4.1-flash-free` in a fresh clean directory per
fixture (only media + skill + binary + `PROMPT.md`), with the current skill
(`ask`, `poses`, cache). Scores are against the same reference documents.

| Fixture | Model | Commands | Score | Fresh | Cost |
|---|---|---|---|---|---|
| easy 14.4 s | DeepSeek V4.1 Flash free | 7 | **8/8** | 18 224 | $0 |
| easy 14.4 s | GLM 5.3 Flash free | 6 | **8/8** | 24 294 | $0 |
| easy 14.4 s | Flint 40B free | 14 | **8/8** | 76 196 | $0 |
| visual boundary 6 s | DeepSeek V4.1 Flash free | 4 | **16/16 refusal** | 17 652 | $0 |
| visual boundary 6 s | GLM 5.3 Flash free | 4 | **16/16 refusal** | 24 130 | $0 |
| visual boundary + caption 6 s | DeepSeek V4.1 Flash free | 4 | **16/16 caption-aware** | 31 961 | $0 |
| hard 4 min | DeepSeek V4.1 Flash free | 8 | **16/16** | 19 396 | $0 |
| hard 4 min | GLM 5.3 Flash free | 10 | **16/16** | 23 025 | $0 |
| long 20 min | DeepSeek V4.1 Flash free | 24 | **14/16** | 31 570 | $0 |

Reading it:

- **Both free models reach the maximum score on easy** (8/8) and hard (16/16).
  Without captions both refuse every visual-boundary question without
  inventing a colour, action, expression or background; with `--caption` the
  agent answers six of eight from the model observation and still refuses the
  two the caption does not contain. DeepSeek-free is cheaper in tokens on both
  scored fixtures (18 224 / 19 396 vs 24 294 / 23 025); GLM-free needed one
  retry after passing `doc` instead of `doc.json`. Neither needs a paid quota.
- **Long is 14/16, and that is a real finding.** The new AOSC document reports
  4/4 speakers and correct quotes, but **no overlapping turns** on `fvhrk`.
  Question 4 asks for an overlap span; the agent correctly said the document
  contains none, which scores 0 against the old reference. The old document's
  eight overlap turns were S1 fragments (`students`, `from`, …) duplicating
  words inside S2 sentences — likely an artifact of the window merge, not a
  second speaker. The new behaviour may be cleaner, but the old benchmark
  question cannot be passed from it. Do not add a threshold hack or duplicate
  words; the fix is speaker-attributed ASR, which is the next cycle.
- **Free is not cheaper in tokens.** Fresh is 18–19 k against the paid medians
  8 818 (easy) and 14 773 (hard); the dollar cost is $0, but the free tier is
  quota-limited. Do not compare fresh across models; compare score per dollar
  and note the quota.
- **The command budget held.** 4 commands on the visual fixture, 7 on easy,
  8 on hard. `ask` was used for both absence questions (deadline, budget cut),
  and the hard run correctly said "at least 4 speakers" because the document
  carries `diarization_speaker_cap`.
- **Free models are not interchangeable.** Flint 40B free got 8/8 but spent
  76 196 fresh in 14 commands: it ran `--help` and `doctor`, then repeated the
  same timeline/speakers/ask queries after a path mistake. DeepSeek free got
  the same score in 7 commands and 18 224 fresh. The skill's command budget is
  only effective when the model follows it; measure the model, not just the
  skill.

Caveat: one session per cell, one free model, manual scoring. The long cell is
still running; it will be added when the session closes.

## Limits

- Four fixtures, 26 questions, fifteen sessions. This is a smoke benchmark, not
  a statistical claim; all fixtures except the VoxConverse meeting are synthetic.
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
- The long fixture has **no independent overlap annotation**. Its Q4 is scored
  against the old window document's spans, which were S1 fragments duplicating
  words inside S2 sentences. The new AOSC document has no such spans and its
  free run scores 14/16; read that as a benchmark-reference limitation, not
  proof that the recording has no simultaneous speech. An annotated overlap
  shard is needed before claiming an overlap score.

## Reproduce

```bash
# clean fixture with only media + skill + full-metal binary + prompt
scripts/bench/prepare_agent_fixture.sh --name hard \
  --media /private/tmp/bench-hard/meeting.mp4
scripts/bench/session_tokens.sh ses_…      # fresh = input + output

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
