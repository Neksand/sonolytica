//! Turning independent ASR and diarization outputs into speaker turns.
//!
//! Neither model knows about the other. The merge is deterministic and
//! conservative:
//!
//! * a word belongs to the diarization turn with the largest temporal overlap;
//! * the utterance range is clamped to the assigned turn envelope plus a small
//!   boundary tolerance, so a speaker turn never claims time the diarizer did
//!   not attribute to the speaker — the text stays complete even when its first
//!   word starts slightly before the turn;
//! * a word with no turn inside the tolerance is left out of the dialogue (it
//!   stays in the raw speech segment, so nothing is lost);
//! * consecutive words of one speaker become one utterance, split on speaker
//!   change or a gap longer than [`MAX_WORD_GAP`];
//! * diarization turns that no word covers are still emitted, with empty text,
//!   so "who spoke when" is complete even without ASR.

use sonolytica_core::{Confidence, MediaTime, SpeakerId, TimeRange};
use sonolytica_providers::{AsrSegment, DiarTurn};

/// Speech that is not word-aligned is split when the gap exceeds this.
const MAX_WORD_GAP: MediaTime = MediaTime::from_micros(800_000);
/// A word with no overlapping diarization turn is still attributed to the
/// nearest turn if it lies this close: diarization boundaries lag real speech
/// onsets by a few hundred milliseconds.
const MAX_TURN_GAP: MediaTime = MediaTime::from_micros(750_000);
/// Attribute up to this much of the utterance range outside its turn; anything
/// further is the diarizer's silence, not the speaker's time.
const TURN_TOLERANCE: MediaTime = MediaTime::from_millis(300);

#[derive(Debug, Clone)]
pub struct Utterance {
    pub range: TimeRange,
    pub speaker: SpeakerId,
    pub text: String,
    /// Index of the ASR segment this utterance came from, used as the parent
    /// in the document hierarchy.
    pub parent_asr: Option<usize>,
    pub confidence: Option<Confidence>,
}

fn best_turn(range: TimeRange, turns: &[DiarTurn]) -> Option<&DiarTurn> {
    let mut best: Option<(&DiarTurn, MediaTime)> = None;
    for turn in turns {
        let overlap = turn.range.overlap(&range);
        if overlap <= MediaTime::ZERO {
            continue;
        }
        match best {
            Some((_, current)) if current >= overlap => {}
            _ => best = Some((turn, overlap)),
        }
    }
    if let Some((turn, _)) = best {
        return Some(turn);
    }

    // No overlap: fall back to temporal proximity, but only within a small
    // gap and never across a different speaker's turn that is closer.
    let mut nearest: Option<(&DiarTurn, MediaTime)> = None;
    for turn in turns {
        let gap = if range.end <= turn.range.start {
            turn.range.start.saturating_sub(range.end)
        } else {
            range.start.saturating_sub(turn.range.end)
        };
        if gap > MAX_TURN_GAP {
            continue;
        }
        match nearest {
            Some((_, current)) if current <= gap => {}
            _ => nearest = Some((turn, gap)),
        }
    }
    nearest.map(|(turn, _)| turn)
}

/// Clamp an utterance span to the assigned turn envelope plus
/// [`TURN_TOLERANCE`]. Falls back to the span when they are disjoint (a word
/// attributed by proximity), so text is never dropped for a range reason.
fn clamp_to_turn(range: TimeRange, turn_start: MediaTime, turn_end: MediaTime) -> TimeRange {
    let allowed = TimeRange::new(
        turn_start.saturating_sub(TURN_TOLERANCE),
        turn_end.saturating_add(TURN_TOLERANCE),
    );
    range.intersection(&allowed).unwrap_or(range)
}

struct Builder {
    speaker: SpeakerId,
    text: String,
    start: MediaTime,
    end: MediaTime,
    turn_start: MediaTime,
    turn_end: MediaTime,
    confidence: Option<Confidence>,
    parent: Option<usize>,
}

