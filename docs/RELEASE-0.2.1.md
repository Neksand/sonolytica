# SONOLYTICA 0.2.1

**Windows 10/11 x64 support, with the same local, citable document as macOS and
Linux.**

This is a platform release: no schema change, no benchmark regression, no new
model. The pipeline, 14 pinned models, 17 query subcommands, 14 MCP tools, cache
and HTML report are the same code.

## Windows

- Native **x86_64-pc-windows-msvc** binary, one zip for **Windows 10 and 11**.
  Both share the Win32 x64 ABI; separate binaries would be byte-identical.
- `install.ps1`: downloads the release zip, verifies SHA-256, installs
  `sonolytica.exe` and `sonolytica-eval.exe` under
  `%LOCALAPPDATA%\sonolytica\bin`.
- `%USERPROFILE%` fallback for the model/cache/skill directories; native
  PowerShell does not set `HOME`.
- Windows font fallback for the OCR drawing test.
- CI builds and tests the model-free workspace on Windows and builds the full
  provider stack (`ort`, whisper.cpp, Kaldi fbank) in release mode.
- FFmpeg remains an external runtime requirement:
  `winget install Gyan.FFmpeg`.

**Windows 8/8.1, Windows 7, Windows on ARM and 32-bit Windows are not
supported.** Windows 8 is end-of-life and outside the current Rust/MSVC, ONNX
Runtime and FFmpeg toolchains; a separate legacy build would be an unmaintained
second product. Use a supported OS, Linux, or Docker.

## Install

Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/neksand/sonolytica/main/install.ps1 | iex
sonolytica setup
```

macOS arm64 / Linux x86_64 / Linux arm64:

```bash
curl -fsSL https://raw.githubusercontent.com/neksand/sonolytica/main/install.sh | sh
sonolytica setup
```

## Platforms

| Platform | Artifact |
|---|---|
| Windows 10/11 x64 | `sonolytica-x86_64-pc-windows-msvc.zip` |
| macOS arm64 | `sonolytica-aarch64-apple-darwin.tar.gz` |
| Linux x86_64 | `sonolytica-x86_64-unknown-linux-gnu.tar.gz` |
| Linux arm64 | `sonolytica-aarch64-unknown-linux-gnu.tar.gz` |

Every archive has a matching `.sha256`.

## Unchanged evidence

| Claim | Value |
|---|---|
| hard 4-minute fixture | 16/16 structured; blind judge 15/16, 19 valid citations |
| long 20-minute meeting | 16/16*; 4/4 speakers, DER 6.40 % |
| easy fixture | 8/8 |
| 20-minute full audio pipeline | 45.1 s (26× realtime), 2.07 GB peak RSS |
| OCR | 12/12 slide lines; 7/7 CJK terminal |
| tests | 173 model-free / 188 all-features |

`*` Q4 corrected: the `fvhrk` RTTM has only `spk02` in the questioned window;
the old overlap spans were merge artifacts. See `docs/bench/overlap.md`.

## Known limits

- Real overlapping speech needs an overlap-aware diarizer mask; Parakeet
  recovered the hard-fixture line only with a reference mask.
- Sortformer caps at 4 speakers.
- Captions are keyframe observations, not free-form visual Q&A.
- Whisper word alignment is English-only.
- Music has tempo/key/sections/loudness, no notes or stems.

These are the same boundaries as 0.2.0 and remain on the roadmap in the README.

## License

Code MIT OR Apache-2.0. Models keep their own permissive licenses and are never
vendored. See `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` and `docs/MODELS.md`.
