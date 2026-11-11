# ADRs

Architecture decision records, in the order the decisions were made:

1. [Rust core with an isolated provider boundary](0001-rust-core-provider-boundary.md)
2. [FFmpeg as a subprocess](0002-ffmpeg-subprocess.md)
3. [Hierarchical tracks plus typed relation edges](0003-hierarchical-timeline.md)
4. [whisper.cpp for ASR, ONNX Runtime for the rest](0004-runtime-selection.md)
5. [Decode speech regions, not the whole file](0005-region-based-asr.md)

A new ADR is required when a decision changes a public trait, the document
schema, a runtime dependency, or a stated limitation.
