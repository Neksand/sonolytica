# OCR: design decision (not implemented yet)

On-screen text is the next video capability after keyframes. This file records
the decision and the measured facts so the implementation does not have to
rediscover them. Status: **research done, code not written**.

## Decision

Use **PP-OCRv5 mobile det + rec**, ONNX, in-process through the existing `ort`
runtime. Two models, 21.4 MB total, Apache-2.0, no Python and no new system
binary. Tesseract was rejected as the default: it is competitive on English but
needs a system package plus `tessdata`, and it cannot read CJK with the `eng`
data (measured: 4/7 lines on a dark screencast).

| Artifact | Size | SHA-256 |
|---|---|---|
| PP-OCRv5 mobile det ONNX | 4,826,518 B | `a431985659dc921974177a95adcfbb90fd9e51989a5e04d70d0b75f597b6e61d` |
| PP-OCRv5 mobile rec ONNX | 16,534,782 B | `da72dc72ca4dc220df0dfde68c1dedc31c58d3e76a25871122e5056227d50092` |
| character dictionary (plain text) | 74,012 B | `d1979e9f794c464c0d2e0b70a7fe14dd978e9dc644c0e71f14158cdf8342af1b` |

Sources: `huggingface.co/PaddlePaddle/PP-OCRv5_mobile_det_onnx` and
`..._mobile_rec_onnx` at the pinned revisions; dictionary derived from the rec
`inference.yml`. Licenses: Apache-2.0 (both), ONNX Runtime MIT.

## Interface (measured with onnxruntime 1.31)

Detection: input `x` f32 `[N,3,H,W]`, output `fetch_name_0` `[1,1,H,W]` already
sigmoided.

- RGB → **BGR**, `/255`, mean `[0.485,0.456,0.406]`, std `[0.229,0.224,0.225]`.
- Long side 1280 (960 loses spaces at 16 px); dimensions **rounded to /32**.
  The model silently rounds a 960×541 input to 960×544; assert
  `output.shape == input.shape` or the coordinate mapping breaks.
- DB post-processing: threshold 0.3, box threshold 0.6, unclip 1.5, max 1000
  candidates, drop sides < 3 px, scale boxes by `original / resized`.

Recognition: input `x` f32 `[N,3,48,W]` with dynamic width, output
`fetch_name_0` `[1,T,18385]`.

- **BGR**, `x/127.5 - 1.0`.
- `W = ceil(48 * w / h)`, rounded up to /8, capped at 4000. Do not squash to
  320: fixed width produced `3 1 %` fragmentation on a 500 px line.
- CTC table = `["<blank>"] + 18383 dictionary + [" "]`; blank index **0**;
  greedy emit when `index != 0 && index != previous`.
- Dictionary line 0 is U+3000; never trim dictionary lines on load.
- Merge boxes on the same line when vertical overlap > 0.6 and horizontal gap
  < 0.8 × height; otherwise `$` splits off a monospace line.

## Measured quality

Five synthetic 1280-wide images, CPU:

| Image | Result |
|---|---|
| clean slide, 6 lines | 6/6 exact, confidence 0.92–0.99 |
| dark terminal, 7 lines (CJK) | 7/7 exact, including `中文测试：季度营收增长 31%` |
| 1080p slide downscaled to 1280 | 6/6 lines, 2 lost spaces at 17 px text |
| blurred, q35, noisy | 5/6 lines exact, 2 character errors |

Timing: cold session load 62 ms; detection 1280×704 ≈ 90 ms; recognition of 6
lines ≈ 60 ms. About 150 ms per keyframe.

## Risks

1. DB post-processing geometry fails silently (missing or offset text). Use an
   explicit `Vec<u32>` label array with an iterative stack.
2. A YAML parser for the dictionary would shift punctuation indices; ship the
   normalised plain-text dictionary.
3. The workspace has no JPEG decoder; add `image` with only the `jpeg` feature,
   or request `rgb24` from the keyframe FFmpeg call.
4. The rec output is ~51 MB at the 4000 px cap; keep the width cap.

Estimated implementation: ~450–500 lines (detection + recognition + CTC +
line merging in `crates/providers/src/ocr/`, two `ModelSpec` entries, CLI and
JSON wiring, dictionary via `include_str!`). The riskiest part is DB
post-processing, not the models.

## Integration shape

- `Capability::Vision` already exists; add a `text_overlay` segment kind with
  `text` and optional `bbox`, one per merged line, on a `text` track.
- Run on keyframes only, in `ultra` (or an explicit `--modules vision`), and
  deduplicate identical text across adjacent frames into one range.
- Every line carries the keyframe's instant; the segment range is the frame's
  range, not a claim about when the text appeared.
