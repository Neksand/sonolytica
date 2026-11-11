# Architecture

## 1. The problem, stated precisely

An agent that "hears" needs four things that are easy to confuse:

1. **Perception** — turning samples into labels (words, speakers, beats, scenes).
2. **Time** — every label must be placeable on one timeline, at a resolution
   that survives merging across independent models.
3. **Evidence** — who produced a label, with which model, at what confidence,
   and what was *not* analysed.
4. **Access** — a shape a program can query without re-deriving the world.

Most systems solve (1) and stop. That produces a transcript file, then a second
file for speakers, then a third for music, and an agent that cannot answer
"what happened at 00:22?" without ad-hoc glue. SONOLYTICA is organised around
(2)–(4); perception is pluggable.

## 2. Design principles

1. **The document is the product.** Models are inputs to it, not the API.
2. **Absence is first-class.** A missing model, a failed stage and an
   unmeasured property must be distinguishable from a zero.
3. **Time is integer microseconds, half-open ranges, one timeline.** Floats
   drift; overlapping edits assume half-open intervals.
4. **Providers are replaceable and synchronous.** Inference is blocking work;
   the engine owns the async boundary. No `async_trait` in provider contracts.
5. **Heavy dependencies are feature-gated.** The core builds and its tests run
   with no cmake, no ONNX Runtime, no models.
6. **Every artifact is pinned by SHA-256 and license.** A model that cannot be
   verified is not loaded.
7. **The shortest diff is where all callers route through.** Duplicated
   validation is a bug, not defence.

## 3. System shape

```
                    ┌─────────────────────────────────────────────┐
                    │                sonolytica-core              │
                    │  MediaTime · TimeRange · Document · Runs     │
                    │  Confidence · AnalysisConfig · validation    │
                    └───────────────▲─────────────────────────────┘
                                    │ (pure data, no I/O)
   ┌──────────────┐   ┌─────────────┴──────────────┐   ┌───────────────────┐
   │sonolytica-   │   │      sonolytica-engine     │   │ sonolytica-       │
   │dsp           │──►│ planner · stage isolation  │◄──│ providers         │
   │STFT · tempo  │   │ merge · provenance         │   │ VAD · ASR · diar. │
   │key · chords  │   │ spawn_blocking boundary    │   │ registry · models │
   └──────────────┘   └─────────────▲──────────────┘   └─────────▲─────────┘
                                    │                            │
                          ┌─────────┴─────────┐        ┌─────────┴──────────┐
                          │ sonolytica-media  │        │ whisper.cpp (C++)  │
                          │ ffprobe / ffmpeg  │        │ ONNX Runtime (C++) │
                          └───────────────────┘        └────────────────────┘
                                    │
                        ┌───────────┴────────────┐
                        │ sonolytica-cli         │  analyze · inspect ·
                        │ sonolytica-eval        │  models · doctor
                        └────────────────────────┘
```

Data flows one way. `core` never depends on anything but serde/sha2.
`dsp` is model-free and allocation-bounded (streaming frames, not a
`frames × bins` matrix). `providers` owns all native runtimes. `engine` is the
only crate that knows the order of stages.

## 4. Decisions and alternatives

### 4.1 Language and runtime

| Option | Verdict |
|---|---|
| **Rust core, Rust-first providers, isolated native adapters** | **Chosen.** Memory safety at a trust boundary (untrusted media), predictable resource use, one deployable binary, strong types for the schema. |
| Python core with Rust extensions | Rejected. Deployment, GIL/threading, memory footprint; and the project would lose its main differentiator. Python appears only as an optional, isolated provider later. |
| C++ core | Rejected. Build fragility, no borrow checker at the media-parsing boundary, harder contribution path. |

Non-Rust model runtimes are *behind traits*: whisper.cpp and ONNX Runtime are C
libraries linked into the provider crate only; a future Python bridge will be a
separate process with a JSON-lines protocol.

### 4.2 Media decoding

