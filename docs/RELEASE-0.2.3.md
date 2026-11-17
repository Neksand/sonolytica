# SONOLYTICA 0.2.3

**Two front doors, one document: a human report path and an AI-agent MCP path.**

This is an onboarding and skill release. No schema change, no model change, no
benchmark rewrite.

## Human path

- [`docs/START-HERE.md`](START-HERE.md) walks from install to an HTML report
  with a public-domain 11-second sample.
- `analyze` → `report` produces one shareable page with chapters, quotes,
  keyframes and OCR; `scripts/demo.sh` wraps the same path.
- No MCP client, JSON reading or query syntax required.

## Agent path

- The same guide gives the MCP config for macOS/Linux and Windows, a first
  prompt, the expected tool sequence (`analyze_media` → `query_brief` →
  `query_ask` → `query_search`/`query_words` → `query_verify`) and a smoke test.
- The skill now chooses the right mode explicitly:
  - human-facing request → analyze + report;
  - agent-facing question → query/MCP with citations;
  - both → analyze once, reuse the document.
- `sonolytica setup` installs the updated skill; re-run it after upgrading.

## Unchanged

- Schema 1.6.0, 14 pinned models, 17 query subcommands, 14 MCP tools.
- 184 model-free / 201 all-features tests; the same benchmark and accuracy
  claims.
- Windows 10/11, macOS arm64 and Linux x86_64/arm64 release packaging.

## Install

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/neksand/sonolytica/main/install.sh | sh
sonolytica setup

# Windows 10/11 x64 (PowerShell)
irm https://raw.githubusercontent.com/neksand/sonolytica/main/install.ps1 | iex
sonolytica setup
```

Then choose a path in [`docs/START-HERE.md`](START-HERE.md).
