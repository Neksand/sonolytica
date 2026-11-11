# ADR-0004: whisper.cpp for ASR, ONNX Runtime for the rest

- **Status:** accepted
- **Date:** 2026-11-10

## Context

Local inference options in Rust today: `whisper-rs` (whisper.cpp bindings),
`candle` (pure Rust), `ort` (ONNX Runtime), `tract` (pure-Rust ONNX),
`sherpa-onnx` (bundled C++ toolkit). They differ in speed, model coverage,
build requirements and platform acceleration.

## Decision

- **ASR:** whisper.cpp through `whisper-rs`, feature `whisper`. It is the
  fastest maintained local Whisper on Apple silicon (Metal) and CPU, and it
  exposes token timestamps, which word timing needs.
- **VAD / diarization / future classifiers:** ONNX Runtime through `ort`,
  isolated in `providers/src/onnx.rs`. One runtime, one environment per process.
- **Fallback:** candle is the documented pure-Rust option if a build cannot
  ship a C toolchain. It is not implemented yet; claiming otherwise would be
  false.
- **Models are loaded lazily** so discovery does not allocate hundreds of
  megabytes for models the caller never selects.

## Consequences

- `cmake` and a C/C++ toolchain are required for the speech feature; the core
  remains buildable without them.
- `ort` is still 2.0-rc. It is pinned exactly (`=2.0.0-rc.15`) and its API is
  touched in exactly one module, so an upgrade is a contained change.
- `sherpa-onnx` was rejected: it duplicates whisper.cpp *and* ONNX Runtime in a
  single large bundle and hides the pipeline semantics this project exists to
  own.
- No Python runtime in the product path.

## Alternatives rejected

- Candle as the default: 2–4× slower and much heavier to compile for the same
  quality.
- ONNX Whisper: requires exported graphs, and was slower than whisper.cpp in
  this configuration.
- Cloud ASR: a provider, deferred; local-first is a requirement.