| Option | Verdict |
|---|---|
| **FFmpeg subprocess (`ffprobe` JSON, `ffmpeg` f32le pipe)** | **Chosen.** Handles real-world containers, no C linkage, no GPL entanglement, no ABI churn, trivially replaceable. Cost: one process per job (small vs inference) and ~2× decode when both 16 kHz and 32 kHz views are needed. |
| `ffmpeg-next` bindings | Rejected: compile-time FFmpeg dependency, GPL build coupling, version pinning pain. |
| `symphonia` only | Rejected as the primary path (no video, narrower container support), but it is the natural fallback for pure-Rust builds. |

Security: argv-only invocation, `-protocol_whitelist file,pipe`, `-nostdin`,
timeouts with `kill_on_drop`, byte budgets checked before spawn. See
`docs/SECURITY.md`.

### 4.3 Inference runtimes

| Option | Verdict |
|---|---|
| **Whisper via whisper.cpp** | **Chosen for ASR.** Fastest maintained local Whisper (Metal on Apple silicon), token timestamps for words. Cost: cmake + C toolchain, feature-gated. |
| Candle (pure Rust) | Kept as the documented fallback; 2–4× slower and heavier compile, but no native toolchain. |
| **ONNX Runtime (`ort`) for VAD, diarization, classifiers** | **Chosen.** One runtime covers small models well, CoreML/CPU execution providers, prebuilt binaries. Risk: `ort` is still 2.0-rc; pinned exactly and isolated in `onnx.rs`. |
| `tract` (pure Rust ONNX) | Rejected for now: slower and less operator coverage; good candidate for a "no native deps" profile later. |
| sherpa-onnx | Rejected as a dependency: duplicates whisper.cpp + ort in one large C++ bundle, less control over the pipeline. |
| Cloud ASR | Deferred: a provider, not a core path. Local-first is a product requirement. |

Silero VAD, pyannote segmentation and WeSpeaker embeddings are all real
networks. There is no energy-threshold "VAD" pretending to be a speech
classifier; DSP silence detection is reported as *silence*, not speech.

### 4.4 Timeline representation

| Option | Verdict |
|---|---|
| Flat list of events | Rejected. Loses containment (a speaker turn inside an utterance), and every consumer re-derives structure. |
| **Hierarchical tracks + typed relation edges** | **Chosen.** Tracks group modality; segments form parent/child trees (utterance → speaker turn); relations capture facts that are not containment (overlap, interruption, alignment, accompaniment). Segments live in one arena, so edges are compact ids. |
| Free-form property graph / RDF | Rejected as the primary store: no invariants, heavy queries, awkward serialisation for agents. Typed edges give the 90% case without the cost. |
| Frame-grid tensors | Rejected as output (size, no semantics). Kept internally: DSP is frame-oriented and streams. |
| Database-first (SQLite/Postgres) | Rejected for v1. Documents are small enough to scan (thousands of segments), and JSON is portable. A SQLite FTS5 index for transcript search is a later, additive layer. |

Hierarchy rules are enforced by `Document::validate`: ids equal positions,
parents exist, are in another track, and contain their children; relations
reference real segments; confidence is finite and in `[0,1]`; timestamps never
exceed the media duration.

### 4.5 Orchestration

| Option | Verdict |
|---|---|
| **In-process stages + provider registry + `spawn_blocking`** | **Chosen.** One binary, no network hop, easy debugging, deterministic ordering. Concurrency at the process level is enough for v1. |
| Microservices per modality | Rejected: operational cost with no benefit at this scale, and brittle IPC for large PCM buffers. |
| Actor framework (Actix/Ractor) | Rejected: abstraction without a need; `tokio` + `JoinSet` covers later parallelism. |
| External workflow engine (Temporal, etc.) | Rejected for v1: a second system to operate. |

Failure isolation is structural: each optional stage computes its output, then
records a `RunRecord` with `Complete`/`Partial`/`Failed`, then its segments are
added. A failure adds a warning and continues. Only probe/decode are fatal.

