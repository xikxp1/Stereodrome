//! BinauralSource wraps a Rodio Source with bs2b stereo crossfeed processing.
//! Pipeline: inner source -> bs2b crossfeed -> output.

use bs2b::{Bs2b, Level, streaming::CallbackAdapter};
use rodio::source::SeekError;
use rodio::{ChannelCount, SampleRate, Source};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BinauralPreset {
    Default,
    Cmoy,
    Jmeier,
    Aggressive,
}

impl BinauralPreset {
    fn level(&self) -> Level {
        match self {
            Self::Default => Level::DEFAULT,
            Self::Cmoy => Level::CMOY,
            Self::Jmeier => Level::JMEIER,
            // Strong profile for A/B testing audibility.
            Self::Aggressive => Level::new(850, 15).unwrap_or(Level::JMEIER),
        }
    }
}

/// A Source wrapper that applies bs2b binaural crossfeed.
pub struct BinauralSource<S>
where
    S: Source<Item = f32>,
{
    inner: S,
    adapter: Option<CallbackAdapter>,
    channels: ChannelCount,
    input_buffer: Vec<f32>,
    output_buffer: Vec<f32>,
    output_pos: usize,
}

impl<S> BinauralSource<S>
where
    S: Source<Item = f32>,
{
    pub fn new(source: S, preset: &BinauralPreset) -> Self {
        let channels = source.channels();
        let sample_rate = source.sample_rate();
        let level = preset.level();

        // Fallback to default sample rate if input sample rate is outside bs2b bounds.
        let adapter = if channels.get() >= 2 {
            let processor = Bs2b::new(sample_rate.get(), level).unwrap_or_else(|_| {
                let mut bs2b = Bs2b::default();
                bs2b.set_level(level);
                bs2b
            });
            CallbackAdapter::new(processor, channels.get() as usize).ok()
        } else {
            None
        };

        let ch = channels.get() as usize;
        Self {
            inner: source,
            adapter,
            channels,
            input_buffer: vec![0.0; ch],
            output_buffer: vec![0.0; ch],
            // Start exhausted to trigger first fill
            output_pos: ch,
        }
    }
}

impl<S> Iterator for BinauralSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        // Return buffered output if available
        if self.output_pos < self.channels.get() as usize {
            let sample = self.output_buffer[self.output_pos];
            self.output_pos += 1;
            return Some(sample);
        }

        // Collect a full frame from inner source into pre-allocated buffer
        let ch = self.channels.get() as usize;
        for i in 0..ch {
            self.input_buffer[i] = self.inner.next()?;
        }

        if ch >= 2 {
            self.output_buffer[..ch].copy_from_slice(&self.input_buffer[..ch]);
            if let Some(adapter) = self.adapter.as_mut() {
                let _ = adapter.process(&mut self.output_buffer[..ch]);
            }
        } else if ch == 1 {
            self.output_buffer[0] = self.input_buffer[0];
        }

        self.output_pos = 1;
        Some(self.output_buffer[0])
    }
}

impl<S> Source for BinauralSource<S>
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
        let result = self.inner.try_seek(pos);
        if result.is_ok() {
            if let Some(adapter) = self.adapter.as_mut() {
                adapter.processor_mut().clear();
            }
            self.output_pos = self.channels.get() as usize;
        }
        result
    }
}
