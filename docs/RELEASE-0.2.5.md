# SONOLYTICA 0.2.5

**Marketplace-ready skill package for Agensi.io — free listing.**

This is a packaging and skill release. No schema change, no model change, no
benchmark rewrite. The CLI, document and MCP server are the same 0.2.4 code.

## Skill package

- Added explicit **Prerequisites** to `SKILL.md`: local binary, FFmpeg, models,
  `sonolytica setup`; if the binary is missing, the agent must stop rather than
  invent a transcript.
- Added `scripts/package-skill.sh` and a release asset:
  `sonolytica-skill-0.2.5.zip`.
- Zip contents: `SKILL.md` at root, `README.md`, `references/`, licenses and
  `NOTICE`. No binary, no models, no media; well under the 50 MB limit.
- The skill works in agents with local shell access. Shell-less MCP clients use
  `sonolytica mcp`; the zip does not host a backend.

## Agensi listing

Recommended as a **free** listing, not paid: Agensi does not host the runtime
and the buyer installs the free open-source CLI and models separately. Free
listing copy, requirements and the reply to Agensi are in
[`docs/AGENSI.md`](AGENSI.md).

## Unchanged

- Schema 1.6.0, 14 models, 17 query subcommands, 14 MCP tools.
- 187 model-free / 204 all-features tests.
- Windows/macOS/Linux packaging and Docker GHCR.

## Install

```bash
# macOS / Linux
curl -fsSL https://raw.githubusercontent.com/neksand/sonolytica/main/install.sh | sh
sonolytica setup

# Windows
irm https://raw.githubusercontent.com/neksand/sonolytica/main/install.ps1 | iex
sonolytica setup
```
