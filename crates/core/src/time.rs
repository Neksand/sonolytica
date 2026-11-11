//! Integer media time and half-open time ranges.

use std::fmt;
use std::ops::{Add, Sub};

use serde::{Deserialize, Serialize};

/// A point in time on the media timeline, in microseconds from the start of
/// the container.
///
/// Integer microseconds are used because floating point seconds accumulate
/// error across a long recording and do not compare exactly; sample-accurate
/// conversion only needs a rate and a sample index.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct MediaTime(i64);

impl MediaTime {
    pub const ZERO: Self = Self(0);
    pub const MAX: Self = Self(i64::MAX);

    pub const fn from_micros(micros: i64) -> Self {
        Self(micros)
    }

    pub const fn micros(self) -> i64 {
        self.0
    }

    pub fn from_secs_f64(seconds: f64) -> Self {
        if !seconds.is_finite() {
            return Self::ZERO;
        }
        Self((seconds * 1_000_000.0).round() as i64)
    }

    pub const fn from_millis(millis: i64) -> Self {
        Self(millis * 1_000)
    }

    /// Convert whisper-style centisecond timestamps.
    pub const fn from_centis(centis: i64) -> Self {
        Self(centis * 10_000)
    }

    /// Convert a sample index at `sample_rate` to a timeline position.
    pub fn from_sample(sample: usize, sample_rate: u32) -> Self {
        if sample_rate == 0 {
            return Self::ZERO;
        }
        Self((sample as i128 * 1_000_000 / sample_rate as i128) as i64)
    }

    /// Interval to the nearest sample index at `sample_rate`.
    pub fn to_sample(self, sample_rate: u32) -> usize {
        if self.0 <= 0 || sample_rate == 0 {
            return 0;
        }
        (self.0 as i128 * sample_rate as i128 / 1_000_000) as usize
    }

    pub fn as_secs_f64(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }

    pub fn as_millis(self) -> i64 {
        self.0.div_euclid(1_000)
    }

    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

impl Add for MediaTime {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for MediaTime {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl fmt::Display for MediaTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let total_ms = self.0.max(0) / 1_000;
        let hours = total_ms / 3_600_000;
        let minutes = (total_ms / 60_000) % 60;
        let seconds = (total_ms / 1_000) % 60;
        let millis = total_ms % 1_000;
        write!(f, "{hours:02}:{minutes:02}:{seconds:02}.{millis:03}")
    }
}

/// A half-open interval `[start, end)` on the media timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct TimeRange {
    pub start: MediaTime,
    pub end: MediaTime,
}

impl TimeRange {
    /// Construct a range. An inverted range is normalized to an empty range at
    /// its start rather than silently producing a negative duration.
    pub fn new(start: MediaTime, end: MediaTime) -> Self {
        if end < start {
            Self { start, end: start }
        } else {
            Self { start, end }
        }
    }

    pub fn empty(at: MediaTime) -> Self {
        Self { start: at, end: at }
    }

    pub fn from_secs(start: f64, end: f64) -> Self {
        Self::new(
            MediaTime::from_secs_f64(start),
            MediaTime::from_secs_f64(end),
        )
    }

    pub fn from_samples(start: usize, end: usize, sample_rate: u32) -> Self {
        Self::new(
            MediaTime::from_sample(start, sample_rate),
            MediaTime::from_sample(end, sample_rate),
        )
    }

    pub fn duration(&self) -> MediaTime {
        MediaTime(self.end.0 - self.start.0)
    }

    pub fn duration_secs(&self) -> f64 {
        self.duration().as_secs_f64()
    }

    pub fn is_empty(&self) -> bool {
        self.end <= self.start
    }

    pub fn contains(&self, t: MediaTime) -> bool {
        t >= self.start && t < self.end
    }

    pub fn contains_range(&self, other: &Self) -> bool {
        other.start >= self.start && other.end <= self.end
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    pub fn intersection(&self, other: &Self) -> Option<Self> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        (start < end).then_some(Self { start, end })
    }

    /// Duration of the intersection with `other`, zero when disjoint.
    pub fn overlap(&self, other: &Self) -> MediaTime {
        self.intersection(other)
            .map_or(MediaTime::ZERO, |r| r.duration())
    }

    pub fn union(&self, other: &Self) -> Self {
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    pub fn expand(&self, pad: MediaTime) -> Self {
        Self {
            start: self.start.saturating_sub(pad),
            end: self.end.saturating_add(pad),
        }
    }
}

impl fmt::Display for TimeRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverted_range_normalizes_to_empty() {
        let r = TimeRange::from_secs(5.0, 2.0);
        assert!(r.is_empty());
        assert_eq!(r.start, MediaTime::from_secs_f64(5.0));
        assert_eq!(r.duration(), MediaTime::ZERO);
    }

    #[test]
    fn sample_conversion_is_exact_for_common_rates() {
        // 0.5 s is exactly 8000 samples at 16 kHz and 16000 at 32 kHz.
        assert_eq!(MediaTime::from_secs_f64(0.5).to_sample(16_000), 8_000);
        assert_eq!(MediaTime::from_secs_f64(0.5).to_sample(32_000), 16_000);
        assert_eq!(
            MediaTime::from_sample(8_000, 16_000),
            MediaTime::from_millis(500)
        );
    }

    #[test]
    fn overlap_uses_half_open_semantics() {
        let a = TimeRange::from_secs(0.0, 2.0);
        let b = TimeRange::from_secs(2.0, 4.0);
        assert!(!a.intersects(&b), "touching ranges do not overlap");
        assert_eq!(a.overlap(&b), MediaTime::ZERO);

        let c = TimeRange::from_secs(1.5, 3.0);
        assert_eq!(a.overlap(&c), MediaTime::from_millis(500));
    }

    #[test]
    fn display_is_stable() {
        assert_eq!(
            MediaTime::from_millis(3_723_456).to_string(),
            "01:02:03.456"
        );
        assert_eq!(MediaTime::ZERO.to_string(), "00:00:00.000");
    }
}
