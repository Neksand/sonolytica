//! VAD gate for diarization turns. Probe build, see docs/EVAL.md.

use sonolytica_core::{MediaTime, TimeRange};
use sonolytica_providers::{DiarTurn, SpeechRegion};

const MIN_COVERAGE: f32 = 0.5;
const SHORT_TURN: MediaTime = MediaTime::from_millis(500);
const MAX_TRIM: MediaTime = MediaTime::from_millis(500);

#[must_use]
pub fn gate_turns(turns: &[DiarTurn], regions: &[SpeechRegion]) -> Vec<DiarTurn> {
    if regions.is_empty() {
        return turns.to_vec();
    }
    let speech = merged_regions(regions);

    turns
        .iter()
        .filter_map(|turn| {
            let duration = turn.range.duration();
            let covered = speech
                .iter()
                .map(|range| range.overlap(&turn.range))
                .fold(MediaTime::ZERO, MediaTime::saturating_add);
            let protected = speech.iter().any(|range| turn.range.contains_range(range));
            let coverage = if duration > MediaTime::ZERO {
                covered.micros() as f32 / duration.micros() as f32
            } else {
                0.0
            };

            if !protected && duration >= SHORT_TURN && coverage < MIN_COVERAGE {
                return None;
            }

            let mut range = turn.range;
            let first = speech
                .iter()
                .find(|speech| speech.intersects(&range))
                .map(|speech| speech.start);
            let last = speech
                .iter()
                .rev()
                .find(|speech| speech.intersects(&range))
                .map(|speech| speech.end);
            if let Some(first) = first {
                if first.saturating_sub(range.start) > MAX_TRIM {
                    range.start = first;
                }
            }
            if let Some(last) = last {
                if range.end.saturating_sub(last) > MAX_TRIM {
                    range.end = last;
                }
            }

            (!range.is_empty()).then(|| DiarTurn {
                range,
                speaker: turn.speaker.clone(),
                confidence: turn.confidence,
            })
        })
        .collect()
}

fn merged_regions(regions: &[SpeechRegion]) -> Vec<TimeRange> {
    let mut ranges: Vec<TimeRange> = regions
        .iter()
        .map(|region| region.range)
        .filter(|range| !range.is_empty())
        .collect();
    ranges.sort_by_key(|range| range.start);
    let mut merged: Vec<TimeRange> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match merged.last_mut() {
            Some(current) if range.start <= current.end => {
                current.end = current.end.max(range.end);
            }
            _ => merged.push(range),
        }
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use sonolytica_core::{Confidence, SpeakerId};

    fn turn(speaker: &str, start: f64, end: f64) -> DiarTurn {
        DiarTurn {
            range: TimeRange::from_secs(start, end),
            speaker: SpeakerId::new(speaker),
            confidence: Confidence::new(0.9),
        }
    }

    fn region(start: f64, end: f64) -> SpeechRegion {
        SpeechRegion {
            range: TimeRange::from_secs(start, end),
            probability: Some(0.9),
        }
    }

    #[test]
    fn no_regions_is_a_no_op() {
        let turns = vec![turn("S1", 0.0, 10.0)];
        let gated = gate_turns(&turns, &[]);
        assert_eq!(gated.len(), 1);
        assert_eq!(gated[0].range, turns[0].range);
    }

    #[test]
    fn noise_blob_is_dropped() {
        // Ten seconds of "speaker" in a file whose speech is elsewhere.
        let turns = vec![turn("S1", 0.0, 10.0)];
        let regions = vec![region(12.0, 14.0)];
        assert!(gate_turns(&turns, &regions).is_empty());
    }

    #[test]
    fn short_reply_survives_low_coverage() {
        let turns = vec![turn("S1", 2.0, 2.3)];
        let regions = vec![region(5.0, 6.0)];
        assert_eq!(gate_turns(&turns, &regions).len(), 1);
    }

    #[test]
    fn quiet_speech_inside_a_turn_is_protected() {
        // A long turn with a fully contained region keeps the whole turn even
        // though coverage is only 10 %.
        let turns = vec![turn("S1", 0.0, 10.0)];
        let regions = vec![region(4.0, 5.0)];
        assert_eq!(gate_turns(&turns, &regions).len(), 1);
    }

    #[test]
    fn coverage_sums_across_two_regions() {
        // 0.75 s + 1.5 s of the 3 s turn = 75 %.
        let turns = vec![turn("S1", 0.0, 3.0)];
        let regions = vec![region(0.0, 0.75), region(1.5, 3.0)];
        assert_eq!(gate_turns(&turns, &regions).len(), 1);
    }

    #[test]
    fn overlap_turns_are_judged_independently() {
        let turns = vec![turn("S1", 0.0, 4.0), turn("S2", 0.0, 4.0)];
        let regions = vec![region(0.0, 4.0), region(20.0, 21.0)];
        let gated = gate_turns(&turns, &regions);
        assert_eq!(gated.len(), 2, "both simultaneous turns are speech");
    }

    #[test]
    fn gross_overhang_is_trimmed_and_lag_is_not() {
        // Start overhang 2 s (trimmed), end overhang 0.3 s (kept).
        let turns = vec![turn("S1", 0.0, 10.3)];
        let regions = vec![region(2.0, 10.0)];
        let gated = gate_turns(&turns, &regions);
        assert_eq!(gated.len(), 1);
        assert_eq!(gated[0].range, TimeRange::from_secs(2.0, 10.3));
    }
}
