//! Forced word alignment from wav2vec2 CTC emissions.
//!
//! Whisper's word times are derived from token timestamps and drift at word
//! boundaries; citations and cut points need boundaries that come from the
//! acoustics. This module aligns a known transcript against frame-level CTC
//! posteriors from `wav2vec2-base-960h` with a Viterbi pass over the known
//! token sequence, so every word boundary is the first/last frame where its
//! tokens were actually emitted.
//!
//! Measured graph (ONNX Runtime, CPU, 16 kHz mono): one output frame per 20 ms
//! (convolution stride 320 samples), 32 classes, `<pad>` (id 0) is the CTC
//! blank and `|` (id 4) separates words. Input is raw f32, normalised per
//! utterance to zero mean and unit variance, matching the Hugging Face
//! feature extractor with `do_normalize = true`.
//!
//! Ceiling: one pass covers at most 30 s. The engine splits longer regions and
//! keeps Whisper timings for the remainder; the provider rejects longer input
//! rather than silently folding a long transcript onto the first frames. This
//! is deliberate: a second chunk would need overlap bookkeeping and a merge
//! policy, and the local ASR already decodes in short VAD regions.
//!
//! The tokenizer, CTC alignment and word-range maths are compiled without the
//! `align` feature so they can be unit-tested where ONNX Runtime is absent.

#![cfg_attr(not(feature = "align"), allow(dead_code))]

use std::collections::HashMap;
use std::sync::OnceLock;

#[cfg(feature = "align")]
use std::sync::Mutex;

use sha2::{Digest, Sha256};
use sonolytica_core::{Confidence, MediaTime, TimeRange, Word};

use crate::models::ModelStore;
use crate::traits::{ProviderError, ProviderResult};

#[cfg(feature = "align")]
use crate::models::find_model;
#[cfg(feature = "align")]
use ort::session::Session;
#[cfg(feature = "align")]
use ort::value::Tensor;

/// Model id in the registry.
pub const MODEL_ID: &str = "wav2vec2-base-960h";

/// Model input rate. The engine already decodes speech at this rate.
pub const SAMPLE_RATE: usize = 16_000;

/// One CTC frame covers 320 samples: the conv stack stride (5 · 2⁶).
pub const FRAME_SAMPLES: usize = 320;

/// Seconds per output frame, verified against the graph (49 frames for 1 s,
/// 149 for 3 s: 50 frames/s with the 400-sample receptive field removed).
pub const FRAME_SECS: f64 = FRAME_SAMPLES as f64 / SAMPLE_RATE as f64;

/// Hard cap for a single alignment pass; see the module ceiling note.
pub const MAX_ALIGN_MILLIS: i64 = 30_000;

const MAX_ALIGN_SAMPLES: usize = MAX_ALIGN_MILLIS as usize * SAMPLE_RATE / 1_000;

/// The convolutional stack needs at least one full 400-sample window.
pub const MIN_ALIGN_SAMPLES: usize = 400;

/// `Wav2Vec2FeatureExtractor(normalize_eps=1e-7)`.
const NORMALIZE_EPS: f32 = 1e-7;

/// Verbatim `vocab.json` from the pinned export (token -> index).
const VOCAB_JSON: &str = include_str!("wav2vec2_vocab.json");

/// SHA-256 of that file, from the same pinned revision as the model.
const VOCAB_SHA256: &str = "4178db26b3c7570f6a47f14ac6a1c7b32950b8c2800fb097287e53776934f1c5";

static VOCAB: OnceLock<Result<Wav2Vec2Vocab, String>> = OnceLock::new();

/// The model's vocabulary: id -> token, token character -> id, plus the two
/// indices the aligner must not guess (blank and word separator).
#[derive(Debug, Clone)]
struct Wav2Vec2Vocab {
    labels: Vec<String>,
    index: HashMap<char, usize>,
    blank: usize,
    word_separator: usize,
}

