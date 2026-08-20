//! 12-band graphic equalizer using fundsp biquad bell filters.
//! Applies per-band gain in dB across fixed center frequencies.

use fundsp::audionode::AudioNode;
use fundsp::biquad::{Biquad, BiquadCoefs};
use num_traits::ToPrimitive;
use rodio::source::SeekError;
use rodio::{ChannelCount, SampleRate, Source};
use std::time::Duration;

pub const EQ_BAND_COUNT: usize = 12;
pub const EQ_MIN_DB: f32 = -12.0;
pub const EQ_MAX_DB: f32 = 12.0;
const EQ_Q: f32 = 1.1;

/// Center frequencies (Hz) for the 12-band graphic EQ.
pub const EQ_BAND_FREQUENCIES: [f32; EQ_BAND_COUNT] = [
    32.0, 64.0, 125.0, 250.0, 500.0, 1_000.0, 2_000.0, 4_000.0, 8_000.0, 12_000.0, 16_000.0,
    20_000.0,
];

#[derive(Debug, Clone)]
pub struct EqualizerSettings {
    pub bands_db: Vec<f32>,
}

impl EqualizerSettings {
    #[must_use]
    pub fn new(mut bands_db: Vec<f32>) -> Self {
        bands_db.truncate(EQ_BAND_COUNT);
        bands_db.resize(EQ_BAND_COUNT, 0.0);
        for value in &mut bands_db {
            *value = value.clamp(EQ_MIN_DB, EQ_MAX_DB);
        }
        Self { bands_db }
    }

    #[must_use]
    pub fn is_flat(&self) -> bool {
        self.bands_db.iter().all(|v| v.abs() < 0.01)
    }
}

#[must_use]
pub fn default_bands_db() -> Vec<f32> {
    vec![0.0; EQ_BAND_COUNT]
}

#[must_use]
pub fn sanitize_bands_db(input: &[f32]) -> Vec<f32> {
    let mut output = vec![0.0; EQ_BAND_COUNT];
    for (slot, value) in output.iter_mut().zip(input.iter().copied()) {
        *slot = value.clamp(EQ_MIN_DB, EQ_MAX_DB);
    }
    output
}

/// A Source wrapper that applies a 12-band equalizer.
pub struct EqualizerSource<S>
where
    S: Source<Item = f32>,
{
    inner: S,
    channels: ChannelCount,
    filters: Vec<Vec<Biquad<f32>>>,
    output_buffer: Vec<f32>,
    output_pos: usize,
}

impl<S> EqualizerSource<S>
where
    S: Source<Item = f32>,
{
    pub fn new(source: S, settings: &EqualizerSettings) -> Self {
        let channels = source.channels();
        let sample_rate = sample_rate_as_f32(source.sample_rate().get());
        let max_center = sample_rate * 0.475;

        let gains: Vec<f32> = settings
            .bands_db
            .iter()
            .map(|db| 10.0_f32.powf(db / 20.0))
            .collect();

        let channel_count = usize::from(channels.get());
        let mut filters = Vec::with_capacity(channel_count);
        for _ in 0..channel_count {
            let mut channel_filters = Vec::with_capacity(EQ_BAND_COUNT);
            for (frequency, gain) in EQ_BAND_FREQUENCIES
                .iter()
                .copied()
                .zip(gains.iter().copied())
            {
                let mut biquad = Biquad::<f32>::new();
                biquad.set_sample_rate(f64::from(sample_rate));
                biquad.set_coefs(BiquadCoefs::bell(
                    sample_rate,
                    frequency.min(max_center),
                    EQ_Q,
                    gain,
                ));
                biquad.reset();
                channel_filters.push(biquad);
            }
            filters.push(channel_filters);
        }

        let ch = channel_count;
        Self {
            inner: source,
            channels,
            filters,
            output_buffer: vec![0.0; ch],
            // Start exhausted to trigger first fill.
            output_pos: ch,
        }
    }
}

impl<S> Iterator for EqualizerSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.output_pos < usize::from(self.channels.get()) {
            let sample = self.output_buffer.get(self.output_pos).copied()?;
            self.output_pos = self.output_pos.checked_add(1)?;
            return Some(sample);
        }

        let ch = usize::from(self.channels.get());
        for (channel_filters, output) in self
            .filters
            .iter_mut()
            .zip(self.output_buffer.iter_mut())
            .take(ch)
        {
            let mut sample = self.inner.next()?;
            for filter in channel_filters {
                let input = [sample].into();
                sample = filter.tick(&input).iter().next().copied().unwrap_or(sample);
            }
            *output = sample.clamp(-1.0, 1.0);
        }

        self.output_pos = 1;
        self.output_buffer.first().copied()
    }
}

impl<S> Source for EqualizerSource<S>
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
            for channel_filters in &mut self.filters {
                for filter in channel_filters {
                    filter.reset();
                }
            }
            self.output_pos = usize::from(self.channels.get());
        }
        result
    }
}

fn sample_rate_as_f32(sample_rate: u32) -> f32 {
    sample_rate.to_f32().unwrap_or(48_000.0)
}
