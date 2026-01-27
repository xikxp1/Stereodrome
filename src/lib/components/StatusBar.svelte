<script lang="ts">
  interface Props {
    itemCount?: number;
    totalDuration?: number;
    totalSize?: number;
    itemType?: "songs" | "artists" | "albums";
  }

  let {
    itemCount = 0,
    totalDuration = 0,
    totalSize = 0,
    itemType = "songs",
  }: Props = $props();

  function formatDuration(seconds: number): string {
    if (!seconds) return "0 minutes";
    const hours = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);

    if (hours > 0) {
      return `${hours}.${Math.floor(mins / 6)} hours`;
    }
    return `${mins} minutes`;
  }

  function formatSize(bytes: number): string {
    if (!bytes) return "0 MB";
    const mb = bytes / (1024 * 1024);
    if (mb >= 1024) {
      return `${(mb / 1024).toFixed(1)} GB`;
    }
    return `${mb.toFixed(1)} MB`;
  }

  function getItemLabel(count: number, type: string): string {
    if (count === 1) {
      return type === "songs"
        ? "song"
        : type === "artists"
          ? "artist"
          : "album";
    }
    return type;
  }

  const statusText = $derived(() => {
    const parts: string[] = [];

    if (itemCount > 0) {
      parts.push(`${itemCount} ${getItemLabel(itemCount, itemType)}`);
    }

    if (totalDuration > 0) {
      parts.push(formatDuration(totalDuration));
    }

    if (totalSize > 0) {
      parts.push(formatSize(totalSize));
    }

    return parts.join(", ") || `No ${itemType}`;
  });
</script>

<div
  class="h-6 flex items-center justify-center px-4 select-none bg-base-200 border-t border-base-300"
>
  <span class="text-xs opacity-60">{statusText()}</span>
</div>
