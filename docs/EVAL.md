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
| Dataset | VoxConverse (CC BY 4.0, `diarizers-community` parquet mirror) |
| Dev subset | first 5 recordings of `dev-00000-of-00005` |
| Test subset | first 5 recordings of `test-00000-of-00011` |
| Reference speech | 1051 s dev / 3973 s test |
| Model | `whisper-base.en` + pyannote-3.0 + WeSpeaker |
| Pipeline | `--modules vad,transcribe,diarize --language en` |
| Clustering | 1.5 s chunks (0.75 s hop), average linkage, threshold 0.45, `max_speakers` 12 |
| Collar / frame | 0.25 s / 10 ms |
| **DER dev** | **11.81 %** (duration-weighted) |
| **DER test** | **17.46 %** |

Dev, final configuration:

| file | DER | missed | false alarm | confusion | speakers ref → hyp |
|---|---|---|---|---|---|
| rcxzg | 11.00 % | 6.09 % | 0.13 % | 4.78 % | 4 → 11 |
| fsaal | 10.53 % | 5.47 % | 1.87 % | 3.19 % | 7 → 12 |
| vmaiq | 12.96 % | 5.54 % | 0.90 % | 6.52 % | 17 → 12 |
| tfvyr | 1.14 % | 1.14 % | 0.00 % | 0.00 % | 1 → 1 |
| qpylu | 8.00 % | 2.61 % | 5.39 % | 0.00 % | 2 → 2 |

Test, final configuration:

| file | DER | missed | false alarm | confusion | speakers ref → hyp |
|---|---|---|---|---|---|
| uedkc | 21.46 % | 4.77 % | 2.39 % | 14.30 % | 7 → 11 |
| heolf | 27.46 % | 4.47 % | 2.45 % | **20.53 %** | 12 → 10 |
| otmpf | 6.10 % | 4.66 % | 1.20 % | 0.23 % | 3 → 4 |
| myjoe | 12.72 % | 5.14 % | 5.03 % | 2.54 % | 2 → 11 |
| jrfaz | 9.67 % | 6.27 % | 0.83 % | 2.57 % | 9 → 12 |

Reading it:

- **Confusion dominates** on crowded files: speech is heard (missed ≤ 6 %) but
  attributed to the wrong person. `heolf` (12 speakers) is the worst at 20.53 %.
- The speaker budget was the binding constraint: with `max_speakers = 8`,
  `fsaal` was forced into 8 clusters and scored 23.37 %; with 12 it scores
  10.53 %. The cap was distorting the clustering, not protecting it.
- Over-segmentation now shows on two-speaker material (`myjoe` 2 → 11) but
  costs little DER (12.72 %) because the extra clusters are short. Speaker
  count remains the open problem: too many on simple files, too few on `heolf`.
- Single-speaker material is nearly perfect (1.14 %, all missed).
- `qpylu` and `myjoe` show 5 % false alarm: turns extend past reference speech.

### How the number moved

| Configuration | dev | test |
|---|---|---|
| 10 s regions, single linkage, threshold 0.60, max 8 | 18.60 % | — |
| 1.5 s chunks, average linkage, threshold 0.45, max 8 | 15.46 % | 19.90 % (held out at the time) |
| same, `max_speakers` 12 | **11.81 %** | **17.46 %** |

The threshold sweep on the dev pilot: 0.35 → 15.40 %, 0.45 → 15.46 %,
0.55 → 16.10 %, 0.60 → 16.26 %; 0.45 is the knee. The first run with 1.5 s
chunks and the old threshold 0.60 made `qpylu` much worse (10.36 → 38.46 %) by
splitting two speakers into five — short chunks need a lower cut.

The test shard was held out for the first measurement (19.90 %). It has since
been used to choose `max_speakers`, so **17.46 % is a pilot, not a held-out
result**. The next report needs a subset that has not influenced any decision.

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

Caveats: 5 recordings per split; in-the-wild YouTube audio (music, noise,
overlap); overlap is flagged but not split; greedy speaker mapping in
`sonolytica-eval` (an optimal assignment would lower confusion slightly);
0.25 s collar, frame 10 ms — do not compare with a paper that uses a different
collar.

### Tried and rejected: silhouette-based splitting

Hypothesis: let the dendrogram decide the cut — build down to one cluster, then
choose the number of speakers above the threshold floor by the best mean
silhouette on cosine distance.

| | dev | test |
|---|---|---|
| before | 11.81 % | 17.46 % |
| with silhouette | 13.35 % | 17.46 % |

It changed nothing on the test pilot and broke the single-speaker recording
(`tfvyr`: 1 → 3 speakers, DER 1.14 → 62.55 %): silhouette rewards compact
outlier clusters, and a lone voice with music or noise fragments looks like
several. Reverted. Speaker counting needs a model trained for it (Sortformer,
or an eigengap/NME estimate with a noise model), not another unsupervised
score on the same embeddings.

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
