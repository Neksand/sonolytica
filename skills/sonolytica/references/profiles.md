# Profiles

A profile is one decision: how much analysis to pay for. It expands to a set of
capabilities and, for `ultra`, an exit-code contract.

| | `structure` | `standard` | `ultra` |
|---|---|---|---|
| VAD (speech regions) | yes | yes | yes |
| Speaker diarization | yes | yes | yes |
| Music / acoustic aggregates | yes | yes | yes |
| Shot boundaries (video) | yes | yes | yes |
| Speech recognition (words) | **no** | yes | yes |
| Forced alignment (word-precise) | – | – | English |
| Music notes / bass | – | – | later |
| OCR / keyframes (video) | – | – | later |
| Exit 2 when a requested capability failed or did not run | no | no | **yes** |
| Relative cost on the 15 s fixture | ~4.7 s | ~8 s | ~8 s |

`standard` is the default. Costs above are one machine and one 15 s file, not a
benchmark — use them only to choose, not to promise a duration.

## When to use which

**structure** — you need orientation, not words:
- "что это за запись, сколько людей говорит, где паузы";
- a first pass on a multi-hour recording before paying for ASR;
- music-only material where the transcript is irrelevant;
- privacy: no speech leaves the audio buffer.

**standard** — anything about content: quotes, search, who said what, chapters,
summaries with citations.

**ultra** — when a silently degraded document is unacceptable:
- you asked for diarization and the model is not installed;
- a stage failed and you want the run to fail instead of returning half a
  document;
- automation where exit code 2 must trigger a retry with models installed.

Today `ultra` adds forced word alignment (English) and the no-silent-
degradation guarantee; OCR and keyframes are already in `standard`. Note
transcription is still a separate milestone. Do not promise note-level detail
yet.

## What the profile does to the document

The document records the effective capability list in `config.modules`. Two
documents analysed with different profiles are not directly comparable.
`config.asr_model`, `config.language` and the DSP/VAD/diarization parameters are
also part of the provenance hash (`runs[].params_sha256`).

## Choosing without asking

- Media under ~30 minutes, question about content → `standard`.
- Media over ~2 hours, question about structure → `structure`, then re-run
  `standard` on the interesting window only if the tool can cut it (it cannot
  yet: re-run on the file).
- Music file, question about tempo/key/chords → `standard` or `structure`; the
  music stage does not need ASR.
- English speech → the default `whisper-base.en`; other languages →
  `whisper-small` **with** `--language xx`. Auto-detection on short or clean
  audio can mislabel English and ruin the transcript.
- The user says "полный транскрипт", "дословно", "для монтажа" → `standard`
  today; say that word-precise alignment is not available yet if they need
  sub-second cutting decisions.
- Any automation where a missing model must not pass silently → `ultra`.

## Changing the preset

`--profile` and `--modules` are mutually exclusive. To run an unusual
combination, name the modules explicitly:

```bash
sonolytica analyze FILE --out doc.json --modules vad,transcribe,diarize
```

`--modules` accepts: `vad`, `transcribe`, `diarize`, `music`, `acoustic`.
