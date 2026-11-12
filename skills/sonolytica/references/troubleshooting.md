# Troubleshooting

## The tool says something is missing

| Message / symptom | Meaning | Action |
|---|---|---|
| `warnings[code=provider_unavailable]` | a capability had no model or provider | `sonolytica models fetch <ids>`, then re-run |
| `warnings[code=diarization_vad_gated]` | the VAD gate removed diarizer turns that did not overlap speech | not an error; the removed time was music/noise |
| `warnings[code=diarization_vad_gate_unavailable]` | the stricter gate pass failed | speaker turns are ungated; read the message |
| `warnings[code=analysis_cached]` | this document came from a cached analysis of the same bytes and parameters | valid data; `--no-cache` only when a fresh run is required |
| `warnings[code=caption_unavailable]` | caption model missing or build without `vision` | re-run with the model installed; do not describe the frame yourself |
| `warnings[code=pose_unavailable]` | pose model missing or build without `vision` | re-run with the model installed; do not infer poses |
| `warnings[code=diarization_speaker_cap]` | all four Sortformer slots are in use; 5+ voices may be merged | say "at least N" speakers; do not cite the count as exact |
| `run.status = partial` | it ran but produced less than asked | read `reason`; re-run with the model installed |
| `run.status = failed` | it ran and errored | read `warnings` for the same `run` id |
| `doctor` shows ffmpeg NOT AVAILABLE | nothing can be decoded | install FFmpeg (7+) |
| `doctor` shows `build: minimal (DSP only)` | this build has no speech providers | install a build with `--features full` / `full-metal` |

A missing capability is **not** a failed run: it produces no run and a warning.
Start with `query brief` (it summarizes runs and warnings), then use
`query runs` or `query summary` when you need the full detail.

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
- Diarization quality depends on clean turns; overlapping speech is split into
  two simultaneous `speaker_turn` segments when Sortformer is installed, and
  only flagged on the pyannote fallback.
- Sortformer tracks at most four speakers. On crowded audio, labels can merge;
  if all four are active and the room sounds busier, say "at least four", not
  an exact count.
- On a single-voice lecture, a high speaker count is a diarizer failure: say so.
- With a `diarization_speaker_cap` warning the four-slot cap was reached:
  the speaker count is a **lower bound** and voices may be merged. Say "at
  least N"; without the warning the count is exact for up to four speakers,
  regardless of file length.
- Short files (< ~10 s) can produce one speaker only.

**Language is wrong in an old document** (e.g. `"my"` for English audio):
that was a provider bug fixed for new runs. In an existing document, say the
language metadata is unreliable and do not use it as evidence. Re-run to fix.

**Numbers add up differently.** `speech_duration` is VAD-measured speech for the
whole file; the sum of `speaker_turn` durations is diarization. Speaker turns
are also clipped to the diarized turn (plus 300 ms), so a word outside it stays
in `query words` but not in the dialogue. Different measurements, not an error.
For "who spoke most" use `query speakers`.

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

- `query timeline --type scene` empty → either the file has no video stream or
  the scene stage did not run. Check `query runs` for a `scene` run and
  `inspect --json` for a video stream.
- `inspect --json` is the only source of resolution/frame-rate facts until
  scenes exist.
- Never describe frames you were not given; never claim "видео без звука" from
  the audio pipeline alone.

## When to stop and report

If two attempts produce contradictory documents (different `sha256` for the
same file, or a stage that fails only sometimes), stop and report the run
records. That is an engine bug, not something to work around in prose.
