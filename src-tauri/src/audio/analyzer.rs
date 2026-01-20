//! Audio sample analyzer that wraps a Rodio Source to capture PCM samples
//! for spectrum analysis while forwarding audio to the sink.

use ringbuf::traits::Producer;
use ringbuf::HeapProd;
use rodio::Source;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

/// A Source wrapper that captures PCM samples to a ring buffer while forwarding to output.
/// This allows real-time spectrum analysis without affecting audio playback.
pub struct AnalyzingSource<S>
where
    S: Source<Item = f32>,
{
    inner: S,
    producer: Arc<Mutex<HeapProd<f32>>>,
}

impl<S> AnalyzingSource<S>
where
    S: Source<Item = f32>,
{
    pub fn new(source: S, producer: Arc<Mutex<HeapProd<f32>>>) -> Self {
        Self {
            inner: source,
            producer,
        }
    }
}

impl<S> Iterator for AnalyzingSource<S>
where
    S: Source<Item = f32>,
{
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        let sample = self.inner.next()?;

        // Try to push sample to ring buffer (non-blocking, drops if full)
        if let Ok(mut prod) = self.producer.try_lock() {
            let _ = prod.try_push(sample);
        }

        Some(sample)
    }
}

impl<S> Source for AnalyzingSource<S>
where
    S: Source<Item = f32>,
{
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }

    fn channels(&self) -> u16 {
        self.inner.channels()
    }

    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}