Prerequisite rule: when ASR is requested and a VAD is available, VAD runs even
if not requested — decoding speech regions instead of the whole file avoids
Whisper's long-form early-end-of-text failure and skips silence. This is
visible in the run list, not hidden.

### 4.6 Storage and caching

Documents are written as JSON; hashing the input is on by default so results can
be tied to exact bytes. There is deliberately **no cache in v1**: a
content-addressed cache is only worth its invalidation complexity once
inference dominates interactive use. `/docs/ROADMAP.md` lists it with the key
design (media hash + config hash + model digest).

### 4.7 Interfaces

| Interface | Status |
|---|---|
| CLI (`sonolytica`) | Implemented. Designed for scripts and agents: JSON to stdout, summary to stderr, typed `query` subcommands instead of `jq`, and exit code 2 with `--strict` (or `--profile ultra`) when a stage failed or a requested capability never ran. |
| Rust SDK | The `sonolytica-engine` crate *is* the SDK (`Engine::analyze`). |
| MCP | Next phase. It will be `sonolytica mcp`, using the official `rmcp` SDK (stable MCP 2026-07-28), because agents should subscribe to a server, not shell out and parse logs. The CLI already exposes every operation it needs. |
| HTTP server | Optional later (`axum`), same engine. Not implemented to avoid two half-finished server paths. |

## 5. Temporal model details

- **Unit:** `MediaTime(i64)` microseconds from container start. `TimeRange` is
  `[start, end)`. Sample positions convert exactly through the sample rate.
- **Cross-modal:** every provider returns media-timeline ranges. Providers that
  work on slices add their slice offset (Whisper centiseconds → microseconds).
- **Clamping:** model outputs are clamped to the media duration; a segment that
  ends beyond the file is a validator violation, so this cannot pass silently.
- **Overlap and interruption:** two turns of different speakers that intersect
  produce an `overlaps` edge; if the later turn starts before the earlier ends
  *and* outlasts it, an additional `interrupts` edge. Thresholds (150 ms
  overlap, 300 ms prior turn) are explicit in code and documented as a
  heuristic derived from the diarization output, with no confidence value
  invented for it.
- **Uncertainty:** `Confidence` is a probability or a documented score
  (autocorrelation strength for tempo, profile margin for key). Where the
  number is not calibrated, the provider's `notes` say so.

## 6. Performance and memory

- Decode targets are chosen per capability: 16 kHz mono for VAD/ASR/diarization,
  32 kHz mono for music (16 kHz of bandwidth, standard for chroma/tempo).
- DSP streams frames; the full spectrogram is never materialised. Per-frame
  work is O(fft log fft) with fixed buffers.
- ASR runs per speech region, which also cuts inference on silence-heavy
  recordings.
- Whisper models load lazily: discovery lists them, only the selected model
  allocates memory.
- Peak memory is dominated by decoded PCM (4 bytes/sample) and the model
  weights, both documented in `docs/MODELS.md`.

No latency claims are made yet; `sonolytica-eval report` prints per-stage
milliseconds and the realtime factor for a given machine.

## 7. Known limits

- Diarization uses 10 s segmentation windows, 1.5 s embeddings (0.75 s hop)
  clustered by average linkage. Overlap is detected but **not** split between
  two speakers, and the speaker count is not estimated: `max_speakers` is a cap,
  not a prediction.
- Region-driven ASR can lose words at a region boundary; the first phoneme of
  an utterance is the most common casualty. This is measurable with the WER
  harness and will drive forced alignment later.
- Chord/key estimation is template-based, not a trained MIR model; confidence
  is a score, not a probability.
- Speech emotion, intent, topic segmentation and summarisation are not
  implemented. They are listed in the roadmap as analysis blocks, not added as
  empty fields.
- Video analysis is not implemented in this cycle. The schema already has
  scene/object segments and the timeline is modality-agnostic.
