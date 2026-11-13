# Windows

SONOLYTICA ships one native **x64** build for **Windows 10 and Windows 11**.
Both share the same Win32 ABI; separate binaries would be byte-identical and
only create two support surfaces. The release is built with the MSVC toolchain.

CI runs the model-free suite on `windows-2022` (the Windows 10-era Server
kernel) and the full provider build on `windows-latest` (the Windows 11-era
Server kernel). These are the closest GitHub-hosted proxies for the two desktop
generations; there is no separate Windows 10 or 11 binary.

## Supported

| OS | Status | Notes |
|---|---|---|
| Windows 11 x64 (23H2+) | supported | same zip as Windows 10 |
| Windows 10 x64 (22H2) | supported | same zip as Windows 11 |
| Windows 8 / 8.1 | **not supported** | end-of-life; see below |
| Windows 7 | **not supported** | end-of-life |
| Windows on ARM | **not supported** | use Linux arm64 or Docker on a supported host |
| 32-bit Windows | **not supported** | the models and release are x64 |

Windows 8/8.1 is not a packaging omission. It is outside the support matrix of
the current Rust/MSVC target, ONNX Runtime prebuilt binaries and FFmpeg builds,
and it receives no security updates. A legacy build would be an unmaintained
second product; use a supported Windows, Linux, or the Docker image instead.

## Install

PowerShell 5.1 or newer:

```powershell
irm https://raw.githubusercontent.com/neksand/sonolytica/main/install.ps1 | iex
```

The script downloads the latest
`sonolytica-x86_64-pc-windows-msvc.zip`, verifies its SHA-256, and installs
`sonolytica.exe` and `sonolytica-eval.exe` to:

```text
%LOCALAPPDATA%\sonolytica\bin
```

It does not modify `PATH`. Add the directory for the current session:

```powershell
$env:Path = "$env:LOCALAPPDATA\sonolytica\bin;$env:Path"
```

Or persist it for your user:

```powershell
[Environment]::SetEnvironmentVariable(
  'Path',
  "$env:LOCALAPPDATA\sonolytica\bin;$env:Path",
  'User'
)
```

If script execution is blocked:

```powershell
Set-ExecutionPolicy -Scope Process Bypass
irm https://raw.githubusercontent.com/neksand/sonolytica/main/install.ps1 | iex
```

The binaries are not code-signed yet; SmartScreen may ask for confirmation.
Verify the SHA-256 from the release page if your policy requires it.

### Manual install

1. Download `sonolytica-x86_64-pc-windows-msvc.zip` and its `.sha256` from the
   [latest release](https://github.com/neksand/sonolytica/releases/latest).
2. Compare the hash:

   ```powershell
   (Get-FileHash .\sonolytica-x86_64-pc-windows-msvc.zip -Algorithm SHA256).Hash
   Get-Content .\sonolytica-x86_64-pc-windows-msvc.zip.sha256
   ```

3. Extract the zip and put `sonolytica.exe` and `sonolytica-eval.exe` in a
   directory on `PATH`.

### FFmpeg

FFmpeg 7+ is a runtime requirement and is not bundled in the Windows zip:

```powershell
winget install Gyan.FFmpeg
# or
choco install ffmpeg
```

Open a new terminal, then check:

```powershell
ffmpeg -version
sonolytica doctor
```

### Models and first run

```powershell
sonolytica setup          # core models (~1.5 GB) + agent skill
sonolytica setup --full   # OCR, faces, objects, pose, Florence captions
```

Models and the analysis cache live under:

```text
%USERPROFILE%\.cache\sonolytica\models
%USERPROFILE%\.cache\sonolytica\cache
```

Override them with `SONOLYTICA_MODEL_DIR` and `SONOLYTICA_CACHE_DIR` (or
`--models-dir` / `--cache-dir`). Models are downloaded on demand, verified by
SHA-256 and never uploaded.

## MCP on Windows

Use the absolute path if `sonolytica` is not on the client's `PATH`:

```json
{
  "mcpServers": {
    "sonolytica": {
      "command": "C:\\Users\\you\\AppData\\Local\\sonolytica\\bin\\sonolytica.exe",
      "args": ["mcp"]
    }
  }
}
```

The server is local stdio JSON-RPC; it does not open a port. All 14 tools run
the same executable and cache as the CLI.

## Shell completion

```powershell
sonolytica completions powershell | Out-String | Invoke-Expression
```

To persist it, write the output to your PowerShell profile.

## Build from source

Requirements: Rust 1.85+, Visual Studio 2022 Build Tools (Desktop development
with C++), CMake, LLVM/libclang, FFmpeg.

```powershell
choco install cmake llvm ffmpeg -y
$env:LIBCLANG_PATH = 'C:\Program Files\LLVM\bin'

cargo build --release -p sonolytica-cli --features full
cargo build --release -p sonolytica-eval
```

The full build uses ONNX Runtime (downloaded by `ort` for
`x86_64-pc-windows-msvc`), whisper.cpp (CMake) and Kaldi fbank. The release job
does exactly this in `.github/workflows/release.yml`; CI also runs the built
`sonolytica.exe --version` and parses `install.ps1` on Windows.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `ffmpeg not found` | install FFmpeg, open a new terminal, run `where.exe ffmpeg` |
| `sonolytica` is not recognized | add `%LOCALAPPDATA%\sonolytica\bin` to `PATH` |
| `libclang.dll` not found while building | install LLVM and set `LIBCLANG_PATH` |
| SmartScreen warning | unsigned binary; verify the release SHA-256 |
| Model download fails behind a proxy | set `HTTPS_PROXY` for the process |
| MCP client cannot start the server | use the absolute `sonolytica.exe` path in the config |
| Diarization warning `diarization_speaker_cap` | expected with more than 4 speakers; see `docs/MODELS.md` |

## What is not Windows-specific

The document schema, 17 query subcommands, 14 MCP tools, HTML report, cache and
model registry are identical on macOS, Linux and Windows. The same benchmark
claims apply; Windows is a packaging and runtime-support addition, not a fork.
