import { useQuery, useQueryClient } from "@tanstack/react-query";

import { SelectableList } from "@/components/SelectableList";
import { useStereodrome } from "@/context/StereodromeContext";
import { stereodromeCore } from "@/services/stereodromeCore";

export function DownloadsScreen() {
  const stereodrome = useStereodrome();
  const queryClient = useQueryClient();
  const cacheStats = useQuery({
    queryKey: ["audio-cache-stats"],
    queryFn: stereodromeCore.getAudioCacheStats,
  });

  async function refreshCache() {
    await queryClient.invalidateQueries({ queryKey: ["audio-cache-stats"] });
    await stereodrome.refreshOfflineSongIds();
  }

  const options = cacheStats.isLoading
    ? []
    : [
        {
          label: "Cache Usage",
          sublabel: `${formatBytes(cacheStats.data?.total_size ?? 0)} / ${formatBytes(
            cacheStats.data?.max_size ?? 0
          )}`,
          onSelect: refreshCache,
        },
        ...(stereodrome.offlineMode
          ? []
          : [
              {
                label: "Prefetch Next",
                sublabel: "Download upcoming queue item",
                onSelect: async () => {
                  await stereodromeCore.prefetchNext();
                  await refreshCache();
                },
              },
            ]),
        {
          label: "Clear Audio Cache",
          sublabel: `${cacheStats.data?.file_count ?? 0} cached files`,
          onSelect: async () => {
            await stereodromeCore.clearAudioCache();
            await refreshCache();
          },
        },
      ];

  return (
    <SelectableList
      empty={cacheStats.isLoading ? "Loading downloads" : "No download actions"}
      options={options}
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
