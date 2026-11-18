# Start here — two ways to use SONOLYTICA

SONOLYTICA has one engine and two front doors. Choose the one that matches how
you work; both use the same local models, the same document and the same cache.

| You are | Start with | You get |
|---|---|---|
| A **person** reviewing a recording | `sonolytica analyze` + `sonolytica report` | A self-contained HTML page with chapters, quotes, keyframes and OCR |
| An **AI agent** answering questions | `sonolytica mcp` + the installed skill | Cited answers with exact ranges, or a checked absence |

A 20-minute file is one document either way. The human path opens it; the agent
path queries it. You can switch at any time without re-analysing.

## Install once (both paths)

Requirements: **FFmpeg 7+** and about 1.5 GB for the core models.

### macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/neksand/sonolytica/main/install.sh | sh
sonolytica setup                 # models + agent skill
sonolytica doctor                # FFmpeg, providers, build
```

If `doctor` says FFmpeg is missing:

```bash
# macOS
brew install ffmpeg
# Debian/Ubuntu
sudo apt install ffmpeg
```

### Windows 10/11 x64

```powershell
irm https://raw.githubusercontent.com/neksand/sonolytica/main/install.ps1 | iex
winget install Gyan.FFmpeg       # or: choco install ffmpeg
sonolytica setup
sonolytica doctor
```

`setup --full` adds OCR, faces, objects, pose and Florence captions
(+~0.5 GB). Use it when the media has slides, screens or people you care about.
The base `setup` is enough for speech, speakers, music and the report; if you
use the Path A command below unchanged, run `sonolytica setup --full` first (or
drop `--ocr --caption`).

## Path A — Human: read the result

Use this when you want to watch, skim, share or print the analysis yourself.

```bash
sonolytica analyze talk.mp4 --out talk.json --profile standard --ocr --caption --summary
sonolytica report talk.json --out talk.html --media talk.mp4
open talk.html                  # macOS; use `start talk.html` on Windows
```

`--media` is optional: when supplied, the report gets a player and clicking a
turn's range seeks to that moment and copies the exact range. Without it, the
report still has transcript search and copyable ranges.

Need subtitles or notes for an editor?

```bash
sonolytica export talk.json --format srt --out talk.srt
sonolytica export talk.json --format vtt --out talk.vtt
sonolytica export talk.json --format markdown --speakers --ocr --out talk.md
```

`--speakers` prefixes cues with the diarized speaker; `--ocr` adds on-screen
text to Markdown. The export is a view over the same document, not a new
analysis.

The report contains:

- chapters and scene cuts;
- speaker turns and exact quotes;
- keyframes from the video;
- OCR text per keyframe;
- music/acoustic measurements with confidence;
- warnings and partial stages, shown rather than hidden.

No media handy? The 11-second public-domain JFK excerpt used in the tests works:

```bash
curl -fsSL https://github.com/ggerganov/whisper.cpp/raw/master/samples/jfk.wav -o jfk.wav
sonolytica analyze jfk.wav --out jfk.json --profile standard --ocr --summary
sonolytica report jfk.json --out jfk.html
```

From this repository, the same path is one command:

```bash
scripts/demo.sh talk.mp4
# document: talk.sonolytica.json
# report:   talk.sonolytica.html
```

If you only need a quick map in the terminal:

```bash
sonolytica query brief talk.json
sonolytica query search talk.json budget
```

Good first files: a 1–5 minute meeting clip, a lecture with slides, or a podcast
intro. Long files are fine; the first pass is `structure`, then `standard`.

## Path B — Agent: answer with citations

Use this when Claude Desktop, Cursor, an IDE agent or your own MCP client should
answer questions about the file. The agent does not need a shell.

### 1. Add the MCP server

macOS/Linux:

```json
{
  "mcpServers": {
    "sonolytica": {
      "command": "sonolytica",
      "args": ["mcp"]
    }
  }
}
```

Windows:

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

Restart the client. `sonolytica setup` already installed the skill into Claude
Code, Codex, OpenCode and Cursor. If you use another runtime:

```bash
sonolytica install-skill --target opencode   # or claude, codex, cursor, all
```

### 2. Ask a question, not a format

```text
Use SONOLYTICA for /Users/me/meeting.mp4.
What did they decide about the launch date?
Quote the exact words and give the timestamp. If it was never said, say so.
```

The agent should:

1. call `analyze_media` only if no document exists;
2. call `query_brief` to see stages, speakers and chapters;
3. call `query_ask` with the question; this ranks evidence and reports absent
   terms;
4. use `query_search` / `query_words` for the exact quote and range;
5. use `query_verify` before presenting a quote as checked;
6. say **partial** or **failed** if a stage did not run.

Expected answer shape:

```text
«We are moving the launch to 14 March.» — S2, 00:14:12.300–00:14:19.800
S1 asked about the partner webinar; no decision was stated for it.
Stage status: complete; diarization used 2 speakers.
```

### 3. Smoke-test the server

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' |
  sonolytica mcp
```

You should see `tools` and 14 tools. The full tool list and per-tool arguments
are in [`MCP.md`](MCP.md).

## Which path should I use?

- **“Give me the report / notes / a page I can share”** → Path A.
- **“Ask my agent questions about this file”** → Path B.
- **“I want both”** → analyze once in Path A, then point the MCP `document`
  argument at the same JSON. `report` does not consume or invalidate it.

## What to expect (and not expect)

- The first run downloads/loads models; later runs reuse the model store and the
  analysis cache.
- A missing model disables one stage with a warning; it does not fabricate that
  stage. `--profile ultra` exits 2 instead of silently degrading.
- The document is local. Nothing is uploaded.
- Speaker labels are local to the document, not identities.
- Music BPM/key/chords appear only when `music.present` is true; speech can
  still have loudness.
- Closed captions, emotion, speaker identity and free-form visual Q&A are not
  claims this release makes.

Platform details: [`WINDOWS.md`](WINDOWS.md), [`DOCKER.md`](DOCKER.md).
Commands: [`README.md`](../README.md#command-reference).
