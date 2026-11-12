# Launch brief

**One line:** SONOLYTICA turns one media file into one typed, timestamped
document that a text-only agent can cite — local, private, and free after the
models are downloaded.

It is not a multimodal model. It is the **media layer for agents that cannot
see or hear**: exact quotes, ranges, speakers, on-screen text, shots, keyframes,
captions, pose and music facts, with provenance on every claim. The agent reads
the document through `query` or MCP; every answer is checkable.

## Measured scoreboard

Same fixtures, same prompts, 0/2 per task. Sources: `docs/BENCHMARK.md`,
`docs/EVAL.md`; raw outputs in `docs/bench/2026-11-12/`.

| Fixture | SONOLYTICA + skill | Gemini + skill | Gemini alone | TwelveLabs Pegasus |
|---|---|---|---|---|
| hard 4 min (max 16) | **16/16** | **16/16** | 14/16 | 10/16 |
| easy 14.4 s (max 8) | **8/8** | **8/8** | 6/8 | 3/8 |
| long 20 min (max 16) | **16/16*** | **16/16** | not run¹ | 10/16 |
| blind judge, hard | **15/16**, **19 valid citations** | 16/16, 19 | 10/16, 7 | 7/16, 3 |

`*` Q4 corrected: the `fvhrk` RTTM has only `spk02` at 19:15–19:32; the old
"overlap" spans were merge artifacts. The AOSC document reports no overlap,
which is correct. Old long runs rescored 14/16 / 13/16.

¹ Antigravity ran out of credits during the baseline.

Other measured facts:

| Area | Number |
|---|---|
| Diarization `fvhrk` | **4/4** speakers, DER **6.40 %**, JER 6.29 % (was 2/4, 15.99 %) |
| 1–4 speaker no-regression | counts exact; DER holds or improves (`dxbbt` 10.89 → 5.66) |
| Throughput | **26× realtime** on the 20-minute `fvhrk`; 45 s end-to-end, 2.07 GB peak RSS |
| ASR | WER **6.05 %**, CER 2.25 % (LibriSpeech test-clean, `whisper-base.en`) |
| Free models | easy **8/8 / 18 224**, hard **16/16 / 19 396**, long **16/16 / 31 570**, all **$0** |
| Visual boundary | **16/16** refusal without captions; **16/16 caption-aware**, 6/8 answered from the caption, 0 unsupported guesses |
| Cache | **0.01 s** hit vs 7.64 s first run on the 14.4 s fixture |

## How much stronger — and where not

**Defensible lead: structured, citable media.**
- Matches the top score on all three scored fixtures, including the long
  20-minute meeting.
- **2.7× more valid citations than Gemini alone** in the blind judge (19 vs 7)
  and 6.3× more than TwelveLabs (3), with exact ranges; tied with Gemini+skill
  at 19 citations while scoring 15/16 vs 16/16.
- **Local and $0 marginal cost** after models; TwelveLabs is $1.75/hour of video.
- **26× realtime** analysis on a 20-minute file; queries are instant.
- MCP + Docker + one-command setup; no shell required for the agent.

**Not a claim:** "better than Gemini at seeing." Florence-2 captions answer
"what is the person wearing / doing" on a keyframe (6/8 visual-boundary
questions), but open-ended visual semantics remain Gemini/TwelveLabs territory.
There is no free-form VQA model yet.

**Known gaps, stated plainly:**
- **Real overlap**: the hard fixture has a 1.4 s S3/S4 overlap. Parakeet
  multitalker with a reference mask recovered the missing line, but the AOSC
  diarizer's S4 activity there is ≤0.011, so the product mask is empty. Do not
  claim solved overlap; the next cycle needs an overlap-aware diarizer.
- **5+ speakers**: Sortformer caps at 4; a four-slot hypothesis carries
  `diarization_speaker_cap`, so the count is a lower bound.
- **Visual semantics**: keyframe-bound, model-generated, can hallucinate
  (slide background called "black" on a red slide).
- **Non-English alignment**: English wav2vec2; other languages keep Whisper
  token times.
- **Music**: tempo/key/sections/loudness; no notes or stems.
- **Tiny clips**: a bare Whisper call can be cheaper than the full pipeline.
- **Free models are not interchangeable**: Flint 40B got 8/8 but spent 76 196
  fresh in 14 commands; DeepSeek free was 18 224.

## Real use cases

1. **Meeting and interview intelligence for agents.** Who said what, exact
   quotes, action items, decisions, budget changes, absence checks — MCP tools
   for Claude Desktop/Cursor, no upload.
2. **Lecture, webinar and screencast Q&A.** Transcript + slides OCR + chapters
   + keyframes; a study guide with timestamps.
3. **Podcast/video edit search.** Find the moment, the speaker, the music
   section, the BPM; export clips.
4. **Legal, compliance and security review.** Exact quotes and ranges, local
   data, no cloud; "was X ever mentioned" is a checked absence, not a guess.
5. **Offline/private deployments.** Hospitals, law firms, newsrooms, field
   teams; Docker image and a `/data` volume.
6. **Agent/RAG over media.** One document per file, citations, `query verify`;
   no vector database needed for a single recording.

## Launch checklist

- [x] release binary + checksummed `install.sh`
- [x] `sonolytica setup` (core models + skill), `--full` vision
- [x] MCP stdio server (14 tools) + `docs/MCP.md`
- [x] Dockerfile + GHCR workflow; image verified on arm64 (790 MB)
- [x] agent skill for Claude Code, Codex, OpenCode, Cursor
- [x] measured benchmark, eval methods, model licences, `NOTICE`
- [ ] tag `v0.2.0` and publish GitHub release (user pushes)
- [ ] brew tap (needs a separate repository)
- [ ] 2-minute demo video and three example documents
- [ ] MCP directory listings (Claude/Cursor) and a launch post
- [ ] real-overlap benchmark on AMI and an overlap-aware mask

## What to say / not say

Say: local, citable, exact ranges, 4/4 on the 20-minute meeting, 16/16 on the
hard and easy structured tasks, $0 after models, MCP/Docker/one-command setup.

Do not say: "beats Gemini at vision", "unlimited speakers", "solves overlapping
speech", or "works for every language". The honest edge is **structured media
with citations**, not general perception.

## Positioning

For an AI agent, media is not a prompt — it is a database. SONOLYTICA builds
that database once, locally, and makes every answer checkable. Gemini and
TwelveLabs are stronger when the question is "what is happening in this
picture"; SONOLYTICA is stronger when the question is "who said this, when
exactly, and can you prove it".
