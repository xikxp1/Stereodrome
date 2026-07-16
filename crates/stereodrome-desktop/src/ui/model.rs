use std::sync::Arc;

use gpui::{AnyWindowHandle, Context, Task};
use stereodrome_audio::{PlaybackState, spectrum::SpectrumData};
use stereodrome_desktop::{DesktopBackend, DesktopEvent};
use tokio::task::JoinHandle;

use stereodrome_desktop::operations::{
    auth::{self, ConnectParams, ConnectionStatus},
    library::{LibraryContentUpdatedEvent, LibrarySyncStatus},
    normalization::AnalysisProgress,
    queue::{self, QueueState},
    settings::{self, ConnectivitySettings, PlaybackSettings, SyncSettings},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationView {
    Music,
    Search,
}

#[derive(Debug, Clone)]
pub struct NavigationState {
    pub active_view: NavigationView,
    pub queue_open: bool,
    pub settings_open: bool,
    pub search_focus_generation: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    pub song_id: Option<String>,
    pub row: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct UpdaterState {
    pub status: &'static str,
}

#[derive(Default)]
pub struct WindowPresence {
    pub main: Option<AnyWindowHandle>,
    pub mini: bool,
    pub nano: bool,
    pub cover_art: bool,
}

impl WindowPresence {
    pub fn auxiliary_count(&self) -> usize {
        usize::from(self.mini) + usize::from(self.nano) + usize::from(self.cover_art)
    }
}

#[derive(Debug, Clone)]
pub struct AuthState {
    pub status: ConnectionStatus,
    pub initialized: bool,
    pub initializing: bool,
    pub connecting: bool,
    pub error: Option<String>,
    generation: u64,
}

impl AuthState {
    fn empty() -> Self {
        Self {
            status: empty_connection_status(),
            initialized: false,
            initializing: false,
            connecting: false,
            error: None,
            generation: 0,
        }
    }

    fn next_generation(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.generation
    }

    fn accepts(&self, generation: u64) -> bool {
        self.generation == generation
    }

    fn visible_surface(&self) -> VisibleSurface {
        if self.initializing && !self.initialized {
            VisibleSurface::Restoring
        } else if self.status.server_url.is_some() {
            VisibleSurface::Library
        } else {
            VisibleSurface::Login
        }
    }

    fn offline(&self, connectivity: &ConnectivitySettings) -> bool {
        connectivity.manual_offline_enabled
            || (self.status.server_url.is_some() && !self.status.connected)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibleSurface {
    Restoring,
    Login,
    Library,
}

pub struct DesktopModel {
    pub auth: AuthState,
    pub connectivity: ConnectivitySettings,
    pub navigation: NavigationState,
    pub selection: SelectionState,
    pub playback: PlaybackState,
    pub queue: QueueState,
    pub spectrum: SpectrumData,
    pub spectrum_enabled: bool,
    pub playback_settings: PlaybackSettings,
    pub sync_settings: SyncSettings,
    pub updater: UpdaterState,
    pub windows: WindowPresence,
    pub quitting: bool,
    pub action_error: Option<String>,
    pub normalization_progress: Option<AnalysisProgress>,
    pub library_sync_status: Option<LibrarySyncStatus>,
    pub last_library_update: Option<LibraryContentUpdatedEvent>,
    pub cache_revision: u64,
    pub queue_ended: bool,
    backend: Arc<DesktopBackend>,
    previous_volume: f32,
    subscriptions_started: bool,
    subscription_tasks: Vec<JoinHandle<()>>,
    _message_task: Option<Task<()>>,
}

enum ModelMessage {
    Playback(PlaybackState),
    Spectrum(SpectrumData),
    Event(DesktopEvent),
}

impl DesktopModel {
    pub fn new(backend: Arc<DesktopBackend>) -> Self {
        let playback = backend.subscribe_playback().borrow().clone();
        let spectrum = backend.subscribe_spectrum().borrow().clone();
        let state = backend.state();
        let queue = queue::get_queue(&state);
        let connectivity = settings::get_connectivity_settings(&state.settings);
        let playback_settings = settings::get_playback_settings(&state.settings);
        let sync_settings = settings::get_sync_settings(&state.settings);
        let previous_volume = playback.volume.max(0.01);

        Self {
            auth: AuthState::empty(),
            connectivity,
            navigation: NavigationState {
                active_view: NavigationView::Music,
                queue_open: false,
                settings_open: false,
                search_focus_generation: 0,
            },
            selection: SelectionState::default(),
            playback,
            queue,
            spectrum,
            spectrum_enabled: true,
            playback_settings,
            sync_settings,
            updater: UpdaterState { status: "idle" },
            windows: WindowPresence::default(),
            quitting: false,
            action_error: None,
            normalization_progress: None,
            library_sync_status: None,
            last_library_update: None,
            cache_revision: 0,
            queue_ended: false,
            backend,
            previous_volume,
            subscriptions_started: false,
            subscription_tasks: Vec::new(),
            _message_task: None,
        }
    }

    pub fn visible_surface(&self) -> VisibleSurface {
        self.auth.visible_surface()
    }

    pub fn offline(&self) -> bool {
        self.auth.offline(&self.connectivity)
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        self.start_subscriptions(cx);
        self.restore_session(cx);
    }

    fn start_subscriptions(&mut self, cx: &mut Context<Self>) {
        if self.subscriptions_started {
            return;
        }
        self.subscriptions_started = true;

        let mut playback = self.backend.subscribe_playback();
        let mut spectrum = self.backend.subscribe_spectrum();
        let Some(mut events) = self.backend.take_event_receiver() else {
            self.action_error = Some("Backend event receiver was already subscribed".to_string());
            cx.notify();
            return;
        };
        let (sender, receiver) = async_channel::unbounded();
        let playback_sender = sender.clone();
        let spectrum_sender = sender.clone();
        let event_sender = sender;
        let runtime = self.backend.runtime_handle();

        self.subscription_tasks.push(runtime.spawn(async move {
            while playback.changed().await.is_ok() {
                let value = playback.borrow_and_update().clone();
                if playback_sender
                    .send(ModelMessage::Playback(value))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
        self.subscription_tasks.push(runtime.spawn(async move {
            while spectrum.changed().await.is_ok() {
                let value = spectrum.borrow_and_update().clone();
                if spectrum_sender
                    .send(ModelMessage::Spectrum(value))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }));
        self.subscription_tasks.push(runtime.spawn(async move {
            while let Some(event) = events.recv().await {
                if event_sender.send(ModelMessage::Event(event)).await.is_err() {
                    break;
                }
            }
        }));

        let weak = cx.weak_entity();
        self._message_task = Some(cx.spawn(async move |_, cx| {
            while let Ok(message) = receiver.recv().await {
                if weak
                    .update(cx, |model, cx| model.apply_message(message, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
    }

    fn apply_message(&mut self, message: ModelMessage, cx: &mut Context<Self>) {
        match message {
            ModelMessage::Playback(playback) => {
                if playback.volume > 0.0 {
                    self.previous_volume = playback.volume;
                }
                self.playback = playback;
            }
            ModelMessage::Spectrum(spectrum) => self.spectrum = spectrum,
            ModelMessage::Event(event) => match event {
                DesktopEvent::PlaybackEnded => {}
                DesktopEvent::QueueChanged(queue) => {
                    self.queue = queue;
                    self.queue_ended = false;
                }
                DesktopEvent::QueueEnded => self.queue_ended = true,
                DesktopEvent::AudioCacheChanged(_) => {
                    self.cache_revision = self.cache_revision.wrapping_add(1);
                }
                DesktopEvent::NormalizationProgress(progress) => {
                    self.normalization_progress = Some(progress);
                }
                DesktopEvent::LibrarySyncStatusChanged(status) => {
                    self.library_sync_status = Some(status);
                }
                DesktopEvent::LibraryContentUpdated(update) => {
                    self.last_library_update = Some(update);
                }
                DesktopEvent::PlaybackSettingsChanged(settings) => {
                    self.playback_settings = settings;
                }
                DesktopEvent::ConnectivitySettingsChanged(settings) => {
                    self.connectivity = settings;
                }
                DesktopEvent::SyncSettingsChanged(settings) => self.sync_settings = settings,
            },
        }
        cx.notify();
    }

    pub fn restore_session(&mut self, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let generation = self.auth.next_generation();
        self.auth.initializing = true;
        self.auth.connecting = false;
        self.auth.error = None;
        cx.notify();

        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            auth::restore_session(&state.settings, &state)
                .await
                .map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Session restore task failed: {error}")));
            weak.update(cx, |model, cx| {
                if model.quitting || !model.auth.accepts(generation) {
                    return;
                }
                model.auth.initializing = false;
                model.auth.initialized = true;
                match result {
                    Ok(status) => {
                        model.auth.status = status;
                        model.auth.error = None;
                    }
                    Err(error) => model.auth.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn connect(&mut self, params: ConnectParams, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        if self.connectivity.manual_offline_enabled {
            self.auth.error = Some("Offline mode is enabled".to_string());
            cx.notify();
            return;
        }
        let params = ConnectParams {
            url: params.url.trim().to_string(),
            username: params.username.trim().to_string(),
            password: params.password,
        };
        if params.url.is_empty() || params.username.is_empty() || params.password.is_empty() {
            self.auth.error = Some("Server URL, username, and password are required".to_string());
            cx.notify();
            return;
        }

        let generation = self.auth.next_generation();
        self.auth.connecting = true;
        self.auth.error = None;
        cx.notify();

        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            auth::connect_server(&state.settings, &state, params)
                .await
                .map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Connect task failed: {error}")));
            weak.update(cx, |model, cx| {
                if model.quitting || !model.auth.accepts(generation) {
                    return;
                }
                model.auth.connecting = false;
                model.auth.initialized = true;
                match result {
                    Ok(status) => {
                        model.auth.status = status;
                        model.auth.error = None;
                    }
                    Err(error) => model.auth.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn disconnect(&mut self, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let generation = self.auth.next_generation();
        self.auth.connecting = true;
        self.auth.error = None;
        cx.notify();

        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            auth::disconnect_server(&state)
                .await
                .map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Disconnect task failed: {error}")));
            weak.update(cx, |model, cx| {
                if model.quitting || !model.auth.accepts(generation) {
                    return;
                }
                model.auth.connecting = false;
                match result {
                    Ok(()) => {
                        model.auth.status = empty_connection_status();
                        model.auth.error = None;
                        model.selection = SelectionState::default();
                    }
                    Err(error) => model.auth.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn set_manual_offline(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let state = self.backend.state();
        match settings::set_connectivity_settings(
            &state,
            ConnectivitySettings {
                manual_offline_enabled: enabled,
            },
        ) {
            Ok(settings) => {
                self.connectivity = settings;
                self.restore_session(cx);
            }
            Err(error) => {
                self.action_error = Some(error.to_string());
                cx.notify();
            }
        }
    }

    pub fn stop_subscriptions(&mut self, cx: &mut Context<Self>) {
        for task in self.subscription_tasks.drain(..) {
            task.abort();
        }
        self._message_task.take();
        self.subscriptions_started = false;
        cx.notify();
    }

    pub fn begin_quit(&mut self, cx: &mut Context<Self>) -> bool {
        if self.quitting {
            return false;
        }
        self.quitting = true;
        self.auth.next_generation();
        self.stop_subscriptions(cx);
        true
    }

    pub fn set_action_error(&mut self, error: impl ToString, cx: &mut Context<Self>) {
        self.action_error = Some(error.to_string());
        cx.notify();
    }

    pub fn toggle_queue(&mut self, cx: &mut Context<Self>) {
        if !self.quitting {
            self.navigation.queue_open = !self.navigation.queue_open;
            cx.notify();
        }
    }

    pub fn toggle_spectrum(&mut self, cx: &mut Context<Self>) {
        if !self.quitting {
            self.spectrum_enabled = !self.spectrum_enabled;
            cx.notify();
        }
    }

    pub fn focus_search(&mut self, cx: &mut Context<Self>) {
        if !self.quitting {
            self.navigation.active_view = NavigationView::Search;
            self.navigation.search_focus_generation =
                self.navigation.search_focus_generation.wrapping_add(1);
            cx.notify();
        }
    }

    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        if !self.quitting {
            self.navigation.settings_open = true;
            cx.notify();
        }
    }

    pub fn navigate_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let row = self
            .selection
            .row
            .unwrap_or(if delta < 0 { 0 } else { usize::MAX });
        self.selection.row = Some(if delta < 0 {
            row.saturating_sub(delta.unsigned_abs())
        } else if row == usize::MAX {
            0
        } else {
            row.saturating_add(delta as usize)
        });
        cx.notify();
    }

    pub fn toggle_playback(&mut self, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let result = if self.playback.is_playing {
            stereodrome_desktop::operations::playback::pause_playback(&self.backend.state())
        } else {
            stereodrome_desktop::operations::playback::resume_playback(&self.backend.state())
        };
        if let Err(error) = result {
            self.set_action_error(error, cx);
        }
    }

    pub fn play_selection(&mut self, cx: &mut Context<Self>) {
        let Some(song_id) = self.selection.song_id.clone() else {
            return;
        };
        if self.quitting {
            return;
        }
        let state = self.backend.state();
        let runtime = self.backend.runtime_handle();
        let operation_runtime = runtime.clone();
        let task = runtime.spawn(async move {
            stereodrome_desktop::operations::playback::play_song_by_id(
                &operation_runtime,
                state,
                &song_id,
            )
            .await
            .map_err(|error| error.to_string())
        });
        self.observe_action(task, cx);
    }

    pub fn play_next(&mut self, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let state = self.backend.state();
        let runtime = self.backend.runtime_handle();
        let operation_runtime = runtime.clone();
        let task = runtime.spawn(async move {
            queue::play_next(&operation_runtime, state, None)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
        self.observe_action(task, cx);
    }

    pub fn play_previous(&mut self, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let state = self.backend.state();
        let runtime = self.backend.runtime_handle();
        let operation_runtime = runtime.clone();
        let task = runtime.spawn(async move {
            queue::play_previous(&operation_runtime, state)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
        self.observe_action(task, cx);
    }

    fn observe_action(&mut self, task: JoinHandle<Result<(), String>>, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Backend action task failed: {error}")));
            if let Err(error) = result {
                weak.update(cx, |model, cx| model.set_action_error(error, cx))
                    .ok();
            }
        })
        .detach();
    }

    pub fn seek_by(&mut self, delta: f64, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let position = (self.playback.position + delta).clamp(0.0, self.playback.duration.max(0.0));
        if let Err(error) = stereodrome_desktop::operations::playback::seek_playback(
            &self.backend.state(),
            position,
        ) {
            self.set_action_error(error, cx);
        }
    }

    pub fn adjust_volume(&mut self, delta: f32, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        self.set_volume((self.playback.volume + delta).clamp(0.0, 1.0), cx);
    }

    pub fn toggle_mute(&mut self, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let volume = if self.playback.volume > 0.0 {
            self.previous_volume = self.playback.volume;
            0.0
        } else {
            self.previous_volume.clamp(0.01, 1.0)
        };
        self.set_volume(volume, cx);
    }

    fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        if let Err(error) =
            stereodrome_desktop::operations::playback::set_volume(&self.backend.state(), volume)
        {
            self.set_action_error(error, cx);
        }
    }

    pub fn toggle_shuffle(&mut self, cx: &mut Context<Self>) {
        if !self.quitting && !self.queue.items.is_empty() {
            queue::toggle_shuffle(&self.backend.state());
            cx.notify();
        }
    }

    pub fn cycle_repeat(&mut self, cx: &mut Context<Self>) {
        if !self.quitting && !self.queue.items.is_empty() {
            queue::cycle_repeat_mode(&self.backend.state());
            cx.notify();
        }
    }

    pub fn reroll_next(&mut self, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        if let Err(error) = queue::reroll_next_queue_item(&self.backend.state()) {
            self.set_action_error(error, cx);
        }
    }
}

fn empty_connection_status() -> ConnectionStatus {
    ConnectionStatus {
        connected: false,
        server_url: None,
        username: None,
        server_version: None,
    }
}

#[cfg(test)]
mod tests {
    use super::{AuthState, VisibleSurface};
    use stereodrome_desktop::operations::settings::ConnectivitySettings;

    #[test]
    fn stale_auth_generations_are_rejected() {
        let mut auth = AuthState::empty();
        let stale = auth.next_generation();
        let current = auth.next_generation();
        assert!(!auth.accepts(stale));
        assert!(auth.accepts(current));
    }

    #[test]
    fn authentication_and_offline_states_match_the_shell_contract() {
        let mut auth = AuthState::empty();
        let mut connectivity = ConnectivitySettings::default();

        auth.initializing = true;
        assert_eq!(auth.visible_surface(), VisibleSurface::Restoring);

        auth.initializing = false;
        auth.initialized = true;
        assert_eq!(auth.visible_surface(), VisibleSurface::Login);

        auth.status.connected = true;
        auth.status.server_url = Some("https://music.example".to_string());
        assert_eq!(auth.visible_surface(), VisibleSurface::Library);
        assert!(!auth.offline(&connectivity));

        auth.status.connected = false;
        assert_eq!(auth.visible_surface(), VisibleSurface::Library);
        assert!(auth.offline(&connectivity));

        connectivity.manual_offline_enabled = true;
        auth.status.connected = true;
        assert_eq!(auth.visible_surface(), VisibleSurface::Library);
        assert!(auth.offline(&connectivity));

        auth.status = super::empty_connection_status();
        connectivity.manual_offline_enabled = false;
        assert_eq!(auth.visible_surface(), VisibleSurface::Login);
        assert!(!auth.offline(&connectivity));
    }
}
