# Benchmark: does the skill make an agent better at media?

Date: 2026-11-11. One fixture, four questions, three scored sessions and two
contaminated attempts that are recorded as failures rather than hidden.

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

## Limits

- One fixture, four questions, three sessions. This is a smoke benchmark, not a
  statistical claim.
- Sessions are not the same model: A/C are Nemotron VL, B is LongCat. The
  skill's effect is visible in A vs C; B adds a second model.
- Scoring is manual, by comparing each answer with the reference document.
- The baseline had a `whisper` CLI available; in a shell without it, A would
  score lower. That is a fair part of "what an agent can do with a shell".

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