impl Wav2Vec2Vocab {
    fn parse(json: &str) -> ProviderResult<Self> {
        let map: HashMap<String, u32> = serde_json::from_str(json)
            .map_err(|e| ProviderError::Inference(format!("wav2vec2 vocab is not JSON: {e}")))?;
        if map.is_empty() {
            return Err(ProviderError::Inference("wav2vec2 vocab is empty".into()));
        }
        let size = map.values().max().copied().unwrap_or(0) as usize + 1;
        let mut labels = vec![String::new(); size];
        for (token, id) in &map {
            let slot = labels.get_mut(*id as usize).ok_or_else(|| {
                ProviderError::Inference(format!("wav2vec2 vocab index {id} is out of range"))
            })?;
            *slot = token.clone();
        }
        if let Some(gap) = labels.iter().position(String::is_empty) {
            return Err(ProviderError::Inference(format!(
                "wav2vec2 vocab has no token for index {gap}"
            )));
        }

        let blank = *map.get("<pad>").ok_or_else(|| {
            ProviderError::Inference("wav2vec2 vocab has no `<pad>` CTC blank".into())
        })? as usize;
        let word_separator = *map.get("|").ok_or_else(|| {
            ProviderError::Inference("wav2vec2 vocab has no `|` word separator".into())
        })? as usize;

        let mut index = HashMap::new();
        for (token, id) in &map {
            let mut chars = token.chars();
            if let (Some(ch), None) = (chars.next(), chars.next()) {
                index.insert(ch, *id as usize);
            }
        }

        Ok(Self {
            labels,
            index,
            blank,
            word_separator,
        })
    }

    fn len(&self) -> usize {
        self.labels.len()
    }

    /// Vocabulary lookup. The 960h model is uppercase-only; the transcript's
    /// original case is preserved in the emitted words.
    fn id(&self, ch: char) -> Option<usize> {
        self.index.get(&ch.to_ascii_uppercase()).copied()
    }
}

fn vocab() -> ProviderResult<&'static Wav2Vec2Vocab> {
    match VOCAB.get_or_init(|| {
        let digest = format!("{:x}", Sha256::digest(VOCAB_JSON.as_bytes()));
        if digest != VOCAB_SHA256 {
            return Err(format!(
                "embedded wav2vec2 vocabulary digest mismatch: expected {VOCAB_SHA256}, got {digest}"
            ));
        }
        Wav2Vec2Vocab::parse(VOCAB_JSON).map_err(|e| e.to_string())
    }) {
        Ok(vocab) => Ok(vocab),
        Err(message) => Err(ProviderError::Inference(message.clone())),
    }
}

/// One word of the transcript plus its position in the flat token sequence.
#[derive(Debug, Clone, PartialEq, Eq)]
struct WordTokens {
    text: String,
    /// Inclusive range into [`Tokenized::tokens`].
    first: usize,
    last: usize,
}

#[derive(Debug, Default)]
struct Tokenized {
    /// CTC token ids including `|` separators between words.
    tokens: Vec<u32>,
    words: Vec<WordTokens>,
}

/// Uppercase ASCII letters and `'` are tokens; whitespace is the `|` separator;
/// everything else is skipped without breaking the current word. No token is
/// invented, so a transcript in an unsupported script yields no words.
fn tokenize(text: &str, vocab: &Wav2Vec2Vocab) -> Tokenized {
    let mut tokens: Vec<u32> = Vec::new();
    let mut words: Vec<WordTokens> = Vec::new();
    let mut current_text = String::new();
    let mut current: Option<usize> = None;

    for ch in text.chars() {
        if ch.is_whitespace() {
            if let Some(first) = current.take() {
                words.push(WordTokens {
                    text: std::mem::take(&mut current_text),
                    first,
                    last: tokens.len() - 1,
                });
            }
            continue;
        }
        let Some(id) = vocab.id(ch) else {
            continue;
        };
        if current.is_none() {
            if !words.is_empty() {
                tokens.push(vocab.word_separator as u32);
            }
            current = Some(tokens.len());
        }
        tokens.push(id as u32);
        current_text.push(ch);
    }
    if let Some(first) = current {
        words.push(WordTokens {
            text: current_text,
            first,
            last: tokens.len() - 1,
        });
    }

    Tokenized { tokens, words }
}

/// In-place log-softmax over `frames` rows of `classes`.
fn log_softmax(values: &mut [f32], frames: usize, classes: usize) {
    for frame in 0..frames {
        let row = &mut values[frame * classes..(frame + 1) * classes];
        let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let sum: f64 = row.iter().map(|v| f64::from(*v - max).exp()).sum();
        let log_sum = max + sum.ln() as f32;
        for value in row.iter_mut() {
            *value -= log_sum;
        }
    }
}

