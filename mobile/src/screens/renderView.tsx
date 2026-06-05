import type { ViewInstance } from "@/context/ViewContext";
import {
  AlbumListScreen,
  isRankedAlbumListKind,
} from "@/screens/AlbumListScreen";
import { AlbumScreen } from "@/screens/AlbumScreen";
import { AlbumsScreen } from "@/screens/AlbumsScreen";
import { ArtistScreen } from "@/screens/ArtistScreen";
import { ArtistsScreen } from "@/screens/ArtistsScreen";
import { ConnectScreen } from "@/screens/ConnectScreen";
import { DownloadsScreen } from "@/screens/DownloadsScreen";
import { HomeScreen } from "@/screens/HomeScreen";
import { LoadingScreen } from "@/screens/LoadingScreen";
import { MusicScreen } from "@/screens/MusicScreen";
import { NowPlayingScreen } from "@/screens/NowPlayingScreen";
import { PlaylistScreen } from "@/screens/PlaylistScreen";
import { PlaylistsScreen } from "@/screens/PlaylistsScreen";
import { QueueScreen } from "@/screens/QueueScreen";
import { SearchScreen } from "@/screens/SearchScreen";
import { SettingsScreen } from "@/screens/SettingsScreen";
import { SongsScreen } from "@/screens/SongsScreen";

export function renderView(view: ViewInstance) {
  switch (view.name) {
    case "loading":
      return <LoadingScreen />;
    case "connect":
      return <ConnectScreen />;
    case "music":
      return <MusicScreen />;
    case "artists":
      return <ArtistsScreen />;
    case "artist":
      return (
        <ArtistScreen
          artistId={view.params?.artistId ?? ""}
          title={view.params?.title ?? "Artist"}
        />
      );
    case "albums":
      return <AlbumsScreen />;
    case "albumList": {
      const kind = view.params?.kind;
      return isRankedAlbumListKind(kind) ? (
        <AlbumListScreen kind={kind} />
      ) : (
        <AlbumsScreen />
      );
    }
    case "album":
      return (
        <AlbumScreen
          albumId={view.params?.albumId ?? ""}
          title={view.params?.title ?? "Album"}
        />
      );
    case "songs":
      return <SongsScreen />;
    case "playlists":
      return <PlaylistsScreen />;
    case "playlist":
      return (
        <PlaylistScreen
          playlistId={view.params?.playlistId ?? ""}
          title={view.params?.title ?? "Playlist"}
        />
      );
    case "queue":
      return <QueueScreen />;
    case "downloads":
      return <DownloadsScreen />;
    case "search":
      return <SearchScreen />;
    case "nowPlaying":
      return <NowPlayingScreen />;
    case "settings":
      return <SettingsScreen category={view.params?.category} />;
    case "home":
    default:
      return <HomeScreen />;
  }
}
