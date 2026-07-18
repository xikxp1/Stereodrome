use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use gpui::{AnyWindowHandle, Context, Modifiers, Task};
use stereodrome_audio::{PlaybackState, spectrum::SpectrumData};
use stereodrome_desktop::{
    DesktopBackend, DesktopEvent, audio::queue::QueueItem, client::AlbumListEntry,
};
use tokio::task::JoinHandle;

use stereodrome_desktop::lastfm::{self, LastfmStatus};
use stereodrome_desktop::operations::{
    auth::{self, ConnectParams, ConnectionStatus},
    cover_art,
    library::{
        self, Album, Artist, LibraryContentUpdatedEvent, LibrarySyncStatus, ScanStatus, Song,
    },
    normalization::{self, AnalysisProgress},
    playlist::{self, Playlist, PlaylistSong},
    queue::{self, QueueState},
    search::{self, SearchResults},
    settings::{
        self, ConnectivitySettings, NormalizationSettings, NotificationSettings, PlaybackSettings,
        SyncSettings, SystemTimePreferences,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavigationView {
    Music,
    Artists,
    Albums,
    RecentlyAdded,
    RecentlyPlayed,
    MostPlayed,
    Playlists,
    Search,
}

#[derive(Debug, Clone)]
pub struct NavigationState {
    pub active_view: NavigationView,
    pub queue_open: bool,
    pub search_focus_generation: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    pub song_id: Option<String>,
    pub song_ids: Vec<String>,
    pub row: Option<usize>,
}

#[derive(Default)]
pub struct LibraryState {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub songs: Vec<Song>,
    pub playlists: Vec<Playlist>,
    pub playlist_songs: Vec<PlaylistSong>,
    pub offline_song_ids: HashSet<String>,
    pub selected_genre: Option<String>,
    pub selected_artist_id: Option<String>,
    pub selected_album_id: Option<String>,
    pub selected_playlist_id: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub search_query: String,
    pub search_results: Option<SearchResults>,
    pub search_pending: bool,
    pub discovery_albums: Vec<AlbumListEntry>,
    pub discovery_loading: bool,
    generation: u64,
    search_generation: u64,
    discovery_generation: u64,
    playlist_generation: u64,
}

impl LibraryState {
    fn accepts_search(&self, generation: u64) -> bool {
        self.search_generation == generation
    }
}

#[derive(Debug, Clone)]
pub struct UpdaterState {
    pub status: &'static str,
    pub version: Option<String>,
    pub notes: Option<String>,
    pub busy: bool,
}

#[derive(Default)]
pub struct WindowPresence {
    pub main: Option<AnyWindowHandle>,
    pub mini: Option<AnyWindowHandle>,
    pub nano: Option<AnyWindowHandle>,
    pub cover_art: Option<AnyWindowHandle>,
    pub settings: Option<AnyWindowHandle>,
}

impl WindowPresence {
    pub fn auxiliary_count(&self) -> usize {
        usize::from(self.mini.is_some())
            + usize::from(self.nano.is_some())
            + usize::from(self.cover_art.is_some())
            + usize::from(self.settings.is_some())
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
    fn apply_status(&mut self, mut status: ConnectionStatus) {
        if status.server_version.is_none()
            && status.server_url == self.status.server_url
            && status.username == self.status.username
        {
            status
                .server_version
                .clone_from(&self.status.server_version);
        }
        self.status = status;
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
    pub notification_settings: NotificationSettings,
    pub normalization_settings: NormalizationSettings,
    pub lastfm_status: LastfmStatus,
    pub updater: UpdaterState,
    pub windows: WindowPresence,
    pub quitting: bool,
    pub tray_available: bool,
    pub action_error: Option<String>,
    pub normalization_progress: Option<AnalysisProgress>,
    pub library_sync_status: Option<LibrarySyncStatus>,
    pub time_preferences: SystemTimePreferences,
    pub scan_status: Option<ScanStatus>,
    pub last_library_update: Option<LibraryContentUpdatedEvent>,
    pub library: LibraryState,
    pub cache_revision: u64,
    pub queue_ended: bool,
    pub cover_art_path: Option<PathBuf>,
    pub current_cover_art_path: Option<PathBuf>,
    pub cover_art_paths: HashMap<String, PathBuf>,
    cover_art_requests: HashSet<String>,
    cover_art_generation: u64,
    current_cover_art_generation: u64,
    scan_generation: u64,
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
    RefreshConnection,
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
        let notification_settings = settings::get_notification_settings(&state.settings);
        let normalization_settings = settings::get_normalization_settings(&state.settings);
        let lastfm_status = lastfm::lastfm_status(&state.settings, &state);
        let library_sync_status = library::get_library_sync_status(&state).ok();
        let time_preferences = settings::get_system_time_preferences();
        let previous_volume = playback.volume.max(0.01);

        Self {
            auth: AuthState::empty(),
            connectivity,
            navigation: NavigationState {
                active_view: NavigationView::Music,
                queue_open: false,
                search_focus_generation: 0,
            },
            selection: SelectionState::default(),
            playback,
            queue,
            spectrum,
            spectrum_enabled: true,
            playback_settings,
            sync_settings,
            notification_settings,
            normalization_settings,
            lastfm_status,
            updater: UpdaterState {
                status: "idle",
                version: None,
                notes: None,
                busy: false,
            },
            windows: WindowPresence::default(),
            quitting: false,
            tray_available: false,
            action_error: None,
            normalization_progress: None,
            library_sync_status,
            time_preferences,
            scan_status: None,
            last_library_update: None,
            library: LibraryState::default(),
            cache_revision: 0,
            queue_ended: false,
            cover_art_path: None,
            current_cover_art_path: None,
            cover_art_paths: HashMap::new(),
            cover_art_requests: HashSet::new(),
            cover_art_generation: 0,
            current_cover_art_generation: 0,
            scan_generation: 0,
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
    pub fn refresh_connection_status(&mut self, cx: &mut Context<Self>) {
        let state = self.backend.state();
        if let Ok(status) = auth::get_connection_status(&state.settings, &state) {
            let previous = self.auth.status.clone();
            self.auth.apply_status(status);
            if self.auth.status != previous {
                cx.notify();
            }
        }
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
        let refresh_sender = sender.clone();
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
        self.subscription_tasks.push(runtime.spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(10)).await;
                if refresh_sender
                    .send(ModelMessage::RefreshConnection)
                    .await
                    .is_err()
                {
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
                let previous_cover_art_id = self
                    .playback
                    .song
                    .as_ref()
                    .and_then(|song| song.cover_art_id.as_deref());
                let cover_art_id = playback
                    .song
                    .as_ref()
                    .and_then(|song| song.cover_art_id.clone());
                let cover_changed = previous_cover_art_id != cover_art_id.as_deref();
                self.playback = playback;
                if cover_changed {
                    self.refresh_current_cover_art(cover_art_id, cx);
                }
            }
            ModelMessage::Spectrum(spectrum) => {
                if !self.spectrum_enabled || !self.playback.is_playing {
                    return;
                }
                self.spectrum = spectrum;
            }
            ModelMessage::RefreshConnection => self.refresh_connection_status(cx),
            ModelMessage::Event(event) => match event {
                DesktopEvent::PlaybackEnded => {}
                DesktopEvent::QueueChanged(queue) => {
                    self.queue = queue;
                    self.queue_ended = false;
                }
                DesktopEvent::QueueEnded => self.queue_ended = true,
                DesktopEvent::AudioCacheChanged(_) => {
                    self.cache_revision = self.cache_revision.wrapping_add(1);
                    if self.offline() {
                        self.refresh_library(cx);
                    }
                }
                DesktopEvent::NormalizationProgress(progress) => {
                    self.normalization_progress = Some(progress);
                }
                DesktopEvent::LibrarySyncStatusChanged(status) => {
                    self.library_sync_status = Some(status);
                }
                DesktopEvent::LibraryContentUpdated(update) => {
                    self.last_library_update = Some(update);
                    self.refresh_library(cx);
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
                        model.auth.apply_status(status);
                        model.auth.error = None;
                        model.refresh_library(cx);
                        model.refresh_library_statuses(cx);
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
                        model.auth.apply_status(status);
                        model.auth.error = None;
                        model.refresh_library(cx);
                        model.refresh_library_statuses(cx);
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
        self.invalidate_library();
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
                        model.auth.apply_status(empty_connection_status());
                        model.auth.error = None;
                        model.selection = SelectionState::default();
                        model.library = LibraryState::default();
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

    pub fn navigate(&mut self, view: NavigationView, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        self.navigation.active_view = view;
        if view == NavigationView::Search {
            self.navigation.search_focus_generation =
                self.navigation.search_focus_generation.wrapping_add(1);
        } else if matches!(
            view,
            NavigationView::RecentlyAdded
                | NavigationView::RecentlyPlayed
                | NavigationView::MostPlayed
        ) {
            self.load_discovery(view, cx);
        }
        cx.notify();
    }

    fn load_discovery(&mut self, view: NavigationView, cx: &mut Context<Self>) {
        if self.offline() {
            self.library.discovery_albums.clear();
            self.library.discovery_loading = false;
            return;
        }
        let Some(account) = self.account_key() else {
            return;
        };
        let list_type = match view {
            NavigationView::RecentlyAdded => "newest",
            NavigationView::RecentlyPlayed => "recent",
            NavigationView::MostPlayed => "frequent",
            _ => return,
        };
        self.library.discovery_generation = self.library.discovery_generation.wrapping_add(1);
        let generation = self.library.discovery_generation;
        self.library.discovery_loading = true;
        self.library.error = None;
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            library::get_album_list(&state, list_type.to_string(), Some(100), Some(0))
                .await
                .map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Discovery task failed: {error}")));
            weak.update(cx, |model, cx| {
                if model.quitting
                    || model.library.discovery_generation != generation
                    || model.account_key().as_ref() != Some(&account)
                    || model.navigation.active_view != view
                {
                    return;
                }
                model.library.discovery_loading = false;
                match result {
                    Ok(albums) => {
                        model.library.discovery_albums = albums;
                        model.library.error = None;
                    }
                    Err(error) => model.library.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn refresh_library(&mut self, cx: &mut Context<Self>) {
        let Some(account) = self.account_key() else {
            self.library = LibraryState::default();
            cx.notify();
            return;
        };
        self.library.generation = self.library.generation.wrapping_add(1);
        let generation = self.library.generation;
        self.library.loading = true;
        self.library.error = None;
        let offline = self.offline();
        let state = self.backend.state();
        let task_state = Arc::clone(&state);
        let task = self.backend.runtime_handle().spawn(async move {
            load_library_snapshot(task_state, offline).map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Library load task failed: {error}")));
            weak.update(cx, |model, cx| {
                if model.quitting
                    || model.library.generation != generation
                    || model.account_key().as_ref() != Some(&account)
                {
                    return;
                }
                model.library.loading = false;
                match result {
                    Ok(snapshot) => {
                        model.library.artists = snapshot.artists;
                        model.library.albums = snapshot.albums;
                        model.library.songs = snapshot.songs;
                        model.library.playlists = snapshot.playlists;
                        model.library.offline_song_ids = snapshot.offline_song_ids;
                        model.library.error = None;
                        model.reconcile_library_selection();
                    }
                    Err(error) => model.library.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn refresh_library_statuses(&mut self, cx: &mut Context<Self>) {
        if self.offline() || self.account_key().is_none() {
            self.library_sync_status = None;
            self.scan_status = None;
            cx.notify();
            return;
        }
        match library::get_library_sync_status(&self.backend.state()) {
            Ok(status) => self.library_sync_status = Some(status),
            Err(error) => self.action_error = Some(error.to_string()),
        }
        self.update_scan_status(false, cx);
        cx.notify();
    }

    pub fn sync_library(&mut self, full_reconcile: bool, cx: &mut Context<Self>) {
        if self.quitting || self.offline() {
            return;
        }
        self.action_error = None;
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            if full_reconcile {
                library::reconcile_library_state(&state).await
            } else {
                library::sync_library(&state).await
            }
            .map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Library sync task failed: {error}")));
            weak.update(cx, |model, cx| {
                if model.quitting {
                    return;
                }
                match result {
                    Ok(_) => {
                        model.refresh_library(cx);
                        model.refresh_library_statuses(cx);
                    }
                    Err(error) => {
                        model.action_error = Some(error);
                        model.refresh_library_statuses(cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn start_scan(&mut self, cx: &mut Context<Self>) {
        self.update_scan_status(true, cx);
    }

    fn update_scan_status(&mut self, start: bool, cx: &mut Context<Self>) {
        if self.quitting || self.offline() || self.account_key().is_none() {
            return;
        }
        self.scan_generation = self.scan_generation.wrapping_add(1);
        let generation = self.scan_generation;
        let state = self.backend.state();
        let runtime = self.backend.runtime_handle();
        let initial_state = Arc::clone(&state);
        let initial = runtime.spawn(async move {
            if start {
                library::start_scan(&initial_state).await
            } else {
                library::get_scan_status(&initial_state).await
            }
            .map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let mut result = initial
                .await
                .unwrap_or_else(|error| Err(format!("Scan status task failed: {error}")));
            loop {
                let keep_polling = weak
                    .update(cx, |model, cx| {
                        if model.quitting || model.scan_generation != generation {
                            return false;
                        }
                        match &result {
                            Ok(status) => {
                                model.scan_status = Some(status.clone());
                                model.action_error = None;
                                cx.notify();
                                status.scanning
                            }
                            Err(error) => {
                                model.action_error = Some(error.clone());
                                cx.notify();
                                false
                            }
                        }
                    })
                    .unwrap_or(false);
                if !keep_polling {
                    break;
                }
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let poll_state = Arc::clone(&state);
                result = runtime
                    .spawn(async move {
                        library::get_scan_status(&poll_state)
                            .await
                            .map_err(|error| error.to_string())
                    })
                    .await
                    .unwrap_or_else(|error| Err(format!("Scan status task failed: {error}")));
            }
        })
        .detach();
        cx.notify();
    }

    pub fn set_search_query(&mut self, query: String, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        self.library.search_query = query.clone();
        self.library.search_generation = self.library.search_generation.wrapping_add(1);
        let generation = self.library.search_generation;
        let query = query.trim().to_string();
        if query.is_empty() {
            self.library.search_results = None;
            self.library.search_pending = false;
            cx.notify();
            return;
        }
        let Some(account) = self.account_key() else {
            return;
        };
        self.library.search_pending = true;
        let offline = self.offline();
        let offline_song_ids = self.library.offline_song_ids.clone();
        let album_ids = self
            .library
            .albums
            .iter()
            .map(|album| album.id.clone())
            .collect::<HashSet<_>>();
        let artist_ids = self
            .library
            .artists
            .iter()
            .map(|artist| artist.id.clone())
            .collect::<HashSet<_>>();
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let mut results = search::search_library(&state, query, Some(50))
                .map_err(|error| error.to_string())?;
            if offline {
                results
                    .songs
                    .retain(|song| offline_song_ids.contains(&song.id));
                results.albums.retain(|album| album_ids.contains(&album.id));
                results
                    .artists
                    .retain(|artist| artist_ids.contains(&artist.id));
            }
            Ok::<_, String>(results)
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Search task failed: {error}")));
            weak.update(cx, |model, cx| {
                if model.quitting
                    || !model.library.accepts_search(generation)
                    || model.account_key().as_ref() != Some(&account)
                {
                    return;
                }
                model.library.search_pending = false;
                match result {
                    Ok(results) => {
                        model.library.search_results = Some(results);
                        model.library.error = None;
                    }
                    Err(error) => model.library.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn select_song(&mut self, row: usize, song_id: String, cx: &mut Context<Self>) {
        self.selection.row = Some(row);
        self.selection.song_ids = vec![song_id.clone()];
        self.selection.song_id = Some(song_id);
        cx.notify();
    }

    pub fn select_song_with_modifiers(
        &mut self,
        row: usize,
        song_id: String,
        visible_song_ids: &[String],
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        update_song_selection(
            &mut self.selection,
            row,
            song_id,
            visible_song_ids,
            modifiers,
        );
        cx.notify();
    }

    pub fn ensure_visible_song_selection(&mut self, cx: &mut Context<Self>) {
        if !self.selection.song_ids.is_empty() {
            return;
        }
        let song_ids = self.visible_song_ids();
        self.selection.row = (!song_ids.is_empty()).then_some(0);
        self.selection.song_id = song_ids.first().cloned();
        self.selection.song_ids = song_ids;
        cx.notify();
    }

    pub fn select_genre(&mut self, genre: Option<String>, cx: &mut Context<Self>) {
        self.library.selected_genre = genre;
        self.library.selected_artist_id = None;
        self.library.selected_album_id = None;
        self.selection = SelectionState::default();
        cx.notify();
    }

    pub fn select_artist(&mut self, artist_id: Option<String>, cx: &mut Context<Self>) {
        self.library.selected_artist_id = artist_id;
        self.library.selected_album_id = None;
        self.selection = SelectionState::default();
        cx.notify();
    }

    pub fn select_album(&mut self, album_id: Option<String>, cx: &mut Context<Self>) {
        self.library.selected_album_id = album_id;
        self.selection = SelectionState::default();
        cx.notify();
    }

    pub fn select_playlist(&mut self, playlist_id: String, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        self.navigation.active_view = NavigationView::Playlists;
        self.library.selected_playlist_id = Some(playlist_id.clone());
        self.library.playlist_songs.clear();
        self.library.playlist_generation = self.library.playlist_generation.wrapping_add(1);
        let generation = self.library.playlist_generation;
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            playlist::get_playlist_songs(&state, playlist_id).map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Playlist load task failed: {error}")));
            weak.update(cx, |model, cx| {
                if model.quitting || model.library.playlist_generation != generation {
                    return;
                }
                match result {
                    Ok(songs) => {
                        model.library.playlist_songs = songs;
                        model.library.error = None;
                    }
                    Err(error) => model.library.error = Some(error),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn clear_playlist(&mut self, cx: &mut Context<Self>) {
        self.library.playlist_generation = self.library.playlist_generation.wrapping_add(1);
        self.library.selected_playlist_id = None;
        self.library.playlist_songs.clear();
        cx.notify();
    }

    pub fn create_playlist(&mut self, name: String, cx: &mut Context<Self>) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.action_error = Some("Playlist name is required".to_string());
            cx.notify();
            return;
        }
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            playlist::create_playlist(state, name, None)
                .await
                .map(|playlist| Some(playlist.id))
                .map_err(|error| error.to_string())
        });
        self.finish_playlist_mutation(task, false, cx);
    }

    pub fn rename_playlist(&mut self, playlist_id: String, name: String, cx: &mut Context<Self>) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.action_error = Some("Playlist name is required".to_string());
            cx.notify();
            return;
        }
        let selected_id = playlist_id.clone();
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            playlist::update_playlist(state, playlist_id, name)
                .await
                .map(|()| Some(selected_id))
                .map_err(|error| error.to_string())
        });
        self.finish_playlist_mutation(task, false, cx);
    }

    pub fn delete_playlist(&mut self, playlist_id: String, cx: &mut Context<Self>) {
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            playlist::delete_playlist(state, playlist_id)
                .await
                .map(|()| None)
                .map_err(|error| error.to_string())
        });
        self.finish_playlist_mutation(task, true, cx);
    }

    pub fn add_songs_to_playlist(
        &mut self,
        playlist_id: String,
        song_ids: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        let selected_id = playlist_id.clone();
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            playlist::add_songs_to_playlist(state, playlist_id, song_ids)
                .await
                .map(|()| Some(selected_id))
                .map_err(|error| error.to_string())
        });
        self.finish_playlist_mutation(task, false, cx);
    }

    pub fn remove_playlist_songs(&mut self, positions: Vec<i32>, cx: &mut Context<Self>) {
        let Some(playlist_id) = self.library.selected_playlist_id.clone() else {
            return;
        };
        let selected_id = playlist_id.clone();
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            playlist::remove_songs_from_playlist(state, playlist_id, positions)
                .await
                .map(|()| Some(selected_id))
                .map_err(|error| error.to_string())
        });
        self.finish_playlist_mutation(task, false, cx);
    }

    pub fn set_playlist_saved_offline(
        &mut self,
        playlist_id: String,
        saved_offline: bool,
        cx: &mut Context<Self>,
    ) {
        let selected_id = playlist_id.clone();
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            playlist::set_playlist_saved_offline(state, playlist_id, saved_offline)
                .await
                .map(|_| Some(selected_id))
                .map_err(|error| error.to_string())
        });
        self.finish_playlist_mutation(task, false, cx);
    }

    pub fn sync_playlists(&mut self, cx: &mut Context<Self>) {
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            playlist::sync_playlists(state)
                .await
                .map(|_| None)
                .map_err(|error| error.to_string())
        });
        self.finish_playlist_mutation(task, false, cx);
    }

    fn finish_playlist_mutation(
        &mut self,
        task: JoinHandle<Result<Option<String>, String>>,
        clear_selection: bool,
        cx: &mut Context<Self>,
    ) {
        if self.quitting {
            task.abort();
            return;
        }
        self.action_error = None;
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Playlist task failed: {error}")));
            weak.update(cx, |model, cx| {
                if model.quitting {
                    return;
                }
                match result {
                    Ok(selected_id) => {
                        model.refresh_library(cx);
                        if clear_selection {
                            model.clear_playlist(cx);
                        } else if let Some(playlist_id) = selected_id {
                            model.select_playlist(playlist_id, cx);
                        }
                    }
                    Err(error) => {
                        model.action_error = Some(error);
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn request_cover_art(&mut self, cover_art_id: String, cx: &mut Context<Self>) {
        if self.quitting
            || cover_art_id.is_empty()
            || self.cover_art_paths.contains_key(&cover_art_id)
            || !self.cover_art_requests.insert(cover_art_id.clone())
        {
            return;
        }
        let requested_id = cover_art_id.clone();
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            cover_art::get_cover_art_path(&state, cover_art_id, Some(256))
                .await
                .map(PathBuf::from)
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task.await;
            weak.update(cx, |model, cx| {
                model.cover_art_requests.remove(&requested_id);
                if let Ok(Ok(path)) = result {
                    model.cover_art_paths.insert(requested_id, path);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn refresh_current_cover_art(&mut self, cover_art_id: Option<String>, cx: &mut Context<Self>) {
        self.current_cover_art_generation = self.current_cover_art_generation.wrapping_add(1);
        let generation = self.current_cover_art_generation;
        self.current_cover_art_path = None;
        let Some(cover_art_id) = cover_art_id else {
            return;
        };
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            cover_art::get_cover_art_path(&state, cover_art_id, Some(128))
                .await
                .map(PathBuf::from)
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let Ok(Ok(path)) = task.await else {
                return;
            };
            weak.update(cx, |model, cx| {
                if model.current_cover_art_generation == generation {
                    model.current_cover_art_path = Some(path);
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub fn show_cover_art(&mut self, cover_art_id: String, cx: &mut Context<Self>) {
        if self.quitting || cover_art_id.is_empty() {
            return;
        }
        self.cover_art_generation = self.cover_art_generation.wrapping_add(1);
        let generation = self.cover_art_generation;
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn(async move {
            cover_art::get_cover_art_path(&state, cover_art_id, None)
                .await
                .map(PathBuf::from)
                .map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Cover art task failed: {error}")));
            let should_open = weak
                .update(cx, |model, cx| {
                    if model.quitting || model.cover_art_generation != generation {
                        return false;
                    }
                    match result {
                        Ok(path) => {
                            model.cover_art_path = Some(path);
                            model.action_error = None;
                            cx.notify();
                            true
                        }
                        Err(error) => {
                            model.action_error = Some(error);
                            cx.notify();
                            false
                        }
                    }
                })
                .unwrap_or(false);
            if should_open && let Some(model) = weak.upgrade() {
                cx.update(|cx| {
                    if let Err(error) = super::windows::open_cover_art_window(model, cx) {
                        weak.update(cx, |model, cx| model.set_action_error(error, cx))
                            .ok();
                    }
                });
            }
        })
        .detach();
    }

    fn invalidate_library(&mut self) {
        self.library.generation = self.library.generation.wrapping_add(1);
        self.library.search_generation = self.library.search_generation.wrapping_add(1);
        self.library.discovery_generation = self.library.discovery_generation.wrapping_add(1);
        self.library.playlist_generation = self.library.playlist_generation.wrapping_add(1);
        self.scan_generation = self.scan_generation.wrapping_add(1);
        self.library_sync_status = None;
        self.scan_status = None;
        self.library.loading = false;
        self.library.search_pending = false;
        self.library.discovery_loading = false;
    }

    fn account_key(&self) -> Option<(String, String)> {
        Some((
            self.auth.status.server_url.clone()?,
            self.auth.status.username.clone()?,
        ))
    }

    fn reconcile_library_selection(&mut self) {
        if self
            .library
            .selected_artist_id
            .as_ref()
            .is_some_and(|id| !self.library.artists.iter().any(|artist| &artist.id == id))
        {
            self.library.selected_artist_id = None;
        }
        if self
            .library
            .selected_album_id
            .as_ref()
            .is_some_and(|id| !self.library.albums.iter().any(|album| &album.id == id))
        {
            self.library.selected_album_id = None;
        }
        if self
            .selection
            .song_id
            .as_ref()
            .is_some_and(|id| !self.library.songs.iter().any(|song| &song.id == id))
        {
            self.selection = SelectionState::default();
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
        self.navigate(NavigationView::Search, cx);
    }

    pub fn open_settings(&mut self, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            if let Some(model) = weak.upgrade() {
                cx.update(|cx| {
                    if let Err(error) = super::windows::open_settings_window(model, cx) {
                        weak.update(cx, |model, cx| model.set_action_error(error, cx))
                            .ok();
                    }
                });
            }
        })
        .detach();
    }

    pub fn navigate_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.quitting
            || (self.navigation.active_view == NavigationView::Search
                && self.library.search_pending)
        {
            return;
        }
        let count = self.visible_song_count();
        let Some(row) = next_selection_row(self.selection.row, delta, count) else {
            self.selection = SelectionState::default();
            cx.notify();
            return;
        };
        let song_id = if self.navigation.active_view == NavigationView::Playlists {
            self.library
                .playlist_songs
                .get(row)
                .map(|song| song.id.clone())
        } else {
            self.library
                .songs
                .iter()
                .filter(|song| self.song_visible_in_active_view(song))
                .nth(row)
                .map(|song| song.id.clone())
        };
        self.selection.row = Some(row);
        self.selection.song_ids = song_id.iter().cloned().collect();
        self.selection.song_id = song_id;
        cx.notify();
    }

    fn visible_song_count(&self) -> usize {
        if self.navigation.active_view == NavigationView::Playlists {
            return self
                .library
                .selected_playlist_id
                .as_ref()
                .map_or(0, |_| self.library.playlist_songs.len());
        }
        self.library
            .songs
            .iter()
            .filter(|song| self.song_visible_in_active_view(song))
            .count()
    }

    fn visible_song_ids(&self) -> Vec<String> {
        if self.navigation.active_view == NavigationView::Playlists {
            return self
                .library
                .playlist_songs
                .iter()
                .map(|song| song.id.clone())
                .collect();
        }
        self.library
            .songs
            .iter()
            .filter(|song| self.song_visible_in_active_view(song))
            .map(|song| song.id.clone())
            .collect()
    }

    fn queue_item_for_song(&self, song_id: &str) -> Option<QueueItem> {
        if self.navigation.active_view == NavigationView::Playlists {
            let song = self
                .library
                .playlist_songs
                .iter()
                .find(|song| song.id == song_id)?;
            return Some(QueueItem {
                song_id: song.id.clone(),
                title: song.title.clone(),
                artist: song
                    .artist
                    .clone()
                    .unwrap_or_else(|| "Unknown Artist".to_string()),
                album: song
                    .album
                    .clone()
                    .unwrap_or_else(|| "Unknown Album".to_string()),
                duration: i64::from(song.duration.unwrap_or(0)),
            });
        }
        let song = self.library.songs.iter().find(|song| song.id == song_id)?;
        Some(QueueItem {
            song_id: song.id.clone(),
            title: song.title.clone(),
            artist: song
                .artist
                .clone()
                .unwrap_or_else(|| "Unknown Artist".to_string()),
            album: song
                .album
                .clone()
                .unwrap_or_else(|| "Unknown Album".to_string()),
            duration: i64::from(song.duration.unwrap_or(0)),
        })
    }

    fn selected_queue_items(&self) -> Vec<QueueItem> {
        let song_ids = if self.selection.song_ids.is_empty() {
            self.selection.song_id.iter().collect::<Vec<_>>()
        } else {
            self.selection.song_ids.iter().collect::<Vec<_>>()
        };
        song_ids
            .into_iter()
            .filter_map(|song_id| self.queue_item_for_song(song_id))
            .collect()
    }

    fn song_visible_in_active_view(&self, song: &Song) -> bool {
        match self.navigation.active_view {
            NavigationView::Music => {
                self.library
                    .selected_genre
                    .as_deref()
                    .is_none_or(|genre| song.genre.as_deref() == Some(genre))
                    && self
                        .library
                        .selected_artist_id
                        .as_deref()
                        .is_none_or(|artist_id| song.artist_id == artist_id)
                    && self
                        .library
                        .selected_album_id
                        .as_deref()
                        .is_none_or(|album_id| song.album_id == album_id)
            }
            NavigationView::Artists | NavigationView::Albums => {
                self.library.selected_album_id.as_deref().map_or_else(
                    || {
                        self.library
                            .selected_artist_id
                            .as_deref()
                            .is_some_and(|artist_id| song.artist_id == artist_id)
                    },
                    |album_id| song.album_id == album_id,
                )
            }
            NavigationView::Search => self
                .library
                .search_results
                .as_ref()
                .is_some_and(|results| results.songs.iter().any(|result| result.id == song.id)),
            NavigationView::RecentlyAdded
            | NavigationView::RecentlyPlayed
            | NavigationView::MostPlayed
            | NavigationView::Playlists => false,
        }
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
        if self.quitting
            || (self.navigation.active_view == NavigationView::Search
                && self.library.search_pending)
        {
            return;
        }
        let Some(song_id) = self.selection.song_id.clone() else {
            return;
        };
        let song_ids = self.visible_song_ids();
        let state = self.backend.state();
        let runtime = self.backend.runtime_handle();
        let operation_runtime = runtime.clone();
        let task = runtime.spawn(async move {
            queue::play_song_with_queue(&operation_runtime, state, song_id, song_ids)
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

    pub fn mini_player_position(&self) -> Option<settings::MiniPlayerPosition> {
        settings::read_mini_player_position(&self.backend.state().ui_state)
    }

    pub fn persist_mini_player_position(
        &mut self,
        position: settings::MiniPlayerPosition,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) =
            settings::write_mini_player_position(&self.backend.state().ui_state, position)
        {
            self.set_action_error(error, cx);
        }
    }

    pub fn seek_by(&mut self, delta: f64, cx: &mut Context<Self>) {
        self.seek_to(self.playback.position + delta, cx);
    }

    pub fn seek_to(&mut self, position: f64, cx: &mut Context<Self>) {
        if self.quitting {
            return;
        }
        let position = position.clamp(0.0, self.playback.duration.max(0.0));
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

    pub fn set_volume(&mut self, volume: f32, cx: &mut Context<Self>) {
        let volume = volume.clamp(0.0, 1.0);
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

    pub fn add_selection_to_queue(&mut self, play_next: bool, cx: &mut Context<Self>) {
        let items = self.selected_queue_items();
        if items.is_empty() {
            return;
        }
        let result = if play_next {
            queue::insert_next_songs_in_queue(&self.backend.state(), items)
        } else {
            queue::add_songs_to_queue(&self.backend.state(), items)
        };
        if let Err(error) = result {
            self.set_action_error(error, cx);
        }
    }

    pub fn play_queue_item(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.quitting || index >= self.queue.items.len() {
            return;
        }
        let state = self.backend.state();
        let runtime = self.backend.runtime_handle();
        let operation_runtime = runtime.clone();
        let task = runtime.spawn(async move {
            queue::play_queue_item(&operation_runtime, state, index)
                .await
                .map_err(|error| error.to_string())
        });
        self.observe_action(task, cx);
    }

    pub fn remove_queue_item(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Err(error) = queue::remove_from_queue(&self.backend.state(), index) {
            self.set_action_error(error, cx);
        }
    }

    pub fn move_queue_item(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        if from >= self.queue.items.len() || to >= self.queue.items.len() {
            return;
        }
        if let Err(error) = queue::move_queue_item(&self.backend.state(), from, to) {
            self.set_action_error(error, cx);
        }
    }

    pub fn clear_queue(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = queue::clear_queue(&self.backend.state()) {
            self.set_action_error(error, cx);
        }
    }

    pub fn save_playback_settings(&mut self, settings: PlaybackSettings, cx: &mut Context<Self>) {
        match settings::set_playback_settings(&self.backend.state(), settings) {
            Ok(settings) => self.playback_settings = settings,
            Err(error) => self.action_error = Some(error.to_string()),
        }
        cx.notify();
    }

    pub fn save_sync_settings(&mut self, settings: SyncSettings, cx: &mut Context<Self>) {
        match settings::set_sync_settings(&self.backend.state(), settings) {
            Ok(settings) => self.sync_settings = settings,
            Err(error) => self.action_error = Some(error.to_string()),
        }
        cx.notify();
    }

    pub fn save_notification_settings(
        &mut self,
        settings: NotificationSettings,
        cx: &mut Context<Self>,
    ) {
        match settings::set_notification_settings(&self.backend.state().settings, settings.clone())
        {
            Ok(()) => self.notification_settings = settings,
            Err(error) => self.action_error = Some(error.to_string()),
        }
        cx.notify();
    }

    pub fn save_normalization_settings(
        &mut self,
        settings: NormalizationSettings,
        cx: &mut Context<Self>,
    ) {
        match settings::set_normalization_settings(&self.backend.state().settings, settings.clone())
        {
            Ok(()) => self.normalization_settings = settings,
            Err(error) => self.action_error = Some(error.to_string()),
        }
        cx.notify();
    }

    pub fn set_cache_root(&mut self, root: Option<String>, cx: &mut Context<Self>) {
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn_blocking(move || {
            stereodrome_desktop::operations::cache::set_cache_root(&state, root)
                .map(|_| ())
                .map_err(|error| error.to_string())
        });
        self.observe_action(task, cx);
    }

    pub fn cache_summary(&self) -> Result<(String, u64, u64, u64), String> {
        let state = self.backend.state();
        let locations = stereodrome_desktop::operations::cache::get_cache_locations(&state)
            .map_err(|error| error.to_string())?;
        let stats = stereodrome_desktop::operations::cache::get_audio_cache_stats(state)
            .map_err(|error| error.to_string())?;
        Ok((
            locations.cache_root,
            stats.file_count,
            stats.total_size,
            stats.max_size,
        ))
    }

    pub fn set_max_cache_size(&mut self, size: u64, cx: &mut Context<Self>) {
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn_blocking(move || {
            stereodrome_desktop::operations::cache::set_max_cache_size(state, size)
                .map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Cache size task failed: {error}")));
            weak.update(cx, |model, cx| match result {
                Ok(_) => {
                    model.cache_revision = model.cache_revision.wrapping_add(1);
                    model.action_error = None;
                    cx.notify();
                }
                Err(error) => model.set_action_error(error, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn refresh_cache_summary(&mut self, cx: &mut Context<Self>) {
        self.cache_revision = self.cache_revision.wrapping_add(1);
        cx.notify();
    }

    pub fn analyze_library_loudness(&mut self, cx: &mut Context<Self>) {
        let state = self.backend.state();
        let runtime = self.backend.runtime_handle();
        if let Err(error) = normalization::analyze_all_songs(&runtime, state) {
            self.set_action_error(error, cx);
        }
    }

    pub fn clear_normalization_data(&mut self, cx: &mut Context<Self>) {
        if let Err(error) = normalization::clear_normalization_data(&self.backend.state()) {
            self.set_action_error(error, cx);
        } else {
            self.normalization_progress = None;
            cx.notify();
        }
    }

    pub fn clear_audio_cache(&mut self, cx: &mut Context<Self>) {
        let state = self.backend.state();
        let task = self.backend.runtime_handle().spawn_blocking(move || {
            stereodrome_desktop::operations::cache::clear_audio_cache(state)
                .map_err(|error| error.to_string())
        });
        self.observe_action(task, cx);
    }

    pub fn begin_lastfm_auth(&mut self, cx: &mut Context<Self>) {
        let state = self.backend.state();
        let runtime = self.backend.runtime_handle();
        let task = runtime.spawn(async move {
            lastfm::begin_auth(&state.settings)
                .await
                .map(|auth| auth.auth_url)
                .map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Last.fm auth task failed: {error}")));
            weak.update(cx, |model, cx| match result {
                Ok(url) => {
                    model.refresh_lastfm_status();
                    cx.open_url(&url);
                    cx.notify();
                }
                Err(error) => model.set_action_error(error, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn complete_lastfm_auth(&mut self, cx: &mut Context<Self>) {
        let state = self.backend.state();
        let runtime = self.backend.runtime_handle();
        let task = runtime.spawn(async move {
            lastfm::complete_auth(&state.settings, &state)
                .await
                .map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Last.fm completion task failed: {error}")));
            weak.update(cx, |model, cx| match result {
                Ok(status) => {
                    model.lastfm_status = status;
                    cx.notify();
                }
                Err(error) => model.set_action_error(error, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn retry_lastfm_queue(&mut self, cx: &mut Context<Self>) {
        let state = self.backend.state();
        let runtime = self.backend.runtime_handle();
        let task = runtime.spawn(async move {
            lastfm::retry_queue(&state.settings, &state)
                .await
                .map(|_| lastfm::lastfm_status(&state.settings, &state))
                .map_err(|error| error.to_string())
        });
        let weak = cx.weak_entity();
        cx.spawn(async move |_, cx| {
            let result = task
                .await
                .unwrap_or_else(|error| Err(format!("Last.fm retry task failed: {error}")));
            weak.update(cx, |model, cx| match result {
                Ok(status) => {
                    model.lastfm_status = status;
                    cx.notify();
                }
                Err(error) => model.set_action_error(error, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn disconnect_lastfm(&mut self, cx: &mut Context<Self>) {
        let state = self.backend.state();
        match lastfm::disconnect(&state.settings, &state) {
            Ok(status) => self.lastfm_status = status,
            Err(error) => self.action_error = Some(error.to_string()),
        }
        cx.notify();
    }

    fn refresh_lastfm_status(&mut self) {
        let state = self.backend.state();
        self.lastfm_status = lastfm::lastfm_status(&state.settings, &state);
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

struct LibrarySnapshot {
    artists: Vec<Artist>,
    albums: Vec<Album>,
    songs: Vec<Song>,
    playlists: Vec<Playlist>,
    offline_song_ids: HashSet<String>,
}

fn load_library_snapshot(
    state: Arc<stereodrome_desktop::state::DesktopState>,
    offline: bool,
) -> Result<LibrarySnapshot, stereodrome_desktop::AppError> {
    let mut artists = library::get_artists(&state)?;
    let mut albums = library::get_albums(&state, None)?;
    let mut songs = library::get_songs(&state, None, None)?;
    let mut playlists = playlist::get_playlists(&state)?;
    let offline_song_ids = if offline {
        stereodrome_desktop::operations::cache::get_offline_song_ids(Arc::clone(&state))?
            .into_iter()
            .collect::<HashSet<_>>()
    } else {
        HashSet::new()
    };

    if offline {
        songs.retain(|song| offline_song_ids.contains(&song.id));
        let album_ids = songs
            .iter()
            .map(|song| song.album_id.as_str())
            .collect::<HashSet<_>>();
        let artist_ids = songs
            .iter()
            .map(|song| song.artist_id.as_str())
            .collect::<HashSet<_>>();
        albums.retain(|album| album_ids.contains(album.id.as_str()));
        artists.retain(|artist| artist_ids.contains(artist.id.as_str()));
        playlists.retain(|playlist| playlist.saved_offline);
    }

    Ok(LibrarySnapshot {
        artists,
        albums,
        songs,
        playlists,
        offline_song_ids,
    })
}

fn update_song_selection(
    selection: &mut SelectionState,
    row: usize,
    song_id: String,
    visible_song_ids: &[String],
    modifiers: Modifiers,
) {
    if modifiers.shift
        && let Some(anchor) = selection.row
    {
        let (start, end) = if anchor <= row {
            (anchor, row)
        } else {
            (row, anchor)
        };
        selection.song_ids = visible_song_ids
            .get(start..=end)
            .unwrap_or_default()
            .to_vec();
    } else if modifiers.platform || modifiers.control {
        if let Some(index) = selection
            .song_ids
            .iter()
            .position(|selected| selected == &song_id)
        {
            selection.song_ids.remove(index);
        } else {
            selection.song_ids.push(song_id.clone());
        }
    } else {
        selection.song_ids = vec![song_id.clone()];
    }
    selection.row = Some(row);
    selection.song_id = selection
        .song_ids
        .contains(&song_id)
        .then_some(song_id)
        .or_else(|| selection.song_ids.last().cloned());
}

fn next_selection_row(current: Option<usize>, delta: isize, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let row = match current {
        None if delta < 0 => count - 1,
        None => 0,
        Some(row) if delta < 0 => row.saturating_sub(delta.unsigned_abs()),
        Some(row) => row.saturating_add(delta as usize).min(count - 1),
    };
    Some(row)
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
    use super::{
        AuthState, LibraryState, SelectionState, VisibleSurface, next_selection_row,
        update_song_selection,
    };
    use gpui::Modifiers;
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
    fn stale_search_generations_are_rejected() {
        let mut library = LibraryState::default();
        library.search_generation = library.search_generation.wrapping_add(1);
        let stale = library.search_generation;
        library.search_generation = library.search_generation.wrapping_add(1);

        assert!(!library.accepts_search(stale));
        assert!(library.accepts_search(library.search_generation));
    }

    #[test]
    fn keyboard_selection_stays_within_visible_rows() {
        assert_eq!(next_selection_row(None, 1, 3), Some(0));
        assert_eq!(next_selection_row(None, -1, 3), Some(2));
        assert_eq!(next_selection_row(Some(0), -1, 3), Some(0));
        assert_eq!(next_selection_row(Some(2), 1, 3), Some(2));
        assert_eq!(next_selection_row(Some(1), 1, 0), None);
    }

    #[test]
    fn song_selection_supports_ranges_and_platform_toggle() {
        let songs = ["a", "b", "c", "d"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let mut selection = SelectionState {
            song_id: Some("b".to_string()),
            song_ids: vec!["b".to_string()],
            row: Some(1),
        };
        update_song_selection(
            &mut selection,
            3,
            "d".to_string(),
            &songs,
            Modifiers {
                shift: true,
                ..Default::default()
            },
        );
        assert_eq!(selection.song_ids, ["b", "c", "d"]);

        update_song_selection(
            &mut selection,
            2,
            "c".to_string(),
            &songs,
            Modifiers {
                platform: true,
                ..Default::default()
            },
        );
        assert_eq!(selection.song_ids, ["b", "d"]);
    }

    #[test]
    fn status_refresh_preserves_version_for_the_same_account() {
        let mut auth = AuthState::empty();
        auth.apply_status(super::ConnectionStatus {
            connected: true,
            server_url: Some("https://music.example".to_string()),
            username: Some("listener".to_string()),
            server_version: Some("1.2.3".to_string()),
        });
        auth.apply_status(super::ConnectionStatus {
            connected: false,
            server_url: Some("https://music.example".to_string()),
            username: Some("listener".to_string()),
            server_version: None,
        });
        assert_eq!(auth.status.server_version.as_deref(), Some("1.2.3"));

        auth.apply_status(super::empty_connection_status());
        assert_eq!(auth.status.server_version, None);
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
