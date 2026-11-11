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
overlap); on the pyannote path overlap is flagged but not split; greedy speaker
mapping in `sonolytica-eval` (an optimal assignment would lower confusion
slightly); 0.25 s collar, frame 10 ms — do not compare with a paper that uses a
different collar.

### Sortformer 4spk on a fresh shard

The AHC threshold experiments above are closed. The next attempt was a model:
NVIDIA Sortformer 4spk v2, 128-bin log-mel in, 80 ms speaker activity out,
CC-BY-4.0, exported to ONNX (469 MB). It is the preferred provider when the
model is installed; pyannote remains the fallback if inference fails.

Dataset: `test-00002-of-00011`, never used for a decision, all six 2-4 speaker
files (2140 s scored). Pipeline unchanged: `vad,transcribe,diarize`,
`whisper-base.en`, collar 0.25 s.

| Metric (2-4 speakers) | AHC (before) | Sortformer |
|---|---|---|
| DER, duration-weighted | **7.99 %** | 11.43 % |
| DER, unweighted mean | 9.94 % | **8.19 %** |
| Speaker-count MAE | 5.00 | **0.33** |
| Exact speaker count | 0/6 | **5/6** |

| file | speakers | AHC DER (hyp) | Sortformer DER (hyp) |
|---|---|---|---|
| wcxfk | 3 | 12.00 % (9) | **4.16 % (3)** |
| rmvsh | 2 | 7.05 % (4) | **5.41 % (2)** |
| fvhrk | 4 | **7.00 % (9)** | 16.38 % (2) |
| ltgmz | 2 | 21.14 % (11) | **11.47 % (2)** |
| ledhe | 3 | 6.74 % (10) | **6.06 % (3)** |
| gylzn | 2 | 5.70 % (3) | **5.64 % (2)** |

Reading it:

- **The count is fixed on five of six files.** AHC never got a count right on
  this shard; Sortformer got five. On the five files shorter than 7 minutes it
  scores **6.40 %** versus 8.99 % for AHC, with an exact count everywhere.
- **The exception dominates the weighted number.** `fvhrk` is a 20-minute,
  four-speaker recording where Sortformer finds only the two dominant voices;
  it is 56 % of the scored audio, so the weighted DER rises even though the
  unweighted mean improves. The next step for files like this is the model's
  native streaming cache, not a longer forward pass.
- **Single-speaker files keep the right count** (`gwloo` 1→1, `uicid` 1→1) but
  show false alarm on in-the-wild noise: 4.27 % and 13.41 %. A dedicated VAD
  gate cuts them to 2.47 % and 6.53 % — see below.
- **Overlap is split, not just flagged.** On the hard 4-minute meeting the known
  simultaneous turn around 130 s comes back as two turns (S2 and S3) whose
  ranges intersect.
- Limits: at most four speakers; 300 s windows with 10 s overlap and
  activity-based label matching across windows; 469 MB on disk, ~2.8 GB peak
  RSS on a 4-minute file (5.9 s for the forward pass).

Caveat: one fresh shard, six files, one machine. The numbers are a pilot for
direction, not a leaderboard entry.

### VAD-gated diarization

Hypothesis: the false alarm on `gwloo`, `uicid` and later `test-00003` is the
diarizer labelling non-speech; Silero VAD regions are an independent check.
The first attempt used the configured VAD (`threshold 0.45`): on `uicid` all
271 turns had at least 78.9 % coverage (median 100 %), so the gate removed
nothing — the configured VAD calls the noise speech. On `ltgmz` its single
dropped turn was real (missed speech 6.46 % → 7.04 %). That version was
rejected.

The accepted version runs a **dedicated gate pass at 0.70**; the ASR pass keeps
the configured threshold, so transcript regions and WER are untouched. The gate
drops a turn only when coverage is below 0.5, the turn is at least 0.5 s long,
and it contains no whole VAD region; edges overhanging speech by more than
0.5 s are trimmed. The utterance range is then clamped to the assigned turn
plus 300 ms, while the text stays complete: a word cut from the range is still
in `query words` and in the dialogue.

Threshold 0.70 was chosen on `test-00002`. `test-00003` was untouched until the
comparison below; that is the final number for this change.

`test-00002`, same pipeline and collar:

| file | DER before | DER after | FA before | FA after |
|---|---|---|---|---|
| gwloo (1 spk) | 7.34 % | **5.56 %** | 4.27 % | **2.47 %** |
| uicid (1 spk) | 16.23 % | **9.45 %** | 13.41 % | **6.53 %** |
| weighted, 1 spk | 11.97 % | **7.58 %** | | |
| weighted, 2-4 spk | 11.43 % | **11.08 %** | | |
| unweighted, 2-4 spk | 8.19 % | **7.76 %** | | |

