# Workflow: video

A video document has two layers on one timeline: audio (speech, speakers,
music) and **shots** (technical cuts). There is no visual understanding in this
release: no objects, no faces, no keyframes, no on-screen text.

## What the document tells you

```bash
sonolytica inspect video.mp4 --json                    # streams, resolution, fps
sonolytica analyze video.mp4 --out video.json --profile standard --summary
sonolytica query runs video.json                       # is there a `scene` run?
sonolytica query timeline video.json --type scene      # shots
```

Each scene segment carries:

- `range` — the shot on the media timeline;
- `index` — shot number from 0;
- `cut: true` — the scene starts at a detected cut (`false` for the first).

Cuts come from 2 fps grayscale frame signatures: a spike in perceptual hash and
histogram distance above an adaptive threshold. Precision is one sample,
±0.5 s. They are **edits**, not topics and not chapters.

## Answer shape

- "Сколько склеек?" → count scenes with `cut: true`.
- "Где меняется кадр?" → the `start_display` of each such scene.
- "Что на экране / какие слайды / что написано?" → **not analysed**. Say so and
  offer the audio answer if the question also has a speech part.
- "Что говорят в третьей сцене?" → intersect the scene range with
  `query transcript DOC --from ... --to ...`; quote with the speech ranges, not
  the scene range.

## Rules

- Never describe pixels, objects, actions or people. The document has none.
- A scene index is not a chapter number. Do not map it to a topic.
- Do not claim the video is silent or has no picture from the audio pipeline
  alone; `inspect` answers that.
- If `query runs` has no `scene` run for a file with a video stream, the stage
  did not run (the profile did not include it, or it failed — check warnings).

## Audio + video together

The unique value is alignment: a cut inside a sentence, a shot change while
someone says a number. You may state both facts with their own ranges — the cut
from the scene segment, the words from the speech segment — and must not imply
the cut caused the words.
