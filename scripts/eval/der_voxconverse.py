#!/usr/bin/env python3
"""Diarization error rate on VoxConverse (dev or test split).

VoxConverse is CC BY 4.0; audio and RTTM are fetched by hand, never in CI.
The Hugging Face mirror `diarizers-community/voxconverse` ships one parquet
per shard with the audio bytes and the reference turn list:

    curl -LO https://huggingface.co/datasets/diarizers-community/voxconverse/\
resolve/main/data/test-00002-of-00011.parquet

    python3 scripts/eval/der_voxconverse.py \
        --parquet /private/tmp/sono-eval/voxconverse-test2.parquet \
        --count 0 --min-speakers 2 --max-speakers 4

Requires `pyarrow` (evaluation only, not a project dependency).
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path


def find_binary(name: str, override: str | None) -> str:
    if override:
        return override
    candidate = Path("target/debug") / name
    return str(candidate) if candidate.exists() else name


def ensure_full_build(sonolytica: str) -> None:
    """Refuse to benchmark a DSP-only binary: there would be no speaker turns."""
    result = subprocess.run(
        [sonolytica, "doctor"], capture_output=True, text=True, check=True
    )
    if "build: minimal" in result.stdout:
        raise SystemExit(
            "this is a DSP-only build; rebuild with "
            "`cargo build -p sonolytica-cli --features full-metal`"
        )


def safe_id(value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9_-]", "_", value)
    return cleaned or "clip"


def row_speakers(row: dict) -> int:
    return len(set(row["speakers"]))


def load_rows(
    parquet: Path,
    count: int,
    skip: int,
    min_speakers: int,
    max_speakers: int,
) -> list[dict]:
    import pyarrow.parquet as pq  # imported here so --help works without it

    reader = pq.ParquetFile(parquet)
    rows: list[dict] = []
    seen = 0
    for group in range(reader.num_row_groups):
        for row in reader.read_row_group(group).to_pylist():
            seen += 1
            if seen <= skip:
                continue
            speakers = row_speakers(row)
            if not min_speakers <= speakers <= max_speakers:
                continue
            rows.append(row)
            if count > 0 and len(rows) >= count:
                return rows
    if not rows:
        raise SystemExit(f"no rows in {parquet} matched the speaker filter")
    return rows


def write_rttm(path: Path, file_id: str, row: dict) -> None:
    lines = []
    for start, end, speaker in zip(
        row["timestamps_start"], row["timestamps_end"], row["speakers"]
    ):
        duration = float(end) - float(start)
        if duration <= 0:
            continue
        lines.append(
            f"SPEAKER {file_id} 1 {float(start):.3f} {duration:.3f} "
            f"<NA> <NA> {speaker} <NA> <NA>"
        )
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--parquet", required=True, type=Path)
    parser.add_argument("--count", type=int, default=5, help="0 means every matching row")
    parser.add_argument("--skip", type=int, default=0, help="skip this many rows first")
    parser.add_argument("--min-speakers", type=int, default=1)
    parser.add_argument("--max-speakers", type=int, default=99)
    parser.add_argument("--dataset", default="VoxConverse dev (diarizers-community parquet mirror)")
    parser.add_argument("--model", default="whisper-base.en")
    parser.add_argument("--language", default="en")
    parser.add_argument("--sonolytica", default=None)
    parser.add_argument("--eval", dest="eval_bin", default=None)
    parser.add_argument("--workdir", type=Path, default=None)
    parser.add_argument("--json-out", type=Path, default=None)
    parser.add_argument(
        "--reuse",
        action="store_true",
        help="score an existing document in --workdir instead of analysing again",
    )
    args = parser.parse_args()

    sonolytica = find_binary("sonolytica", args.sonolytica)
    eval_bin = find_binary("sonolytica-eval", args.eval_bin)
    ensure_full_build(sonolytica)
    workdir = args.workdir or Path(tempfile.mkdtemp(prefix="sono-der-"))
    audio_dir = workdir / "audio"
    audio_dir.mkdir(parents=True, exist_ok=True)

    results = []
    for index, row in enumerate(
        load_rows(args.parquet, args.count, args.skip, args.min_speakers, args.max_speakers),
        start=1,
    ):
        audio = row["audio"]
        original = audio.get("path") or f"clip_{index}.wav"
        file_id = safe_id(Path(str(original)).stem)
        suffix = Path(str(original)).suffix or ".wav"
        audio_path = audio_dir / f"{file_id}{suffix}"
        audio_path.write_bytes(audio["bytes"])

        rttm_path = workdir / f"{file_id}.rttm"
        write_rttm(rttm_path, file_id, row)
        document_path = workdir / f"{file_id}.json"

        print(f"[{index}] {file_id}", file=sys.stderr)
        if not (args.reuse and document_path.exists()):
            subprocess.run(
                [
                    sonolytica,
                    "analyze",
                    str(audio_path),
                    "--out",
                    str(document_path),
                    "--modules",
                    "vad,transcribe,diarize",
                    "--language",
                    args.language,
                    "--model",
                    args.model,
                ],
                check=True,
                stdout=subprocess.DEVNULL,
            )
        document = json.loads(document_path.read_text(encoding="utf-8"))
        if not any(
            segment["kind"]["type"] == "speaker_turn"
            for segment in document["segments"]
        ):
            reasons = "; ".join(
                warning["message"] for warning in document.get("warnings", [])
            )
            raise SystemExit(
                f"no speaker turns for {file_id}: "
                + (reasons or "diarization produced nothing")
            )
        scored = subprocess.run(
            [
                eval_bin,
                "der",
                "--rttm",
                str(rttm_path),
                "--document",
                str(document_path),
                "--file-id",
                file_id,
                "--json",
            ],
            capture_output=True,
            text=True,
        )
        if scored.returncode != 0:
            raise SystemExit(
                f"sonolytica-eval der failed on {file_id}: {scored.stderr.strip()}"
            )
        score = json.loads(scored.stdout)
        score["file_id"] = file_id
        results.append(score)
        print(
            f"  DER {score['der'] * 100:.2f}%  missed {score['missed_speech'] * 100:.2f}%"
            f"  false alarm {score['false_alarm'] * 100:.2f}%"
            f"  confusion {score['confusion'] * 100:.2f}%",
            file=sys.stderr,
        )

    unweighted = sum(entry["der"] for entry in results) / len(results)
    scored_seconds = sum(entry.get("scored_seconds", 0.0) for entry in results)
    weighted = (
        sum(entry["der"] * entry.get("scored_seconds", 0.0) for entry in results)
        / scored_seconds
        if scored_seconds > 0
        else unweighted
    )
    count_errors = [
        abs(entry["reference_speakers"] - entry["hypothesis_speakers"])
        for entry in results
    ]
    exact_counts = sum(1 for error in count_errors if error == 0)
    summary = {
        "dataset": args.dataset,
        "license": "CC BY 4.0",
        "files": len(results),
        "speaker_filter": [args.min_speakers, args.max_speakers],
        "model": args.model,
        "language": args.language,
        "modules": ["vad", "transcribe", "diarize"],
        "collar_seconds": 0.25,
        "scored_seconds": scored_seconds,
        "der": weighted,
        "unweighted_mean_der": unweighted,
        "speaker_count_mae": sum(count_errors) / len(count_errors),
        "speaker_count_exact": exact_counts / len(count_errors),
        "per_file": results,
    }
    print(json.dumps(summary, indent=2))
    if args.json_out:
        args.json_out.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
