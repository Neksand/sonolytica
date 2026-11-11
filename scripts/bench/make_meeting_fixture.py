#!/usr/bin/env python3
"""Build the hard benchmark fixture: 4 minutes, 4 voices, slides, overlaps.

macOS `say` (fixture generation only), FFmpeg and Pillow are required. The
output is `meeting.mp4` plus `timeline.json`; benchmark ground truth is taken
from a reference analysis of the result, not from this script.

    python3 scripts/bench/make_meeting_fixture.py --out /private/tmp/bench-hard
"""

from __future__ import annotations

import argparse
import json
import math
import subprocess
import wave
from pathlib import Path

RATE = 16_000
DURATION = 240.0
WIDTH, HEIGHT = 1280, 720
FONT = "/System/Library/Fonts/Supplemental/Arial.ttf"

VOICES = {"S1": "Alice", "S2": "Albert", "S3": "Daniel", "S4": "Fred"}

SLIDES = [
    (0, "Q4 LAUNCH PLAN", "Weekly sync", (24, 32, 48)),
    (40, "PRICING PAGE — FRIDAY", "Beta group has access", (48, 24, 24)),
    (80, "LEGAL REVIEW: APPROVED", "Terms and privacy signed off", (24, 48, 32)),
    (120, "MARKETING BUDGET +20%", "Covers launch week and webinar", (40, 32, 16)),
    (160, "RISK: SATURDAY MAINTENANCE", "Payment provider window", (56, 24, 40)),
    (200, "NEXT STEPS: COPY BY THURSDAY", "Announcement on Friday", (24, 40, 56)),
]

# (speaker, start seconds, text). Line 7 starts before line 6 ends: overlap.
LINES = [
    ("S1", 6.0, "Welcome everyone. Today we are reviewing the launch plan for the new pricing page."),
    ("S2", 44.0, "The pricing page goes live on Friday, and the beta group already has access."),
    ("S3", 52.0, "I confirm the beta group has the new copy and the test accounts."),
    ("S1", 88.0, "Legal reviewed the terms and the privacy notice yesterday."),
    ("S2", 96.0, "Legal signed off, so the schedule holds. No blockers from my side."),
    ("S3", 128.0, "We have twenty percent more marketing budget than last quarter."),
    ("S4", 130.0, "The budget also covers the partner webinar in November."),
    ("S4", 168.0, "One risk: the payment provider has maintenance on Saturday morning."),
    ("S3", 176.0, "I will send the final copy by Thursday so we can announce on Friday."),
    ("S1", 215.0, "Thank you everyone. We meet again on Monday."),
]


def run(command: list[str]) -> None:
    subprocess.run(command, check=True)


def line_wav(out: Path, voice: str, text: str) -> Path:
    aiff = out.with_suffix(".aiff")
    wav = out.with_suffix(".wav")
    run(["say", "-v", voice, "-o", str(aiff), text])
    run(
        [
            "ffmpeg",
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-i",
            str(aiff),
            "-ar",
            str(RATE),
            "-ac",
            "1",
            "-c:a",
            "pcm_s16le",
            str(wav),
        ]
    )
    aiff.unlink(missing_ok=True)
    return wav


def read_samples(path: Path) -> list[int]:
    with wave.open(str(path), "rb") as handle:
        raw = handle.readframes(handle.getnframes())
    return list(int.from_bytes(raw[i : i + 2], "little", signed=True) for i in range(0, len(raw), 2))


