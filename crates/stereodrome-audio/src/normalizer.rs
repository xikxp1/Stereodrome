//! Audio normalizer that wraps a Rodio Source to apply a constant gain factor
//! for loudness normalization (EBU R128 / `ReplayGain`).

use rodio::source::SeekError;
use rodio::{ChannelCount, SampleRate, Source};
use std::time::Duration;

/// A Source wrapper that applies a constant linear gain factor to every sample.
/// Used for loudness normalization so all tracks play at a consistent volume.
pub struct NormalizingSource<S>
where
    S: Source<Item = f32>,
{
    inner: S,
    gain: f32,
    clamp: bool,
}

impl<S> NormalizingSource<S>
where
    S: Source<Item = f32>,
{
    pub fn new(source: S, gain: f32) -> Self {
        Self {
            inner: source,
            gain,
            clamp: true,
        }
    }

    /// Create with explicit clamp control. When dynamics processing is active,
    /// the limiter handles peak control, so clamping is unnecessary.
    pub fn with_clamp(source: S, gain: f32, clamp: bool) -> Self {
        Self {
            inner: source,
            gain,
            clamp,
        }
    }
}

impl<S> Iterator for NormalizingSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;
        let normalized = sample * self.gain;
        if self.clamp {
            Some(normalized.clamp(-1.0, 1.0))
        } else {
            Some(normalized)
        }
    }
}

impl<S> Source for NormalizingSource<S>
where
    S: Source<Item = f32>,
{
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }

    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(pos)
    }
}
