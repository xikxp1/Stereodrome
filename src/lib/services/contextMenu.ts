import {
  Menu,
  MenuItem,
  PredefinedMenuItem,
  Submenu,
} from "@tauri-apps/api/menu";

export async function showSongContextMenu(opts: {
  selectionCount: number;
  playlists: { id: string; name: string }[];
  onPlayNext: () => void;
  onAddToQueue: () => void;
  showGoToArtist?: boolean;
  showGoToAlbum?: boolean;
  disableGoToArtist?: boolean;
  disableGoToAlbum?: boolean;
  onGoToArtist?: () => void;
  onGoToAlbum?: () => void;
  onRemoveFromPlaylist?: () => void;
  onAddToPlaylist: (playlistId: string) => void;
  onNewPlaylist: () => void;
}) {
  const selectionLabel =
    opts.selectionCount === 1
      ? "1 song selected"
      : `${opts.selectionCount} songs selected`;

  const items: (MenuItem | PredefinedMenuItem | Submenu)[] = [
    await MenuItem.new({ text: selectionLabel, enabled: false }),
    await PredefinedMenuItem.new({ item: "Separator" }),
    await MenuItem.new({ text: "Play Next", action: () => opts.onPlayNext() }),
    await MenuItem.new({
      text: "Add to Queue",
      action: () => opts.onAddToQueue(),
    }),
  ];

  if (
    opts.showGoToArtist ||
    opts.showGoToAlbum ||
    opts.onGoToArtist ||
    opts.onGoToAlbum
  ) {
    items.push(await PredefinedMenuItem.new({ item: "Separator" }));
    if (opts.showGoToArtist || opts.onGoToArtist) {
      items.push(
        await MenuItem.new({
          text: "Go to Artist",
          enabled: !opts.disableGoToArtist && !!opts.onGoToArtist,
          action: () => opts.onGoToArtist?.(),
        })
      );
    }
    if (opts.showGoToAlbum || opts.onGoToAlbum) {
      items.push(
        await MenuItem.new({
          text: "Go to Album",
          enabled: !opts.disableGoToAlbum && !!opts.onGoToAlbum,
          action: () => opts.onGoToAlbum?.(),
        })
      );
    }
  }

  if (opts.onRemoveFromPlaylist) {
    items.push(await PredefinedMenuItem.new({ item: "Separator" }));
    items.push(
      await MenuItem.new({
        text: "Remove from Playlist",
        action: () => opts.onRemoveFromPlaylist!(),
      })
    );
  }

  const playlistSubItems: (MenuItem | PredefinedMenuItem)[] = [];
  for (const p of opts.playlists) {
    playlistSubItems.push(
      await MenuItem.new({
        text: p.name,
        action: () => opts.onAddToPlaylist(p.id),
      })
    );
  }
  if (playlistSubItems.length > 0) {
    playlistSubItems.push(await PredefinedMenuItem.new({ item: "Separator" }));
  }
  playlistSubItems.push(
    await MenuItem.new({
      text: "New Playlist\u2026",
      action: () => opts.onNewPlaylist(),
    })
  );

  items.push(await PredefinedMenuItem.new({ item: "Separator" }));
  items.push(
    await Submenu.new({ text: "Add to Playlist", items: playlistSubItems })
  );

  const menu = await Menu.new({ items });
  await menu.popup();
  await menu.close();
}

export async function showPlaylistContextMenu(opts: {
  savedOffline: boolean;
  onToggleSavedOffline: () => void;
  onRename: () => void;
  onDelete: () => void;
}) {
  const menu = await Menu.new({
    items: [
      await MenuItem.new({
        text: opts.savedOffline ? "Remove Offline Save" : "Save Offline",
        action: () => opts.onToggleSavedOffline(),
      }),
      await PredefinedMenuItem.new({ item: "Separator" }),
      await MenuItem.new({ text: "Rename", action: () => opts.onRename() }),
      await MenuItem.new({ text: "Delete", action: () => opts.onDelete() }),
    ],
  });
  await menu.popup();
  await menu.close();
}

export async function showQueueableContextMenu(opts: {
  onPlayNext: () => void;
  onAddToQueue: () => void;
  onGoToArtist?: () => void;
  onGoToAlbum?: () => void;
}) {
  const items: (MenuItem | PredefinedMenuItem)[] = [
    await MenuItem.new({
      text: "Play Next",
      action: () => opts.onPlayNext(),
    }),
    await MenuItem.new({
      text: "Add to Queue",
      action: () => opts.onAddToQueue(),
    }),
  ];

  if (opts.onGoToArtist || opts.onGoToAlbum) {
    items.push(await PredefinedMenuItem.new({ item: "Separator" }));
    if (opts.onGoToArtist) {
      items.push(
        await MenuItem.new({
          text: "Go to Artist",
          action: () => opts.onGoToArtist?.(),
        })
      );
    }
    if (opts.onGoToAlbum) {
      items.push(
        await MenuItem.new({
          text: "Go to Album",
          action: () => opts.onGoToAlbum?.(),
        })
      );
    }
  }

  const menu = await Menu.new({ items });
  await menu.popup();
  await menu.close();
}