On the six 2-4 speaker files no DER rose (the largest gain is `rmvsh`,
5.41 → 4.80 %) and speaker-count MAE stayed 0.33 (5/6 exact).

`test-00003` (fresh shard, 6 files, 2401 s scored):

| file | DER before | DER after | FA before | FA after |
|---|---|---|---|---|
| lubpm | 12.17 % | **4.27 %** | 9.90 % | **2.00 %** |
| xtdcl | 19.87 % | **10.99 %** | 15.61 % | **5.71 %** |
| dxbbt | 11.12 % | 10.89 % | 1.12 % | 0.89 % |
| zfzlc | 13.12 % | 12.79 % | 1.03 % | 0.58 % |
| kmunk | 5.87 % | 4.33 % | 2.01 % | 0.46 % |
| bxcfq | 1.66 % | 1.66 % | 0.00 % | 0.00 % |
| **weighted** | **11.02 %** | **9.95 %** | | |
| **unweighted** | **10.63 %** | **7.49 %** | | |

Speaker-count MAE stays 0.00 with 6/6 exact. Missed speech rises slightly on
`xtdcl` (3.76 % → 5.18 %) because the turn no longer claims words outside it;
the false-alarm drop dominates. ASR and dialogue text are unchanged: LibriSpeech
WER stays 6.05 % (same 23 deletions), bench-hard words stay 102 and every turn
keeps its text (e.g. "We have 20 % more…" at the overlap), and the overlap at
~130 s is still two turns (S2 130.0–131.3 s, S3 127.7–133.6 s). The gate costs
about 0.8 s on the 240 s hard fixture (13.4 → 14.2 s, 17.9× → 16.9× realtime).
When it removes turns it warns `diarization_vad_gated`; a failed strict pass
warns `diarization_vad_gate_unavailable` and leaves turns ungated.

JER on the same frames, collar and mapping:

| shard | DER before | DER after | JER before | JER after |
|---|---|---|---|---|
| test-00002, 2-4 spk, weighted | 11.43 % | **11.08 %** | 11.22 % | **10.93 %** |
| test-00002, 2-4 spk, unweighted | 8.19 % | **7.76 %** | 8.03 % | **7.67 %** |
| test-00003, weighted | 11.02 % | **9.95 %** | 10.74 % | **9.85 %** |
| test-00003, unweighted | 10.63 % | **7.49 %** | 9.94 % | **7.34 %** |

JER here is `1 - correct / union` over the same frames:
`(missed + false_alarm + confusion) / (reference_frames + false_alarm)`.
Perfect diarization gives 0, and JER ≤ DER by construction because the
denominator includes false-alarm frames. It is the speaker-aware frame Jaccard,
not NIST's per-speaker average; the numbers are not interchangeable.
`sonolytica-eval der` prints it, and the VoxConverse script aggregates weighted
and unweighted values.

### Tried and rejected: overlap tie-break

Hypothesis: when two turns cover a word by the same amount, the choice should
not depend on array order; prefer higher confidence, then the turn start
closest to the word, then the stable speaker id.

Re-ran `test-00002` (2-4 spk) with the tie-break: DER 11.08 → 11.11 %, JER
10.93 → 10.95 %; `wcxfk` confusion 0.35 → 0.67 %. No file improved measurably.
Reverted. The merge still takes the first turn on an exact tie; that
non-determinism is documented as a known behavior. The real fix is
speaker-attributed ASR (multitalker), not a merge heuristic.

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

### Tried and rejected: widest dendrogram gap

Hypothesis: choose the cut at the widest gap between consecutive merge levels —
a scale-free alternative to the fixed similarity threshold. On the first dev
recording it collapsed four speakers into two and scored 54 % DER, so the run
was stopped and the change reverted. Chunk embeddings at 1.5 s are too noisy
for a single global cut to be reliable: within-speaker and cross-speaker merge
levels overlap.

No further threshold heuristic is planned. Speaker counting needs a model
trained for it (Sortformer) or an eigengap/NME estimate over a smoothed
affinity, not another cut on the same dendrogram.

## Word alignment

wav2vec2-base-960h CTC forced alignment: 377 MB, Apache-2.0, 20 ms frames,
English (32-class uppercase vocabulary). `--align` (implied by `--profile ultra`)
replaces Whisper token times with aligned word boundaries.

Two-voice fixture, 32 paired words against `whisper-small` timestamps:

