<script lang="ts">
  import LazyImage from "$lib/components/LazyImage.svelte";
  import { ArrowLeft } from "lucide-svelte";

  interface Props {
    title: string;
    subtitle?: string;
    coverArtId?: string | null;
    onBack?: () => void;
  }

  let { title, subtitle, coverArtId, onBack }: Props = $props();
</script>

<div
  class="flex items-center gap-3 px-4 py-3 border-b border-base-300 bg-base-100"
>
  <button
    class="btn btn-ghost btn-sm btn-square"
    onclick={() => onBack?.()}
    title="Back"
  >
    <ArrowLeft class="size-4" />
  </button>

  {#if coverArtId}
    <LazyImage {coverArtId} size={64} alt={title} class="w-12 h-12 rounded" />
  {/if}

  <div class="min-w-0 flex-1">
    <h2 class="font-semibold text-base truncate">{title}</h2>
    {#if subtitle}
      <p class="text-sm opacity-60 truncate">{subtitle}</p>
    {/if}
  </div>
</div>
