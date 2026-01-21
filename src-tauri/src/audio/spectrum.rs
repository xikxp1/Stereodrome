//! Spectrum analyzer using FFT to compute frequency band magnitudes.
//! Processes audio samples from a ring buffer and emits spectrum data via Tauri events.

use ringbuf::traits::Consumer;
use ringbuf::HeapCons;
use rustfft::{num_complex::Complex, FftPlanner};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// FFT window size - 2048 samples provides good frequency resolution at 44.1kHz
const FFT_SIZE: usize = 2048;

/// Number of frequency bands for visualization
const NUM_BANDS: usize = 12;

/// Update rate for spectrum emission (30Hz)
#[allow(dead_code)]
const UPDATE_INTERVAL_MS: u64 = 33;

/// Spectrum data emitted to frontend
#[derive(Debug, Clone, serde::Serialize)]
pub struct SpectrumData {
    /// Normalized band magnitudes (0.0 to 1.0)
    pub bands: Vec<f32>,
    /// Peak level for the current window
    pub peak: f32,
}

impl Default for SpectrumData {
    fn default() -> Self {
        Self {
            bands: vec![0.0; NUM_BANDS],
            peak: 0.0,
        }
    }
}

/// Frequency band boundaries in Hz
/// 12 bands from 60Hz-10kHz with focus on midrange
const BAND_FREQUENCIES: [(f32, f32); NUM_BANDS] = [
    (60.0, 100.0),     // Sub-bass
    (100.0, 170.0),    // Bass low
    (170.0, 280.0),    // Bass high
    (280.0, 450.0),    // Low-mids
    (450.0, 750.0),    // Mids low
    (750.0, 1200.0),   // Mids
    (1200.0, 2000.0),  // Mids high
    (2000.0, 3200.0),  // Upper-mids
    (3200.0, 4600.0),  // Presence
    (4600.0, 6000.0),  // Brilliance low
    (6000.0, 8000.0),  // Brilliance high
    (8000.0, 10000.0), // Highs
];

/// Per-band scaling to balance frequencies
const BAND_GAINS: [f32; NUM_BANDS] = [
    0.5,  // Sub-bass
    0.55, // Bass low
    0.6,  // Bass high
    0.7,  // Low-mids
    0.8,  // Mids low
    0.9,  // Mids
    1.0,  // Mids high
    1.2,  // Upper-mids
    1.4,  // Presence
    1.6,  // Brilliance low
    1.8,  // Brilliance high
    2.1,  // Highs
];

/// Spectrum analyzer that processes audio samples and computes FFT
pub struct SpectrumAnalyzer {
    sample_buffer: Vec<f32>,
    fft_input: Vec<Complex<f32>>,
    fft_output: Vec<Complex<f32>>,
    window: Vec<f32>,
    smoothed_bands: Vec<f32>,
    planner: FftPlanner<f32>,
    sample_rate: u32,
}

impl SpectrumAnalyzer {
    pub fn new(sample_rate: u32) -> Self {
        // Precompute Hann window
        let window: Vec<f32> = (0..FFT_SIZE)
            .map(|i| {
                let t = i as f32 / (FFT_SIZE - 1) as f32;
                0.5 * (1.0 - (2.0 * std::f32::consts::PI * t).cos())
            })
            .collect();

        Self {
            sample_buffer: Vec::with_capacity(FFT_SIZE),
            fft_input: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            fft_output: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            window,
            smoothed_bands: vec![0.0; NUM_BANDS],
            planner: FftPlanner::new(),
            sample_rate,
        }
    }

