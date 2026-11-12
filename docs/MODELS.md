# Models

Every model is a registry entry in `crates/providers/src/models.rs`, pinned by
SHA-256, with its license and a one-line statement of what it does. Nothing is
downloaded implicitly; `sonolytica models fetch <id>` verifies the digest
before the file becomes visible to a provider.

The artifacts are **not** vendored in this repository. Weights are large, and
their licenses are their own.

## Registry

| id | Capability | Approx. size | License | Source |
|---|---|---|---|---|
| `silero-vad-v5` | VAD | 2.3 MB | MIT | silero-vad repository (`src/silero_vad/data/silero_vad.onnx`) |
| `whisper-tiny.en` | ASR (English) | 78 MB | MIT | ggerganov/whisper.cpp GGML |
| `whisper-base.en` | ASR (English) | 148 MB | MIT | ggerganov/whisper.cpp GGML |
| `whisper-small` | ASR (multilingual) | 488 MB | MIT | ggerganov/whisper.cpp GGML |
| `sortformer-4spk-v2` | Diarization (up to 4 speakers, overlap) | 469 MB | CC-BY-4.0 | ONNX export of `nvidia/diar_streaming_sortformer_4spk-v2` |
| `pyannote-segmentation-3.0` | Diarization (speaker change) | 6 MB | MIT | pyannote-rs release (ONNX export of pyannote/segmentation-3.0) |
| `wespeaker-campplus` | Speaker embeddings | 28 MB | Apache-2.0 | pyannote-rs release (WeSpeaker CAM++) |
| `pp-ocrv5-det` / `pp-ocrv5-rec` | OCR detection + recognition | 4.8 / 16.5 MB | Apache-2.0 | PaddlePaddle ONNX release |
| `yunet-face-detection` | Face detection (count and boxes) | 0.2 MB | MIT | OpenCV Zoo, `face_detection_yunet_2023mar.onnx` |
| `yolos-tiny` | Object detection (91 COCO classes) | 26 MB | Apache-2.0 | Xenova/yolos-tiny ONNX export |

Pinned digests live in the registry and are printed by `sonolytica models
list`. If a file's digest does not match, it is rejected and the run reports a
`provider_unavailable` warning naming the fix.

## What each model is used for — and what it is not

- **Silero VAD** is a speech/non-speech classifier on 16 kHz audio. It is used
  to find speech regions, not to identify speakers or words. DSP silence
  detection is separate and reported under `analyses.acoustic.silence`; the two
  are not interchangeable.
- **Whisper** produces text, language and word timestamps from token-level
  timestamps (10 ms resolution). This is **not forced alignment**: word
  boundaries are model estimates. Long-form decoding is done per detected
  speech region plus 300 ms of digital silence, because whole-file decoding can
  drop everything after a short utterance when the model emits an early
  end-of-text (an upstream Whisper/whisper.cpp behaviour, reproduced with a
  bare `whisper_full` call on a two-voice fixture).
- **Sortformer 4spk v2** is an end-to-end diarizer: 128-bin NeMo log-mel in,
  per-speaker activity at 80 ms out. SONOLYTICA computes the mel front-end in
  Rust and thresholds the activity into turns. It resolves overlap into two
  simultaneous turns, which the pyannote path only flags. It tracks at most
  four speakers; beyond that voices are collapsed, so the doc says "up to 4".
  Long files are processed in 300 s windows with 10 s overlap and the speaker
  channels are matched by activity co-occurrence across the seam. Past ten
  minutes voices can merge even with fewer than four speakers; the document
  carries a `diarization_speaker_cap` warning so an agent cannot cite the count
  as exact.
- **pyannote segmentation-3.0** emits powerset classes on 10 s windows:
  silence, three local speakers, or a two-speaker overlap. It remains the
  fallback when Sortformer is not installed or inference fails. SONOLYTICA uses
  single-speaker regions for embeddings and reports overlap globally; it does
  not invent two simultaneous speaker turns from the overlap classes.
- **YOLOS-tiny** detects 91 COCO object classes on extracted keyframes. It is a
  DETR-style query model: softmax over class ids (id 91 is background), boxes
  normalised to the resized image, threshold 0.5. Class, box and score only; no
  tracking, attributes or identity. Small objects and dense scenes are missed
  at keyframe resolution — the document says "not detected", never "absent".
- **YuNet** detects faces on extracted keyframes: boxes, five landmarks and a
  score. It does **not** produce embeddings, names or identities; the document
  stores `object` segments with class `face` and a box. Counts are per sampled
  keyframe, not per frame, so a face that appears between keyframes is missed.
- **WeSpeaker CAM++** produces 256-dimensional speaker embeddings from Kaldi
  fbank features. Clustering (cosine similarity, average linkage, budget-aware
  merging) is implemented in `crates/providers/src/diarize.rs`, not in a model.
  Speaker labels are document-local.

## Feature flags

| Feature | Enables | Native requirement |
|---|---|---|
| `silero` | VAD via ONNX Runtime | prebuilt ONNX Runtime (downloaded by `ort`) |
| `whisper` | ASR via whisper.cpp | cmake, C/C++ toolchain |
| `diarize` | Sortformer, pyannote segmentation and WeSpeaker embeddings via ONNX Runtime; Kaldi fbank for the fallback | ONNX Runtime (fbank path needs cmake) |
| `vision` | YuNet face detection via ONNX Runtime | prebuilt ONNX Runtime |
| `metal` | whisper.cpp Metal backend (Apple silicon) | macOS |
| `full` | all of the above | cmake, toolchain, ONNX Runtime |

Without any of them the binary still runs `inspect`, DSP music analysis and
acoustic analysis, and the test suite passes.

## Reproducibility

- `media.sha256` identifies the exact input bytes.
- `RunRecord.params_sha256` is the SHA-256 of the serialized `AnalysisConfig`.
- `RunRecord.model.sha256` identifies the weights.
- `RunRecord.device` records where it ran (`cpu`, `metal`).

Given those four plus the model files, a result can be reproduced or its
divergence explained.

## Licensing notes

- The project itself is MIT OR Apache-2.0.
- All registered models are permissively licensed (MIT / Apache-2.0 /
  CC-BY-4.0). GPL music-analysis crates (`bliss-audio`, `aubio-rs`, `sonogram`)
  were evaluated and rejected; GPL is also blocked in CI by `cargo-deny`.
  Sortformer's CC-BY-4.0 attribution stays in its `ModelSpec` and in
  `docs/EVAL.md`.
- FFmpeg is invoked as a separate process. A distributed binary does not link
  it, but users who redistribute FFmpeg must comply with FFmpeg's own license.
  Homebrew's build is GPL; an LGPL build works identically.
- Datasets for evaluation are **not** bundled. Dataset license and citation
  requirements are documented in `docs/ROADMAP.md` and must be recorded in any
  published benchmark report.

## Adding a model

1. Add a `ModelSpec` with a real URL, SHA-256, license and size.
2. Implement the relevant provider trait; keep the runtime behind a feature.
3. Return `ProviderError::ModelMissing` with an actionable hint.
4. Add it to `docs/MODELS.md` with its limits.
5. Prove it with a functional test, and only then claim anything about quality.
