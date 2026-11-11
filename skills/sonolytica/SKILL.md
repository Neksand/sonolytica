---
name: sonolytica
description: Use when asked to analyze, transcribe, quote, summarize, search or answer questions about an audio or video file — what was said, who said it, when it happened, what music or sounds are present, what is visible on screen. Triggers: transcribe, transcript, subtitles, captions, diarization, who said what, timestamp, find the moment, podcast, interview, meeting notes, lecture, screencast, music analysis, BPM, key, chords, notes, video scenes, on-screen text, media metadata, расшифровка, транскрипт, кто говорил, таймкод, анализ видео, анализ музыки.
---

# SONOLYTICA — media analysis with citations

SONOLYTICA turns a media file into one typed, timestamped document. You read
that document and answer from it. You never listen to the audio yourself, never
paraphrase inside quotation marks, and never invent a timestamp, a word, a
speaker, an emotion or an event.

**Iron rule:** every factual claim you make must be traceable to a `query`
result from the document. If it is not in the document, say so.

## 1. The one-minute model

```
analyze FILE  →  document.json  (tracks, segments, analyses, runs, warnings)
query COMMAND →  the answer to one question, as a table or typed JSON
```

- `runs` = what actually executed, with status.
- `warnings` = what was skipped, and why.
- `analyses` = aggregates (speech totals, music, acoustic).
- `segments` = the timeline: speech, speaker turns, music labels, events.
- `query` is the only reading interface. Do not load the whole JSON into context.

## 2. Choose the profile before you spend money

| Profile | Runs | Use when |
|---|---|---|
| `structure` | VAD, diarization, music/acoustic aggregates | "what is this, who talks, how is it structured", long recordings, first pass |
| `standard` | + speech recognition | any question about content, quotes, search |
| `ultra` | standard, plus exit 2 if any requested capability failed or never ran | automation that must not silently degrade |

`ultra` does **not** add detail in this release: forced alignment, notes/bass,
OCR and keyframes are separate milestones. Do not promise the user "more
detail" from `ultra`; promise "nothing silently skipped".

If the user did not say, pick `standard` for media under ~30 minutes and
`structure` for a first pass on very long files. Ask only when the cost is
material (multi-hour media, `ultra`). Details: `references/profiles.md`.

## 3. Run it

```bash
sonolytica doctor                          # ffmpeg + models + providers
sonolytica models list                     # what is installed and verified
sonolytica analyze FILE --out doc.json --profile standard --summary
sonolytica query runs doc.json             # do this before reading content
sonolytica query summary doc.json
```

`analyze` prints the document to stdout unless `--out` is given; `--summary`
goes to stderr; `--strict` (or `--profile ultra`) exits 2 when a stage failed or
a requested capability never ran. A missing model disables one capability,
never the whole run.

If a document already exists (the user points you at a `.json`), skip `doctor`
and `analyze` and start at `query runs`.

## 4. Read in this order

1. `query runs` — complete / partial / failed per stage.
2. `query summary` — media, counts, analyses, warnings.
3. `query ...` for the actual question.

If a stage is `partial` or `failed`, say so **before** answering, in one line.
Do not silently answer from half a document.

## 5. Query vocabulary

| Question | Command |
|---|---|
| what happened between 00:10 and 00:20 | `query timeline doc --from 00:00:10 --to 00:00:20` |
| what is active at 00:12.450 | `query at doc 00:00:12.450` |
| the whole transcript, untruncated | `query transcript doc` |
| every word of one utterance | `query words doc 4` |
| find a word, with its timestamp | `query search doc pricing` |
| who spoke most, how long | `query speakers doc` |
| what are the chapters | `query chapters doc` |
| tempo / key / chords / loudness | `query music doc` |
| silence and levels | `query acoustic doc` |
| which stages ran or failed | `query runs doc` |
| media facts and totals | `query summary doc` |

Rules of use:

- Times: `HH:MM:SS.mmm`, `MM:SS`, `12.5s`, `950ms`, or microseconds. A bare
  integer is **microseconds**; `12.5` is seconds.
- Add `--json` when you need exact values; read the table when you need a
  citation for a human.
- `--type` filters `timeline` to one segment type.
- Tables truncate long text at 80 characters with `…`; for anything you will
  quote, use `query transcript` or `query words` — they never truncate.
- `query chapters` splits on **structure** (silence, speaker change, scene cut),
  not on topics. Report it as "главы записи", never as "темы разговора".
- On a `search` row the speaker is derived from the refining turn; the raw
  `speech` segment itself has no speaker field.
- All commands are read-only; run as many as you need.
- Full reference with JSON shapes: `references/query-cheatsheet.md`.

## 6. Timestamps and ranges — canonical rules

- Quote ranges as `HH:MM:SS.mmm–HH:MM:SS.mmm` from `start_display`/`end_display`.
- Never round. "On what second?" gets the exact range, optionally followed by
  `(≈3.1 s)` if the user wants a rough figure.
- Which range supports which claim:

| Claim | Cite |
|---|---|
| an exact word | `words[].range` from `query search` |
| what was said | the `speech` segment range |
| who spoke, and when | the `speaker_turn` range |
| "who said X" | both, and name the two: they can differ by ASR padding (up to ~300 ms) |

- `speech_duration` in `analyses[speech]` is **measured speech** (VAD), not the
  sum of turns. For "how much did S1 speak" use `query speakers`. The two will
  not add up; never average them or present one as the other.
- `query speakers` reports one aggregate range per speaker (first turn start to
  last turn end). It is **not** continuous speech: get the individual turns from
  `query timeline --type speaker_turn`.
