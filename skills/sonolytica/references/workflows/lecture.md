# Workflow: lecture / talk / screencast

Goal: a navigable outline with quotes, not a full retelling.

## Commands, in order

```bash
sonolytica analyze lecture.mp4 --out lecture.json --profile standard --summary
sonolytica query brief lecture.json          # status, speakers, chapters, counts
sonolytica query chapters lecture.json --min-gap 3s   # longer pause = new chapter
```

Then, per question:

```bash
sonolytica query timeline lecture.json --from 00:18:00 --to 00:22:00
sonolytica query search lecture.json "gradient descent"
```

For video lectures, shot boundaries exist (technical cuts); on-screen text,
slides and frames do not:

```bash
sonolytica query timeline lecture.json --type scene    # cuts only
```

## Answer shape

1. **План лекции**: главы с диапазонами (главы строятся по паузам и сменам
   спикера, поэтому это структура записи, а не конспект).
2. **Ответ по теме**: цитаты; если тема не названа дословно, найди ближайший
   фрагмент через `search` и скажи, чем он отличается от вопроса.
3. **Что не покрыто**: слайды, формулы, схемы — если OCR/кадров нет.

## Rules

- A lecture has one speaker: if diarization reports 3+, that is usually a
  failure of the diarizer on a single voice. Say the speaker count looks wrong
  instead of inventing participants.
- Do not turn `title_hint` into a lecture outline heading in quotes; it is the
  first words of a segment.
- Long pauses > 3 s are a better chapter signal for lectures than speaker
  changes; use `--min-gap 3s`.
- If the user asks "объясни, что он сказал про X", the answer is a summary of
  quotes, not a paraphrase presented as the speaker's words.

## Screencasts

- Until OCR lands, the document describes only the audio track. Say that
  explicitly when the question is about what is on screen.
- `sonolytica inspect FILE --json` gives container, streams, resolution and
  frame rate — useful to confirm the file has video at all.
