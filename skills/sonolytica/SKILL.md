---
name: sonolytica
description: Use SONOLYTICA to analyze audio or video and answer questions about what was said, who spoke, what music or sounds are present, and when things happened. Use for transcription, speaker diarization, music (BPM/key/chords), loudness, silence, and timeline questions about media files.
---

# SONOLYTICA

SONOLYTICA turns a media file into a typed, timestamped JSON document. Read the
document; do not re-derive facts from audio or invent quotes.

## When to use

- User asks what is said in an audio/video file, or who said it.
- User asks when something happened ("at what point did she mention pricing?").
- User asks about music: tempo, key, chords, loudness, silence, structure.
- User wants a machine-readable transcript with timestamps.

## Prerequisites

```bash
sonolytica doctor        # shows FFmpeg, model directory, available providers
```

If a capability is missing, fetch the pinned models (verified by SHA-256):

```bash
sonolytica models fetch silero-vad-v5 whisper-base.en \
  pyannote-segmentation-3.0 wespeaker-campplus
```

`whisper-base.en` is English-only. For other languages use `whisper-small` and
pass `--language`.

## Analyze

```bash
sonolytica analyze FILE --out doc.json --summary \
  --modules vad,transcribe,diarize,music,acoustic \
  --language en --model whisper-base.en
```

- Omit `--modules` to run every available audio capability.
- `--summary` prints a human digest to stderr; JSON is the contract.
- `--strict` exits 2 if any stage failed (useful in automation).
- Long files: raise `--modules` selectively; DSP-only is fast.

## Read the document

Always check these first:

1. `schema_version` — must be `1.0.0` for this skill.
2. `runs[]` — per-stage status: `complete`, `partial`, `failed`.
3. `warnings[]` — what was skipped and why (e.g. model unavailable).
4. `analyses[]` — speech summary, music, acoustic.

Then answer from `segments[]`:

- `kind.type == "speech"` — ASR utterance with `words[]` (text + range +
  confidence). Use for quoting.
- `kind.type == "speaker_turn"` — speaker-attributed turn; `parent` points to
  the speech segment. `text` may be empty (speaker activity without words).
- `relations[]` — `overlaps`, `interrupts`, etc. between segment ids.
- `analyses[type=speech]` — language, speaker_count, speech_ratio, word_count.
- `analyses[type=music]` — bpm, beats, key, chords, loudness.
- `analyses[type=acoustic]` — silence regions, levels, dynamic range.

## Timestamps

All times are **integer microseconds** from the start of the container, and
ranges are half-open. Convert for humans:

```
12_450_000 µs -> 00:00:12.450
```

Never output a timestamp that is not literally present in `segments[].range` or
`words[].range`. Quote at word precision when the user asks "when exactly".

## Rules for answering

- Quote transcribed text verbatim from `speech.text` / `words[].text`. Do not
  paraphrase inside quotation marks; summarise outside them.
- Cite the range: "…(00:00:07.081–00:00:08.718)".
- If `runs[].status == "partial"` or a warning says a model was unavailable,
  say so explicitly: e.g. "speakers were not separated; diarization model not
  installed".
- If `analyses[type=speech].speaker_count == 0`, do not attribute speech to
  anyone.
- `confidence` is optional. For tempo/key it is a heuristic score, not a
  calibrated probability; do not present it as certainty.
- Speaker labels (`S1`, `S2`) are document-local. Never claim they identify a
  real person.
- Silence in `analyses[type=acoustic].silence` is measured silence, not "no
  transcription". Say which one you mean.

## Useful shell queries

```bash
# dialogue with timestamps
jq -r '.segments[] | select(.kind.type=="speaker_turn" and .kind.text!="")
       | "\(.range.start/1000000) \(.kind.speaker) \(.kind.text)"' doc.json

# everything in a window
jq '.segments[] | select(.range.start < 30000000 and .range.end > 12000000)' doc.json

# word-level timing
jq '.segments[] | select(.kind.type=="speech") | .kind.words[]
    | select(.text=="pricing") | .range' doc.json
```

## Limits you must respect

- No emotion, intent, topic segmentation or summarisation in this release.
  Do not infer them from the document.
- Diarization reports overlap but does not split overlapping speech between two
  speakers.
- Words at ASR region boundaries can be missing; check the surrounding segment
  before claiming a word was never said.
- Video scene/vision analysis is not implemented yet; `inspect` still reports
  container and stream metadata.

## Evaluation (when asked about accuracy)

```bash
sonolytica-eval wer --reference ref.txt --hypothesis doc.json
sonolytica-eval der --rttm reference.rttm --document doc.json --file-id NAME
sonolytica-eval report --document doc.json
```

Report numbers only from an actual run, with the dataset and conditions named.
