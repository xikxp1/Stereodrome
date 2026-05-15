import { useQuery, useQueryClient } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { stereodromeCore } from "@/services/stereodromeCore";

export function DownloadsScreen() {
  const queryClient = useQueryClient();
  const cacheStats = useQuery({
    queryKey: ["audio-cache-stats"],
    queryFn: stereodromeCore.getAudioCacheStats,
  });

  async function refreshCache() {
    await queryClient.invalidateQueries({ queryKey: ["audio-cache-stats"] });
  }

  return (
    <SelectableList
      empty={cacheStats.isLoading ? "Loading downloads" : "No download actions"}
      options={[
        {
          label: "Cache Usage",
          sublabel: `${formatBytes(cacheStats.data?.total_size ?? 0)} / ${formatBytes(
            cacheStats.data?.max_size ?? 0
          )}`,
          onSelect: refreshCache,
        },
        {
          label: "Prefetch Next",
          sublabel: "Download upcoming queue item",
          onSelect: async () => {
            await stereodromeCore.prefetchNext();
            await refreshCache();
          },
        },
        {
          label: "Clear Audio Cache",
          sublabel: `${cacheStats.data?.file_count ?? 0} cached files`,
          onSelect: async () => {
            await stereodromeCore.clearAudioCache();
            await refreshCache();
          },
        },
      ]}
    />
  );
}

function formatBytes(bytes: number) {
  if (bytes < 1024 * 1024) {
    return `${Math.round(bytes / 1024)} KB`;
  }
  if (bytes < 1024 * 1024 * 1024) {
    return `${Math.round(bytes / (1024 * 1024))} MB`;
  }
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}
