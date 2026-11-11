# Query cheatsheet

All commands read one document and never modify it. `--json` prints a typed
object on stdout; without it you get a plain table for humans.

```bash
DOC=doc.json
sonolytica query <command> "$DOC" [flags]
```

## Time input

| Form | Meaning |
|---|---|
| `00:00:12.450`, `00:00:12` | clock time |
| `02:03.500` | `MM:SS.mmm`; minutes may exceed 59 |
| `12.5s`, `950ms`, `1500us` | suffixed |
| `12.5` | seconds |
| `12450000` | **microseconds** (no dot, no suffix) |

Bad input exits 1 with a message on stderr. `--to` earlier than `--from` is an
error, not an empty result.

## brief

```bash
sonolytica query brief "$DOC"
sonolytica query brief "$DOC" --json
```

One compact call (typically 2–4 KB of JSON) for the first pass and for broad
questions. Nothing is inferred beyond counts:

- `media`: path, duration, sha256, `has_video`;
- `config`: modules, requested language, ASR model, `align_words`;
- `complete`, `runs` (capability/provider/status/duration), `warnings`;
- `speech`: words, language, speakers, VAD ratio and duration when the speech
  analysis block exists (otherwise counts are derived from segments and the
  ratio is `null`);
- `speakers`: id, speaking time, turns, first/last (up to 8; `speaker_count` is
  the true total);
- `speech.overlaps`: spans with two or more simultaneous turns, in time order
  (capped at 20), each with its speakers;
- `chapters`: up to 5 structural chapters (same rule as `query chapters`), with
  `chapter_count` as the true total;
- `video`: scene, keyframe and text-overlay counts, `scene_cuts` (start of each
  detected cut in microseconds, up to 50) and `ocr` (one line per keyframe
  instant, up to 10; the full list is `timeline --type text_overlay`);
- `music`: BPM, key, section count when the music block exists;
- `acoustic`: peak, LRA, silence count when the acoustic block exists.

JSON:

```json
{ "media": { "path": "…", "duration_us": 155600000, "duration_display": "00:02:35.600", "sha256": "…", "has_video": false },
  "config": { "modules": ["vad", "transcribe", "diarize"], "language": "en", "asr_model": "whisper-base.en", "align_words": false },
  "complete": true,
  "runs": [ { "capability": "vad", "provider": "silero-vad", "status": "complete", "duration_ms": 120 } ],
  "warnings": [],
  "speech": { "word_count": 402, "language": "en", "speaker_count": 2, "speech_ratio": 0.61, "speech_duration_us": 94900000, "overlaps": [ { "start_us": 1158470000, "end_us": 1159510000, "speakers": ["S1", "S2"] } ] },
  "speaker_count": 2,
  "speakers": [ { "id": "S1", "speech_duration_us": 52000000, "speech_display": "00:00:52.000", "turn_count": 12, "first_start_us": 1200000, "last_end_us": 149000000 } ],
  "chapter_count": 4,
  "chapters": [ { "start": 0, "end": 61200000, "start_display": "00:00:00.000", "end_display": "00:01:01.200", "title_hint": "…", "speakers": ["S1"], "segment_count": 9 } ],
  "video": { "scenes": 6, "keyframes": 6, "text_overlays": 12, "scene_cuts": [40000000, 80000000], "ocr": [ { "start_us": 41000000, "text": "PRICING PAGE — FRIDAY" } ] },
  "music": null,
  "acoustic": { "peak_dbfs": -1.2, "lra_lu": 6.3, "silence_count": 3 } }
```

`brief` is a map, not a quote source: use `search` / `words` / `transcript` for
exact wording and ranges. A `null` block means that stage did not run or had no
data — say so; do not fill it from the audio yourself.

Minimum commands per question — start at the top and add only what is missing:

| Question | Commands |
|---|---|
| what is this / who speaks / how is it structured | `brief` |
| who spoke most, how long, when | `brief` |
| exact quote or "find the moment" | `brief` + `search` + `words` (or `transcript`) |
| what was said in a window | `brief` + `timeline --from … --to …` |
| what is on screen | `brief` + `timeline --type text_overlay` |
| look at a frame | `brief` + `frames` |
| chords, beats, detailed loudness | `brief` + `music` / `acoustic` |
| full chapter or speaker list | `brief` + `chapters` / `speakers` |

## timeline

```bash
sonolytica query timeline "$DOC" --from 00:00:10 --to 00:00:20 --type speaker_turn
```