/// Viterbi forced alignment of `tokens` against `log_probs` (`[frames, classes]`).
///
/// Returns the inclusive frame range of every token, in token order. `None`
/// when the transcript cannot fit (more tokens than frames, a blank token in
/// the transcript, or no finite path).
fn token_frames(
    log_probs: &[f32],
    frames: usize,
    classes: usize,
    tokens: &[u32],
    blank: usize,
) -> Option<Vec<(usize, usize)>> {
    if tokens.is_empty() || frames == 0 || tokens.len() > frames {
        return None;
    }
    if tokens.iter().any(|token| *token as usize == blank) {
        return None;
    }

    let states = 2 * tokens.len() + 1;
    let label = |state: usize| -> usize {
        if state % 2 == 0 {
            blank
        } else {
            tokens[state / 2] as usize
        }
    };
    // Repeating a token requires a blank between the two emissions; CTC
    // permits a two-state jump only when the labels differ.
    let can_skip = |state: usize| -> bool { state >= 2 && label(state - 2) != label(state) };

    let mut dp = vec![f32::NEG_INFINITY; frames * states];
    let mut back = vec![0_u8; frames * states];
    dp[0] = log_probs[label(0)];
    dp[1] = log_probs[label(1)];

    for frame in 1..frames {
        let (before, rest) = dp.split_at_mut(frame * states);
        let previous = &before[(frame - 1) * states..];
        let current = &mut rest[..states];
        let row = &log_probs[frame * classes..(frame + 1) * classes];
        for state in 0..states {
            let mut best = previous[state];
            let mut from = 0_u8;
            if state >= 1 && previous[state - 1] > best {
                best = previous[state - 1];
                from = 1;
            }
            if can_skip(state) && previous[state - 2] > best {
                best = previous[state - 2];
                from = 2;
            }
            if best > f32::NEG_INFINITY {
                current[state] = best + row[label(state)];
                back[frame * states + state] = from;
            }
        }
    }

    let last = states - 1;
    let tail = (frames - 1) * states;
    let end_state = if states > 1 && dp[tail + last - 1] > dp[tail + last] {
        last - 1
    } else {
        last
    };
    if !dp[tail + end_state].is_finite() {
        return None;
    }

    let mut state = end_state;
    let mut path = vec![0_usize; frames];
    for frame in (0..frames).rev() {
        path[frame] = state;
        state -= back[frame * states + state] as usize;
    }

    let mut ranges = vec![(usize::MAX, 0_usize); tokens.len()];
    for (frame, &path_state) in path.iter().enumerate() {
        if path_state % 2 == 0 {
            continue;
        }
        let token = path_state / 2;
        let range = &mut ranges[token];
        if range.0 == usize::MAX {
            range.0 = frame;
        }
        range.1 = frame;
    }
    if ranges.iter().any(|(first, _)| *first == usize::MAX) {
        return None;
    }
    Some(ranges)
}

/// Word ranges from token frames: first token's first frame to last token's
/// last frame (inclusive), clamped to the audio actually fed to the model.
fn words_from_frames(
    tokenized: &Tokenized,
    ranges: &[(usize, usize)],
    log_probs: &[f32],
    classes: usize,
    audio_secs: f64,
) -> Vec<Word> {
    let frame_time = |frame: usize| frame as f64 * FRAME_SECS;
    let mut words = Vec::with_capacity(tokenized.words.len());
    for word in &tokenized.words {
        let (start_frame, _) = ranges[word.first];
        let (_, last_frame) = ranges[word.last];
        let start = frame_time(start_frame);
        let end = frame_time(last_frame + 1).min(audio_secs);
        if end <= start {
            continue;
        }

        // Mean posterior of the frames the word's tokens were emitted on.
        let mut sum = 0.0_f64;
        let mut count = 0_usize;
        for (token, &id) in tokenized
            .tokens
            .iter()
            .enumerate()
            .take(word.last + 1)
            .skip(word.first)
        {
            let (first, last) = ranges[token];
            let id = id as usize;
            for frame in first..=last {
                sum += f64::from(log_probs[frame * classes + id].exp());
                count += 1;
            }
        }
        let confidence = if count > 0 {
            Confidence::new((sum / count as f64) as f32)
        } else {
            None
        };

        words.push(Word {
            text: word.text.clone(),
            range: TimeRange::new(
                MediaTime::from_secs_f64(start),
                MediaTime::from_secs_f64(end),
            ),
            confidence,
        });
    }
    words
}

