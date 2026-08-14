//! Soft-knee RMS compressor for dynamics processing.
//! Reduces dynamic range by attenuating signals above a threshold,
//! making perceived loudness more consistent across tracks.

use num_traits::ToPrimitive;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DynamicsPreset {
    Light,
    Medium,
    Heavy,
}

pub struct Compressor {
    threshold_db: f32,
    ratio: f32,
    knee_db: f32,
    attack_coeff: f32,
    release_coeff: f32,
    makeup_gain: f32,
    envelope_sq: f32,
}

impl Compressor {
    #[must_use]
    pub fn new(preset: &DynamicsPreset, sample_rate: u32) -> Self {
        let (threshold_db, ratio, knee_db) = match preset {
            DynamicsPreset::Light => (-20.0_f32, 2.0_f32, 6.0_f32),
            DynamicsPreset::Medium => (-18.0, 3.0, 8.0),
            DynamicsPreset::Heavy => (-15.0, 4.0, 10.0),
        };

        let attack_secs = 0.020_f32;
        let release_secs = 0.200;
        let sr = sample_rate_as_f32(sample_rate);
        let attack_coeff = (-1.0 / (attack_secs * sr)).exp();
        let release_coeff = (-1.0 / (release_secs * sr)).exp();

        // Auto makeup gain: compensate for gain reduction at threshold
        let makeup_db = -threshold_db * (1.0 - 1.0 / ratio) / 2.0;
        let makeup_gain = 10.0_f32.powf(makeup_db / 20.0);

        Self {
            threshold_db,
            ratio,
            knee_db,
            attack_coeff,
            release_coeff,
            makeup_gain,
            envelope_sq: 0.0,
        }
    }

    /// Process a stereo frame and return the compressed output.
    pub fn process_frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        // Instantaneous power (average of L and R squared)
        let power = (left * left + right * right) * 0.5;

        // Update RMS envelope with asymmetric smoothing
        let coeff = if power > self.envelope_sq {
            self.attack_coeff
        } else {
            self.release_coeff
        };
        self.envelope_sq = coeff * self.envelope_sq + (1.0 - coeff) * power;

        // Convert envelope to dB (avoid log of zero)
        let level_db = 10.0 * (self.envelope_sq + 1e-10).log10();

        // Soft-knee gain computation
        let gain_db = self.compute_gain_reduction(level_db);

        let gain = 10.0_f32.powf(gain_db / 20.0) * self.makeup_gain;
        (left * gain, right * gain)
    }

    /// Compute gain reduction in dB using soft-knee characteristic.
    fn compute_gain_reduction(&self, level_db: f32) -> f32 {
        let half_knee = self.knee_db * 0.5;
        let knee_start = self.threshold_db - half_knee;
        let knee_end = self.threshold_db + half_knee;

        if level_db <= knee_start {
            // Below knee: no compression
            0.0
        } else if level_db >= knee_end {
            // Above knee: full ratio compression
            (self.threshold_db + (level_db - self.threshold_db) / self.ratio) - level_db
        } else {
            // In knee: quadratic interpolation
            let x = level_db - knee_start;
            let slope = (1.0 / self.ratio - 1.0) / (2.0 * self.knee_db);
            slope * x * x
        }
    }

    /// Reset compressor state (call on seek).
    pub fn reset(&mut self) {
        self.envelope_sq = 0.0;
    }
}

fn sample_rate_as_f32(sample_rate: u32) -> f32 {
    sample_rate.to_f32().unwrap_or(48_000.0)
}
