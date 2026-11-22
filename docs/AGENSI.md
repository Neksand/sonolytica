# Agensi listing — free skill package

Status: **ready to upload** as a free listing. No account was created and
nothing was submitted to Agensi from this repository.

Agensi ships skill zips, not live servers. SONOLYTICA is a local CLI plus
models, so creating a paid listing would conflict with Agensi's own rule that
a paid skill must not require a separately accessed external service. The
honest listing is **free**.

## Build the package

```bash
./scripts/package-skill.sh
# dist/sonolytica-skill-0.2.5.zip
```

The zip contains `SKILL.md` at the root, `README.md`, `references/`, licenses
and `NOTICE`. It contains no binary, model, media or backend. Current size is
about 36 KB zipped (77 KB unpacked), well below the 50 MB upload limit.

## Listing fields

**Name**

```text
SONOLYTICA — Citable Media Analysis
```

**Short description**

```text
Turn audio and video into timestamped, citable documents your AI agent can
search, quote and verify. Local, private, no upload.
```

**Long description**

```markdown
## What it does

SONOLYTICA turns one audio or video file into one typed, timestamped document:
speech with word timings and speaker turns, scenes and keyframes, on-screen
text, faces, objects, pose, captions, music and acoustics.

Your agent queries that document instead of guessing. Every answer can carry an
exact quote and range, or a checked absence ("the transcript never mentions X").
The document is generated locally; media is not uploaded.

## Great for

- meeting and interview notes with speaker-attributed quotes;
- lecture, webinar and screencast Q&A with slide OCR;
- podcast/video edit search by moment, speaker, music or BPM;
- legal, compliance and private deployments where upload is not allowed;
- agents that need citations instead of a plausible summary.

## Requirements (important)

This is a **free skill package**, not a hosted service. It calls a local
open-source CLI and does not include the CLI or models.

- macOS arm64, Linux x86_64/arm64, or Windows 10/11 x64;
- an agent with local shell access;
- FFmpeg 7+;
- about 1.5 GB of core models, downloaded by `sonolytica setup`.

Install the CLI:

macOS/Linux:
```bash
curl -fsSL https://raw.githubusercontent.com/neksand/sonolytica/main/install.sh | sh
sonolytica setup
```

Windows PowerShell:
```powershell
irm https://raw.githubusercontent.com/neksand/sonolytica/main/install.ps1 | iex
sonolytica setup
```

Then run `sonolytica doctor`. If it fails, the agent must stop and ask for the
missing prerequisite; it must not invent a transcript.

Shell-less MCP clients (Claude Desktop, Cursor, IDE agents) can use the separate
`sonolytica mcp` server instead.

## What is in the package

`SKILL.md`, references for meetings/interviews/lectures/music/video, citation
rules, troubleshooting, query cheatsheet, licenses and NOTICE. No binary, no
models, no scripts that execute automatically.

## Limits

- 4-speaker diarization cap; real overlapping speech is labeled, not split word
  by word.
- Captions are keyframe observations, not free-form visual Q&A.
- English forced alignment; other languages use Whisper token times.
- Music gives tempo/key/chords/loudness, not notes or stems.

## Open source

Code: MIT OR Apache-2.0. Models keep their own permissive licenses. Source:
https://github.com/neksand/sonolytica
```

**Tags**

```text
media-analysis, transcription, speaker-diarization, video-understanding,
on-screen-text, OCR, RAG, citations, local-first, privacy, MCP, Whisper,
podcast, meeting-notes, lecture-notes
```

**Price:** Free.

**Affiliate:** use the link Agensi provides for the creator account, if the
plan supports free listings. Ask before assuming it applies to a free listing.

## Reply to Joeri

```text
Hi Joeri,

Thanks — we'd like to list it.

We'll make it a free listing. SONOLYTICA is a local open-source CLI plus
locally downloaded models; the skill package itself is Markdown/references and
does not include the runtime. Per your point 2, a paid listing would require a
separately installed service, so free with explicit setup requirements is the
honest option.

The zip is ~36 KB, has SKILL.md at the root, no binary/model/media, and states
macOS/Linux/Windows requirements, FFmpeg, `sonolytica setup`, and that the agent
must stop if `doctor` fails. It works in agents with local shell access;
shell-less clients can use the separate MCP server.

Two questions:
1. Do affiliate links apply to free listings?
2. Is requiring a local CLI + models acceptable for the listing, provided it is
   clearly disclosed as a prerequisite (not hosted by Agensi)?

Best,
Nurassyl
```

## Upload steps

1. Sign in at <https://agensi.io/sell> with the creator account.
2. Enable creator mode.
3. Upload `dist/sonolytica-skill-0.2.5.zip`.
4. Paste the name, short description, long description, tags and free price.
5. Submit for automated scan + manual review.
6. If review asks for changes, keep the free model and update the copy; do not
   claim that Agensi hosts the runtime.

Do not publish a paid listing until the package can run without a separately
installed binary and models.
