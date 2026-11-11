# Evaluation

Numbers here were produced on this machine by the scripts in `scripts/eval/`.
Nothing is copied from a paper, a model card or another project. Every row
names the dataset, the subset, the model and the date, because a number without
those is not a measurement.

Machine: Apple M5 Pro, 24 GB, macOS 26.6.2, FFmpeg 9.0.2, Rust 1.99.0.
Binary: `target/debug/sonolytica`, built with `--features full-metal`.
Run date: 2026-11-11.

These are **baselines**, not claims of state-of-the-art performance. They exist
so that every later change can be judged by a delta instead of an opinion.

## Speech recognition — WER

| Field | Value |
|---|---|
| Dataset | LibriSpeech `test-clean` (CC BY 4.0) |
| Subset | 100 utterances, every 26th of 2620 in sorted order (deterministic) |
| Model | `whisper-base.en` |
| Pipeline | `--modules vad,transcribe --language en` |
| Reference words | 2048 |
| **WER** | **6.05 %** |
| CER | 2.25 % |
| Substitutions / insertions / deletions | 88 / 13 / 23 |
| Empty hypotheses | 0 |

Reading it:

- 6.05 % on clean read speech is close to what a `base.en` model can do; the
  remaining errors are rare words, names and punctuation-adjacent tokens
  ("flour-fatten" → "flour-faten").
- Deletions are 23 of 2048 words (1.1 %). Region-boundary word loss is real but
  not the dominant term on this material — contrary to what the two-voice
  fixture suggested.
- Zero empty hypotheses: VAD did not drop a single utterance in this subset.

Caveats: read speech, single speaker, studio quality, no noise, no overlap.
In-the-wild audio will be worse; do not generalise this number to meetings.

## Speaker diarization — DER

| Field | Value |
|---|---|
| Dataset | VoxConverse `dev` (CC BY 4.0, `diarizers-community` parquet mirror) |
| Subset | first 5 recordings of shard `dev-00000-of-00005` |
| Reference speech | 1051 s |
| Model | `whisper-base.en` + pyannote-3.0 + WeSpeaker |
| Pipeline | `--modules vad,transcribe,diarize --language en` |
| Collar / frame | 0.25 s / 10 ms |
| **DER** | **18.60 %** (duration-weighted) |
| Unweighted mean over files | 15.28 % |

| file | DER | missed | false alarm | confusion | speakers ref → hyp |
|---|---|---|---|---|---|
| rcxzg | 24.74 % | 6.00 % | 0.13 % | **18.61 %** | 4 → 8 |
| fsaal | 23.37 % | 5.42 % | 1.87 % | **16.08 %** | 7 → 8 |
| vmaiq | 16.81 % | 5.38 % | 0.90 % | **10.52 %** | 17 → 8 |
| tfvyr | 1.14 % | 1.14 % | 0.00 % | 0.00 % | 1 → 1 |
| qpylu | 10.36 % | 2.71 % | 7.64 % | 0.00 % | 2 → 2 |

Reading it:

- **Confusion dominates**, not missed speech: the diarizer hears the speech
  (missed ≤ 6 %) but assigns the wrong speaker 10–19 % of the time on files with
  4+ speakers. That is the 10 s non-overlapping window and single-linkage
  cosine clustering, as documented in `docs/ARCHITECTURE.md` §7.
- The 17-speaker file is capped at `max_speakers = 8` (default). It is scored as
  it ran, not re-tuned for the benchmark.
- Single-speaker material is nearly perfect (1.14 %, all missed), which is the
  expected behaviour of a clustering step that found one cluster.

Caveats: 5 recordings only; in-the-wild YouTube audio (music, noise, overlap);
greedy speaker mapping in `sonolytica-eval` (an optimal assignment would lower
confusion slightly); 0.25 s collar, frame 10 ms — do not compare with a paper
that uses a different collar.

## What the numbers say to do next

The planned order was forced alignment, then diarization. The measurements
argue for swapping that:

1. **Diarization first.** DER 18.60 % with confusion at 10–19 % is the largest
   user-visible error ("who said what"). The fix is known: overlapping windows,
   better clustering, overlap handling.
2. **Alignment second.** WER 6.05 % is near the model ceiling and deletions are
   1.1 %; alignment improves *when* a word was said (citations, cutting), not
   *what* was said. Still worth doing, but it does not move the headline.
3. **Do not touch the ASR model yet.** `base.en` on clean speech is not the
   bottleneck. A larger model is a cost decision, not a quality fix.

## Reproduce

```bash
# Data (manual, never in CI): see scripts/eval/README.md
cargo build -p sonolytica-cli --features full-metal
cargo build -p sonolytica-eval

python3 scripts/eval/wer_librispeech.py \
    --root /tmp/sono-eval/LibriSpeech/test-clean --count 100 \
    --json-out /tmp/sono-eval/wer.json

python3 scripts/eval/der_voxconverse.py \
    --parquet /tmp/sono-eval/voxconverse/dev-00000-of-00005.parquet \
    --count 5 --json-out /tmp/sono-eval/der.json
```

## What these numbers do not say

- They are not comparable to a published result unless the dataset subset,
  collar, frame step, model and decoding parameters match.
- They were produced on one machine; run times will differ elsewhere, the error
  rates should not.
- No parameter was tuned on these splits. `dev` is for decisions, `test` for the
  final number; changing the subset after seeing the result invalidates it.