/// Whether every frame's most likely token is blank. The forced path would
/// still emit the transcript, but on silence that is fabrication, not
/// alignment.
fn all_frames_blank(log_probs: &[f32], frames: usize, classes: usize, blank: usize) -> bool {
    (0..frames).all(|frame| {
        let row = &log_probs[frame * classes..(frame + 1) * classes];
        row.iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .is_none_or(|(best, _)| best == blank)
    })
}

/// Per-utterance zero-mean, unit-variance normalisation.
fn normalize(audio: &[f32]) -> Vec<f32> {
    let count = audio.len() as f64;
    let mean = audio.iter().map(|s| f64::from(*s)).sum::<f64>() / count;
    let variance = audio
        .iter()
        .map(|s| {
            let delta = f64::from(*s) - mean;
            delta * delta
        })
        .sum::<f64>()
        / count;
    let scale = (variance + f64::from(NORMALIZE_EPS)).sqrt();
    audio
        .iter()
        .map(|s| ((f64::from(*s) - mean) / scale) as f32)
        .collect()
}

/// Whether the alignment build and model file are present, so the engine can
/// decide between running alignment and keeping Whisper timings before paying
/// the load and digest-verification cost. A corrupt file still fails at load;
/// the engine turns that into `align_unavailable`.
pub fn available(store: &ModelStore) -> bool {
    #[cfg(feature = "align")]
    {
        find_model(MODEL_ID).is_some_and(|spec| store.is_present(spec))
    }
    #[cfg(not(feature = "align"))]
    {
        let _ = store;
        false
    }
}

#[cfg(not(feature = "align"))]
fn feature_disabled() -> ProviderError {
    ProviderError::Unsupported("this build was compiled without the `align` feature".into())
}

#[cfg(not(feature = "align"))]
#[derive(Debug)]
pub struct Wav2Vec2Aligner {
    _private: std::marker::PhantomData<()>,
}

#[cfg(not(feature = "align"))]
impl Wav2Vec2Aligner {
    pub fn load(_store: &ModelStore, _threads: usize) -> ProviderResult<Self> {
        Err(feature_disabled())
    }

    pub fn align(&self, _audio: &[f32], _text: &str) -> ProviderResult<Vec<Word>> {
        Err(feature_disabled())
    }
}

#[cfg(feature = "align")]
pub struct Wav2Vec2Aligner {
    session: Mutex<Session>,
}

#[cfg(feature = "align")]
impl std::fmt::Debug for Wav2Vec2Aligner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wav2Vec2Aligner").finish_non_exhaustive()
    }
}

#[cfg(feature = "align")]
impl Wav2Vec2Aligner {
    /// Load the pinned model. Digests are verified by [`ModelStore::require`].
    pub fn load(store: &ModelStore, threads: usize) -> ProviderResult<Self> {
        let spec = find_model(MODEL_ID).ok_or_else(|| {
            ProviderError::Inference("wav2vec2-align is not in this build's model registry".into())
        })?;
        let path = store.require(spec)?;
        // The vocabulary is part of the contract: a swapped vocab would shift
        // every class id and silently align the wrong tokens.
        vocab()?;
        let session = crate::onnx::build_session(&path, threads)?;
        Ok(Self {
            session: Mutex::new(session),
        })
    }

