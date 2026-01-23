<script lang="ts">
  import "../app.css";
  import { QueryClientProvider } from "@tanstack/svelte-query";
  import { queryClient } from "$lib/db/queryClient";
  import { connection } from "$lib/stores/connection.svelte";
  import { notifications } from "$lib/services/notifications.svelte";
  import { mediaControls } from "$lib/services/mediaControls.svelte";
  import { trayControls } from "$lib/services/trayControls.svelte";
  import { onMount } from "svelte";

  let { children } = $props();

  onMount(() => {
    connection.checkStatus();
    notifications.init();
    mediaControls.init();
    trayControls.init();

    return () => {
      mediaControls.destroy();
      trayControls.destroy();
    };
  });
</script>

<QueryClientProvider client={queryClient}>
  {@render children()}
</QueryClientProvider>
