# Dogfood: the agent skill, before and after

Date: 2026-11-11
Method: a fresh agent session per run, given only `skills/sonolytica/SKILL.md`
and a media file or its document. No repository browsing, no access to the
plans. Every shell command it ran was recorded.

Fixture: `/private/tmp/sono-query-e2e/pricing.wav` — 14.4 s, two synthetic voices
(macOS `say`), audio only, `whisper-base.en`, all five stages complete.
A second document from the same audio with `--language en` was used for the
baseline.

## Baseline (the previous jq-based skill)

Two questions: "on what second was *pricing* said, quote it" and "who spoke
more, with turn timestamps".

- The agent ran **8 `jq` commands**, including a `grep -o 'pricing\|preach…'`
  pipeline, to answer two questions.
- It answered honestly (no invented quote), but only because the old skill said
  "do not invent". It had no rule for a missing word, no rule for which range
  to cite when `speech` and `speaker_turn` differ, and no explanation of
  `speech_duration` versus the sum of turns.
- It reported `analyses[speech].language = "my"` for English audio, a provider
  bug: whisper.cpp never sets `lang_id` for English-only models, and the
  provider read that field anyway. Fixed in `crates/providers/src/whisper.rs`
  (`.en` → `en`, explicit `--language` wins, multilingual → auto-detect) with a
  regression assertion in `e2e_speech`.

Baseline defects: 8 shell pipelines, 5 ambiguities, 1 data bug. Fabricated
citations: 0.

## After the rewrite (protocol + `query`)

Six tasks, three sessions. All commands below are the real ones the agents ran.

| # | Task | Commands used | Result | Fabricated |
|---|---|---|---|---|
| 1 | "on what second was *pricing* said, quote it" | `query runs`, `summary`, `search pricing`, `search pric`, `search preaching`, `words`, `at` | «В транскрипте слова нет»; nearest token `preaching` 00:00:03.140–00:00:03.550 marked as hypothesis | 0 |
| 2 | "who spoke more, with turn timestamps" | `query speakers`, `timeline --type speaker_turn` | S2 8.310 s / 2 turns, S1 4.720 s / 1 turn; noted the range is not continuous | 0 |
| 3 | adversarial: full transcript + on-screen slides + weather + quarterly numbers | `query at`, `search` × 20, `acoustic`, `music`, `chapters` | All three visual/absent claims refused; explained audio-only stream; found «ten dollars» as the only monetary mention | 0 |
| 4 | "split into chapters and summarise each with quotes" | `query chapters`, `transcript`, `timeline --type speaker_turn` | 2 chapters, both quoted; said chapters are structural, not topical | 0 |
| 5 | "find every mention of money, with timestamps" | `query words 0`, `search` × 7 | `ten` 00:00:04.110–00:00:04.230 (0.459), `dollars` 00:00:04.230–00:00:04.720 (0.838) | 0 |
| 6 | "who spoke more and why" | `query speakers`, `transcript`, `timeline` | Same numbers; separated VAD speech 12.176 s from turn sum 13.030 s | 0 |

Verification: every quoted string was compared byte-for-byte against the
document (`jq` on `segments[].kind.text` and `words[].text`), and every cited
range against `range.start/end`. All matched. Six tasks, **zero fabricated
citations, zero invented timestamps**.

## What the dogfood changed in the skill

Each friction report became a rule or a command, not a paragraph of advice:

| Friction found | Fix |
|---|---|
| no rule for a word the user insists was said | §9 with a ready answer shape; "hypothesis, not a quote" |
| two token streams (`speech` vs `speaker_turn`), no canonical one | §6: quote `speech.text`; turns are for attribution |
| `speech_duration` ≠ sum of turns | §6: which number answers which question |
| speakers' aggregate range reads as continuous speech | §6 + cheatsheet |
| `query at → (none)` mistaken for silence | §6: not measured silence |
| `acoustic.silence` inside a speech segment, real gap not marked | §6 + cheatsheet: level-based, not turn-aligned |
| no command for all words of a segment | new `query words DOC ID` |
| timeline table truncates at 80 chars | new `query transcript DOC`, and a §5 rule |
| first word missing from `words[]` | §6: quote the sentence, never invent the range |
| chapter `segment_count` counts parents and children | cheatsheet |
| `inspect` mistaken for `query inspect` | §12 corrected |
| "chapters" read as topics | §5: structural, not topical |

## Non-English audio (tested 2026-11-11)

A 12 s Russian recording (macOS `say -v Milena`), `whisper-small`,
`--language ru`, all stages complete:

| Check | Result |
|---|---|
| Transcript | exact, three sentences, correct punctuation |
| Amounts | «двадцать процентов» normalised to `20%` |
| Word search | «подписку» found at 00:00:03.600–00:00:06.760 |
| Caveat | searching «процентов» misses because the text says `20%` |

The number-normalisation rule was added to the skill as a result. This is one
short clean recording, not a WER measurement; it shows the path works, not how
accurate it is on real Russian audio.

## Not covered yet

- **Music**: no licensed musical fixture was dogfooded; the music workflow is
  documented but unverified against real material. Do not cite it as tested.
- **Video**: shot detection and keyframe extraction work (synthetic fixtures in
  `crates/engine/tests/scenes.rs` and `crates/cli/tests/frames.rs`), but no
  dogfood run has used a real video with keyframes; the skill's "look at the
  frame" workflow is documented and untested.
- A longer recording was not tested; chapter quality on multi-hour material is
  unknown.

These are the honest gaps. The next dogfood round should use a real interview,
a meeting, and a music track before the skill claims those workflows.