    /// Align `text` to `audio` (16 kHz mono f32). Word ranges are relative to
    /// the start of `audio`; the engine adds the region offset.
    ///
    /// Returns no words when the transcript has no known tokens or the model
    /// emitted only blanks. Never guesses a boundary.
    pub fn align(&self, audio: &[f32], text: &str) -> ProviderResult<Vec<Word>> {
        let vocab = vocab()?;
        let tokenized = tokenize(text, vocab);
        if tokenized.tokens.is_empty() || audio.is_empty() {
            return Ok(Vec::new());
        }
        if audio.len() < MIN_ALIGN_SAMPLES {
            return Err(ProviderError::Unsupported(format!(
                "alignment needs at least {MIN_ALIGN_SAMPLES} samples, got {}",
                audio.len()
            )));
        }
        if audio.len() > MAX_ALIGN_SAMPLES {
            return Err(ProviderError::Unsupported(format!(
                "alignment is capped at {} s; split longer audio before aligning",
                MAX_ALIGN_MILLIS / 1_000
            )));
        }
        if let Some(position) = audio.iter().position(|s| !s.is_finite()) {
            return Err(ProviderError::Unsupported(format!(
                "non-finite sample at index {position}"
            )));
        }

        let samples = normalize(audio);
        let tensor = Tensor::from_array((vec![1_i64, samples.len() as i64], samples))
            .map_err(|e| ProviderError::Inference(e.to_string()))?;
        // The outputs borrow the session, so copy them out before the guard
        // is released.
        let (dims, mut log_probs) = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| ProviderError::Inference("alignment session poisoned".into()))?;
            let outputs = session
                .run(ort::inputs!["input_values" => tensor])
                .map_err(|e| ProviderError::Inference(format!("alignment run failed: {e}")))?;
            let (shape, data) = outputs
                .get("logits")
                .ok_or_else(|| {
                    ProviderError::Inference("alignment output `logits` is missing".into())
                })?
                .try_extract_tensor::<f32>()
                .map_err(|e| ProviderError::Inference(e.to_string()))?;
            let dims: Vec<usize> = shape.iter().map(|dimension| *dimension as usize).collect();
            (dims, data.to_vec())
        };
        if dims.len() != 3 || dims[0] != 1 || dims[2] != vocab.len() {
            return Err(ProviderError::Inference(format!(
                "alignment output shape {dims:?} does not match [1, frames, {}]",
                vocab.len()
            )));
        }
        let frames = dims[1];
        let classes = dims[2];
        if frames == 0 {
            return Ok(Vec::new());
        }

        log_softmax(&mut log_probs, frames, classes);
        if all_frames_blank(&log_probs, frames, classes, vocab.blank) {
            return Ok(Vec::new());
        }

        let Some(ranges) =
            token_frames(&log_probs, frames, classes, &tokenized.tokens, vocab.blank)
        else {
            return Ok(Vec::new());
        };
        let audio_secs = audio.len() as f64 / SAMPLE_RATE as f64;
        Ok(words_from_frames(
            &tokenized, &ranges, &log_probs, classes, audio_secs,
        ))
    }
}

/// Loads the model on first use. Discovery must not allocate hundreds of
/// megabytes for a pipeline that never asks for alignment.
#[derive(Debug)]
pub struct LazyWav2Vec2Aligner {
    store: ModelStore,
    threads: usize,
    inner: OnceLock<Result<Wav2Vec2Aligner, String>>,
}

impl LazyWav2Vec2Aligner {
    pub fn new(store: ModelStore, threads: usize) -> Self {
        Self {
            store,
            threads,
            inner: OnceLock::new(),
        }
    }

    /// Load once so a failure is reported as one `align_unavailable` warning
    /// instead of one per region.
    pub fn ensure_loaded(&self) -> ProviderResult<()> {
        self.get().map(|_| ())
    }

    pub fn align(&self, audio: &[f32], text: &str) -> ProviderResult<Vec<Word>> {
        self.get()?.align(audio, text)
    }

