# ADR-0005: Decode speech regions, not the whole file

- **Status:** accepted
- **Date:** 2026-11-10
- **Evidence:** reproduced with a bare `whisper_full` call (no SONOLYTICA code
  in the path) on a two-voice fixture.

## Context

During end-to-end verification, a 15 s file containing two utterances
transcribed only the first. The audio was complete (`ffprobe` and the decoded
sample count agreed), and the same file in reverse order transcribed both
voices. A minimal probe using whisper.cpp directly reproduced the truncation
with every parameter combination tried (token timestamps on/off, `no_context`
on/off, Metal and CPU).

The mechanism is whisper.cpp's `single_timestamp_ending` heuristic: when a
decoded chunk ends with a bare timestamp token, the rest of the 30 s encoder
window is skipped. On audio where a short utterance is followed by a different
voice, this can discard most of the file.

## Decision

When a VAD provider is available, ASR is executed per detected speech region:

1. VAD regions are merged when the gap is under 250 ms (one utterance).
2. Each region is padded by 600 ms, but never across the midpoint of a gap and
   never into a neighbouring region (overlap would duplicate words at seams).
3. Each slice gets 300 ms of digital silence prepended, because Whisper drops
   the first word of a chunk that starts on speech; the slice offset accounts
   for the padding, so timestamps remain on the media timeline.
4. If every region fails, one fallback pass over the whole buffer is attempted
   so an over-strict VAD cannot produce an empty transcript.

Without a VAD provider, the whole buffer is decoded with the known limitation
recorded in `docs/MODELS.md`.

## Consequences

- Long silences cost nothing, and the truncation failure is bounded to a single
  region.
- VAD runs as a prerequisite of ASR and appears in `runs`; it is not hidden.
- Cost: more encoder calls on fragmented audio (each call pads to the 30 s
  window). On speech-dense audio this is a modest slowdown; on real recordings
  with pauses it is usually a net win.
- Known residual: words can be clipped at region boundaries when VAD onset
  detection lags. This is measurable with the WER harness and is the argument
  for forced alignment in a later phase.

## Alternatives rejected

- Disabling token timestamps to dodge the heuristic: loses word timing, which
  is a core feature.
- Fixed sliding windows (20 s, 2 s overlap): predictable, but every window pays
  the full encoder cost and seams still need deduplication.
- Patching whisper.cpp: vendoring a fork of a C++ inference engine is a larger
  maintenance burden than working within its documented behaviour.
