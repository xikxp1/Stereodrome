use std::sync::Mutex;

use tokio::sync::{mpsc, watch};

use crate::audio::player::AudioPlayer;
use crate::cache::AudioCacheChangedEvent;
use crate::operations::library::{LibraryContentUpdatedEvent, LibrarySyncStatus};
use crate::operations::normalization::AnalysisProgress;
use crate::operations::queue::QueueState;
use crate::operations::settings::{ConnectivitySettings, PlaybackSettings, SyncSettings};

pub use stereodrome_audio::PlaybackState;
pub use stereodrome_audio::spectrum::SpectrumData;

#[derive(Debug, Clone)]
pub enum DesktopEvent {
    PlaybackEnded,
    QueueChanged(QueueState),
    QueueEnded,
    AudioCacheChanged(AudioCacheChangedEvent),
    NormalizationProgress(AnalysisProgress),
    LibrarySyncStatusChanged(LibrarySyncStatus),
    LibraryContentUpdated(LibraryContentUpdatedEvent),
    PlaybackSettingsChanged(PlaybackSettings),
    ConnectivitySettingsChanged(ConnectivitySettings),
    SyncSettingsChanged(SyncSettings),
}

pub struct DesktopEvents {
    playback_tx: watch::Sender<PlaybackState>,
    spectrum_tx: watch::Sender<SpectrumData>,
    durable_tx: mpsc::UnboundedSender<DesktopEvent>,
    durable_rx: Mutex<Option<mpsc::UnboundedReceiver<DesktopEvent>>>,
}

impl DesktopEvents {
    pub fn new(audio_player: &AudioPlayer) -> Self {
        let playback = audio_player.state_handle().get_gapless_state().0;
        let (playback_tx, _) = watch::channel(playback);
        let (spectrum_tx, _) = watch::channel(SpectrumData::default());
        let (durable_tx, durable_rx) = mpsc::unbounded_channel();
        Self {
            playback_tx,
            spectrum_tx,
            durable_tx,
            durable_rx: Mutex::new(Some(durable_rx)),
        }
    }

    pub fn subscribe_playback(&self) -> watch::Receiver<PlaybackState> {
        self.playback_tx.subscribe()
    }

    pub fn subscribe_spectrum(&self) -> watch::Receiver<SpectrumData> {
        self.spectrum_tx.subscribe()
    }

    pub fn publish_playback(&self, playback: PlaybackState) {
        self.playback_tx.send_replace(playback);
    }

    pub fn publish_spectrum(&self, spectrum: SpectrumData) {
        self.spectrum_tx.send_replace(spectrum);
    }

    pub fn audio_cache_changed(&self, reason: &'static str) {
        let _ = self
            .durable_tx
            .send(DesktopEvent::AudioCacheChanged(AudioCacheChangedEvent {
                reason,
            }));
    }

    pub fn normalization_progress(&self, progress: AnalysisProgress) {
        let _ = self
            .durable_tx
            .send(DesktopEvent::NormalizationProgress(progress));
    }

    pub fn library_sync_status_changed(&self, status: LibrarySyncStatus) {
        let _ = self
            .durable_tx
            .send(DesktopEvent::LibrarySyncStatusChanged(status));
    }

    pub fn library_content_updated(&self, event: LibraryContentUpdatedEvent) {
        let _ = self
            .durable_tx
            .send(DesktopEvent::LibraryContentUpdated(event));
    }

    pub fn queue_changed(&self, state: QueueState) {
        let _ = self.durable_tx.send(DesktopEvent::QueueChanged(state));
    }

    pub fn queue_ended(&self) {
        let _ = self.durable_tx.send(DesktopEvent::QueueEnded);
    }

    pub fn playback_ended(&self) {
        let _ = self.durable_tx.send(DesktopEvent::PlaybackEnded);
    }

    pub fn playback_settings_changed(&self, settings: PlaybackSettings) {
        let _ = self
            .durable_tx
            .send(DesktopEvent::PlaybackSettingsChanged(settings));
    }

    pub fn connectivity_settings_changed(&self, settings: ConnectivitySettings) {
        let _ = self
            .durable_tx
            .send(DesktopEvent::ConnectivitySettingsChanged(settings));
    }

    pub fn sync_settings_changed(&self, settings: SyncSettings) {
        let _ = self
            .durable_tx
            .send(DesktopEvent::SyncSettingsChanged(settings));
    }

    pub fn take_durable_receiver(&self) -> Option<mpsc::UnboundedReceiver<DesktopEvent>> {
        self.durable_rx
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
    }
}
