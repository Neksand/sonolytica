# Troubleshooting

## The tool says something is missing

| Message / symptom | Meaning | Action |
|---|---|---|
| `warnings[code=provider_unavailable]` | a capability had no model or provider | `sonolytica models fetch <ids>`, then re-run |
| `run.status = partial` | it ran but produced less than asked | read `reason`; re-run with the model installed |
| `run.status = failed` | it ran and errored | read `warnings` for the same `run` id |
| `doctor` shows ffmpeg NOT AVAILABLE | nothing can be decoded | install FFmpeg (7+) |
| `doctor` shows `build: minimal (DSP only)` | this build has no speech providers | install a build with `--features full` / `full-metal` |

A missing capability is **not** a failed run: it produces no run and a warning.
Check both `query runs` and `query summary`.

Models live in `~/.cache/sonolytica/models` (override with `--models-dir` or
`SONOLYTICA_MODEL_DIR`). `sonolytica models list` shows which are verified; a
`PRESENT, DIGEST MISMATCH` entry is refused at load time, not used.

## Analyse problems

**Empty transcript, but the file has sound.**
1. `query acoustic` — is there measured silence for most of the file?
2. `query runs` — did `vad` and `transcribe` run, and with which model?
3. A `.en` model on non-English audio yields confident nonsense: check
   `config.asr_model` and pass `--language` with a multilingual model.
4. Check `speech_ratio`: if it is very low, the VAD threshold or the input gain
   is the suspect.

**First word of every phrase is missing.** Known ASR region-boundary limit.
Do not claim the word was never said; note the limit.

**Speakers look wrong (one person split into two, or everyone is S1).**
- Diarization quality depends on clean turns; overlapping speech is flagged,
  not split.
- On a single-voice lecture, a high speaker count is a diarizer failure: say so.
- Short files (< ~10 s) can produce one speaker only.

**Language is wrong in an old document** (e.g. `"my"` for English audio):
that was a provider bug fixed for new runs. In an existing document, say the
language metadata is unreliable and do not use it as evidence. Re-run to fix.

**Numbers add up differently.** `speech_duration` is VAD-measured speech for the
whole file; the sum of `speaker_turn` durations is diarization. Different
measurements, not an error. For "who spoke most" use `query speakers`.

## Query problems

| Error | Cause |
|---|---|
| `could not parse …: schema `X` is not compatible with 1` | document from another major version; re-run `analyze` |
| `unknown segment type` | `--type` typo; valid: speech, speaker_turn, music_label, acoustic_event, scene, object |
| `invalid time` | use `HH:MM:SS.mmm`, `MM:SS`, `12.5s`, `950ms`, or µs |
| `--to is before --from` | swap them; half-open window must be non-empty |
| exit 1 on a negative time like `-3s` | pass it as a value: `-- -3s` or `--from=-3s` |
| `(none)` | the query succeeded; the document has no match. Not an error |

## Exit codes

- `analyze` returns 0 even with a partial document, unless `--strict` or
  `--profile ultra`.
- `--strict`/`ultra` return 2 when a run failed **or** a requested capability
  produced no run at all.
- `query` returns 1 only on bad input; an empty result is 0.

## Video

- `query timeline --type scene` empty → the visual layer did not run.
- `inspect --json` is the only source of resolution/frame-rate facts until
  scenes exist.
- Never describe frames you were not given; never claim "видео без звука" from
  the audio pipeline alone.

## When to stop and report

If two attempts produce contradictory documents (different `sha256` for the
same file, or a stage that fails only sometimes), stop and report the run
records. That is an engine bug, not something to work around in prose.
