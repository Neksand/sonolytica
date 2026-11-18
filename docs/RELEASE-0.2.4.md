# SONOLYTICA 0.2.4

**The human editing layer: interactive reports and subtitle/notes export.**

This is a presentation release. No schema change, no model change, no
benchmark rewrite. Agents still use the same document, query and MCP tools.

## Report

- Transcript search with a live match count.
- Click a range to copy it; with `--media`, the report embeds a local player and
  the same click seeks to that moment.
- `sonolytica report talk.json --out talk.html --media talk.mp4`
- The report remains one self-contained HTML file plus copied keyframes. Inline
  JavaScript only; no network requests.

## Export

New command over the same document:

```bash
sonolytica export talk.json --format srt --out talk.srt
sonolytica export talk.json --format vtt --out talk.vtt
sonolytica export talk.json --format markdown --speakers --ocr --out talk.md
```

- SRT/WebVTT ranges are millisecond-exact document ranges.
- `--speakers` prefixes cues with the diarized speaker when available; it does
  not invent a speaker if diarization did not run.
- Markdown can include OCR text from `--ocr`.
- No text is rewritten or summarised.

## Two front doors, still

- Human: analyze → report → search/seek, or export subtitles/notes.
- Agent: `sonolytica mcp` → cited answers; unchanged 14 tools and skill.
- Both use one document and one cache; `docs/START-HERE.md` covers both.

## Tests

**187 model-free / 204 all-features.**

Release packaging is unchanged: Windows 10/11 x64, macOS arm64, Linux
x86_64/arm64, Docker GHCR, checksummed installers.
