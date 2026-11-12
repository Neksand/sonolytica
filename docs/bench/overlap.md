# Real overlap benchmark (AMI)

The `fvhrk` long score is **not** an overlap benchmark. Its RTTM for the
questioned window (19:15–19:32) contains only `spk02`; the old document's
three "overlap" spans were S1 word fragments duplicated inside S2 sentences by
the window merge. The new AOSC document correctly reports no overlap, so the
long free run is 16/16 after correcting Q4. Do not tune a model to that old
artifact.

A real overlap claim needs a corpus with **independently annotated overlapping
speech**. AMI is the first choice (CC-BY-4.0, word-level annotations).

## Fixture

Six one-minute windows from AMI test meetings, two per band:

| Band | Overlap in the window | Purpose |
|---|---|---|
| low | < 10 % | no-regression guard |
| medium | 10–25 % | natural interruptions |
| high | ≥ 25 % | the hard case the multitalker model is for |

Use different meetings for development and the final number; do not tune on the
same windows you report.

## Metrics

- **cpWER** with optimal speaker permutation (≤4 speakers: brute force).
- **SA-WER** (speaker-attributed WER).
- **Overlap-word recall**: reference words whose span intersects another
  speaker's word; fraction matched with the correct speaker within ±200 ms.
- **DER/JER** as guardrails; they are diarization metrics and should barely
  move.
- Wall time and RSS.

The repo's `sonolytica-eval der` is frame-level turn scoring; add a small
`sonolytica-eval cpwer` or use MeetEval in Python **for evaluation only**.
Python is not a runtime dependency.

## Baseline and gate

1. Current pipeline: `vad,transcribe,diarize,align_words`.
2. Deterministic per-turn re-transcription (2–3 days): re-run Whisper on each
   diarized turn, align, keep words whose midpoint is in the producing turn's
   activity, dedupe staggered words. It cannot split simultaneous words; this
   is the measurement ceiling of the current model.
3. Speaker-conditioned ASR (7–10 days): Parakeet TDT 0.6B v3 multitalker
   fine-tune, CC-BY-4.0, ~2.6 GB, `parakeet-rs` 0.4.4 (same `ort` rc.15),
   speaker masks from the existing Sortformer turns. Do not load its bundled
   second Sortformer.

**Gate:** on the high-overlap band, overlap-word recall improves by ≥20 points
over the current pipeline and cpWER is ≤50 % of the deterministic baseline;
non-overlap WER regresses by ≤0.5 absolute; RSS ≤3.5 GB; a 1 h two-speaker file
finishes in ≤10 min. If the deterministic baseline moves recall by <10 points,
its ceiling is too low; go to the multitalker model.

## Hard-fixture finding (2026-11-13)

The 4-minute synthetic hard meeting has a real 1.4 s overlap at 130.0–131.4:
S3 says the marketing-budget line and S4 says "The budget also covers the
partner webinar in November." The old `bench-brief2` document had an **empty**
S2 overlap turn at 130.0–131.28; the new AOSC document has no overlap and
assigns only the tail ("week .") to S4.

A Parakeet TDT multitalker pass with the **reference** S4 mask recovered the
whole line with word times 129.84–134.24. With masks from the AOSC document it
cannot: the model's per-frame S4 activity in 126–134 s is at most 0.011, so
the mask is empty. The gap is the **diarizer's overlap sensitivity**, not the
ASR model. Do not integrate Parakeet alone; it needs an overlap-aware mask
(old offline v2, Nemotron-3-Diarization, or separation). This is the next
quality cycle and a launch caveat.

## Why not just lower the diarization threshold

The old spans were **not** missed simultaneous speech: the words were already
inside the other speaker's sentence, and there was only one reference speaker.
A lower threshold or duplicated words would manufacture turns. The correct
answer is either an honest "no overlap" or a model that actually separates
voices.

## Repro

AMI: <https://huggingface.co/datasets/edinburghcstr/ami> (CC-BY-4.0). The
research report in `docs/HANDOFF.md` §7 records the model URLs, licences and
published cpWER. The fixture and the eval scorer are **not committed** until
the dataset is downloaded by hand; this file is the protocol, not a result.
