//! Evaluation metrics.
//!
//! Only measurements implemented here are ever reported. No accuracy numbers
//! are hard-coded anywhere in this project; the harness exists so numbers can
//! be produced from licensed data on demand.

use std::collections::HashMap;

/// Word and character error rates for one hypothesis.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct WerScore {
    pub substitutions: u64,
    pub insertions: u64,
    pub deletions: u64,
    pub reference_words: u64,
    pub wer: f32,
    pub reference_chars: u64,
    pub cer: f32,
}

/// Lowercase and strip everything that is not alphanumeric or an apostrophe,
/// so punctuation style does not dominate the score. This is the standard
/// text normalisation used by WER tooling.
pub fn normalize_words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split_whitespace()
        .map(|token| {
            token
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '\'')
                .collect::<String>()
        })
        .filter(|token| !token.is_empty())
        .collect()
}

/// Levenshtein alignment with substitution/insertion/deletion accounting.
fn align(reference: &[String], hypothesis: &[String]) -> (u64, u64, u64) {
    let n = reference.len();
    let m = hypothesis.len();
    // dp[i][j] = (cost, subs, ins, dels) for reference[..i] vs hypothesis[..j]
    let mut dp = vec![vec![(0u32, 0u32, 0u32, 0u32); m + 1]; n + 1];
    for i in 1..=n {
        dp[i][0] = (i as u32, 0, 0, i as u32);
    }
    for j in 1..=m {
        dp[0][j] = (j as u32, 0, j as u32, 0);
    }
    for i in 1..=n {
        for j in 1..=m {
            let substitution_cost = u32::from(reference[i - 1] != hypothesis[j - 1]);
            let (sub_cost, sub_s, sub_i, sub_d) = dp[i - 1][j - 1];
            let substitute = (
                sub_cost + substitution_cost,
                sub_s + substitution_cost,
                sub_i,
                sub_d,
            );

            let (del_cost, del_s, del_i, del_d) = dp[i - 1][j];
            let delete = (del_cost + 1, del_s, del_i, del_d + 1);

            let (ins_cost, ins_s, ins_i, ins_d) = dp[i][j - 1];
            let insert = (ins_cost + 1, ins_s, ins_i + 1, ins_d);

            dp[i][j] = *[substitute, delete, insert]
                .iter()
                .min_by_key(|candidate| candidate.0)
                .expect("non-empty candidate list");
        }
    }
    let (_, subs, ins, dels) = dp[n][m];
    (u64::from(subs), u64::from(ins), u64::from(dels))
}

