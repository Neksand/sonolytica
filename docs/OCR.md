# OCR: implementation notes

On-screen text is read from keyframes with **PP-OCRv5 mobile det + rec** (ONNX,
in-process through `ort`). Two models, 21.4 MB total, Apache-2.0, no Python and
no new system binary. Tesseract was rejected as the default: it is competitive
on English but needs a system package plus `tessdata`, and with `eng` data it
cannot read CJK.

## Status

Implemented in `crates/providers/src/ocr.rs` behind the `ocr` feature (part of
`full`/`full-metal`). CLI: `sonolytica analyze FILE --out doc.json --ocr`.
Each recognised line becomes a `text_overlay` segment on a `text` track, with
text, required `bbox`, confidence, and a point range at the keyframe's instant.
Missing models or a failed frame produce a warning, never a failed analysis.

Measured on real FFmpeg keyframes (1280 px, CPU):

| Fixture | Result |
|---|---|
| slide with two lines | 2/2 exact, confidence 0.987 |
| clean six-line slide | 6/6 exact |
| dark terminal with CJK | 7/7 lines (one `INFO` → `INF0` at small size) |

**CoreML trap:** `ort`'s default device policy auto-registers CoreML on macOS.
CoreML silently mis-executes both PP-OCRv5 graphs (detection collapses,
recognition returns blanks). The shared `onnx::build_session` now sets
`AutoDevicePolicy::PreferCPU`, which also makes the `device: "cpu"` provenance
string true for VAD and diarization.

## Model pins

| Artifact | Size | SHA-256 |
|---|---|---|
| PP-OCRv5 mobile det ONNX | 4,826,518 B | `a431985659dc921974177a95adcfbb90fd9e51989a5e04d70d0b75f597b6e61d` |
| PP-OCRv5 mobile rec ONNX | 16,534,782 B | `da72dc72ca4dc220df0dfde68c1dedc31c58d3e76a25871122e5056227d50092` |
| character dictionary (plain text) | 74,012 B | `d1979e9f794c464c0d2e0b70a7fe14dd978e9dc644c0e71f14158cdf8342af1b` |

Sources: `huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx` and
`..._mobile_rec_onnx` at the pinned revisions; dictionary derived from the rec
`inference.yml`, shipped as `crates/providers/src/ocr_dict.txt`.

## Interface (verified against a Python reference)

Detection: input `x` f32 `[N,3,H,W]`, output `fetch_name_0` `[1,1,H,W]` already
sigmoided.

- RGB → **BGR**, `/255`, mean `[0.485,0.456,0.406]`, std `[0.229,0.224,0.225]`.
- Long side 1280; dimensions **rounded to /32** (the model silently rounds a
  960×541 input to 960×544, which breaks coordinate mapping).
- DB post-processing: threshold 0.3, box threshold 0.6, unclip by
  `area·1.5 / (2·perimeter)`, max 1000 candidates, drop sides < 3 px, scale
  boxes by `original / resized`. A 2 % expansion is not equivalent and destroys
  recognition.
- Connected components use an explicit stack, never recursion.

Recognition: input `x` f32 `[N,3,48,W]`, output `fetch_name_0` `[1,T,18385]`.

- **BGR**, `x/127.5 - 1.0`.
- `W = ceil(48 * w / h)`, rounded up to /8, capped at 4000. Do not squash to
  320; fixed width fragments punctuation.
- CTC table = `["<blank>"] + 18383 dictionary + [" "]`; blank index **0**;
  greedy emit when `index != 0 && index != previous`.
- Dictionary line 0 is U+3000; never trim dictionary lines on load.
- Merge boxes on the same line when vertical overlap > 0.6 and horizontal gap
  < 0.8 × height.

## Tests

- Unit: CTC decoding, box merging, /32 rounding, geometry.
- Opt-in end-to-end (`SONOLYTICA_E2E=1`, `ocr` feature): renders text with
  FFmpeg and asserts the words are found. This machine's FFmpeg lacks
  `drawtext`, so the test skips here; the CLI path was verified on real
  keyframes and on PIL-rendered slides.

## Limits

- One keyframe per scene: text that changes inside a long static shot is missed
  (no cut, no second frame).
- The range is a point at the frame instant, not a claim about how long the
  text was visible.
- Rotated text and non-JPEG inputs are untested.
