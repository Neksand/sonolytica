# Workflow: video

Three layers on one timeline: **audio** (speech, speakers), **shots** (technical
cuts) and **keyframes** (still images). There is no OCR, object or face
analysis; what you can say about the picture comes from the images you open.

## Commands

```bash
sonolytica inspect video.mp4 --json                 # streams, resolution, fps
sonolytica analyze video.mp4 --out video.json --profile standard --summary
sonolytica query runs video.json                    # is there a `scene` run?
sonolytica query timeline video.json --type scene   # cuts as scene segments
sonolytica query frames video.json --from 00:05:00 --to 00:06:00 --max 8
```

`query frames` returns absolute paths. Open those files with your own vision;
they are 1280 px wide JPEGs. If it returns `(none)`, the analysis was run with
`--no-frames`, without `--out`, or the file has no video track.

## Speaking about the picture

- Every visual claim is an **observation**, labelled as such, with the frame's
  `start_display` as its time. It is not a document fact and has no transcript
  range.
- One frame is one instant. Do not describe motion, duration or what happens
  between frames from a single still.
- Do not identify people, guess names, ages or intent.
- If two keyframes look identical, say they are visually indistinguishable
  rather than inventing a difference.
- A scene index is not a topic. "Scene 3 shows the product" is only allowed if
  you opened the frame and are describing it as an observation.

## Answer shapes

- "Сколько склеек?" → count scenes with `cut: true`.
- "Где меняется кадр?" → `start_display` of each cut.
- "Что на экране в 00:05?" → `query frames --from 00:05:00 --to 00:05:01`,
  open the image, describe it as an observation.
- "Что говорят в третьей сцене?" → intersect the scene range with
  `query transcript DOC --from ... --to ...`; quote speech ranges, not the
  scene range.
- "Что написано на слайде?" → without OCR you can only read what is legible in
  the image yourself; state that it is a visual reading, not extracted text.

## Audio + video together

The unique value is alignment: a cut inside a sentence, a shot change while
someone says a number. State both facts with their own ranges and do not imply
causation.
