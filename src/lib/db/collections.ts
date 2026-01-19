import { createQuery } from "@tanstack/svelte-query";
import { getArtists, getAlbums, getSongs } from "$lib/api/commands";

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

export const songsQuery = (albumId?: string) =>
  createQuery(() => ({
    queryKey: ["songs", albumId],
    queryFn: () => getSongs(albumId),
  }));
