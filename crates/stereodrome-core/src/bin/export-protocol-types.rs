//! Emits the TypeScript view of the runtime protocol shared by desktop and mobile.
//!
//! Invoked through `scripts/generate-protocol-types.sh`, which formats the output.

use std::path::Path;
use std::process::ExitCode;

use stereodrome_core::CORE_PROTOCOL_VERSION;
use ts_rs::{Config, TS};

macro_rules! declarations {
    ($config:expr, $($ty:ty),* $(,)?) => {
        vec![$(format!("export {}", <$ty as TS>::decl($config))),*]
    };
}

/// Maps each value-returning command tag to the TypeScript type of its payload.
///
/// Hand-maintained: `runtime::effect::execute` and `process_playback_request`
/// erase results to `serde_json::Value`, so this association exists nowhere in
/// the type system. Commands omitted here resolve to `void` on the client.
/// Keep in sync with `runtime/effect.rs` and `runtime/mod.rs`.
const COMMAND_RESULTS: &[(&str, &str)] = &[
    ("connect", "ConnectionStatus"),
    ("update-server-settings", "ConnectionStatus"),
    ("restore-session", "ConnectionStatus"),
    ("get-connection-status", "ConnectionStatus"),
    ("get-sync-settings", "SyncSettings"),
    ("set-sync-settings", "SyncSettings"),
    ("get-connectivity-settings", "ConnectivitySettings"),
    ("set-connectivity", "ConnectivitySettings"),
    ("run-due-library-sync", "string | null"),
    ("get-scan-status", "ScanStatus"),
    ("start-scan", "ScanStatus"),
    ("get-now-playing", "Array<NowPlayingEntry>"),
    ("get-library-sync-status", "LibrarySyncStatus"),
    ("get-artists", "Array<Artist>"),
    ("get-albums", "Array<Album>"),
    ("get-songs", "Array<Song>"),
    ("get-album-list", "Array<AlbumListEntry>"),
    ("search-library", "SearchResults"),
    ("get-playlists", "Array<Playlist>"),
    ("get-playlist-songs", "Array<Song>"),
    ("create-playlist", "Playlist"),
    ("get-cover-art-uri", "string"),
    ("get-song-cover-art-uri", "string | null"),
    ("get-stream-uri", "string"),
    ("get-audio-cache-stats", "CacheStats"),
    ("get-offline-song-ids", "Array<string>"),
    ("set-max-cache-size", "CacheStats"),
    ("clear-audio-cache", "CacheStats"),
    ("is-song-cached", "DownloadStatus"),
    ("download-song", "DownloadStatus"),
    ("remove-cached-song", "DownloadStatus"),
    ("download-album", "Array<DownloadStatus>"),
    ("download-playlist", "Array<DownloadStatus>"),
    ("set-playlist-saved-offline", "SavedPlaylistOfflineResult"),
    (
        "reconcile-saved-playlists-offline",
        "Array<SavedPlaylistOfflineResult>",
    ),
    (
        "start-saved-playlists-offline-reconcile",
        "Array<SavedPlaylistOfflineResult>",
    ),
    (
        "get-saved-playlists-offline-status",
        "SavedPlaylistOfflineStatus",
    ),
    ("start-queue-prefetch", "Array<DownloadStatus>"),
    ("get-playback-state", "PlaybackState"),
    ("save-playback-position", "PlaybackState"),
    ("get-lastfm-status", "LastfmStatus"),
    ("begin-lastfm-auth", "LastfmAuthStart"),
    ("complete-lastfm-auth", "LastfmStatus"),
    ("disconnect-lastfm", "LastfmStatus"),
    ("get-lastfm-queue", "Array<LastfmQueueItem>"),
    ("retry-lastfm-queue", "number"),
    ("get-audio-processing-settings", "AudioProcessingSettings"),
    ("set-audio-processing", "AudioProcessingSettings"),
    ("export-portable-backup", "BackupSummary"),
    ("import-portable-backup", "BackupSummary"),
    // Queue mutations and navigation all project the resulting queue.
    ("get-queue", "QueueState"),
    ("clear-playback", "QueueState"),
    ("navigate-playback", "QueueState"),
    ("add-to-queue", "QueueState"),
    ("add-songs-to-queue", "QueueState"),
    ("insert-next", "QueueState"),
    ("insert-next-songs", "QueueState"),
    ("remove-from-queue", "QueueState"),
    ("clear-queue", "QueueState"),
    ("move-queue-item", "QueueState"),
    ("toggle-shuffle", "QueueState"),
    ("set-repeat-mode", "QueueState"),
    ("cycle-repeat-mode", "QueueState"),
    ("reroll-next", "QueueState"),
    ("play-queue-item", "QueueItem | null"),
    ("play-next", "QueueItem | null"),
    ("play-previous", "QueueItem | null"),
];

