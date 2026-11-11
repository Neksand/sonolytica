# Document schema (v1.3.0)

One analysis produces one `Document`. It is designed for machine consumption:
typed fields, stable names, explicit units, and no placeholders standing in for
unknown values.

## Conventions

| Concept | Representation |
|---|---|
| Timeline | `MediaTime` = integer **microseconds** from the start of the container |
| Interval | `TimeRange { start, end }`, half-open `[start, end)` |
| Modality | `Track.kind`: `speech`, `speaker_turn`, `music`, `acoustic`, `scene`, `text`, `object`, `face` |
| Identity | `Track.id` and `Segment.id` equal their index in `tracks` / `segments` |
| Hierarchy | `Segment.parent` points into another track (refinement, not grouping) |
| Cross-modal facts | `relations[]` with typed `kind` |
| Confidence | `0.0..=1.0`, finite, optional. Absent means "not estimated" |
| Provenance | every segment points at a `RunRecord` |
| Attachments | `artifacts[]` are files beside the document; a still uses a point range |

`MediaInfo.audio_duration()` (audio stream) and `MediaInfo.duration`
(container) are the authority for clamping. No timestamp may exceed them; the
validator enforces this (`segment_bounds`, `word_bounds`).

## Top-level object

```jsonc
{
  "schema_version": "1.0.0",
  "generator": { "name": "sonolytica", "version": "0.1.0" },
  "created_unix_ms": 1794345592518,

  "media": {
    "path": "/abs/path/interview.m4a",
    "sha256": "89e19f…",          // present unless hashing was disabled
    "size_bytes": 12345678,
    "container": "mp4",
    "duration": 15212938,         // µs
    "bit_rate": 128000,
    "streams": [
      {
        "index": 0, "kind": "audio", "codec": "aac",
        "sample_rate": 44100, "channels": 2,
        "width": null, "height": null, "frame_rate": null,
        "language": "en", "duration": 15212938, "bit_rate": 128000
      }
    ]
  },

  "config": {
    "modules": ["acoustic", "diarize", "music", "transcribe", "vad"],
    "language": "en",
    "translate_to_english": false,
    "asr_model": "whisper-base.en",
    "vad": { "threshold": 0.5, "min_speech_ms": 250, "min_silence_ms": 500, "pad_ms": 100 },
    "diarization": { "threshold": 0.45, "max_speakers": 12, "min_region_ms": 500 },
    "dsp": { "fft_size": 2048, "hop_size": 512, "silence_dbfs": -45.0,
             "min_silence_ms": 500, "detect_tempo": true },
    "limits": { "max_duration_secs": 7200, "max_file_bytes": 8589934592, "timeout_secs": 600 },
    "hash_media": true,
    "align_words": false
  },

  "runs": [
    {
      "id": 0,
      "capability": "transcribe",
      "provider": "whisper-cpp",
      "model": { "id": "whisper-base.en", "version": null,
                 "sha256": "a03779…", "license": "MIT" },
      "params_sha256": "0dd9755f…",   // SHA-256 of the canonical config JSON
      "device": "metal",
      "started_at_unix_ms": 1794345592518,
      "duration_ms": 210,
      "status": { "status": "complete" }   // or {"status":"partial","reason":…}
    }
  ],

  "tracks": [
    { "id": 0, "kind": "speech",       "label": "asr",            "run": 1 },
    { "id": 1, "kind": "speaker_turn", "label": "speaker_turns",  "run": 2 }
  ],

  "segments": [
    {
      "id": 0, "track": 0, "parent": null,
      "range": { "start": 0, "end": 6000000 },
      "kind": {
        "type": "speech",
        "text": "…",
        "language": "en",
        "no_speech_prob": 0.0,
        "words": [
          { "text": "The", "range": { "start": 0, "end": 330000 }, "confidence": 0.668 }
        ]
      },
      "confidence": 0.679,
      "run": 1
    },
    {
      "id": 1, "track": 1, "parent": 0,      // refinement of the utterance
      "range": { "start": 0, "end": 6000000 },
      "kind": { "type": "speaker_turn", "speaker": "S1", "text": "…", "overlap": false },
      "confidence": 0.96,
      "run": 2
    }
  ],

  "relations": [
    { "from": 3, "to": 4, "kind": "overlaps",    "confidence": null },
    { "from": 3, "to": 4, "kind": "interrupts",  "confidence": null }
  ],

  "analyses": [
    { "type": "speech", "language": "en", "speaker_count": 2,
      "speech_duration": 13244937, "speech_ratio": 0.87, "word_count": 31 },
    { "type": "music",
      "bpm": { "value": 98.0, "confidence": 0.31 },
      "beats": [0, 612000, 1224000],
      "key": { "tonic": "FSharp", "mode": "Minor", "confidence": 0.12 },
      "chords": [
        { "range": { "start": 0, "end": 2000000 },
          "root": "FSharp", "quality": "Minor", "confidence": 0.71 }
      ],
      "loudness": { "integrated_lufs": -17.6, "loudness_range_lu": 13.9, "true_peak_dbtp": 0.0 }
    },
    { "type": "acoustic",
      "silence": [ { "start": 0, "end": 500000 } ],
      "dynamic_range_lu": 13.9,
      "mean_rms_dbfs": -24.1,
      "peak_dbfs": 0.0,
      "events": []
    }
  ],

  "warnings": [
    { "code": "provider_unavailable",
      "message": "pyannote-3.0 (diarize) is unavailable: …",
      "run": null }
  ],

  "artifacts": [
    { "kind": "keyframe",
      "path": "interview.json.artifacts/frames/frame_0000.jpg",
      "range": { "start": 1000000, "end": 1000000 },
      "sha256": "85c031…", "width": 1280, "height": null }
  ]
}
```

