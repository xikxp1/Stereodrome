<script lang="ts">
  interface Props {
    itemCount?: number;
    totalDuration?: number;
    totalSize?: number;
  }

  let { itemCount = 0, totalDuration = 0, totalSize = 0 }: Props = $props();

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

  const statusText = $derived(() => {
    const parts: string[] = [];

    if (itemCount > 0) {
      parts.push(`${itemCount} ${itemCount === 1 ? "item" : "items"}`);
    }

    if (totalDuration > 0) {
      parts.push(formatDuration(totalDuration));
    }

    if (totalSize > 0) {
      parts.push(formatSize(totalSize));
    }

    return parts.join(", ") || "No items";
  });
</script>

<div class="status-bar h-6 flex items-center justify-center px-4 select-none">
  <span class="status-bar-text">{statusText()}</span>
</div>
