# ADR-0001: Rust core with an isolated provider boundary

- **Status:** accepted
- **Date:** 2026-11-10

## Context

Perception depends on models whose best implementations are C++, Python or
CUDA-bound. A project can either make one runtime the architecture, or define a
stable boundary and let runtimes compete behind it. The first option makes the
first model easy and every following change expensive.

## Decision

The domain model, DSP, orchestration, CLI and schema are Rust. Model runtimes
are dependencies of the `sonolytica-providers` crate only, behind synchronous
traits (`VadProvider`, `AsrProvider`, `DiarizationProvider`, ...). The engine
depends on the traits, never on a runtime. Python, if ever needed, is a separate
process with a JSON-lines protocol, never the orchestrator.

## Consequences

- Core crates compile and test with no cmake, no ONNX Runtime and no models.
- Adding a model means implementing a trait; the engine does not change.
- Native runtimes can be feature-gated per platform and per license.
- Cost: a trait boundary and owned audio buffers at each stage. Measured as
  negligible against inference.
- Cost: no shared in-process tensors between models (e.g. embeddings handed from
  one Python object to another). Providers recompute or return owned data.

## Alternatives rejected

- Python core: deployment weight, GIL-driven concurrency, and the project's
  premise would be lost.
- C++ core: memory-safety at the untrusted-media boundary, painful builds.
- One runtime (sherpa-onnx) for everything: monolithic, less control over
  pipeline semantics and failure isolation.