impl Builder {
    fn finish(self) -> Option<Utterance> {
        let text = self.text.trim().to_string();
        if text.is_empty() {
            return None;
        }
        Some(Utterance {
            range: clamp_to_turn(
                TimeRange::new(self.start, self.end),
                self.turn_start,
                self.turn_end,
            ),
            speaker: self.speaker,
            text,
            parent_asr: self.parent,
            confidence: self.confidence,
        })
    }
}

/// Build speaker turns from ASR segments and diarization turns.
pub fn build_speaker_turns(asr: &[AsrSegment], turns: &[DiarTurn]) -> Vec<Utterance> {
    if turns.is_empty() {
        return Vec::new();
    }

    let mut utterances: Vec<Utterance> = Vec::new();

    for (asr_index, segment) in asr.iter().enumerate() {
        if segment.words.is_empty() {
            if let Some(turn) = best_turn(segment.range, turns) {
                if !segment.text.trim().is_empty() {
                    utterances.push(Utterance {
                        range: clamp_to_turn(segment.range, turn.range.start, turn.range.end),
                        speaker: turn.speaker.clone(),
                        text: segment.text.trim().to_string(),
                        parent_asr: Some(asr_index),
                        confidence: turn.confidence,
                    });
                }
            }
            continue;
        }

        let mut current: Option<Builder> = None;
        for word in &segment.words {
            let Some(turn) = best_turn(word.range, turns) else {
                if let Some(builder) = current.take() {
                    if let Some(utterance) = builder.finish() {
                        utterances.push(utterance);
                    }
                }
                continue;
            };

            let split = match &current {
                None => true,
                Some(builder) => {
                    builder.speaker != turn.speaker
                        || word.range.start.saturating_sub(builder.end) > MAX_WORD_GAP
                }
            };
            if split {
                if let Some(builder) = current.take() {
                    if let Some(utterance) = builder.finish() {
                        utterances.push(utterance);
                    }
                }
                current = Some(Builder {
                    speaker: turn.speaker.clone(),
                    text: String::new(),
                    start: word.range.start,
                    end: word.range.end,
                    turn_start: turn.range.start,
                    turn_end: turn.range.end,
                    confidence: turn.confidence,
                    parent: Some(asr_index),
                });
            }
            if let Some(builder) = current.as_mut() {
                if !builder.text.is_empty() {
                    builder.text.push(' ');
                }
                builder.text.push_str(&word.text);
                builder.end = builder.end.max(word.range.end);
                builder.turn_start = builder.turn_start.min(turn.range.start);
                builder.turn_end = builder.turn_end.max(turn.range.end);
            }
        }
        if let Some(builder) = current.take() {
            if let Some(utterance) = builder.finish() {
                utterances.push(utterance);
            }
        }
    }

    // Keep "who spoke when" complete: emit turns that no utterance covers.
    for turn in turns {
        let covered: MediaTime = utterances
            .iter()
            .filter(|u| u.speaker == turn.speaker)
            .map(|u| u.range.overlap(&turn.range))
            .fold(MediaTime::ZERO, |acc, v| acc.saturating_add(v));
        // If any word landed in this turn, the word-derived utterance already
        // represents it; a textless duplicate would just add noise.
        if covered == MediaTime::ZERO {
            utterances.push(Utterance {
                range: turn.range,
                speaker: turn.speaker.clone(),
                text: String::new(),
                parent_asr: None,
                confidence: turn.confidence,
            });
        }
    }

    utterances.sort_by_key(|u| (u.range.start, u.range.end));
    utterances
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonolytica_core::Word;

    fn word(text: &str, start: f64, end: f64) -> Word {
        Word {
            text: text.to_string(),
            range: TimeRange::from_secs(start, end),
            confidence: None,
        }
    }

    fn segment(text: &str, start: f64, end: f64, words: Vec<Word>) -> AsrSegment {
        AsrSegment {
            range: TimeRange::from_secs(start, end),
            text: text.to_string(),
            words,
            confidence: None,
            no_speech_prob: None,
        }
    }

    fn turn(speaker: &str, start: f64, end: f64) -> DiarTurn {
        DiarTurn {
            range: TimeRange::from_secs(start, end),
            speaker: SpeakerId::new(speaker),
            confidence: Confidence::new(0.9),
        }
    }

    #[test]
    fn splits_one_asr_segment_at_a_speaker_change() {
        let asr = vec![segment(
            "hello world again",
            0.0,
            4.0,
            vec![
                word("hello", 0.1, 0.8),
                word("world", 2.1, 2.7),
                word("again", 3.0, 3.5),
            ],
        )];
        let turns = vec![turn("S1", 0.0, 1.9), turn("S2", 1.9, 4.2)];

        let utterances = build_speaker_turns(&asr, &turns);
        assert_eq!(utterances.len(), 2, "{utterances:?}");
        assert_eq!(utterances[0].speaker, SpeakerId::new("S1"));
        assert_eq!(utterances[0].text, "hello");
        assert_eq!(utterances[1].speaker, SpeakerId::new("S2"));
        assert_eq!(utterances[1].text, "world again");
        assert_eq!(utterances[1].parent_asr, Some(0));
    }

    #[test]
    fn words_outside_any_turn_are_not_fabricated_into_dialogue() {
        let asr = vec![segment("hello", 0.0, 1.0, vec![word("hello", 0.0, 0.4)])];
        let turns = vec![turn("S1", 2.0, 3.0)];
        let utterances = build_speaker_turns(&asr, &turns);
        // The turn is still present, but textless, and no speaker was invented
        // for the unmatched word.
        assert_eq!(utterances.len(), 1);
        assert!(utterances[0].text.is_empty());
    }

    #[test]
    fn without_diarization_there_are_no_speaker_turns() {
        let asr = vec![segment("hello", 0.0, 1.0, vec![word("hello", 0.0, 0.4)])];
        assert!(build_speaker_turns(&asr, &[]).is_empty());
    }

    #[test]
    fn segment_without_word_timings_falls_back_to_segment_overlap() {
        let asr = vec![segment("a whole sentence", 0.0, 2.0, vec![])];
        let turns = vec![turn("S1", 0.0, 2.0)];
        let utterances = build_speaker_turns(&asr, &turns);
        assert_eq!(utterances.len(), 1);
        assert_eq!(utterances[0].text, "a whole sentence");
    }

    #[test]
    fn word_is_clipped_to_its_turn_with_boundary_tolerance() {
        // Word runs 0.5 s past the turn; 300 ms of that is boundary lag, the
        // rest is the diarizer's silence and must not be claimed.
        let asr = vec![segment("hello", 1.0, 2.5, vec![word("hello", 1.0, 2.5)])];
        let turns = vec![turn("S1", 2.0, 3.0)];
        let utterances = build_speaker_turns(&asr, &turns);
        assert_eq!(utterances.len(), 1);
        assert_eq!(utterances[0].range, TimeRange::from_secs(1.7, 2.5));
    }

    #[test]
    fn boundary_words_keep_their_text_but_not_their_time() {
        // Mirrors the easy fixture: Whisper hears "am the" before Silero's
        // strict region starts, and the diarizer puts the turn at 6.16.
        let asr = vec![segment(
            "am the second speaker",
            5.66,
            7.66,
            vec![
                word("am", 5.66, 5.67),
                word("the", 5.67, 5.70),
                word("second", 6.11, 6.58),
                word("speaker", 6.58, 7.66),
            ],
        )];
        let turns = vec![turn("S2", 6.16, 8.16)];
        let utterances = build_speaker_turns(&asr, &turns);
        assert_eq!(utterances.len(), 1, "{utterances:?}");
        assert_eq!(utterances[0].text, "am the second speaker");
        assert_eq!(utterances[0].range, TimeRange::from_secs(5.86, 7.66));
    }

    #[test]
    fn word_within_boundary_lag_keeps_its_original_range() {
        let asr = vec![segment("hi", 0.9, 1.3, vec![word("hi", 0.9, 1.3)])];
        let turns = vec![turn("S1", 1.0, 2.0)];
        let utterances = build_speaker_turns(&asr, &turns);
        assert_eq!(utterances[0].range, TimeRange::from_secs(0.9, 1.3));
    }
}
