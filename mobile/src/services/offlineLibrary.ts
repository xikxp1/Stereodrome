import type {
  Album,
  Artist,
  SearchResults,
  Song,
  SongFileState,
} from "@/types/music";

export function songFileState(
  songId: string,
  offlineSongIds: Set<string>,
  downloadingSongIds: Set<string>
): SongFileState {
  if (downloadingSongIds.has(songId)) {
    return "downloading";
  }
  return offlineSongIds.has(songId) ? "downloaded" : "not_downloaded";
}

export function visibleSongs(
  songs: Song[],
  offlineMode: boolean,
  offlineSongIds: Set<string>
) {
  return offlineMode
    ? songs.filter((song) => offlineSongIds.has(song.id))
    : songs;
}

export function visibleAlbums(
  albums: Album[],
  songs: Song[],
  offlineMode: boolean,
  offlineSongIds: Set<string>
) {
  if (!offlineMode) {
    return albums;
  }

  const albumIds = new Set(
    visibleSongs(songs, offlineMode, offlineSongIds).map(
      (song) => song.album_id
    )
  );
  return albums.filter((album) => albumIds.has(album.id));
}

export function visibleArtists(
  artists: Artist[],
  songs: Song[],
  offlineMode: boolean,
  offlineSongIds: Set<string>
) {
  if (!offlineMode) {
    return artists;
  }

  const artistIds = new Set(
    visibleSongs(songs, offlineMode, offlineSongIds).map(
      (song) => song.artist_id
    )
  );
  return artists.filter((artist) => artistIds.has(artist.id));
}

export function visibleSearchResults(
  results: SearchResults | undefined,
  songs: Song[],
  offlineMode: boolean,
  offlineSongIds: Set<string>
) {
  if (!results || !offlineMode) {
    return results;
  }

  const offlineSongs = visibleSongs(songs, offlineMode, offlineSongIds);
  const songIds = new Set(offlineSongs.map((song) => song.id));
  const albumIds = new Set(offlineSongs.map((song) => song.album_id));
  const artistIds = new Set(offlineSongs.map((song) => song.artist_id));

  return {
    songs: results.songs.filter((song) => songIds.has(song.id)),
    albums: results.albums.filter((album) => albumIds.has(album.id)),
    artists: results.artists.filter((artist) => artistIds.has(artist.id)),
  };
}
