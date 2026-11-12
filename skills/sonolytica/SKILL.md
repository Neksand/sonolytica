---
name: sonolytica
description: Use when asked to analyze, transcribe, quote, summarize, search or answer questions about an audio or video file — what was said, who said it, when it happened, what music or sounds are present, what is visible on screen. Triggers: transcribe, transcript, subtitles, captions, diarization, who said what, timestamp, find the moment, podcast, interview, meeting notes, lecture, screencast, music analysis, BPM, key, chords, notes, video scenes, on-screen text, media metadata, расшифровка, транскрипт, кто говорил, таймкод, анализ видео, анализ музыки.
---

# SONOLYTICA — media analysis with citations

SONOLYTICA turns a media file into one typed, timestamped document. You read
that document and answer from it. You never listen to the audio yourself, never
paraphrase inside quotation marks, and never invent a timestamp, a word, a
speaker, an emotion or an event. **Iron rule:** every factual claim you make
must be traceable to a `query` result from the document. If it is not in the
document, say so.

**Context budget:** read this file once. Open a `references/` file only when
told to — do not preload them. Aim for `brief` plus 1–3 targeted queries per
question; for an absence claim, search the word once and its stem once.

## 1. The one-minute model

`analyze FILE` writes one document; `query COMMAND` answers one question from
it. `runs` = what executed, `warnings` = what was skipped, `analyses` =
aggregates, `segments` = the timeline. Never load the whole JSON into context.

## 2. Choose the profile before you spend money

| Profile | Runs | Use when |
|---|---|---|
| `structure` | VAD, diarization, music/acoustic aggregates, shot boundaries | "what is this, who talks, how is it structured", long recordings, first pass |
| `standard` | + speech recognition | any question about content, quotes, search |
| `ultra` | standard, plus exit 2 if any requested capability failed or never ran | automation that must not silently degrade |

`scene` is a no-op on audio-only files and is excluded from the `ultra` check.
`ultra` adds forced word alignment (English, wav2vec2 CTC) and fails on a
capability that never ran. Default: `standard` under ~30 minutes, `structure`
for a very long first pass. Details: `references/profiles.md`.

## 3. Run it

```bash
sonolytica doctor                          # ffmpeg + models + providers
sonolytica analyze FILE --out doc.json --profile standard --ocr --summary
sonolytica query brief doc.json            # one compact overview — start here
sonolytica query runs doc.json             # stage status in detail
sonolytica report doc.json --out report.html  # human page: chapters, quotes, frames
```

`--ocr` runs on the keyframes at analyze time: pass it whenever the media may
have on-screen text — it cannot be added later without re-running.
`analyze` prints the document to stdout unless `--out` is given; `--summary`
goes to stderr; `--strict` (or `--profile ultra`) exits 2 when a stage failed or
a requested capability never ran. A missing model disables one capability.

**Language and model:** pass `--language` whenever you know it. English →
`--model whisper-base.en` (default); anything else → `whisper-small` **with**
`--language xx`. Auto-detection can label English as another language and turn
the transcript into garbage. Judge the media, not the user's prompt: a Russian
question about an English recording is normal. If a document already exists,
skip `analyze` and start at `query runs`.

## 4. Read in this order

1. `query brief` — media facts, run status, speech totals, speakers, up to five
   chapters, video counts, scene cuts, one OCR line per keyframe and
   music/acoustic headlines. Most overview questions end here.
2. Only the command the question needs (§5) — for an exact quote, a time
   window, a frame or the full speaker list.
3. Visual questions: set `--ocr --objects --faces` at analyze time, not later.
4. Budget: no `--help`, `doctor`, `models list` or `runs` once `brief` shows the
   stages. Aim for ≤6 commands on a clip, ≤10 on a meeting.

`brief` is a map, not a citation source: quote exact wording and ranges from
`search`, `words` or `transcript`. Full speaker/chapter lists are
`speakers`/`chapters`. If a stage is `partial` or `failed`, say so **before**
answering — never answer from half a document.

## 5. Query vocabulary

| Question | Command |
|---|---|
| overview: what is this, who talks, chapters | `query brief doc` |
| what happened between 00:10 and 00:20 | `query timeline doc --from 00:00:10 --to 00:00:20` |
| what is active at 00:12.450 | `query at doc 00:00:12.450` |
| the whole transcript, untruncated | `query transcript doc` |
| every word of one utterance | `query words doc 4` |
| keyframe images for a window | `query frames doc --from 00:12:00 --to 00:13:00 --max 8` |
| text on screen (after `--ocr`) | `query timeline doc --type text_overlay` |
| find a word, with its timestamp | `query search doc pricing` |
| who spoke most, how long | `query speakers doc` |
| what are the chapters | `query chapters doc` |
| tempo / key / chords / loudness | `query music doc` |
| silence and levels | `query acoustic doc` |
| which stages ran or failed | `query runs doc` |
| media facts and totals | `query summary doc` |