- Window is half-open `[from, to)`; default `from=0`, `to=media duration`.
- Ordered by start, then end, then id.
- `--type`: `speech`, `speaker_turn`, `music_label`, `acoustic_event`, `scene`,
  `text_overlay`, `object`. An unknown value is an error.
- Includes parent and child segments (an utterance and its speaker turn are two
  rows).

JSON:

```json
{ "count": 2, "segments": [ {
  "id": 1, "track": 1, "kind": "speaker_turn",
  "start": 6456000, "end": 8456000,
  "start_display": "00:00:06.456", "end_display": "00:00:08.456",
  "speaker": "S2", "text": "I am the second speaker!", "label": null,
  "confidence": 0.64
} ] }
```

`text` is null for non-text kinds; use `label` for music/acoustic/scene/object.

## at

```bash
sonolytica query at "$DOC" 00:00:07
```

Segments whose range contains the instant (`start <= t < end`), same shape as
`timeline`.

## search

```bash
sonolytica query search "$DOC" pricing
```

- Case-insensitive substring over `speech.text` and over `words[].text`.
- One row per utterance, never one per speech segment and turn.
- The speaker comes from the `speaker_turn` whose `parent` is that utterance.

JSON:

```json
{ "needle": "dollars", "count": 1, "matches": [ {
  "id": 0, "track": 0, "kind": "speech",
  "start": 0, "end": 4860000,
  "start_display": "00:00:00.000", "end_display": "00:00:04.860",
  "speaker": "S1",
  "text": "The quick brown fox … ten dollars.",
  "label": null, "confidence": 0.77,
  "words": [ { "text": "dollars .", "start": 4230000, "end": 4720000,
               "range_display": "00:00:04.230–00:00:04.720",
               "confidence": 0.84 } ]
} ] }
```

`words` contains only the words that matched. A word may carry punctuation
(`"dollars ."`) because Whisper tokenises punctuation separately or attaches it.

Empty result in human mode prints `(none)`; in JSON it is `"count": 0`.

The `speaker` on a match is derived from the turn that refines the utterance;
in the raw document the `speech` segment itself has no speaker field.

## transcript

```bash
sonolytica query transcript "$DOC"
```

Every `speech` segment, in time order, full text (never truncated), with the
derived speaker. This is the command for "give me the transcript".
`--from`/`--to` keep only utterances intersecting the window, so a two-hour
recording can be read in chunks.

```json
{ "count": 3, "segments": [ {
  "id": 0, "track": 0, "kind": "speech",
  "start": 0, "end": 4860000,
  "start_display": "00:00:00.000", "end_display": "00:00:04.860",
  "speaker": "S1",
  "text": "The quick brown fox humps over the lazy dog, our preaching starts at ten dollars.",
  "label": null, "confidence": 0.77
} ] }
```

## words

```bash
sonolytica query words "$DOC" 0
```

Every word of one `speech` segment with its own range:

```json
{ "segment": 0, "speaker": "S1", "text": "…", "start": 0, "end": 4860000,
  "start_display": "00:00:00.000", "end_display": "00:00:04.860",
  "words": [ { "text": "dollars .", "start": 4230000, "end": 4720000,
               "range_display": "00:00:04.230–00:00:04.720",
               "confidence": 0.84 } ] }
```

A `speaker_turn` has no words of its own: it exits 1 and names the parent
segment to ask for instead.

## speakers

```bash
sonolytica query speakers "$DOC"
```

Aggregated from `speaker_turn` segments, longest speaking time first.

JSON:

```json
{ "speaker_count": 2, "speakers": [ {
  "id": "S2", "speech_duration_us": 8310000, "speech_display": "00:00:08.310",
  "turn_count": 2, "first_start_us": 5656000, "last_end_us": 14334000
} ] }
```

This is the authority for "how much did each person speak". Do not use
`analyses[speech].speech_duration` for that — it is measured VAD speech for the
whole file and includes silence inside turns.

The range (`first_start_us`–`last_end_us`) is the extent of a speaker's turns,
not continuous speech; a speaker with a pause in the middle has a wider range
than their speaking time. For individual turns use `query timeline --type
speaker_turn`.

## chapters
```bash
sonolytica query chapters "$DOC" --min-gap 1.2s
```

Boundaries from three cheap signals: silence longer than `--min-gap`, a change
of speaker, a scene cut. Boundaries closer than 5 s are one moment; chapters
shorter than 2 s merge into the previous one.

JSON:

