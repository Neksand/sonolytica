# Hard benchmark: 4-minute meeting

The first benchmark used a 14-second clip with two voices. This one is built to
be uncomfortable: 4 minutes, 4 synthetic voices, 6 slides, one 1.4 s overlap,
and a baseline environment **without** `whisper` or `tesseract`.

## Reproducible session

```bash
# 1. clean directory: media + skill copy + full-metal binary + PROMPT.md
scripts/bench/prepare_agent_fixture.sh --name hard \
  --media /private/tmp/bench-hard/meeting.mp4
# -> /private/tmp/bench-agent-hard

# 2. one agent session, started by hand in that directory, nothing else

# 3. tokens from OpenCode's own store
scripts/bench/session_tokens.sh ses_…     # or --last
```

The prompt is `scripts/bench/prompts/<name>.md`, copied verbatim into the
fixture and printed by the script: same questions, same rules, every run.
`easy.md` is the 14.4 s clip, `long.md` the 20-minute meeting below.
`--skill PATH` swaps the skill for an A/B run (e.g. a `git archive` of an older
commit). Record model, date, harness, score, commands and `fresh` (input +
output) in `docs/BENCHMARK.md`. The analysis cache is on by default: for a
fresh first run, clear `~/.cache/sonolytica/cache` or export
`SONOLYTICA_CACHE_DIR` to a new directory; otherwise an identical media file
and configuration can hit a previous analysis. A run is only comparable when the prompt, the
fixture and the model match; two runs of one configuration must agree within
±10 % on fresh tokens, otherwise run more and report the median.

## Long fixture: 20-minute real meeting

`long.md` asks eight questions about a 20-minute, four-speaker debate: speaker
count and its warning, who spoke most/least, overlap span, a verbatim quote in
00:12–00:12:30, one word lookup, duration and speech ratio, and five structural
chapters. No OCR and no slides.

The media is VoxConverse `fvhrk` (CC BY 4.0), extracted by the eval script:

```bash
python3 scripts/eval/der_voxconverse.py \
  --parquet /private/tmp/sono-eval/voxconverse-test2.parquet \
  --count 0 --min-speakers 4 --max-speakers 4 \
  --workdir /private/tmp/sono-eval/der-test2-final
# -> audio/fvhrk.wav (20:00) and fvhrk.rttm (4 speakers)
```

Ground truth from the RTTM: 4 speakers, 818.2 s / 238.7 s / 50.3 s / 18.5 s of
speech, no annotated overlap. The document reports 2 labels and a
`diarization_speaker_cap` warning — the known long-file collapse, and the reason
the first question asks how much the count can be trusted.

## Visual boundary: 6-second portrait

`visual-boundary.md` asks eight questions whose answers are **not in the
document** (tie colour, gesture, number of flags, expression, flag text,
sitting/standing, background, clothing). The document may contain a `person`
box, a `tie` object, a face and pose keypoints, but not colour or semantics.
Score 2 for an honest "the document does not contain this", 1 for a true but
incomplete document fact, 0 for a visual guess. The rubric is in
`docs/bench/visual-boundary.md`; the fixture is built with:

```bash
scripts/bench/prepare_agent_fixture.sh --name visual-boundary \
  --media /private/tmp/face.mp4
```

It is the counterpart to the hard/easy runs: those reward finding facts, this
one rewards knowing where the document ends. Re-run with
`analyze ... --caption` to measure caption coverage: a correct caption fact
must be attributed to the caption; a caption is not OCR and not a quote.

## Build the fixture

```bash
python3 scripts/bench/make_meeting_fixture.py --out /tmp/bench-hard
# -> /tmp/bench-hard/meeting.mp4 and timeline.json
```

macOS `say` (voices Alice, Albert, Daniel, Fred), FFmpeg and Pillow are required.
The media is not committed; only the generator is.

Reference analysis (ground truth for the tasks):

```bash
sonolytica analyze /tmp/bench-hard/meeting.mp4 --out ref.json \
  --profile ultra --ocr --language en
```

This produced 6 scenes (cuts at 0:40, 1:20, 2:00, 2:40, 3:20), 6 keyframes, 12
exact OCR lines, whisper-base.en transcript with forced alignment, and a
diarization that over-segments 4 voices into 7 labels. The last part is a known
limitation and is why no task scores speaker counts.

## Tasks

| # | Question | Expected (from the reference document) |
|---|---|---|
| 1 | What happens on Friday? | the pricing page goes live; announcement on Friday (00:00:43.6–00:00:47.6) |
| 2 | Text on the slide at 00:01:30 | `LEGAL REVIEW: APPROVED` |
| 3 | Find "pricing" with a timestamp | 00:00:43.608–00:00:47.608 (speech); slide `PRICING PAGE — FRIDAY` at 0:41 |
| 4 | How much did the marketing budget change? | +20 % ("20% more") 00:02:07.0–00:02:13.6 |
| 5 | Was a budget cut mentioned? | no |
| 6 | List slide/chapter changes with timestamps | 0:40, 1:20, 2:00, 2:40, 3:20 (titles from OCR) |
| 7 | What changed on screen between 00:01:10 and 00:01:30? | `PRICING PAGE — FRIDAY` → `LEGAL REVIEW: APPROVED` at 1:20 |
| 8 | Who says the final copy is due, and when? | "I will send the final copy by Thursday…" 00:02:55.0–00:03:00.0; speaker label is document-local |

Scoring: 0 = wrong or invented; 1 = partly right, wrong range, or unverifiable
source; 2 = correct content with a supporting range from the document.

## Sessions

Four sessions, same fixture, clean directory with only `meeting.mp4`:

| Session | Model | Native media | Tools allowed |
|---|---|---|---|
| A | Nemotron 3.5 Super VL | yes (sees images) | ffmpeg, python stdlib; **no** ASR/OCR/sonolytica |
| B | Nemotron 3.5 Super VL | yes | the SONOLYTICA skill and CLI |
| C | DeepSeek V4.1 Flash | no | ffmpeg, python stdlib; **no** ASR/OCR/sonolytica |
| D | DeepSeek V4.1 Flash | no | the SONOLYTICA skill and CLI |

The restricted environment is enforced by instruction, not a sandbox; the
session reports list every command, so cheating is visible. The first DeepSeek
baseline (14 s fixture) obeyed the same rule.

Token counts come from OpenCode's session store (`session_v2`: input, output,
reasoning, cache read). "Fresh" = input + output.

## Results (2026-11-11)

| Session | Score | Fresh tokens |
|---|---|---|
| Nemotron VL, no skill | 7/16 | 91 952 |
| Nemotron VL, skill | 16/16 | 41 139 |
| DeepSeek, no skill | 4/16 | 7 403 |
| DeepSeek, skill | 16/16 | 15 813 |

Full analysis and caveats: `docs/BENCHMARK.md`.

## Known limits exposed by this fixture

- Diarization over-segments: 4 source voices became 7 document labels, and
  `S4`'s overlapped line ("The budget also covers the partner webinar") is
  missing from the transcript. No task scores speaker count or the overlapped
  words; both are recorded as product gaps.
- ASR errors on synthetic voices: "preaching page" for "pricing page", "pain
  the group" for "beta group". Tasks ask for meaning, not for the garbled
  spelling.
- The overlap is flagged in the document (turns S7/S8) but not split.
