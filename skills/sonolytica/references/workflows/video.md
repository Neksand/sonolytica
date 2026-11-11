# Workflow: video

Video has two layers: the audio pipeline (already works) and the visual layer
(scenes, keyframes, on-screen text). Until the visual layer ships, the document
describes sound only — say that plainly.

## Check what exists

```bash
sonolytica inspect video.mp4 --json
sonolytica analyze video.mp4 --out video.json --profile standard --summary
sonolytica query summary video.json --json
```

Look for `counts` and for `scene` segments:

```bash
sonolytica query timeline video.json --type scene
```

Empty result → no visual analysis. Then:

> «Видео не анализировалось: в документе нет сцен и кадров. Доступен только
> аудиоряд — что говорят, кто и когда.»

## When scenes and keyframes exist

The visual layer is not in this release: no document contains scenes or frames
yet, and `query scenes`/`query frames` do not exist yet. When it ships, the
commands will be:

```bash
sonolytica query scenes video.json
sonolytica query frames video.json --from 00:12:00 --to 00:13:00 --max 8
```

Today the correct answer to any visual question is: «видео не анализировалось».

Open the returned image paths yourself, then answer. Rules:

- Every visual statement is an **observation**, never a document fact.
- Cite the frame's `start_display` as the moment, and say it is a frame time.
- Do not describe motion from a still frame; do not claim what is off-screen.
- If two keyframes look identical, the document already deduplicated them;
  do not re-describe the same frame twice.
- OCR text (`text_overlay` segments) is exact and may be cited as a document
  fact, with its range and bbox.

## Budget

Ask for the window you need (`--from/--to`) and a small `--max`. Never request
all frames of a long video: you pay context for every image and most of them
repeat.

## Audio + video together

The unique value is the alignment: a scene change inside a sentence, a slide
shown while someone says the number. Answer "что было на экране, когда он
сказал X" only when both a frame and the quote exist; otherwise say which half
you are missing.
