import { Directory, Paths } from "expo-file-system";
import NativeStereodromeCore from "../../modules/stereodrome-core/src";
import type {
  ConnectionStatus,
  Artist,
  Album,
  AlbumListEntry,
  Playlist,
  SearchResults,
  Song,
  SyncResult,
} from "@/types/music";

type Envelope<T> = { ok: true; value: T } | { ok: false; error: string };

const unavailable =
  "Stereodrome native core is not available in this development build";

let initializePromise: Promise<boolean> | null = null;

function fileUriToPath(uri: string): string {
  if (!uri.startsWith("file://")) {
    return uri;
  }

  return decodeURIComponent(uri.replace(/^file:\/\//, ""));
}

function parseEnvelope<T>(raw: string): T {
  const envelope = JSON.parse(raw) as Envelope<T>;
  if (!envelope.ok) {
    throw new Error(envelope.error);
  }
  return envelope.value;
}

async function invokeJson<T>(
  name: string,
  payload: unknown = null
): Promise<T> {
  await ensureInitialized();

  if (!NativeStereodromeCore?.call) {
    throw new Error(unavailable);
  }
  return parseEnvelope<T>(
    await NativeStereodromeCore.call(name, JSON.stringify(payload))
  );
}

async function ensureInitialized(): Promise<boolean> {
  if (initializePromise) {
    return initializePromise;
  }

  initializePromise = (async () => {
    if (!NativeStereodromeCore?.initialize) {
      throw new Error(unavailable);
    }

    const dataDir = new Directory(Paths.document, "stereodrome");
    dataDir.create({ idempotent: true, intermediates: true });

    const initialized = await NativeStereodromeCore.initialize(
      fileUriToPath(dataDir.uri)
    );
    if (!initialized) {
      throw new Error("Stereodrome Rust core failed to initialize");
    }

    return true;
  })();

  try {
    return await initializePromise;
  } catch (error) {
    initializePromise = null;
    throw error;
  }
}

export const stereodromeCore = {
  initialize: ensureInitialized,
  getConnectionStatus(): Promise<ConnectionStatus> {
    return invokeJson("getConnectionStatus");
  },
  connectServer(params: {
    url: string;
    username: string;
    password: string;
  }): Promise<ConnectionStatus> {
    return invokeJson("connectServer", params);
  },
  restoreSession(): Promise<ConnectionStatus> {
    return invokeJson("restoreSession");
  },
  disconnectServer(): Promise<void> {
    return invokeJson("disconnectServer");
  },
  syncLibrary(): Promise<SyncResult> {
    return invokeJson("syncLibrary");
  },
  getArtists(): Promise<Artist[]> {
    return invokeJson("getArtists");
  },
  getAlbums(artistId?: string): Promise<Album[]> {
    return invokeJson("getAlbums", artistId ?? null);
  },
  getAlbumList(
    listType: string,
    size = 50,
    offset = 0
  ): Promise<AlbumListEntry[]> {
    return invokeJson("getAlbumList", { list_type: listType, size, offset });
  },
  getSongs(albumId?: string, artistId?: string): Promise<Song[]> {
    return invokeJson("getSongs", {
      first: albumId ?? null,
      second: artistId ?? null,
    });
  },
  getPlaylists(): Promise<Playlist[]> {
    return invokeJson("getPlaylists");
  },
  getPlaylistSongs(id: string): Promise<Song[]> {
    return invokeJson("getPlaylistSongs", id);
  },
  searchLibrary(query: string, limit = 25): Promise<SearchResults> {
    return invokeJson("searchLibrary", { query, limit });
  },
  getCoverArtUri(coverArtId: string, size = 512): Promise<string> {
    return invokeJson("getCoverArtUri", { id: coverArtId, size });
  },
  getSongCoverArtUri(songId: string, size = 512): Promise<string | null> {
    return invokeJson("getSongCoverArtUri", { id: songId, size });
  },
  getStreamUri(songId: string): Promise<string> {
    return invokeJson("getStreamUri", songId);
  },
};
