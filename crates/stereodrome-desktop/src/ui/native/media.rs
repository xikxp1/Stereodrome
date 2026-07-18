use std::{ffi::c_void, time::Duration};

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};
use stereodrome_audio::PlaybackState;

pub struct MediaService {
    controls: MediaControls,
}

impl MediaService {
    pub fn new(
        hwnd: Option<*mut c_void>,
    ) -> Result<(Self, async_channel::Receiver<MediaControlEvent>), String> {
        let mut controls = MediaControls::new(PlatformConfig {
            dbus_name: "dev.xikxp1.stereodrome",
            display_name: "Stereodrome",
            hwnd,
        })
        .map_err(|error| format!("Failed to initialize media controls: {error}"))?;
        let (sender, receiver) = async_channel::unbounded();
        controls
            .attach(move |event| {
                let _ = sender.try_send(event);
            })
            .map_err(|error| format!("Failed to attach media controls: {error}"))?;
        Ok((Self { controls }, receiver))
    }

    pub fn update(&mut self, state: &PlaybackState) {
        let _ = self.controls.set_metadata(match state.song.as_ref() {
            Some(song) => MediaMetadata {
                title: Some(&song.title),
                artist: Some(&song.artist),
                album: Some(&song.album),
                duration: Some(Duration::from_secs_f64(state.duration.max(0.0))),
                ..Default::default()
            },
            None => MediaMetadata::default(),
        });
        let progress = Some(MediaPosition(Duration::from_secs_f64(
            state.position.max(0.0),
        )));
        let playback = if state.song.is_none() {
            MediaPlayback::Stopped
        } else if state.is_playing {
            MediaPlayback::Playing { progress }
        } else {
            MediaPlayback::Paused { progress }
        };
        let _ = self.controls.set_playback(playback);
    }
}
