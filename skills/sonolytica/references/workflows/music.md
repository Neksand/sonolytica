# Workflow: music

Goal: describe the track from measurements, not from genre intuition.

## Commands

```bash
sonolytica analyze track.wav --out track.json --profile standard --summary
sonolytica query music track.json --json
sonolytica query acoustic track.json --json
sonolytica query runs track.json
```

`structure` is enough when there is no speech and no lyrics question: it skips
ASR and still runs the music and acoustic stages.

## Reading music output

| Field | What it is | What it is not |
|---|---|---|
| `bpm.value` | tempo estimate from onset autocorrelation | not a beat grid for editing |
| `bpm.confidence` | autocorrelation strength | not a probability |
| `key.tonic` / `key.mode` | Krumhansl profile match | not a guarantee for ambiguous material |
| `key.confidence` | profile margin | a low score means "ambiguous", say that |
| `chords[]` | triad per range, template-based | no sevenths, no inversions, no bass |
| `beats[]` | beat positions, µs | not bars, not downbeats |
| `sections[]` | technical boundaries from chroma self-similarity | not verse/chorus labels |
| `loudness.integrated_lufs` | EBU R128 integrated | not loudness of a section |
| `loudness.loudness_range_lu` | LRA | needs enough programme material |
| `silence[]` | measured digital silence | not "no music" |

## Answer shape

```
Темп: 136.3 BPM (score 0.31)
Тональность: F# minor (score 0.12 — низкая уверенность)
Громкость: −17.6 LUFS integrated, LRA 14.1 LU, true peak −1.0 dBTP
Аккорды (трезвучия): 1 фрагмент; бит: 33 позиции
Секции: 2 смены (00:00:09.920, 00:00:19.840) — технические границы, не «припев»
Что не измерялось: ноты, бас, инструменты
```

## Rules

- Never claim a genre, mood, instrument or artist. The document has none.
- Do not infer time signature from BPM.
- A low key confidence is information: report it.
- Speech and music share the timeline; if the file is a song with vocals, the
  transcript is lyrics-as-heard, not lyrics-as-written. Say which.
- Notes and bass require `ultra` and a note model; until then state that the
  document has no note-level data. When they exist, quote note ranges and
  pitches, and report F1-style uncertainty only if the document carries it.

## Mixing with speech

For a podcast with a music intro, `query timeline --type music_label` is empty
until a music labelling stage exists: music data lives in `analyses`, not in
segments. Do not invent a "music starts at" timestamp from BPM data.
