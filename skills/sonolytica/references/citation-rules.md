# Citation rules

A citation is the only thing that separates this tool from a plausible-sounding
summary. These rules are not style advice; they are what makes the answer
trustworthy.

## 1. Quote verbatim

Copy `text` or `words[].text` character for character, including the
punctuation and spacing the ASR produced. Do not fix grammar inside quotes, do
not translate inside quotes, do not merge two segments into one quote.

```
✅ «…our preaching starts at ten dollars.» (00:00:00.000–00:00:04.860)
✅ Он сказал «ten dollars» (00:00:04.230–00:00:04.720).
❌ Он сказал, что цена — десять долларов (00:00:04).
```

The third line is a paraphrase with an invented citation; the second is a
quote at word precision. Paraphrase outside quotes if you need to.

## 2. Copy timestamps, never compute them

Every range comes from `start_display`/`end_display` or `range_display`.
Do not convert microseconds in your head, do not round to seconds, do not
estimate "около 4 секунд" without marking it `≈`.

Format: `00:00:04.230–00:00:04.720` (en dash, three decimal places).

## 3. Pick the range that supports the claim

| Claim | Range | Why |
|---|---|---|
| a word | `words[].range` | token-level timing |
| a sentence | `speech` segment range | the ASR utterance |
| who spoke | `speaker_turn` range | diarization, may be narrower |
| "who said this" | both, named | they can differ by ASR padding |

A `speech` segment and its `speaker_turn` can differ by up to ~300 ms of
padding. When both appear in your answer, say which is which, e.g. «реплика S1
00:00:00.000–00:00:04.720, транскрипт того же фрагмента до 00:00:04.860».

## 4. Speakers are labels, not people

`S1`/`S2` are stable inside one document only. Never say "Алиса", "мужчина",
"ведущий" unless the user told you and the document agrees. Never claim a
speaker count across files.

## 5. Confidence is not accuracy

- word/segment confidence: model probability for those tokens;
- tempo: autocorrelation strength;
- key: template-match margin.

Report it as a score ("score 0.31") or omit it. Never convert it to a
percentage certainty about the real world.

## 6. Absence is a finding, not a licence to guess

| Situation | Correct answer |
|---|---|
| `query search` empty | «В транскрипте нет слова X.» |
| the word exists but looks like an ASR miss | name the actual token and mark the hypothesis |
| a capability is missing | «диаризация не выполнялась: модель не установлена» |
| the stage ran but found nothing | «речь не распознана» (`partial`) |
| video has no scene segments | «видео не анализировалось»: визуальный слой не запускался |
| `acoustic.silence` is empty in a gap | the gap is not measured silence; do not call it silence |

Bad answers: «не говорили» (the transcript is not the world), «вероятно, он
сказал…» inside quotes, «спикеров двое» when diarization did not run.

A word can be missing from `words[]` while present in `speech.text` (the first
token of a region is the usual casualty). Quote the sentence from `text` and
say the word timing starts at the first token present; never attach a range to
a word that has none.

## 7. The "word not found" protocol

1. `query search DOC <word>` — case-insensitive already.
2. If empty, try a stem (`pricing` → `pric`) and a likely homophone only as a
   *hypothesis*.
3. Report exactly:

```
Слова «pricing» в транскрипте нет. Ближайший по звучанию токен —
«preaching» (00:00:03.140–00:00:03.550). Это гипотеза: документ не
подтверждает, что там было сказано «pricing».
```

4. Mention the known limitation: ASR can drop the first word of a speech
   region, so a missing word near a boundary is weaker evidence than one in the
   middle.

## 8. Two evidence classes in one answer

Keep them visually separate:

```
По документу: … (00:00:12.400–00:00:15.200)
Наблюдение по кадру (00:00:12.400): на экране слайд «Q3 revenue» — это
наблюдение, в транскрипте этой строки нет.
```

Never let an observation borrow a transcript timestamp. The frame time is the
document's, the content is yours.

## 9. What to do when the document contradicts itself

Prefer the more specific, measured field and say which you used:

- `speech_duration` (VAD, whole file) vs `sum(turns)` (diarization): different
  measurements, not an error. For "who spoke most" use `query speakers`.
- `speech.text` vs `turn.text`: the turn text is copied at merge time; if they
  differ, quote the speech segment (it is the ASR output) and note the turn.
- `words[]` can end before `text` does (region boundary). Quote the words for
  word-level timing and the segment text for content.

## 10. Numbers you may state

Only these, and only from the document:

- media duration, size, container, stream parameters;
- segment counts, speaker count, word count;
- speech duration and ratio;
- BPM, key, loudness, LRA, true peak, silence regions;
- run durations (machine speed, not media content).

Anything else is inference: label it.
