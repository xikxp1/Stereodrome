//! Phase-one adapter that executes existing algorithms without rewriting them.

use serde::Serialize;
use serde_json::Value;

use crate::protocol::{CoreCommand, SyncKind};
use crate::{CoreError, CoreResult, StereodromeCore};

#[allow(clippy::too_many_lines)]
pub(crate) async fn execute(core: &StereodromeCore, command: CoreCommand) -> CoreResult<Value> {
    match command {
        CoreCommand::Initialize
        | CoreCommand::GetSnapshot
        | CoreCommand::ReportNetwork { .. }
        | CoreCommand::ReportLifecycle { .. }
        | CoreCommand::RunBackgroundTick
        | CoreCommand::CancelOperation { .. }
        | CoreCommand::GetSavedPlaylistsOfflineStatus
        | CoreCommand::StartQueuePrefetch { .. }
        | CoreCommand::CancelQueuePrefetch { .. }
        | CoreCommand::Shutdown => Err(CoreError::InvalidInput(
            "runtime control command reached the effect adapter".to_string(),
        )),
        CoreCommand::Connect { params } => value(core.connect_server(params).await),
        CoreCommand::UpdateServerSettings { update } => {
            value(core.update_server_settings(update).await)
        }
        CoreCommand::RestoreSession => value(core.restore_session().await),
        CoreCommand::Disconnect => value(core.disconnect_server().await),
        CoreCommand::GetConnectionStatus => value(core.get_connection_status()),
        CoreCommand::GetSyncSettings => value(core.get_sync_settings()),
        CoreCommand::SetSyncSettings { settings } => value(core.set_sync_settings(settings)),
        CoreCommand::GetConnectivitySettings => value(core.get_connectivity_settings()),
        CoreCommand::SetConnectivity { settings } => {
            let settings = core.set_connectivity_settings(settings)?;
            if settings.manual_offline_enabled {
                core.deactivate_session().await;
            }
            value(Ok(settings))
        }
        CoreCommand::StartSync { kind } => match kind {
            SyncKind::Full => value(core.sync_library().await),
            SyncKind::Incremental => value(core.sync_library_incremental().await),
            SyncKind::FullReconcile => value(core.reconcile_library().await),
        },
        CoreCommand::RunDueLibrarySync => value(core.run_due_library_sync().await),
        CoreCommand::GetScanStatus => value(core.get_scan_status().await),
        CoreCommand::StartScan => value(core.start_scan().await),
        CoreCommand::GetLibrarySyncStatus => value(core.get_library_sync_status()),
        CoreCommand::GetArtists => value(core.get_artists()),
        CoreCommand::GetAlbums { artist_id } => value(core.get_albums(artist_id)),
        CoreCommand::GetSongs {
            album_id,
            artist_id,
        } => value(core.get_songs(album_id, artist_id)),
        CoreCommand::GetAlbumList {
            list_type,
            size,
            offset,
        } => value(core.get_album_list(list_type, size, offset).await),
        CoreCommand::SearchLibrary { query, limit } => value(core.search_library(query, limit)),
        CoreCommand::GetPlaylists => value(core.get_playlists().await),
        CoreCommand::GetPlaylistSongs { playlist_id } => {
            value(core.get_playlist_songs(playlist_id).await)
        }
        CoreCommand::CreatePlaylist { name, song_ids } => {
            value(core.create_playlist(name, song_ids).await)
        }
        CoreCommand::RenamePlaylist { playlist_id, name } => {
            value(core.rename_playlist(playlist_id, name).await)
        }
        CoreCommand::DeletePlaylist { playlist_id } => {
            value(core.delete_playlist(playlist_id).await)
        }
        CoreCommand::AddSongsToPlaylist {
            playlist_id,
            song_ids,
        } => value(core.add_songs_to_playlist(playlist_id, song_ids).await),
        CoreCommand::RemoveSongsFromPlaylist {
            playlist_id,
            song_indexes,
        } => value(
            core.remove_songs_from_playlist(playlist_id, song_indexes)
                .await,
        ),
        CoreCommand::GetCoverArtUri { id, size } => value(core.get_cover_art_uri(id, size).await),
        CoreCommand::GetSongCoverArtUri { id, size } => {
            value(core.get_song_cover_art_uri(id, size).await)
        }
        CoreCommand::GetStreamUri { song_id } => value(core.get_stream_uri(song_id)),
        CoreCommand::GetAudioCacheStats => value(core.get_audio_cache_stats()),
        CoreCommand::GetOfflineSongIds => value(core.get_offline_song_ids()),
        CoreCommand::SetMaxCacheSize { max_size } => value(core.set_max_cache_size(max_size)),
        CoreCommand::ClearAudioCache => value(core.clear_audio_cache()),
        CoreCommand::IsSongCached { song_id } => value(core.is_song_cached(song_id)),
        CoreCommand::DownloadSong { song_id } => value(core.download_song(song_id).await),
        CoreCommand::RemoveCachedSong { song_id } => value(core.remove_cached_song(song_id)),
        CoreCommand::DownloadAlbum { album_id } => value(core.download_album(album_id).await),
        CoreCommand::DownloadPlaylist { playlist_id } => {
            value(core.download_playlist(playlist_id).await)
        }
        CoreCommand::SetPlaylistSavedOffline {
            playlist_id,
            saved_offline,
        } => value(
            core.set_playlist_saved_offline(playlist_id, saved_offline)
                .await,
        ),
        CoreCommand::ReconcileSavedPlaylistsOffline
        | CoreCommand::StartSavedPlaylistsOfflineReconcile => {
            value(core.reconcile_saved_playlists_offline().await)
        }
        CoreCommand::GetPlaybackState => value(core.get_playback_state()),
        CoreCommand::SavePlaybackPosition { progress } => {
            value(core.save_playback_position(progress))
        }
        CoreCommand::GetLastfmStatus => value(Ok::<_, CoreError>(core.get_lastfm_status())),
        CoreCommand::BeginLastfmAuth => value(core.begin_lastfm_auth().await),
        CoreCommand::CompleteLastfmAuth => value(core.complete_lastfm_auth().await),
        CoreCommand::DisconnectLastfm => value(core.disconnect_lastfm()),
        CoreCommand::GetLastfmQueue => value(core.get_lastfm_queue()),
        CoreCommand::RetryLastfmQueue => value(core.retry_lastfm_queue().await),
        CoreCommand::GetAudioProcessingSettings => value(core.get_audio_processing_settings()),
        CoreCommand::SetAudioProcessing { settings } => {
            value(core.set_audio_processing_settings(settings))
        }
        CoreCommand::ExportPortableBackup { path } => value(core.export_portable_backup(path)),
        CoreCommand::ImportPortableBackup { path } => value(core.import_portable_backup(path)),
        CoreCommand::GetQueue => value(core.get_queue()),
        CoreCommand::PlaySelection { song_id, song_ids } => {
            value(core.play_song_with_queue(song_id, song_ids))
        }
        CoreCommand::AddToQueue { item } => value(core.add_to_queue(item)),
        CoreCommand::AddSongsToQueue { items } => value(core.add_songs_to_queue(items)),
        CoreCommand::InsertNext { item } => value(core.insert_next(item)),
        CoreCommand::InsertNextSongs { items } => value(core.insert_next_songs(items)),
        CoreCommand::RemoveFromQueue { index } => value(core.remove_from_queue(index)),
        CoreCommand::ClearQueue => value(core.clear_queue()),
        CoreCommand::MoveQueueItem { from, to } => value(core.move_queue_item(from, to)),
        CoreCommand::PlayQueueItem { index } => value(core.play_queue_item(index)),
        CoreCommand::PlayNext { force } => value(core.play_next(force)),
        CoreCommand::PlayPrevious => value(core.play_previous()),
        CoreCommand::ToggleShuffle => value(core.toggle_shuffle()),
        CoreCommand::SetRepeatMode { mode } => value(core.set_repeat_mode(mode)),
        CoreCommand::CycleRepeatMode => value(core.cycle_repeat_mode()),
        CoreCommand::RerollNext => value(core.reroll_next()),
    }
}

fn value<T: Serialize>(result: CoreResult<T>) -> CoreResult<Value> {
    result.and_then(|value| serde_json::to_value(value).map_err(CoreError::from))
}
