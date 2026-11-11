# Evaluation scripts

These scripts run the shipped binary against licensed datasets and print a
JSON summary. They are **manual**: no CI job downloads datasets, and no number
in `docs/EVAL.md` comes from anywhere else.

| Script | Dataset | Metric | License |
|---|---|---|---|
| `wer_librispeech.py` | LibriSpeech test-clean | corpus WER / CER | CC BY 4.0 |
| `der_voxconverse.py` | VoxConverse dev | frame DER, 0.25 s collar | CC BY 4.0 |

Both use the real `sonolytica` binary and delegate scoring to
`sonolytica-eval`, so the metric has exactly one implementation.

## Get the data

```bash
mkdir -p /tmp/sono-eval && cd /tmp/sono-eval

# LibriSpeech test-clean (~346 MB)
curl -LO https://www.openslr.org/resources/12/test-clean.tar.gz
tar xzf test-clean.tar.gz

# VoxConverse dev, one parquet shard (~485 MB) from the HF mirror
mkdir -p voxconverse && cd voxconverse
curl -LO https://huggingface.co/datasets/diarizers-community/voxconverse/\
resolve/main/data/dev-00000-of-00005.parquet
```

`der_voxconverse.py` needs `pyarrow` (`pip install pyarrow`). LibriSpeech needs
nothing beyond the standard library.

## Run

```bash
# Build first; the scripts use target/debug by default.
cargo build -p sonolytica-cli --features full-metal
cargo build -p sonolytica-eval

python3 scripts/eval/wer_librispeech.py \
    --root /tmp/sono-eval/LibriSpeech/test-clean --count 100 \
    --json-out /tmp/sono-eval/wer.json

python3 scripts/eval/der_voxconverse.py \
    --parquet /tmp/sono-eval/voxconverse/dev-00000-of-00005.parquet \
    --count 5 --json-out /tmp/sono-eval/der.json
```

## Rules

- A number is only reportable with: dataset, split, subset size, model,
  parameters, machine, and date. `sonolytica-eval` prints the counts; the
  script adds the rest to the JSON.
- The LibriSpeech subset is deterministic (every *k*-th utterance in sorted
  order), so two runs on the same split are comparable.
- VoxConverse is "in the wild": music, noise, overlapping speech. A high DER
  here is a finding, not a reason to change the subset.
- Never tune on the evaluation split. Use `dev` for decisions and `test` for the
  final number.
