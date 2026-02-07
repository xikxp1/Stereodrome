//! EBU R128 loudness analysis for audio normalization.
//! Decodes audio data and measures integrated loudness (LUFS) and true peak.

use ebur128::{EbuR128, Mode};
use rodio::Decoder;
use rodio::Source;
use std::io::Cursor;

use crate::error::{AppError, AppResult};

pub struct LoudnessResult {
    pub integrated_lufs: f64,
    pub true_peak: f64,
}

/// Analyze audio data for integrated loudness (EBU R128).
///
/// Decodes the full file and measures loudness. Typically takes 1-3 seconds
/// for a 4-minute song at 44.1kHz stereo.
pub fn analyze_loudness(audio_data: Vec<u8>) -> AppResult<LoudnessResult> {
    let byte_len = audio_data.len() as u64;
    let cursor = Cursor::new(audio_data);
    let source = Decoder::builder()
        .with_data(cursor)
        .with_byte_len(byte_len)
        .build()
        .map_err(|e| AppError::Audio(format!("Failed to decode audio for analysis: {e}")))?;

    let channels = source.channels() as u32;
    let sample_rate = source.sample_rate();

    let mut analyzer = EbuR128::new(channels, sample_rate, Mode::I | Mode::TRUE_PEAK)
        .map_err(|e| AppError::Audio(format!("Failed to create EBU R128 analyzer: {e}")))?;

    // Collect samples in chunks for efficient processing
    let chunk_size = (sample_rate as usize) * (channels as usize); // ~1 second of audio
    let mut buffer = Vec::with_capacity(chunk_size);

    for sample in source {
        buffer.push(sample);
        if buffer.len() >= chunk_size {
            analyzer
                .add_frames_f32(&buffer)
                .map_err(|e| AppError::Audio(format!("Failed to add frames: {e}")))?;
            buffer.clear();
        }
    }

    // Process remaining samples
    if !buffer.is_empty() {
        analyzer
            .add_frames_f32(&buffer)
            .map_err(|e| AppError::Audio(format!("Failed to add frames: {e}")))?;
    }

    let integrated_lufs = analyzer
        .loudness_global()
        .map_err(|e| AppError::Audio(format!("Failed to get integrated loudness: {e}")))?;

    // Get max true peak across all channels
    let mut true_peak: f64 = 0.0;
    for ch in 0..channels {
        let peak = analyzer
            .true_peak(ch)
            .map_err(|e| AppError::Audio(format!("Failed to get true peak: {e}")))?;
        if peak > true_peak {
            true_peak = peak;
        }
    }

    Ok(LoudnessResult {
        integrated_lufs,
        true_peak,
    })
}

/// Calculate linear gain factor for normalization.
///
/// Returns a gain multiplier to apply to each audio sample so that the
/// song's perceived loudness matches `target_lufs`.
pub fn calculate_gain(
    track_lufs: f64,
    track_peak: f64,
    target_lufs: f64,
    pre_amp_db: f64,
    prevent_clipping: bool,
) -> f32 {
    // If loudness is -inf (silence), don't apply gain
    if track_lufs.is_infinite() || track_lufs.is_nan() {
        return 1.0;
    }

    let gain_db = target_lufs - track_lufs + pre_amp_db;
    let mut gain = 10.0_f64.powf(gain_db / 20.0);

    if prevent_clipping && track_peak > 0.0 && gain * track_peak > 1.0 {
        gain = 1.0 / track_peak;
    }

    gain as f32
}
