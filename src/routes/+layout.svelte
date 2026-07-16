<script lang="ts">
  import "../app.css";
  import { connection } from "$lib/stores/connection.svelte";
  import { libraryRefresh } from "$lib/services/libraryRefresh.svelte";
  import { notifications } from "$lib/services/notifications.svelte";
  import { mediaControls } from "$lib/services/mediaControls.svelte";
  import { trayControls } from "$lib/services/trayControls.svelte";
  import { onMount } from "svelte";

  let { children } = $props();

  onMount(() => {
    void connection.initialize();
    libraryRefresh.init();
    notifications.init();
    mediaControls.init();
    trayControls.init();

    return () => {
      libraryRefresh.destroy();
      mediaControls.destroy();
      trayControls.destroy();
    };
  });
</script>

{@render children()}
