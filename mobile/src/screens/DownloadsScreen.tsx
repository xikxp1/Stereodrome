import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";

import { SelectableList } from "@/components/SelectableList";
import { useProtectedSelectableAction } from "@/components/protectedSelectableAction";
import { useFileState, useStereodrome } from "@/context/StereodromeContext";
import { stereodromeCore } from "@/services/stereodromeCore";

export function DownloadsScreen() {
  const fileState = useFileState();
  const stereodrome = useStereodrome();
  const queryClient = useQueryClient();
  const cacheStats = useQuery({
    queryKey: ["audio-cache-stats"],
    queryFn: stereodromeCore.getAudioCacheStats,
  });
  const { protectedActionRows } = useProtectedSelectableAction(
    `downloads:${cacheStats.data?.file_count ?? 0}:${cacheStats.data?.total_size ?? 0}`
  );

  useEffect(() => {
    void queryClient.invalidateQueries({ queryKey: ["audio-cache-stats"] });
  }, [
    queryClient,
    fileState.downloadingSongIds.size,
    fileState.offlineSongIds.size,
  ]);

  async function refreshCache() {
    await queryClient.invalidateQueries({ queryKey: ["audio-cache-stats"] });
    await fileState.refreshOfflineSongIds();
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
                label: "Prefetch Queue",
                sublabel: "Download upcoming uncached queue items",
                onSelect: async () => {
                  await stereodromeCore.prefetchNext();
                  await refreshCache();
                },
              },
            ]),
        ...protectedActionRows({
          id: "clear-audio-cache",
          label: "Clear Audio Cache",
          sublabel: `${cacheStats.data?.file_count ?? 0} cached files`,
          confirmLabel: "Confirm Clear",
          confirmSublabel: "Use wheel select to remove cached audio",
          cancelLabel: "Cancel Clear",
          cancelSublabel: "Keep cached audio",
          onConfirm: async () => {
            await stereodromeCore.clearAudioCache();
            await refreshCache();
          },
        }),
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