fn command_value_map(command_decl: &str) -> String {
    let entries = COMMAND_RESULTS
        .iter()
        .map(|(tag, ts_type)| {
            assert!(
                command_decl.contains(&format!("\"type\": \"{tag}\"")),
                "COMMAND_RESULTS lists `{tag}`, which is not a CoreCommand variant"
            );
            format!("  \"{tag}\": {ts_type};")
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("export type CoreCommandValue = {{\n{entries}\n}};")
}

fn render() -> String {
    // The wire format is JSON, where 64-bit integers arrive as JS numbers rather
    // than the `bigint` ts-rs emits by default.
    let config = Config::new().with_large_int("number");
    let declarations = declarations![
        &config,
        // Library and settings models.
        stereodrome_core::ConnectParams,
        stereodrome_core::ServerSettingsUpdate,
        stereodrome_core::ConnectionStatus,
        stereodrome_core::Artist,
        stereodrome_core::Album,
        stereodrome_core::AlbumListEntry,
        stereodrome_core::Song,
        stereodrome_core::Playlist,
        stereodrome_core::SearchResultSong,
        stereodrome_core::SearchResultAlbum,
        stereodrome_core::SearchResultArtist,
        stereodrome_core::SearchResults,
        stereodrome_core::SyncResult,
        stereodrome_core::SyncSettings,
        stereodrome_core::SyncJobStatus,
        stereodrome_core::LibrarySyncStatus,
        stereodrome_core::ConnectivitySettings,
        stereodrome_core::ScanStatus,
        stereodrome_core::NowPlayingEntry,
        stereodrome_core::CacheStats,
        stereodrome_core::DownloadStatus,
        stereodrome_core::SavedPlaylistOfflineResult,
        stereodrome_core::PlaybackState,
        stereodrome_core::PlaybackProgress,
        stereodrome_core::NormalizationMode,
        stereodrome_core::DynamicsPreset,
        stereodrome_core::BinauralPreset,
        stereodrome_core::AudioProcessingSettings,
        stereodrome_core::backup::BackupSummary,
        stereodrome_core::LastfmStatus,
        stereodrome_core::LastfmAuthStart,
        stereodrome_core::LastfmQueueItem,
        // Queue models.
        stereodrome_core::queue::RepeatMode,
        stereodrome_core::queue::QueueItem,
        stereodrome_core::queue::QueueState,
        // Runtime protocol.
        stereodrome_core::CommandId,
        stereodrome_core::OperationId,
        stereodrome_core::SyncKind,
        stereodrome_core::PlatformLifecycle,
        stereodrome_core::JobKind,
        stereodrome_core::PlaybackNavigation,
        stereodrome_core::PlatformPlaybackEvent,
        stereodrome_core::OperationPhase,
        stereodrome_core::OperationSnapshot,
        stereodrome_core::SavedPlaylistOfflineStatus,
        stereodrome_core::RuntimeLifecycle,
        stereodrome_core::ConnectivityState,
        stereodrome_core::DownloadSnapshot,
        stereodrome_core::PlaybackProjectionSong,
        stereodrome_core::PlaybackPhase,
        stereodrome_core::PlaybackOutputState,
        stereodrome_core::PlaybackProjection,
        stereodrome_core::CoreSnapshot,
        stereodrome_core::ProtocolErrorCode,
        stereodrome_core::ProtocolError,
        stereodrome_core::OperationFailure,
        stereodrome_core::CommandStatus,
        stereodrome_core::CoreCommand,
        stereodrome_core::CoreCommandRequest,
        stereodrome_core::CoreCommandResult,
        stereodrome_core::CoreEventKind,
        stereodrome_core::CoreEvent,
        // Referenced by `CoreCommandResult::value`, whose payload type depends on
        // the command that produced it.
        serde_json::Value,
    ];

    let command_values = command_value_map(&<stereodrome_core::CoreCommand as TS>::decl(&config));

    format!(
        "/* eslint-disable */\n\
         // @generated by `scripts/generate-protocol-types.sh`.\n\
         // Source of truth: crates/stereodrome-core/src/{{models,queue,lastfm,backup,protocol}}.rs\n\
         // Do not hand-edit.\n\n\
         export const CORE_PROTOCOL_VERSION = {CORE_PROTOCOL_VERSION} as const;\n\n\
         {}\n\n\
         {command_values}\n",
        declarations.join("\n\n")
    )
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("usage: export-protocol-types <output.ts>...");
        return ExitCode::FAILURE;
    }

    let rendered = render();
    let mut failed = false;

    for arg in &args {
        let path = Path::new(arg);
        if let Some(parent) = path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            eprintln!("failed to create {}: {error}", parent.display());
            failed = true;
            continue;
        }
        if let Err(error) = std::fs::write(path, &rendered) {
            eprintln!("failed to write {}: {error}", path.display());
            failed = true;
        }
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
