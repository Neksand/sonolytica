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
| **DER** | **15.46 %** (duration-weighted) |
| Unweighted mean over files | 11.69 % |

| file | DER | missed | false alarm | confusion | speakers ref → hyp |
|---|---|---|---|---|---|
| rcxzg | 10.57 % | 6.00 % | 0.13 % | 4.43 % | 4 → 8 |
| fsaal | 23.37 % | 5.42 % | 1.87 % | **16.08 %** | 7 → 8 |
| vmaiq | 15.39 % | 5.54 % | 0.90 % | 8.95 % | 17 → 8 |
| tfvyr | 1.14 % | 1.14 % | 0.00 % | 0.00 % | 1 → 1 |
| qpylu | 8.00 % | 2.61 % | 5.39 % | 0.00 % | 2 → 2 |

Reading it:

- **Confusion still dominates** on files with many speakers: the diarizer hears
  the speech (missed ≤ 6 %) but assigns the wrong speaker. The remaining causes
  are visible: `fsaal` invents one speaker too many (7 → 8), and `vmaiq` is
  capped at `max_speakers = 8` against 17 reference speakers.
- Single-speaker material is nearly perfect (1.14 %, all missed).
- `qpylu` shows 5.39 % false alarm: diarization emits turns slightly beyond the
  reference speech boundaries.

### Change measured on this baseline

First measurement (10 s non-overlapping regions, single-linkage clustering,
threshold 0.60): **18.60 %**. After switching to 1.5 s embedding chunks with a
0.75 s hop, average-linkage agglomerative clustering and threshold 0.45:
**15.46 %** (−3.1 pp). Per file: `rcxzg` 24.74 → 10.57, `qpylu` 10.36 → 8.00,
`vmaiq` 16.81 → 15.39, `fsaal` unchanged, `tfvyr` unchanged.

The first attempt with 1.5 s chunks but the old threshold 0.60 made `qpylu`
much worse (10.36 → 38.46 %) by splitting two speakers into five. A threshold
sweep on these same five files gave 0.35 → 15.40 %, 0.45 → 15.46 %, 0.55 →
16.10 %, 0.60 → 16.26 %; 0.45 was chosen as the knee. **This calibration used
the same five pilot files that the table reports**, so the number is a pilot,
not a held-out result. The next honest step is a held-out `test` subset.

Caveats: 5 recordings only; in-the-wild YouTube audio (music, noise, overlap);
greedy speaker mapping in `sonolytica-eval` (an optimal assignment would lower
confusion slightly); 0.25 s collar, frame 10 ms — do not compare with a paper
that uses a different collar.

### Held-out check on the test split

The dev numbers above were produced with a threshold chosen on the same five
files, so they are a pilot. The same configuration was run once on five
recordings from the `test` shard, which had never been inspected:

| Field | Value |
|---|---|
| Subset | first 5 recordings of `test-00000-of-00011` |
| Reference speech | 3973 s |
| **DER** | **19.90 %** (duration-weighted) |
| Unweighted mean | 17.17 % |

| file | DER | speakers ref → hyp |
|---|---|---|
| uedkc | 24.44 % | 7 → 8 |
| heolf | 27.60 % | 12 → 7 |
| otmpf | 5.86 % | 3 → 3 |
| myjoe | 11.98 % | 2 → 7 |
| jrfaz | 15.99 % | 9 → 8 |

Reading it honestly:

- The held-out number is 4.4 pp worse than the dev pilot (19.90 vs 15.46 %).
  Part of the dev gain did not transfer; the threshold is not universal.
- The failure mode is now **speaker-count estimation**, not threshold:
  `myjoe` splits two speakers into seven, `heolf` merges twelve into seven.
  A fixed similarity threshold cannot fix both directions.
- Do not describe 15.46 % as the pipeline's quality. The defensible statement
  today is "≈16–20 % DER on VoxConverse in-the-wild, pilot scale".
- This test shard has now been seen. The next report must use a different
  subset (or the full split) to stay honest.

### Tried and rejected: pruning tiny clusters

Hypothesis: over-segmentation comes from 1–4 s noise clusters, so merge any
cluster shorter than `max(2 s, 1 % of speech)` into the nearest large one.

| | dev | test |
|---|---|---|
| before | 15.46 % | 19.90 % |
| with pruning | 15.61 % | 19.97 % |

It did fix over-segmentation on two-speaker material (`myjoe` 7 → 2,
`rcxzg` 8 → 4) but caused under-segmentation wherever real speakers are brief:
`uedkc` 7 → 3, `heolf` 12 → 5, `fsaal` 7 → 5. Net effect is neutral to worse,
so the change was reverted. A real speaker-count estimate is needed; pruning
by duration cannot distinguish a cough from a quiet participant.

## What the numbers say to do next

The planned order was forced alignment, then diarization. The measurements
argue for swapping that:

1. **Diarization: speaker count, not another threshold.** The held-out check
   shows over- and under-segmentation in the same run (2 → 7 and 12 → 7
   speakers). The next change must estimate the number of speakers and handle
   overlap, not move the similarity cut.
2. **Alignment second.** WER 6.05 % is near the model ceiling and deletions are
   1.1 %; alignment improves *when* a word was said (citations, cutting), not
   *what* was said. Still worth doing, but it does not move the headline.
3. **Do not touch the ASR model yet.** `base.en` on clean speech is not the
   bottleneck. A larger model is a cost decision, not a quality fix.
4. **Fresh data before the next claim.** Both pilot shards have been seen now;
   the next diarization number needs a subset that has not been used for any
   decision.

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
