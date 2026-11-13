# SONOLYTICA 0.2.2

**Trust fixes from real-media dogfood: no ghost music, no fallback noise,
clearer evaluation commands, language-aware ASR choice.**

No schema change, no new model, no benchmark rewrite.

## Fixed

- **Ghost music on speech.** The DSP now gates musical evidence before the
  document is built. BPM, key, chords, beats and sections are `null`/empty when
  there is no evidence; loudness remains valid. Clean speech (the 11-second
  JFK fixture) reports `music not detected`; a click track and a sustained
  triad still pass. Query JSON exposes `present: false/true`.
- **Meaningful tempo confidence.** Tempo confidence is now a normalized
  cross-correlation, comparable across sparse clicks and dense speech. The old
  raw ratio could report `score 0.000` while still publishing a BPM.
- **Fallback warning noise.** A missing optional fallback (pyannote when
  Sortformer is present) no longer emits `provider_unavailable` on every run.
  If no provider covers a requested capability, one warning still fires;
  `sonolytica doctor` still lists every unavailable model.
- **Evaluation CLI.** `sonolytica-eval report DOCUMENT` now works positionally;
  `--document` remains a deprecated alias. `wer` accepts
  `--reference-text`/`--hypothesis-text` for raw strings and `-` for stdin,
  not only file paths.
- **Language-aware ASR choice.** English uses `whisper-base.en` by default;
  non-English requests prefer `whisper-small` when installed; an explicit
  `--model` (including `auto`) wins. `sonolytica doctor` prints the default.

## Default model

`whisper-base.en` remains the fast English default. `whisper-small` is the
quality option and the automatic choice for explicit non-English languages.
This avoids silently changing the 26× realtime benchmark; switching the English
default requires a separate measured decision.

## Upgrade

Re-run `sonolytica analyze` on documents produced by 0.2.1 to apply the music
gate. Existing documents are still readable: query treats low-confidence music
as `present: false` and hides BPM/key/chords.

## Unchanged evidence

The hard/long benchmark, DER 6.40 %, 14 models, schema 1.6.0, MCP tools and
Windows/macOS/Linux packaging are unchanged. Tests: **184 model-free / 201
all-features**. The honest ASR number remains
**6.05 % WER / 2.25 % CER** on LibriSpeech test-clean; 0.00 % on an 11-second
JFK clip is a sanity check, not a general accuracy claim.

## Known limits

- Beatless music with weak or noisy triads may be under-reported by the
  conservative gate. It will not hallucinate music on speech.
- `whisper-small` is not the English default; use `--model whisper-small`.
- Real overlapping speech still needs an overlap-aware diarizer.