    fn get(&self) -> ProviderResult<&Wav2Vec2Aligner> {
        match self.inner.get_or_init(|| {
            Wav2Vec2Aligner::load(&self.store, self.threads).map_err(|error| error.to_string())
        }) {
            Ok(provider) => Ok(provider),
            Err(message) => Err(ProviderError::Inference(message.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_vocab() -> Wav2Vec2Vocab {
        Wav2Vec2Vocab::parse(r#"{"<pad>":0,"A":1,"B":2,"|":3,"'":4,"<s>":5,"</s>":6,"<unk>":7}"#)
            .unwrap()
    }

    /// Logits with a decisive peak at `path[frame]` on every frame.
    fn logits_for(path: &[usize], classes: usize) -> Vec<f32> {
        let mut logits = vec![0.0_f32; path.len() * classes];
        for (frame, target) in path.iter().enumerate() {
            logits[frame * classes + target] = 12.0;
        }
        logits
    }

    fn align_tokens(text: &str, path: &[usize], classes: usize) -> (Tokenized, Vec<Word>) {
        let vocab = tiny_vocab();
        let tokenized = tokenize(text, &vocab);
        let mut log_probs = logits_for(path, classes);
        log_softmax(&mut log_probs, path.len(), classes);
        let ranges = token_frames(
            &log_probs,
            path.len(),
            classes,
            &tokenized.tokens,
            vocab.blank,
        )
        .expect("forced path");
        let words = words_from_frames(
            &tokenized,
            &ranges,
            &log_probs,
            classes,
            path.len() as f64 * FRAME_SECS,
        );
        (tokenized, words)
    }

    #[test]
    fn shipped_vocabulary_matches_the_pinned_digest() {
        // This is the contract with the model graph: class 0 must be blank and
        // `|` must be the separator, or every aligned token shifts.
        let vocab = vocab().expect("embedded vocabulary");
        assert_eq!(vocab.len(), 32);
        assert_eq!(vocab.blank, 0);
        assert_eq!(vocab.word_separator, 4);
        assert_eq!(vocab.id('a'), Some(7));
        assert_eq!(vocab.id('\''), Some(27));
    }

    #[test]
    fn tokenizes_words_and_separators() {
        let vocab = tiny_vocab();
        let tokenized = tokenize("AB A", &vocab);
        assert_eq!(tokenized.tokens, vec![1, 2, 3, 1]);
        assert_eq!(
            tokenized.words,
            vec![
                WordTokens {
                    text: "AB".into(),
                    first: 0,
                    last: 1,
                },
                WordTokens {
                    text: "A".into(),
                    first: 3,
                    last: 3,
                },
            ]
        );
    }

    #[test]
    fn skips_unknown_characters_without_splitting_words() {
        let vocab = tiny_vocab();
        let tokenized = tokenize("A,B A#", &vocab);
        assert_eq!(
            tokenized
                .words
                .iter()
                .map(|w| w.text.as_str())
                .collect::<Vec<_>>(),
            vec!["AB", "A"]
        );
        assert_eq!(tokenized.tokens, vec![1, 2, 3, 1]);

        let empty = tokenize("€ —", &vocab);
        assert!(empty.tokens.is_empty());
        assert!(empty.words.is_empty());
    }

    #[test]
    fn lowercase_and_apostrophes_use_uppercase_tokens() {
        let vocab = tiny_vocab();
        let tokenized = tokenize("ab'a", &vocab);
        assert_eq!(tokenized.tokens, vec![1, 2, 4, 1]);
        assert_eq!(tokenized.words.len(), 1);
        assert_eq!(tokenized.words[0].text, "ab'a");
    }

    #[test]
    fn forced_path_gives_expected_word_boundaries() {
        // A B _ A: word "AB" spans frames 0..=2, word "A" is frame 4.
        let path = [1, 0, 2, 3, 1, 0, 0];
        let (tokenized, words) = align_tokens("AB A", &path, 8);
        assert_eq!(tokenized.words.len(), 2);
        assert_eq!(words.len(), 2);

        assert_eq!(words[0].text, "AB");
        assert_eq!(words[0].range.start, MediaTime::ZERO);
        assert_eq!(words[0].range.end, MediaTime::from_millis(60));
        assert_eq!(words[1].text, "A");
        assert_eq!(words[1].range.start, MediaTime::from_millis(80));
        assert_eq!(words[1].range.end, MediaTime::from_millis(100));
        assert!(
            words[0].range.end <= words[1].range.start,
            "word ranges must not overlap: {words:?}"
        );
        assert!(words.iter().all(|word| word.confidence.is_some()));
    }

    #[test]
    fn repeated_tokens_need_their_own_frames() {
        let (tokenized, words) = align_tokens("AA", &[1, 0, 1, 0], 8);
        assert_eq!(tokenized.tokens, vec![1, 1]);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].range.start, MediaTime::ZERO);
        assert_eq!(words[0].range.end, MediaTime::from_millis(60));
    }

    #[test]
    fn transcript_that_cannot_fit_returns_no_ranges() {
        let vocab = tiny_vocab();
        let path = [1_usize];
        let mut log_probs = logits_for(&path, 8);
        log_softmax(&mut log_probs, path.len(), 8);
        let ranges = token_frames(&log_probs, path.len(), 8, &[1, 2], vocab.blank);
        assert!(ranges.is_none());
    }

    #[test]
    fn a_blank_token_in_the_transcript_is_rejected() {
        let vocab = tiny_vocab();
        let ranges = token_frames(&[0.0; 16], 2, 8, &[1, 0], vocab.blank);
        assert!(ranges.is_none(), "a blank token is not a transcript token");
    }

    #[test]
    fn all_blank_emissions_produce_no_words() {
        let vocab = tiny_vocab();
        let mut log_probs = vec![0.0_f32; 4 * 8];
        // Blank is the peak on every frame.
        for frame in 0..4 {
            log_probs[frame * 8] = 12.0;
        }
        log_softmax(&mut log_probs, 4, 8);
        assert!(all_frames_blank(&log_probs, 4, 8, vocab.blank));
    }
}
