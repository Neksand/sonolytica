# Speech, music and video rules

Detail behind §10 of `SKILL.md`. Open only the section you need.

## Speech

- Speaker labels (`S1`, `S2`) are local to one document; they are not people.
  Never attach a name unless the user supplied it and the document supports it.
- Sortformer tracks at most four speakers. Above 10 minutes the document adds a
  `diarization_speaker_cap` warning: the count is approximate, so say "at least
  N", never an exact count.
- `overlap: true` means two people spoke at once. With Sortformer the document
  has two simultaneous `speaker_turn` segments; on the pyannote fallback it only
  flags the overlap. The transcript is still one ASR stream, so do not invent
  separate words for each speaker.
- Speaker-turn ranges are clamped to the diarized turn (plus 300 ms), while the
  transcript text stays complete. A word outside the range is still in
  `query words`; only the turn's time is clamped.
- Confidence is a model probability for speech, an autocorrelation score for
  tempo, a template margin for key. Say "score", not "accuracy".
- `language` is reported by the model. If it contradicts the text, say the
  metadata is unreliable instead of using it as evidence.
- On a single-voice lecture, a high speaker count is a diarizer failure: say so.
  Short files (< ~10 s) can produce one speaker only.

## Music

- BPM and key are heuristic scores, not calibrated probabilities.
- Chords are triads from a template, with no inversions or extensions.
- `beats` are beat positions, not bars; do not infer time signatures.
- `sections` are technical boundaries from chroma self-similarity: the material
  repeats or changes here. They are not verse/chorus labels; never name them.
- `music.sections` need schema 1.4; older documents simply have none.
- Notes and bass do not exist in this release. Say the document has no
  note-level data; when note transcription lands it will be part of `ultra`.

## Video and OCR

- `sonolytica inspect FILE` gives container and stream metadata only. It is a
  top-level command, not a `query` subcommand.
- The standard profile runs **shot detection** when the file has a video stream.
  `query timeline DOC --type scene` returns scene segments: `index` is the shot
  number, `cut` is true when the scene starts at a detected cut. `brief` lists
  the cut instants and one OCR line per keyframe.
- One keyframe per scene by default. `analyze --frame-stride N` adds a frame
  every N seconds (scene cuts stay first, `--max-frames` still caps the total);
  use it for long static slides or screencasts. Consecutive identical OCR lines
  are collapsed in `brief`; the full list is `timeline --type text_overlay`.
- `analyze --out doc.json` also writes **keyframe images** next to the document
  (`doc.json.artifacts/frames/`) and registers them in `artifacts`.
  `query frames DOC --from T --to T --max 8` returns absolute paths. If
  `--no-frames` was used or `--out` was missing, there are no images.
- **Open the images yourself** and answer visual questions from what you see.
  A visual statement is an observation, not a document fact: label it as such
  and cite the frame's `start_display`. Never attach a transcript range to it.
- **If you cannot see images** (a text-only model), say so. Never claim to have
  looked at a frame or "visually confirmed" OCR text. Cite `text_overlay` as
  OCR, not as sight.
- **On-screen text**: `analyze --ocr --out doc.json` runs OCR (PP-OCRv5) on the
  keyframes and adds `text_overlay` segments with exact text, bbox and
  confidence. `query timeline DOC --type text_overlay` lists them. That text is
  a document fact read at the frame's instant; it does **not** mean the text was
  on screen for the whole shot. If there are no `text_overlay` segments, OCR did
  not run — say so instead of guessing.
- A scene is a technical cut, a frame is a still at one instant. Neither is a
  topic, and a keyframe is not a summary of the shot.
- `analyze --objects` runs YOLOS-tiny and stores 91 COCO classes as `object`
  segments with a box and a score; `--faces` runs YuNet and stores class `face`.
  Both look only at sampled keyframes: no tracking, no attributes, **never
  identity or names**. A face or object between keyframes is missed; use
  `--frame-stride` for dense coverage. "Not detected" is not "not present".