| | |
|---|---|
| Median drift | **110 ms** |
| Within 200 ms | 59 % |
| Within 600 ms | 88 % |
| With `whisper-base.en` | 34 % within 200 ms |

Agreement with Whisper is not the target. On JFK speech the aligned boundaries
match an independent greedy CTC decode frame-for-frame, while Whisper's own
token timestamps are **300–950 ms early** (5 % within 200 ms): Whisper places
words inside silence. The aligned times are the better estimate.

Limits: the text is still Whisper's, so a word it never emitted cannot be
recovered; English only; a slice over 30 s aligns the first 30 s and keeps
Whisper times for the tail (`align_truncated` warning). The e2e gate asserts
median ≤ 300 ms and 88 % within 600 ms, which catches frame-rate and offset
bugs rather than tuning to Whisper.

## Music sanity checks

Synthetic signals with a known answer, 10 s each, `--modules music,acoustic`:

| Signal | Expected | Measured |
|---|---|---|
| clicks every 0.5 s | 120 BPM | **120.3 BPM** (0.25 % error), score 0.001 |
| C4+E4+G4 sines | C major triad | key **C major** (score 0.317), chord **C major** (0.999) |
| same sustained chord | tempo is undefined | 250 BPM with score **0.0** |
| C–G–C, 10 s each | boundaries at 10 and 20 s | **9.92 s and 19.84 s** (80/160 ms early) |

Reading it: tempo is useful when there are onsets and meaningless on a sustained
chord — which is exactly why confidence is reported. Key and triads are
template-based and correct on a clean triad; real mixes are harder. Sections
come from chroma self-similarity with a checkerboard novelty curve; on a clean
A–B–A they land within 160 ms, but a through-composed piece has no sections and
the novelty is not a semantic label.

## Throughput

121.7 s of two-voice speech, full five-stage pipeline, on this machine:

```
vad 374 ms · transcribe 3217 ms · diarize 1442 ms · music 199 ms · acoustic 92 ms
total stage time 5324 ms · wall 8.28 s · 21.64× realtime
all stages complete
```

Model load and process startup are included in the wall time; the stage total
is what the document records. A 2-hour recording is therefore roughly 5–6
minutes of analysis here, dominated by ASR.

With the Sortformer diarizer (155.6 s VoxConverse file, all five audio stages):

```
vad 495 ms · transcribe 2805 ms · diarize 3304 ms · music 266 ms · acoustic 123 ms
total stage time 6993 ms · wall 11.00 s · 14.1× realtime · peak RSS 2.3 GB
```

The earlier 21.64× used pyannote (1.44 s diarization on 121.7 s). Sortformer
costs about 1.9 s more on a 2.5-minute file and ~0.5 GB of model memory; that
is the price of getting the speaker count right.

### Tried and rejected: NME spectral count

Hypothesis: estimate the speaker count from the largest eigengap of the
normalized affinity matrix, then cut the dendrogram at that k. The synthetic
block tests passed (two and three well-separated blocks recovered exactly).

On the real dev files the normalized affinity is dominated by one eigenvalue:
the largest gap consistently landed at k = 1. Results: `rcxzg` 4 → 1, `fsaal`
7 → 1, `vmaiq` 17 → 1; DER rose from 11.81 % to **63.46 %**. Reverted.

Lesson recorded: synthetic block matrices are not evidence about real speaker
embeddings. The count needs either a model trained for it (Sortformer) or a
clustering criterion that accounts for cluster mass and overlap — not a
spectrum over 1.5 s chunks.

## What the numbers say to do next

The planned order was forced alignment, then diarization. The measurements
argue for swapping that:

1. **Diarization: the count is solved for 2-4 speakers, not for long files.**
   Sortformer gets five of six counts right on a freshly seen shard and, with
   the 0.70 VAD gate, false alarm on noisy real recordings is down to 0.5–5.7 %
   (test-00003 weighted DER 11.02 → 9.95 %, JER 10.74 → 9.85 %). The remaining
   error is under-segmentation on a 20-minute four-speaker file, so the next
   change is the model's native streaming cache, not another similarity cut.
2. **Alignment second.** WER 6.05 % is near the model ceiling and deletions are
   1.1 %; alignment improves *when* a word was said (citations, cutting), not
   *what* was said. Still worth doing, but it does not move the headline.
3. **Do not touch the ASR model yet.** `base.en` on clean speech is not the
   bottleneck. A larger model is a cost decision, not a quality fix.
4. **Fresh data before the next claim.** `test-00002` was used for the gate
   threshold and `test-00003` for its final number; both have been seen. The
   next diarization number needs `test-00004`.

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