    /// Process samples from the ring buffer and return spectrum data if enough samples
    pub fn process(&mut self, consumer: &mut HeapCons<f32>) -> Option<SpectrumData> {
        // Read available samples
        while let Some(sample) = consumer.try_pop() {
            self.sample_buffer.push(sample);

            // Keep buffer size manageable
            if self.sample_buffer.len() > FFT_SIZE * 4 {
                self.sample_buffer.drain(0..FFT_SIZE * 2);
            }
        }

        // Need at least FFT_SIZE samples
        if self.sample_buffer.len() < FFT_SIZE {
            return None;
        }

        // Take the most recent FFT_SIZE samples
        let start_idx = self.sample_buffer.len() - FFT_SIZE;
        let samples = &self.sample_buffer[start_idx..];

        // Apply Hann window and convert to complex
        for (i, (&sample, &window)) in samples.iter().zip(self.window.iter()).enumerate() {
            self.fft_input[i] = Complex::new(sample * window, 0.0);
        }

        // Perform FFT
        let fft = self.planner.plan_fft_forward(FFT_SIZE);
        self.fft_output.copy_from_slice(&self.fft_input);
        fft.process(&mut self.fft_output);

        // Calculate magnitudes and aggregate into bands
        let mut bands = [0.0f32; NUM_BANDS];
        let mut band_counts = [0u32; NUM_BANDS];
        let mut peak = 0.0f32;
        let bin_hz = self.sample_rate as f32 / FFT_SIZE as f32;

        // Only use first half of FFT output (positive frequencies)
        for (bin_idx, complex) in self.fft_output.iter().take(FFT_SIZE / 2).enumerate() {
            let freq = bin_idx as f32 * bin_hz;
            let magnitude = complex.norm() / (FFT_SIZE as f32).sqrt();

            peak = peak.max(magnitude);

            // Find which band this frequency belongs to
            for (band_idx, &(low, high)) in BAND_FREQUENCIES.iter().enumerate() {
                if freq >= low && freq < high {
                    bands[band_idx] += magnitude;
                    band_counts[band_idx] += 1;
                    break;
                }
            }
        }

        // Average each band
        for i in 0..NUM_BANDS {
            if band_counts[i] > 0 {
                bands[i] /= band_counts[i] as f32;
            }
        }

        // Apply logarithmic scaling for better perceptual mapping
        const LOG_SCALE: f32 = 50.0;
        let log_denom = (1.0 + LOG_SCALE).log10();
        for band in &mut bands {
            *band = (1.0 + *band * LOG_SCALE).log10() / log_denom;
        }

        // Normalize bands (relative to current frame)
        let max_band = bands.iter().cloned().fold(0.0f32, f32::max);
        if max_band > 0.01 {
            for band in &mut bands {
                *band = (*band / max_band).min(1.0);
            }
        }

        // Apply per-band gain compensation AFTER normalization
        // This ensures higher frequencies get boosted relative to lower ones
        for i in 0..NUM_BANDS {
            bands[i] = (bands[i] * BAND_GAINS[i]).min(1.0);
        }

        // Apply smoothing (decay) for visual appeal
        const ATTACK: f32 = 0.8; // Quick rise
        const DECAY: f32 = 0.3; // Slower fall

        for (i, band) in bands.iter().enumerate() {
            if *band > self.smoothed_bands[i] {
                self.smoothed_bands[i] = self.smoothed_bands[i] * (1.0 - ATTACK) + band * ATTACK;
            } else {
                self.smoothed_bands[i] = self.smoothed_bands[i] * (1.0 - DECAY) + band * DECAY;
            }
        }

        // Normalize peak
        let normalized_peak = (peak * 4.0).min(1.0);

        Some(SpectrumData {
            bands: self.smoothed_bands.clone(),
            peak: normalized_peak,
        })
    }

    /// Clear the sample buffer (call when playback stops)
    pub fn clear(&mut self) {
        self.sample_buffer.clear();
        self.smoothed_bands.fill(0.0);
    }
}

/// Start the spectrum emitter thread that processes samples and emits events
#[allow(dead_code)]
pub fn start_spectrum_emitter(
    app_handle: AppHandle,
    consumer: Arc<Mutex<HeapCons<f32>>>,
    is_playing: Arc<AtomicBool>,
    sample_rate: u32,
) {
    thread::spawn(move || {
        let mut analyzer = SpectrumAnalyzer::new(sample_rate);

        loop {
            thread::sleep(Duration::from_millis(UPDATE_INTERVAL_MS));

            // Only process when playing
            if !is_playing.load(Ordering::SeqCst) {
                // Emit empty spectrum when not playing
                let _ = app_handle.emit("spectrum-data", SpectrumData::default());
                analyzer.clear();
                continue;
            }

            // Process samples and emit if we have data
            if let Ok(mut cons) = consumer.try_lock() {
                if let Some(spectrum) = analyzer.process(&mut cons) {
                    let _ = app_handle.emit("spectrum-data", spectrum);
                }
            }
        }
    });
}
