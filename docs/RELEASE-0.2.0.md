# SONOLYTICA 0.2.0

**Media becomes a document an agent can cite.** Local, typed, timestamped, with
provenance on every claim.

## Highlights

- **AOSC streaming diarization**: 4/4 speakers on the 20-minute `fvhrk`, DER
  6.40 % (was 2/4, 15.99 %); no regression on 1–4 speaker files.
- **Florence-2 keyframe captions** (`--caption`, MIT, 550 MB): descriptions
  such as suit/tie/arms/flags/smile; model-generated observations, not OCR.
- **MoveNet body pose** (`--pose`, Apache-2.0): 17 COCO keypoints per person per
  keyframe, detection only.
- **Analysis cache**: 0.01 s hit vs 7.64 s first run; artifacts travel with the
  document, `analysis_cached` is explicit.
- **`query ask`**: one deterministic evidence pack with absent terms, instead
  of several searches.
- **MCP stdio server** (`sonolytica mcp`): 14 tools for Claude Desktop, Cursor
  and IDE agents; the same binary and cache, no duplicate logic.
- **One-command onboarding**: `sonolytica setup` (core models + skill),
  `--full` for vision; `install.sh` points at it.
- **Docker**: 790 MB image, FFmpeg 7, models in a `/data` volume; verified
  end-to-end on arm64.
- Schema 1.6.0 (pose + captions), 14 pinned models, 173 model-free / 188
  all-features tests.

## Measured

| Fixture | Score | Cost |
|---|---|---|
| hard 4 min | 16/16 | paid median 14 773 fresh; free 19 396, $0 |
| easy 14.4 s | 8/8 | paid median 8 818 fresh; free 18 224, $0 |
| long 20 min | 16/16* | paid median 13 443 fresh; free 31 570, $0 |
| visual boundary | 16/16 refusal; 16/16 caption-aware | 17 652 / 31 961 free |

`*` Q4 corrected: the `fvhrk` RTTM has only `spk02` in the questioned window;
the old overlap spans were merge artifacts. See `docs/bench/overlap.md`.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/neksand/sonolytica/main/install.sh | sh
sonolytica setup                 # core models + skill
sonolytica setup --full          # OCR, faces, objects, pose, Florence
sonolytica mcp                   # MCP clients
```

## Known limits

- Open visual semantics are partial: captions/detectors, no free-form VQA.
- Sortformer caps at 4 speakers; a full four-slot hypothesis is a lower bound.
- Real overlap needs an overlap-aware mask; Parakeet recovered the hard-fixture
  line with a reference mask, but the AOSC S4 activity there is ≤0.011.
- Non-English word alignment is not implemented; Whisper token times remain.
- Music has tempo/key/sections/loudness, no notes or stems.
- A very short clip can be cheaper with a bare Whisper call.

## License

Code MIT OR Apache-2.0. Models keep their own permissive licenses and are not
vendored; see `docs/MODELS.md` and `NOTICE`. FFmpeg is a subprocess.
