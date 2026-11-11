#!/usr/bin/env python3
"""Corpus WER baseline on LibriSpeech test-clean.

Datasets are downloaded by hand, never in CI (see README in this directory).

    curl -LO https://www.openslr.org/resources/12/test-clean.tar.gz
    tar xzf test-clean.tar.gz

    python3 scripts/eval/wer_librispeech.py \
        --root /private/tmp/sono-eval/librispeech/LibriSpeech/test-clean \
        --count 100

The script runs the real `sonolytica analyze` on each utterance, writes the
references and hypotheses in the same order, and delegates the score to
`sonolytica-eval wer` so there is one implementation of the metric.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tempfile
from pathlib import Path


def find_binary(name: str, override: str | None) -> str:
    if override:
        return override
    # `cargo build -p sonolytica-cli` puts both in target/debug.
    candidate = Path("target/debug") / name
    if candidate.exists():
        return str(candidate)
    return name


def ensure_full_build(sonolytica: str) -> None:
    """Refuse to benchmark a DSP-only binary: the numbers would be empty."""
    result = subprocess.run(
        [sonolytica, "doctor"], capture_output=True, text=True, check=True
    )
    if "build: minimal" in result.stdout:
        raise SystemExit(
            "this is a DSP-only build; rebuild with "
            "`cargo build -p sonolytica-cli --features full-metal`"
        )


def utterances(root: Path) -> list[tuple[str, str, Path]]:
    found: list[tuple[str, str, Path]] = []
    for trans in sorted(root.rglob("*.trans.txt")):
        for line in trans.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            utt_id, text = line.split(" ", 1)
            audio = trans.parent / f"{utt_id}.flac"
            if audio.exists():
                found.append((utt_id, text, audio))
    found.sort(key=lambda item: item[0])
    return found


def spread(items: list, count: int) -> list:
    """Deterministic, speaker-diverse subset: every k-th item."""
    if count >= len(items):
        return items
    step = len(items) / count
    return [items[int(index * step)] for index in range(count)]


def transcript_of(document: dict) -> str:
    segments = [
        (segment["range"]["start"], segment["kind"].get("text", ""))
        for segment in document["segments"]
        if segment["kind"]["type"] == "speech"
    ]
    segments.sort(key=lambda pair: pair[0])
    return " ".join(text for _, text in segments if text)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--count", type=int, default=100)
    parser.add_argument("--model", default="whisper-base.en")
    parser.add_argument("--language", default="en")
    parser.add_argument("--sonolytica", default=None)
    parser.add_argument("--eval", dest="eval_bin", default=None)
    parser.add_argument("--workdir", type=Path, default=None)
    parser.add_argument("--json-out", type=Path, default=None)
    args = parser.parse_args()

    sonolytica = find_binary("sonolytica", args.sonolytica)
    eval_bin = find_binary("sonolytica-eval", args.eval_bin)
    ensure_full_build(sonolytica)
    chosen = spread(utterances(args.root), args.count)
    if not chosen:
        print("no utterances found", file=sys.stderr)
        return 1

    workdir = args.workdir or Path(tempfile.mkdtemp(prefix="sono-wer-"))
    workdir.mkdir(parents=True, exist_ok=True)
    references: list[str] = []
    hypotheses: list[str] = []
    empty = 0

    for index, (utt_id, text, audio) in enumerate(chosen, start=1):
        out = workdir / f"{utt_id}.json"
        command = [
            sonolytica,
            "analyze",
            str(audio),
            "--out",
            str(out),
            "--modules",
            "vad,transcribe",
            "--language",
            args.language,
            "--model",
            args.model,
        ]
        subprocess.run(command, check=True, stdout=subprocess.DEVNULL)
        document = json.loads(out.read_text(encoding="utf-8"))
        unavailable = [
            warning["message"]
            for warning in document.get("warnings", [])
            if warning.get("code") == "provider_unavailable"
        ]
        if unavailable:
            raise SystemExit("provider unavailable: " + "; ".join(unavailable))
        hypothesis = transcript_of(document)
        if not hypothesis:
            empty += 1
        references.append(text)
        hypotheses.append(hypothesis)
        print(f"[{index}/{len(chosen)}] {utt_id} {hypothesis or '(empty)'}", file=sys.stderr)

    ref_file = workdir / "references.txt"
    hyp_file = workdir / "hypotheses.txt"
    ref_file.write_text("\n".join(references) + "\n", encoding="utf-8")
    hyp_file.write_text("\n".join(hypotheses) + "\n", encoding="utf-8")

    result = subprocess.run(
        [eval_bin, "wer", "--reference", str(ref_file), "--hypothesis", str(hyp_file), "--json"],
        check=True,
        capture_output=True,
        text=True,
    )
    score = json.loads(result.stdout)
    summary = {
        "dataset": "LibriSpeech test-clean",
        "license": "CC BY 4.0",
        "utterances": len(chosen),
        "empty_hypotheses": empty,
        "model": args.model,
        "language": args.language,
        "modules": ["vad", "transcribe"],
        **score,
    }
    print(json.dumps(summary, indent=2))
    if args.json_out:
        args.json_out.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
