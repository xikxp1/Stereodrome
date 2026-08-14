//! `DynamicsSource` wraps a Rodio `Source` with compression and look-ahead limiting.
//! Pipeline: inner source → soft-knee compressor → fundsp limiter → output.

use fundsp::audiounit::AudioUnit;
use fundsp::prelude32::{limiter, limiter_stereo};
use rodio::source::SeekError;
use rodio::{ChannelCount, SampleRate, Source};
use std::time::Duration;

use crate::compressor::{Compressor, DynamicsPreset};

/// A Source wrapper that applies compression and look-ahead limiting.
pub struct DynamicsSource<S>
where
    S: Source<Item = f32>,
{
    inner: S,
    compressor: Compressor,
    limiter: Box<dyn AudioUnit>,
    channels: ChannelCount,
    input_buffer: Vec<f32>,
    output_buffer: Vec<f32>,
    output_pos: usize,
}

impl<S> DynamicsSource<S>
where
    S: Source<Item = f32>,
{
    pub fn new(source: S, preset: &DynamicsPreset) -> Self {
        let channels = source.channels();
        let sample_rate = source.sample_rate();

        let compressor = Compressor::new(preset, sample_rate.get());

        let mut lim: Box<dyn AudioUnit> = if channels.get() >= 2 {
            Box::new(limiter_stereo(0.01, 0.1))
        } else {
            Box::new(limiter(0.01, 0.1))
        };
        lim.set_sample_rate(f64::from(sample_rate.get()));
        lim.reset();

        let ch = usize::from(channels.get());
        Self {
            inner: source,
            compressor,
            limiter: lim,
            channels,
            input_buffer: vec![0.0; ch],
            output_buffer: vec![0.0; ch],
            // Start exhausted to trigger first fill
            output_pos: usize::from(channels.get()),
        }
    }
}

impl<S> Iterator for DynamicsSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        // Return buffered output if available
        if self.output_pos < usize::from(self.channels.get()) {
            let sample = self.output_buffer.get(self.output_pos).copied()?;
            self.output_pos = self.output_pos.checked_add(1)?;
            return Some(sample);
        }

        // Collect a full frame from inner source into pre-allocated buffer
        let ch = usize::from(self.channels.get());
        for input in self.input_buffer.iter_mut().take(ch) {
            *input = self.inner.next()?;
        }

        // Compress (always stereo-style: mono duplicates the channel)
        let left = self.input_buffer.first().copied()?;
        let right = self.input_buffer.get(1).copied().unwrap_or(left);
        let (comp_l, comp_r) = self.compressor.process_frame(left, right);

        // Limit via fundsp
        if ch >= 2 {
            let input = [comp_l, comp_r];
            self.limiter.tick(&input, &mut self.output_buffer);
        } else {
            let input = [comp_l];
            self.limiter.tick(&input, &mut self.output_buffer);
        }

        self.output_pos = 1;
        self.output_buffer.first().copied()
    }
}

impl<S> Source for DynamicsSource<S>
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
            self.compressor.reset();
            self.limiter.reset();
            self.output_pos = usize::from(self.channels.get());
        }
        result
    }
}
