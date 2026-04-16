import { createQuery } from "@tanstack/svelte-query";
import {
  getArtists,
  getAlbums,
  getAlbumList,
  getSongs,
} from "$lib/api/commands";

// Query factories for library data
export const artistsQuery = () =>
  createQuery(() => ({
    queryKey: ["artists"],
    queryFn: getArtists,
  }));

export const albumsQuery = (artistId?: string) =>
  createQuery(() => ({
    queryKey: ["albums", artistId],
    queryFn: () => getAlbums(artistId),
  }));

export const albumListQuery = (listType: string, size?: number) =>
  createQuery(() => ({
    queryKey: ["albumList", listType, size],
    queryFn: () => getAlbumList(listType, size),
  }));

export const songsQuery = (albumId?: string) =>
  createQuery(() => ({
    queryKey: ["songs", albumId],
    queryFn: () => getSongs(albumId),
  }));
