<script lang="ts">
  import "../app.css";
  import { QueryClientProvider } from "@tanstack/svelte-query";
  import { queryClient } from "$lib/db/queryClient";
  import { connection } from "$lib/stores/connection.svelte";
  import { libraryRefresh } from "$lib/services/libraryRefresh.svelte";
  import { notifications } from "$lib/services/notifications.svelte";
  import { mediaControls } from "$lib/services/mediaControls.svelte";
  import { trayControls } from "$lib/services/trayControls.svelte";
  import { taskbarWidget } from "$lib/services/taskbarWidget.svelte";
  import { onMount } from "svelte";

  let { children } = $props();

  onMount(() => {
    void connection.initialize();
    libraryRefresh.init();
    notifications.init();
    mediaControls.init();
    trayControls.init();
    taskbarWidget.init();

    return () => {
      libraryRefresh.destroy();
      mediaControls.destroy();
      trayControls.destroy();
      taskbarWidget.destroy();
    };
  });
</script>

<QueryClientProvider client={queryClient}>
  {@render children()}
</QueryClientProvider>