- The canonical text for a verbatim quote is the `speech` segment (`query
  transcript` / `query words`). `speaker_turn.text` is a re-tokenised copy made
  at merge time; use it for speaker attribution, not for quoting.
- `query at T` returning `(none)` means no segment covers that instant in the
  markup. It is **not** measured silence — for silence, ask `query acoustic`.
- `analyses[acoustic].silence` is level-based: a run below the threshold for at
  least `min_silence_ms`. It does **not** align with gaps between turns (a short
  gap between speakers is not reported) and can appear **inside** a speech
  segment (a pause in speech). Never use it to prove "nobody spoke" or to fill a
  gap between two turns.
- If a word-level list starts after the beginning of the text (the first word of
  a region is missing), quote the `speech.text` for the sentence and say the
  word timing starts at the first token present; never invent a range for the
  missing word.

## 7. Three evidence classes — never mix them

1. **Document fact** — returned by `query`. Cite the range.
2. **Observation** — something you saw in a frame or heard natively. Label it
   "наблюдение по кадру", not a quote, and do not attach a transcript range.
3. **Inference** — your conclusion. Mark it as such; never inside quotes.

## 8. Answer protocol

1. One line on stage status when not everything is `complete`.
2. The answer, with verbatim quotes and ranges.
3. What the document does **not** cover, when it matters.
4. Self-check before sending:
   - every quote appears character-for-character in a `query` result;
   - every timestamp is copied from the document, not computed from memory;
   - every speaker label is described as document-local;
   - every confidence is described as what it is.

## 9. When the word is not in the transcript

An empty `query search` is a fact about the transcript, not about the world.

Say: "В транскрипте нет слова X." Then optionally: "Возможно, ASR расслышал
его как Y (00:00:03.140)" — as a **hypothesis**, showing the actual token. Never
quote a word the document does not contain, and never answer "не говорили".

Also consider: ASR can drop the first word of a speech region (known limit), so
check the surrounding segment before concluding anything.

When the **user asserts** the word was said ("он точно говорил про pricing"),
keep the same shape and start by acknowledging the gap:

```
В транскрипте слова «pricing» нет — ни в одном сегменте. Возможно, ASR
расслышал его как «preaching» (00:00:03.140–00:00:03.550, confidence 0.996).
Документ не позволяет подтвердить, что там было «pricing»: это гипотеза,
а не цитата.
```

Do not argue about what the user heard. Report what the document contains and
offer the two honest routes: listen to the source, or re-run with a better ASR
model (`--model whisper-small`).

## 10. Speech rules

- Speaker labels (`S1`, `S2`) are local to one document; they are not people.
  Never attach a name unless the user supplied it and the document supports it.
- `overlap: true` means two people spoke at once. The document flags it but does
  not split the words between them.
- Confidence is a model probability for speech, an autocorrelation score for
  tempo, a template margin for key. Say "score", not "accuracy".
- `language` is reported by the model. If it contradicts the text, say the
  metadata is unreliable instead of using it as evidence.

## 11. Music rules

- BPM and key are heuristic scores, not calibrated probabilities.
- Chords are triads from a template, with no inversions or extensions.
- `beats` are beat positions, not bars; do not infer time signatures.
- Notes and bass do not exist in this release. Say the document has no
  note-level data; when note transcription lands it will be part of `ultra`.

## 12. Video rules

- `sonolytica inspect FILE` gives container and stream metadata only. It is a
  top-level command, not a `query` subcommand.
- If the document has no scene segments, video content was not analysed: say so
  and do not describe what is in the picture. A file with only an audio stream
  has no picture at all.
- Scenes, keyframes and OCR are not in this release. When they land they will
  arrive as `query scenes` / `query frames` / `query ocr`; this file will say so.

## 13. Failures and gaps

| Symptom | What to say | What to suggest |
|---|---|---|
| `warnings[].code = provider_unavailable` | that capability did not run | `sonolytica models fetch <ids>` |
| run `partial` | what is missing, from `reason` | re-run with the model installed |
| empty transcript | whether silence was measured | check `acoustic.silence`, then VAD/model |
| `doctor` says ffmpeg missing | nothing can be analysed | install FFmpeg |
| video "not analysed" | the document has no visual data | `--profile ultra` once video lands |

Troubleshooting detail: `references/troubleshooting.md`.

## 14. Workflows

Step-by-step command sequences per media type:

- `references/workflows/interview.md` — interview, podcast, 1:1
- `references/workflows/meeting.md` — meeting, call, standup
- `references/workflows/lecture.md` — lecture, talk, screencast
- `references/workflows/music.md` — track, rehearsal, DJ set
- `references/workflows/video.md` — video navigation and on-screen text

## 15. Limits — do not claim these

- No emotion, intent, topic modelling or abstractive summarisation. You may
  summarise what the document says, extractively, with citations.
- No speaker identity, age, gender or language proficiency.
- No overlap separation (flagged, not split).
- Word timings are Whisper token timestamps (~10 ms resolution, occasionally
  spanning punctuation) until forced alignment is available in `ultra`.
- No visual or musical-detail claims without the corresponding data.

## 16. Accuracy claims

Only from an actual run, naming dataset, subset, model and date:

```bash
sonolytica-eval wer --reference ref.txt --hypothesis doc.json
sonolytica-eval der --rttm reference.rttm --document doc.json --file-id NAME
sonolytica-eval report --document doc.json
```

Never quote a number you did not produce in this session.