def build_audio(out: Path, work: Path) -> tuple[Path, list[dict]]:
    total = int(RATE * DURATION)
    buffer = [0.0] * total
    timeline = []

    for index, (speaker, start, text) in enumerate(LINES):
        wav = line_wav(work / f"line_{index:02}", VOICES[speaker], text)
        samples = read_samples(wav)
        offset = int(start * RATE)
        for i, sample in enumerate(samples):
            if offset + i < total:
                buffer[offset + i] += sample / 32768.0
        timeline.append(
            {
                "speaker": speaker,
                "start_s": start,
                "duration_s": len(samples) / RATE,
                "text": text,
            }
        )

    # A quiet music bed for the first five seconds (no semantic claim).
    for i in range(int(5 * RATE)):
        t = i / RATE
        buffer[i] += 0.06 * (
            math.sin(2 * math.pi * 261.63 * t)
            + math.sin(2 * math.pi * 329.63 * t)
            + math.sin(2 * math.pi * 392.0 * t)
        )

    peak = max(1.0, max(abs(value) for value in buffer))
    frames = bytearray()
    for value in buffer:
        scaled = int(max(-1.0, min(1.0, value / peak * 0.95)) * 32767)
        frames += scaled.to_bytes(2, "little", signed=True)
    audio = work / "meeting.wav"
    with wave.open(str(audio), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(RATE)
        handle.writeframes(bytes(frames))
    return audio, timeline


def make_slides(work: Path) -> list[Path]:
    from PIL import Image, ImageDraw, ImageFont

    title_font = ImageFont.truetype(FONT, 64)
    sub_font = ImageFont.truetype(FONT, 34)
    paths = []
    for index, (_start, title, subtitle, colour) in enumerate(SLIDES):
        image = Image.new("RGB", (WIDTH, HEIGHT), colour)
        draw = ImageDraw.Draw(image)
        for text, font, y in ((title, title_font, 300), (subtitle, sub_font, 400)):
            left, top, right, bottom = draw.textbbox((0, 0), text, font=font)
            draw.text(
                ((WIDTH - (right - left)) / 2, y - (bottom - top) / 2),
                text,
                fill="white",
                font=font,
            )
        path = work / f"slide_{index}.png"
        image.save(path)
        paths.append(path)
    return paths


def build_video(work: Path, audio: Path, slides: list[Path]) -> Path:
    segments = []
    for index, (start, *_rest) in enumerate(SLIDES):
        end = SLIDES[index + 1][0] if index + 1 < len(SLIDES) else DURATION
        segment = work / f"segment_{index}.mp4"
        run(
            [
                "ffmpeg",
                "-y",
                "-hide_banner",
                "-loglevel",
                "error",
                "-loop",
                "1",
                "-t",
                f"{end - start:.3f}",
                "-i",
                str(slides[index]),
                "-vf",
                "scale=1280:720,format=yuv420p",
                "-r",
                "25",
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                str(segment),
            ]
        )
        segments.append(segment)

    listing = work / "segments.txt"
    listing.write_text(
        "\n".join(f"file '{segment.name}'" for segment in segments) + "\n",
        encoding="utf-8",
    )
    output = work / "meeting.mp4"
    run(
        [
            "ffmpeg",
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
            str(listing),
            "-i",
            str(audio),
            "-c:v",
            "copy",
            "-c:a",
            "aac",
            "-shortest",
            str(output),
        ]
    )
    return output


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    out = args.out
    work = out / "work"
    work.mkdir(parents=True, exist_ok=True)

    audio, timeline = build_audio(out, work)
    slides = make_slides(work)
    video = build_video(work, audio, slides)
    final = out / "meeting.mp4"
    video.replace(final)

    overlaps = []
    for i, first in enumerate(timeline):
        for second in timeline[i + 1 :]:
            start = max(first["start_s"], second["start_s"])
            end = min(
                first["start_s"] + first["duration_s"],
                second["start_s"] + second["duration_s"],
            )
            if end - start > 0.3:
                overlaps.append(
                    {
                        "start_s": round(start, 2),
                        "end_s": round(end, 2),
                        "speakers": [first["speaker"], second["speaker"]],
                    }
                )

    manifest = {
        "duration_s": DURATION,
        "slides": [{"start_s": start, "title": title} for start, title, *_ in SLIDES],
        "lines": timeline,
        "overlaps": overlaps,
    }
    (out / "timeline.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(final)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
