import {
  Menu,
  MenuItem,
  PredefinedMenuItem,
  Submenu,
} from "@tauri-apps/api/menu";

export async function showSongContextMenu(opts: {
  playlistId?: string | null;
  playlists: { id: string; name: string }[];
  onPlayNext: () => void;
  onAddToQueue: () => void;
  onRemoveFromPlaylist?: () => void;
  onAddToPlaylist: (playlistId: string) => void;
  onNewPlaylist: () => void;
}) {
  const items: (MenuItem | PredefinedMenuItem | Submenu)[] = [
    await MenuItem.new({ text: "Play Next", action: () => opts.onPlayNext() }),
    await MenuItem.new({
      text: "Add to Queue",
      action: () => opts.onAddToQueue(),
    }),
  ];

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
  onRename: () => void;
  onDelete: () => void;
}) {
  const menu = await Menu.new({
    items: [
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
}) {
  const menu = await Menu.new({
    items: [
      await MenuItem.new({
        text: "Play Next",
        action: () => opts.onPlayNext(),
      }),
      await MenuItem.new({
        text: "Add to Queue",
        action: () => opts.onAddToQueue(),
      }),
    ],
  });
  await menu.popup();
  await menu.close();
}