```json
{ "count": 2, "chapters": [ {
  "start": 0, "end": 5656000,
  "start_display": "00:00:00.000", "end_display": "00:00:05.656",
  "title_hint": "The quick brown fox hum ps over the",
  "speakers": ["S1"], "segment_count": 2
} ] }
```

`title_hint` is the first eight words of the first utterance. It is a hint,
not a generated title; never present it as one.

`segment_count` counts every segment intersecting the chapter, including a
speech segment and its speaker turn — so it is larger than the number of
utterances. Chapters follow structure (silence/speaker/cut), not topics.

## frames

```bash
sonolytica query frames "$DOC" --from 00:12:00 --to 00:13:00 --max 8
```

Keyframe images registered by `analyze --out`. Paths are absolute (resolved
relative to the document) so you can open them directly.

```json
{ "count": 2, "frames": [ {
  "start": 1000000, "start_display": "00:00:01.000",
  "path": "/abs/doc.json.artifacts/frames/frame_0000.jpg",
  "sha256": "85c031…", "width": 1280, "height": null
} ] }
```

Half-open window: a frame exactly at `--from` is included, one exactly at
`--to` is not. No frames means `analyze` ran with `--no-frames`, without
`--out`, or the file had no scene segments.

## text_overlay

`analyze --ocr --out doc.json` runs PP-OCRv5 on the keyframes. Each recognised
line becomes a `text_overlay` segment:

```json
{ "id": 7, "track": 3, "kind": { "type": "text_overlay",
  "text": "NEXT STEP: HIRE TWO ENGINEERS",
  "bbox": { "x": 210.0, "y": 330.0, "width": 860.0, "height": 70.0 } },
  "range": { "start": 8000000, "end": 8000000 }, "confidence": 0.987 }
```

The range is a point at the frame's instant: the text was read from that frame,
not observed for the whole scene. Query it with `query timeline DOC --type
text_overlay`; `query search` covers speech only.

## music

```bash
sonolytica query music "$DOC" --json
```

```json
{ "available": true,
  "bpm": { "value": 98.0, "confidence": 0.31 },
  "key": { "tonic": "f_sharp", "mode": "minor", "confidence": 0.12 },
  "chords": [ { "range": {"start":0,"end":2000000}, "root": "f_sharp",
                "quality": "minor", "confidence": 0.71 } ],
  "sections": [ { "range": {"start":0,"end":9920000}, "novelty": 0.30 } ],
  "loudness": { "integrated_lufs": -17.6, "loudness_range_lu": 13.9,
                "true_peak_dbtp": -1.0 },
  "beat_count": 33 }
```

`available: false` means the music stage did not run. `confidence` is a score,
not a calibrated probability. Chords are triads only. `sections` are technical
boundaries from chroma self-similarity — the material changes here, nothing
about verse/chorus.

## acoustic

```bash
sonolytica query acoustic "$DOC" --json
```

```json
{ "available": true,
  "silence": [ {"start": 8060000, "end": 8960000} ],
  "dynamic_range_lu": 14.1, "mean_rms_dbfs": -19.0, "peak_dbfs": -0.3,
  "event_count": 0 }
```

`silence` is measured silence: below the configured level for at least the
configured minimum. It does not match gaps between speaker turns, and it can
occur inside a `speech` segment (a pause in speech). Do not present it as
"nobody spoke" and do not use it to fill a gap between two turns.

## runs

```bash
sonolytica query runs "$DOC"
```

JSON: `{ "complete": bool, "runs": [...], "warnings": [...] }`.

A run is:

```json
{ "id": 1, "capability": "transcribe", "provider": "whisper-cpp",
  "model_id": "whisper-base.en", "device": "metal", "duration_ms": 661,
  "status": "partial", "reason": "no speech recognised in region 2" }
```

`status` is `complete`, `partial` or `failed`; `reason` is null unless partial
or failed. A requested capability with no provider produces **no run** and a
warning instead — check both.

## summary

```bash
sonolytica query summary "$DOC" --json
```

```json
{ "media": { "path": "…", "sha256": "…", "duration": 15212938, "streams": [...] },
  "config": { "modules": [...], "language": "en", "asr_model": "whisper-base.en" },
  "counts": { "tracks": 2, "segments": 6, "relations": 0 },
  "analyses": [...], "runs": [...], "warnings": [...] }
```

## Exit codes

| Code | Meaning |
|---|---|
| 0 | query answered (an empty result is still 0) |
| 1 | bad input: unreadable file, invalid time, unknown type, schema mismatch |
| 2 | only `analyze --strict` / `--profile ultra`: a requested capability failed or did not run |