Artifact paths are relative to the document file, so the JSON and the images
can be moved together. A still image carries a point range (`start == end`).

## Segment kinds

| `kind.type` | Payload |
|---|---|
| `speech` | `text`, `words[]`, `language`, `no_speech_prob` |
| `speaker_turn` | `speaker` (opaque `S1..`), `text` (may be empty), `overlap` |
| `music_label` | `label` |
| `acoustic_event` | `class` |
| `scene` | `index`, `cut` |
| `text_overlay` | `text`, `bbox` — an OCR line from one keyframe |
| `object` | `class`, `bbox?` |

`scene` segments come from shot detection (2 fps frame signatures, adaptive
threshold); `cut: true` marks a detected edit, the first scene has `cut: false`.
They are technical boundaries, not topics. Object, face and on-screen-text
analysis is not implemented yet.

`speaker` labels are stable within a document, not across recordings. There is
no claim that `S1` in two files is the same person.

## Relation kinds

`overlaps`, `interrupts`, `contains`, `aligns_with`, `occurs_during`,
`same_speaker_as`, `accompanies`.

Relations are directed. Two turns of different speakers that intersect get an
`overlaps` edge; the later turn also gets `interrupts` if it begins before the
earlier turn ends and continues past it.

## Status semantics

| Status | Meaning |
|---|---|
| `complete` | The stage ran and produced its requested output. |
| `partial` | Ran, produced some output, missing something (e.g. "no speech recognised"). |
| `failed` | Did not run or errored; see `warnings` with the same `run` id. |

A capability with no provider does **not** create a failed run: it appears in
`warnings` only. Consumers should check both `runs` and `warnings`.

## Invariants enforced by `Document::validate`

| Code | Rule |
|---|---|
| `schema_version` | Major version matches this build |
| `segment_id`, `track_id`, `run_id` | ids equal their array position |
| `track_ref`, `run_ref`, `parent_ref` | references resolve |
| `parent_contains_child` | a parent's range contains the child's |
| `parent_same_track` | hierarchy crosses tracks (never a self-grouping) |
| `parent_cycle` | no segment is its own parent |
| `relation_ref`, `relation_self` | edges resolve and are not self-loops |
| `confidence_range` | confidence is finite and within `[0,1]` |
| `segment_bounds`, `word_bounds` | timestamps never exceed the media duration |
| `range_order`, `word_order` | end is not before start |
| `artifact_path`, `artifact_bounds` | an artifact has a path and lies within the media |
| `params_hash` | every run has a 64-hex parameter digest |

`sonolytica analyze` refuses to emit a document that violates any of these.

## Versioning

- Adding an optional field or a new `AnalysesBlock` variant: minor version.
- Renaming/removing a field or changing meaning: major version, with a
  migration in `Document::from_json`.
- `serde` defaults are used where a missing field has an unambiguous meaning;
  there are no silent reinterpretations.
- `1.1.0` adds `artifacts[]`; `1.2.0` adds `text_overlay` segments and the
  `text` track kind; `1.3.0` adds the `align` capability and `config.align_words`.
  Every older document stays readable.

## Query commands

`sonolytica query` reads a document and answers one question per invocation.
Nothing is mutated and no model is loaded.

```bash
# Everything that happens between 00:12 and 00:30 (half-open window)
sonolytica query timeline doc.json --from 00:00:12 --to 00:00:30

# Full transcript, untruncated, one line per utterance
sonolytica query transcript doc.json

# One utterance, word by word
sonolytica query words doc.json 0

# One modality only
sonolytica query timeline doc.json --type speaker_turn

# What is active at an instant
sonolytica query at doc.json 00:00:12.450

# Find a word, with its range and speaker
sonolytica query search doc.json pricing

# Speaking time per speaker, longest first
sonolytica query speakers doc.json --json

# Chapters from silences, speaker changes and scene cuts
sonolytica query chapters doc.json

# Tempo, key, chords, loudness; silence and levels
sonolytica query music doc.json --json
sonolytica query acoustic doc.json --json

# Which stages failed, and why
sonolytica query runs doc.json
```

`--type` accepts `speech`, `speaker_turn`, `music_label`, `acoustic_event`,
`scene`, `object`.

Chapters are derived, not stored: `query chapters` boundaries on silence longer
than `--min-gap` (default 1.2 s), a change of speaker, or a scene cut. The
`title_hint` is the first eight words of the chapter's first utterance — a hint,
never a generated title.

### Time input

| Form | Meaning |
|---|---|
| `00:00:12.450`, `00:00:12` | clock time |
| `02:03.500` | `MM:SS.mmm`, minutes may exceed 59 |
| `12.5s`, `950ms` | suffixed, fractional allowed |
| `12.5` | seconds |
| `12450000` | **microseconds** — the document's unit |

In JSON output every range is present twice: `start`/`end` in microseconds and
`start_display`/`end_display` as `HH:MM:SS.mmm`, so a citation needs no
arithmetic.

The same data is still available as raw JSON with `jq`; `query` exists because
an agent should not have to reconstruct the document's structure to answer a
question.
