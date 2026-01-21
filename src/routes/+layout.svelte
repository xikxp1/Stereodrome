<script lang="ts">
  import "../app.css";
  import { QueryClientProvider } from "@tanstack/svelte-query";
  import { queryClient } from "$lib/db/queryClient";
  import { connection } from "$lib/stores/connection.svelte";
  import { notifications } from "$lib/services/notifications.svelte";
  import { onMount } from "svelte";

  let { children } = $props();

  onMount(() => {
    connection.checkStatus();
    notifications.init();
  });
</script>

<QueryClientProvider client={queryClient}>
  {@render children()}
</QueryClientProvider>