Start at `brief`; add `search`+`words` for a quote, `timeline` for a window or
text overlays, `frames` to look, `chapters`/`speakers` for full lists. Routing
table: `references/query-cheatsheet.md`.

Rules of use:

- Times: `HH:MM:SS.mmm`, `MM:SS`, `12.5s`, `950ms`, or microseconds. A bare
  integer is **microseconds**; `12.5` is seconds. Add `--json` for exact values.
- Tables truncate long text at 80 characters with `…`; quote only from
  `query transcript` or `query words` — they never truncate.
- `query chapters` splits on **structure** (silence, speaker change, scene cut),
  not topics. Report it as "главы записи", never as "темы разговора".
- All commands are read-only; `report` writes one HTML page. JSON shapes:
  `references/query-cheatsheet.md`.

## 6. Timestamps and ranges — canonical rules

- Quote ranges as `HH:MM:SS.mmm–HH:MM:SS.mmm` from `start_display`/`end_display`.
  Never round; "on what second?" gets the exact range, optionally plus `(≈3.1 s)`.
  A `speech` start can include ~300 ms of ASR/VAD padding: for "when exactly did
  it start" use `words[].range` or `--align`.
- With `--align` (implied by `ultra`) word times come from forced alignment
  (English, 20 ms frames, median drift ~110 ms); without it they are Whisper
  token times. Say which run produced them.
- Cite `words[].range` for an exact word, the `speech` segment for what was
  said, the `speaker_turn` for who spoke. The last two can differ by padding.
- `speech_duration` is **measured speech** (VAD), not the sum of turns; for "how
  much did S1 speak" use `query speakers` (one aggregate range per speaker, not
  continuous speech).
- `query at T` returning `(none)` is not measured silence; `acoustic.silence` is
  level-based and can appear inside a speech segment, so neither proves "nobody
  spoke".

Detail and edge cases: `references/citation-rules.md`.

## 7. Three evidence classes — never mix them

1. **Document fact** — returned by `query`. Cite the range.
2. **Observation** — something you saw in a frame or heard natively. Label it
   "наблюдение по кадру", not a quote, and do not attach a transcript range.
3. **Inference** — your conclusion. Mark it as such; never inside quotes.

## 8. Answer protocol

1. One line on stage status when not everything is `complete`.
2. The answer, with verbatim quotes and ranges.
3. What the document does **not** cover, when it matters.
4. Self-check: every quote appears character-for-character in a `query` result;
   every timestamp is copied, not computed; labels are document-local. For JSON,
   `query verify` checks quotes and ranges mechanically.

## 9. When the word is not in the transcript

An empty `query search` is a fact about the transcript, not about the world.
Say "В транскрипте нет слова X", then offer the nearest token as a
**hypothesis** ("возможно, ASR расслышал его как Y, 00:00:03.140"), never as a
quote and never as "не говорили". Numbers are normalised: search digits first;
full protocol in `references/citation-rules.md`.

## 10. Speech, music and video rules

- Speaker labels are local to one document; they are not people. Sortformer
  tracks at most four speakers; when all four slots are used a
  `diarization_speaker_cap` warning makes the count a lower bound — say "at
  least N".
- `overlap: true` means two people spoke at once. Sortformer returns two
  simultaneous turns, the pyannote fallback only flags; the transcript is one
  ASR stream, so never invent separate words per speaker. `brief` lists the
  spans (`speech.overlaps`): report every span, not the first one.
- Text-only model: say you cannot see images; never claim to have looked at a
  frame or "visually confirmed" OCR. OCR is a fact read at one instant.
- BPM/key/chords are heuristic scores, sections are technical boundaries, not
  verse/chorus, and notes/bass do not exist yet. `--objects` and `--faces`
  return boxes and scores on sampled keyframes only: never identity, tracking
  or actions.

Per-modality rules: `references/modalities.md`.

## 11. Failures and gaps

| Symptom | What to say | What to suggest |
|---|---|---|
| `warnings[].code = provider_unavailable` | that capability did not run | `sonolytica models fetch <ids>` |
| run `partial` | what is missing, from `reason` | re-run with the model installed |
| empty transcript | whether silence was measured | check `acoustic.silence`, then VAD/model |
| `doctor` says ffmpeg missing | nothing can be analysed | install FFmpeg |
| no `text_overlay` segments | OCR did not run | add `--ocr --out doc.json` |

Troubleshooting detail: `references/troubleshooting.md`.

## 12. Workflows

`references/workflows/` has one file per media type; open only the one you need.

## 13. Limits — do not claim these

- No emotion, intent, topic modelling or abstractive summarisation; summarise
  extractively, with citations. No speaker identity, age, gender or language
  proficiency. No word-level overlap separation: turns are labeled, a merged
  ASR line is not split word by word.
- Word timings are Whisper token times unless `--align`/`ultra` ran; alignment
  is English-only and cannot recover a word the ASR never emitted.

## 14. Accuracy claims

Only from an actual run, naming dataset, model and date; never quote a number you did not produce here.
