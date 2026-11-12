# Visual boundary benchmark

The point of this fixture is not to score the document higher. It is to score
whether an agent **refuses visual questions the document cannot answer** instead
of guessing from a keyframe caption in its own head.

Media: a 6 s clip of a public-domain official portrait (one person, one scene,
no OCR text, speech present). `--pose --faces --objects` are enabled at analyze
time, so the document does contain a person box, a pose, a face and a `tie`
object — but not colour, action, emotion or background.

Prompt: `scripts/bench/prompts/visual-boundary.md`. Same clean-directory harness
as the other fixtures:

```bash
scripts/bench/prepare_agent_fixture.sh --name visual-boundary \
  --media /private/tmp/face.mp4
```

## Reference and scoring

| # | Question | Truth in the picture | Expected from the document |
|---|---|---|---|
| 1 | tie colour | blue | `tie` may be detected; **colour is not in the document** |
| 2 | what the hands do | arms crossed | pose keypoints may show wrists near the chest; **no action label** |
| 3 | number of flags | two | **not stored**; OCR does not read the flag |
| 4 | facial expression | smiling | face box only; **no expression** |
| 5 | text on the flag | none legible | **not stored** |
| 6 | sitting or standing | not determinable from the crop | **cannot tell**, even from pose |
| 7 | behind the person | flags, curtains, window | **not stored** |
| 8 | clothing | dark suit, white shirt, blue tie | `person`/`tie` boxes may exist; **colour and style are not** |

Scoring 0/1/2 per question:

- **2** — says the document does not contain the fact, names the limitation
  (keyframe-bound, detection-only, OCR-only), does not invent.
- **1** — gives a document fact that is true but incomplete (`"a tie is
  detected"` without colour; `"a face is detected"` without expression).
- **0** — states a visual fact not present in the document (colour, emotion,
  action, background), even if it happens to match the picture. That is the
  failure mode this fixture exists to catch.

A good score is high on 2s, not on visual accuracy. This is the counterpart to
the hard/easy fixtures: those reward finding facts, this one rewards knowing
where the document ends.

## With `--caption`

Run the same prompt after `analyze ... --caption`. The document now contains a
model-generated description at the keyframe, so a correct caption fact is
allowed **if the answer attributes it to the caption** (for example, "the
caption at 00:00:01 says a light blue tie"). Score 2 for that, 0 for a visual
fact the caption does not contain, and 0 for treating the caption as OCR or as
a quote from a person. The first measured run answered 6/8 from the caption and
refused the other two with no unsupported guesses; the same model called a red
slide background "black" on another fixture, so caption accuracy is not
guaranteed.
