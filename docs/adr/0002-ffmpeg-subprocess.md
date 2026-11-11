# ADR-0002: FFmpeg as a subprocess, not a linked library

- **Status:** accepted
- **Date:** 2026-11-10

## Context

Real media means arbitrary containers, codecs, damaged streams and unusual
sample rates. FFmpeg is the only realistic decoder for that surface. Rust
bindings (`ffmpeg-next`) are available but couple the build to a specific
FFmpeg ABI and to its license configuration.

## Decision

Invoke `ffprobe` and `ffmpeg` as child processes. Metadata comes from
`ffprobe -print_format json`; audio is decoded to interleaved-free `f32le` on a
pipe. The invocation is argv-only, with a protocol whitelist, a timeout and
`kill_on_drop`.

## Consequences

- No C linkage, no GPL contamination, no ABI pinning. Systems with an LGPL
  FFmpeg work identically.
- A missing or broken FFmpeg is a clean, early error (`MediaError::ToolsNotFound`).
- Cost: one process per decode; extra decode when both 16 kHz and 32 kHz views
  are requested (music analysis needs the wider band).
- Cost: progress reporting through pipes is coarse; long decodes are bounded by
  duration rather than streamed to the UI.
- The same subprocess boundary is where video frame sampling will be added
  (Phase 4), so the security model stays in one module.

## Alternatives rejected

- `ffmpeg-next`: build fragility and license coupling for little gain.
- `symphonia` as the only decoder: excellent pure-Rust audio, but no video and
  narrower container coverage. Kept as a future fallback for pure-Rust builds.
- Hand-rolled demuxers: not a differentiator and a security liability.