pub fn word_error_rate(reference: &str, hypothesis: &str) -> WerScore {
    let reference_words = normalize_words(reference);
    let hypothesis_words = normalize_words(hypothesis);
    let (substitutions, insertions, deletions) = align(&reference_words, &hypothesis_words);

    let ref_count = reference_words.len() as u64;
    let wer = if ref_count == 0 {
        if hypothesis_words.is_empty() {
            0.0
        } else {
            1.0
        }
    } else {
        (substitutions + insertions + deletions) as f32 / ref_count as f32
    };

    let reference_chars: Vec<String> = reference
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .map(String::from)
        .collect();
    let hypothesis_chars: Vec<String> = hypothesis
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .map(String::from)
        .collect();
    let (cer_subs, cer_ins, cer_dels) = align(&reference_chars, &hypothesis_chars);
    let cer = if reference_chars.is_empty() {
        if hypothesis_chars.is_empty() {
            0.0
        } else {
            1.0
        }
    } else {
        (cer_subs + cer_ins + cer_dels) as f32 / reference_chars.len() as f32
    };

    WerScore {
        substitutions,
        insertions,
        deletions,
        reference_words: ref_count,
        wer,
        reference_chars: reference_chars.len() as u64,
        cer,
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RttmSegment {
    pub file: String,
    pub start: f64,
    pub end: f64,
    pub speaker: String,
}

/// Parse speaker segments from an RTTM file (NIST `SPEAKER` lines).
pub fn parse_rttm(text: &str) -> Vec<RttmSegment> {
    let mut segments = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 8 || fields[0] != "SPEAKER" {
            continue;
        }
        let Ok(start) = fields[3].parse::<f64>() else {
            continue;
        };
        let Ok(duration) = fields[4].parse::<f64>() else {
            continue;
        };
        segments.push(RttmSegment {
            file: fields[1].to_string(),
            start,
            end: start + duration,
            speaker: fields[7].to_string(),
        });
    }
    segments
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DerScore {
    pub der: f32,
    pub missed_speech: f32,
    pub false_alarm: f32,
    pub confusion: f32,
    /// Reference speech time actually scored (collar excluded), in seconds.
    /// Lets a caller aggregate several files as one duration-weighted DER
    /// instead of averaging percentages.
    pub scored_seconds: f32,
    pub reference_speakers: usize,
    pub hypothesis_speakers: usize,
    /// Greedy reference/hypothesis speaker mapping that was scored.
    pub mapping: Vec<(String, String)>,
}

/// Frame-based diarization error rate with a collar around reference
/// boundaries.
///
/// Ceiling: speaker mapping is greedy (highest co-occurrence first), not an
/// optimal assignment. That can over-count confusion when two speakers swap
/// turns frequently. It is documented here instead of being hidden; an optimal
/// assignment can replace this when it changes a reported number.
pub fn diarization_error_rate(
    reference: &[RttmSegment],
    hypothesis: &[(f64, f64, String)],
    collar_s: f64,
    frame_s: f64,
) -> Option<DerScore> {
    if reference.is_empty() {
        return None;
    }
    let frame_s = frame_s.max(0.001);
    let start = reference
        .iter()
        .map(|s| s.start)
        .fold(f64::INFINITY, f64::min)
        .min(
            hypothesis
                .iter()
                .map(|(s, _, _)| *s)
                .fold(f64::INFINITY, f64::min),
        );
    let end = reference
        .iter()
        .map(|s| s.end)
        .fold(f64::NEG_INFINITY, f64::max)
        .max(
            hypothesis
                .iter()
                .map(|(_, e, _)| *e)
                .fold(f64::NEG_INFINITY, f64::max),
        );
    if !start.is_finite() || !end.is_finite() || end <= start {
        return None;
    }

    // Speaker sets per frame.
    let frames = ((end - start) / frame_s).ceil() as usize;
    let mut ref_sets: Vec<Vec<&str>> = vec![Vec::new(); frames];
    let mut hyp_sets: Vec<Vec<&str>> = vec![Vec::new(); frames];

    for segment in reference {
        let first = ((segment.start - start) / frame_s).max(0.0) as usize;
        let last = (((segment.end - start) / frame_s).ceil() as usize).min(frames);
        for frame in first..last {
            ref_sets[frame].push(segment.speaker.as_str());
        }
    }
    for (seg_start, seg_end, speaker) in hypothesis {
        let first = ((seg_start - start) / frame_s).max(0.0) as usize;
        let last = (((seg_end - start) / frame_s).ceil() as usize).min(frames);
        for frame in first..last {
            hyp_sets[frame].push(speaker.as_str());
        }
    }

    // Collar: frames within `collar_s` of a reference boundary are ignored.
    let mut boundary_frames: Vec<usize> = Vec::new();
    for segment in reference {
        for edge in [segment.start, segment.end] {
            let frame = ((edge - start) / frame_s).round() as usize;
            let radius = (collar_s / frame_s).round() as usize;
            let lo = frame.saturating_sub(radius);
            let hi = (frame + radius).min(frames.saturating_sub(1));
            boundary_frames.extend(lo..=hi);
        }
    }
    let mut ignore = vec![false; frames];
    for frame in boundary_frames {
        ignore[frame] = true;
    }

    // Greedy mapping from co-occurrence counts.
    let mut pairs: HashMap<(&str, &str), u64> = HashMap::new();
    for frame in 0..frames {
        if ignore[frame] {
            continue;
        }
        for reference_speaker in &ref_sets[frame] {
            for hypothesis_speaker in &hyp_sets[frame] {
                *pairs
                    .entry((*reference_speaker, *hypothesis_speaker))
                    .or_default() += 1;
            }
        }
    }
    let mut ranked: Vec<((&str, &str), u64)> = pairs.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut mapping: HashMap<&str, &str> = HashMap::new();
    let mut used_reference: std::collections::HashSet<&str> = std::collections::HashSet::new();
    let mut used_hypothesis: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for ((reference_speaker, hypothesis_speaker), _) in ranked {
        if used_reference.contains(reference_speaker)
            || used_hypothesis.contains(hypothesis_speaker)
        {
            continue;
        }
        mapping.insert(hypothesis_speaker, reference_speaker);
        used_reference.insert(reference_speaker);
        used_hypothesis.insert(hypothesis_speaker);
    }

    let mut reference_frames = 0_u64;
    let mut missed = 0_u64;
    let mut false_alarm = 0_u64;
    let mut confusion = 0_u64;

    for frame in 0..frames {
        if ignore[frame] {
            continue;
        }
        let reference_set = &ref_sets[frame];
        let hypothesis_set = &hyp_sets[frame];
        if reference_set.is_empty() && hypothesis_set.is_empty() {
            continue;
        }
        if !reference_set.is_empty() {
            reference_frames += 1;
        }
        match (reference_set.is_empty(), hypothesis_set.is_empty()) {
            (false, true) => missed += 1,
            (true, false) => false_alarm += 1,
            _ => {
                // Correct if any mapped hypothesis speaker is in the reference set.
                let correct = hypothesis_set.iter().any(|hypothesis_speaker| {
                    mapping
                        .get(hypothesis_speaker)
                        .is_some_and(|mapped| reference_set.contains(mapped))
                });
                if !correct {
                    confusion += 1;
                }
            }
        }
    }

    if reference_frames == 0 {
        return None;
    }
    let reference_frames_f = reference_frames as f32;
    let mut mapping_pairs: Vec<(String, String)> = mapping
        .iter()
        .map(|(hyp, reference)| ((*reference).to_string(), (*hyp).to_string()))
        .collect();
    mapping_pairs.sort();

    Some(DerScore {
        der: (missed + false_alarm + confusion) as f32 / reference_frames_f,
        missed_speech: missed as f32 / reference_frames_f,
        false_alarm: false_alarm as f32 / reference_frames_f,
        confusion: confusion as f32 / reference_frames_f,
        scored_seconds: reference_frames_f * frame_s as f32,
        reference_speakers: reference
            .iter()
            .map(|s| s.speaker.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        hypothesis_speakers: hypothesis
            .iter()
            .map(|(_, _, s)| s.as_str())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        mapping: mapping_pairs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_text_scores_zero() {
        let score = word_error_rate("Hello, world!", "hello world");
        assert_eq!(score.wer, 0.0);
        assert_eq!(score.cer, 0.0);
    }

    #[test]
    fn substitution_insertion_deletion_are_counted() {
        let score = word_error_rate("a b c d", "a x c d e");
        assert_eq!(score.substitutions, 1);
        assert_eq!(score.insertions, 1);
        assert_eq!(score.deletions, 0);
        assert!((score.wer - 0.5).abs() < 1e-6);
    }

    #[test]
    fn dropped_words_are_penalised() {
        // Two of four reference words are not in the hypothesis. The optimal
        // alignment may express that as deletions or a substitution plus a
        // deletion; the total error count is what must hold.
        let score = word_error_rate("one two three four", "one three");
        let errors = score.substitutions + score.deletions + score.insertions;
        assert_eq!(errors, 2);
        assert!((score.wer - 0.5).abs() < 1e-6);
    }

    #[test]
    fn parses_rttm_speaker_lines() {
        let rttm = "SPEAKER meeting 1 0.000 2.500 <NA> <NA> spk1 <NA> <NA>\n\
                    SPEAKER meeting 1 2.500 1.000 <NA> <NA> spk2 <NA> <NA>\n";
        let segments = parse_rttm(rttm);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].speaker, "spk1");
        assert!((segments[1].end - 3.5).abs() < 1e-9);
    }

    #[test]
    fn perfect_diarization_scores_zero() {
        let reference = vec![
            RttmSegment {
                file: "x".into(),
                start: 0.0,
                end: 2.0,
                speaker: "A".into(),
            },
            RttmSegment {
                file: "x".into(),
                start: 2.0,
                end: 4.0,
                speaker: "B".into(),
            },
        ];
        let hypothesis = vec![(0.0, 2.0, "S1".to_string()), (2.0, 4.0, "S2".to_string())];
        let score = diarization_error_rate(&reference, &hypothesis, 0.25, 0.01).unwrap();
        assert!(score.der < 1e-6, "DER was {}", score.der);
    }

    #[test]
    fn missed_and_invented_speech_are_penalised() {
        let reference = vec![RttmSegment {
            file: "x".into(),
            start: 0.0,
            end: 4.0,
            speaker: "A".into(),
        }];
        let hypothesis = vec![(1.0, 3.0, "S1".to_string())];
        let score = diarization_error_rate(&reference, &hypothesis, 0.0, 0.01).unwrap();
        assert!((score.missed_speech - 0.5).abs() < 0.02, "{score:?}");
        assert!(score.der <= 0.5 + 1e-6);
    }
}
