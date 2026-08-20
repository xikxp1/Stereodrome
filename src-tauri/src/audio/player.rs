use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use stereodrome_core::runtime::StereodromeAudioPort;
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::spectrum;
use crate::media::MediaControlsManager;

pub use stereodrome_audio::SongMetadata;

/// Starts desktop-only live projections from the runtime-owned audio engine.
///
/// This thread never mutates playback or queue state. Automatic transitions,
/// persistence, and preparation remain inside `StereodromeRuntimeHandle`.
pub fn start_position_emitter(
    audio: Arc<StereodromeAudioPort>,
    app_handle: AppHandle,
    running: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let mut position_update_counter: u8 = 0;
        while running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(100));
            let state = audio.playback_state_snapshot();
            let _ = app_handle.emit("playback-state", &state);

            position_update_counter = if position_update_counter == 9 {
                0
            } else {
                position_update_counter.saturating_add(1)
            };
            if position_update_counter == 0
                && let Some(media_controls) = app_handle.try_state::<MediaControlsManager>()
            {
                media_controls.set_playback_status(state.is_playing, state.position);
            }
        }
    });
}

/// Starts the spectrum visualizer tap for the runtime-owned audio engine.
pub fn start_spectrum_emitter(
    audio: Arc<StereodromeAudioPort>,
    app_handle: AppHandle,
    running: Arc<AtomicBool>,
) {
    const DEFAULT_SAMPLE_RATE: u32 = 44_100;

    let consumer = audio.spectrum_consumer();
    thread::spawn(move || {
        let mut analyzer = spectrum::SpectrumAnalyzer::new(DEFAULT_SAMPLE_RATE);
        let mut emitted_idle_state = false;

        while running.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(33));
            let state = audio.playback_state_snapshot();
            if !state.is_playing {
                if !emitted_idle_state {
                    let _ = app_handle.emit("spectrum-data", spectrum::SpectrumData::default());
                    analyzer.clear();
                    emitted_idle_state = true;
                }
                continue;
            }

            emitted_idle_state = false;
            if let Ok(mut consumer) = consumer.try_lock()
                && let Some(data) = analyzer.process(&mut consumer)
            {
                let _ = app_handle.emit("spectrum-data", data);
            }
        }
    });
}
