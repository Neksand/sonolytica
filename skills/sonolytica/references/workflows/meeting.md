# Workflow: meeting / call / standup

Goal: what was decided, who committed to what, and where it was said. The
document contains statements, not intent: do not turn a discussion into a
decision.

## Commands, in order

```bash
sonolytica analyze meeting.m4a --out meeting.json --profile standard --summary
sonolytica query brief meeting.json          # status, speakers, chapters, counts
# only if the brief is not enough:
sonolytica query chapters meeting.json --min-gap 2s
sonolytica query speakers meeting.json
```

For each item the user wants:

```bash
sonolytica query search meeting.json deadline
sonolytica query search meeting.json "will send"
sonolytica query timeline meeting.json --from 00:14:00 --to 00:16:30 --type speaker_turn
```

## Answer shape

```
Решения (только то, что произнесено дословно):
- «…» — S2, 00:14:12.300–00:14:19.800
Открытые вопросы:
- «…» — S1, 00:21:04.100–00:21:09.400
Кто сколько говорил: S1 12:04, S2 08:31
Не анализировалось: …
```

## Rules specific to meetings

- A "decision" is only what someone literally said (`«решили», «договорились»,
  «делаем так»`). A topic discussed is not a decision.
- An "action item" needs a speaker, a verb and, if stated, a deadline. If the
  deadline was not spoken, write "срок не назван" — do not infer it from the
  meeting date.
- `overlap: true` near an action item means the words may belong to the other
  speaker. Flag it.
- If diarization did not run, do not attribute anything: give the timeline
  without speakers and say why.
- Names: only if the speaker literally says their name, and even then it is a
  claim about the recording, not identity verification.

## Pitfalls

- Whisper can merge two speakers into one segment when they talk fast; the turn
  boundaries from diarization are the authority for "who".
- Numbers ("three weeks", "by Friday") are the most valuable and the most
  misheard. Quote with the word-level range when the user will act on it.
- If `--profile ultra` exited 2, a requested capability did not run. Say which,
  from `query runs`, before summarising.
