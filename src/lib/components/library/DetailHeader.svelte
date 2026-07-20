<script lang="ts">
  import type { ComponentType } from "svelte";
  import LazyImage from "$lib/components/LazyImage.svelte";
  import { ArrowLeft } from "lucide-svelte";

  interface Props {
    title: string;
    subtitle?: string;
    coverArtId?: string | null;
    onBack?: () => void;
    actionLabel?: string;
    actionTitle?: string;
    actionDisabled?: boolean;
    actionIcon?: ComponentType;
    onAction?: () => void;
    onCoverArtClick?: () => void;
  }

  let {
    title,
    subtitle,
    coverArtId,
    onBack,
    actionLabel,
    actionTitle,
    actionDisabled = false,
    actionIcon: ActionIcon,
    onAction,
    onCoverArtClick,
  }: Props = $props();
</script>

<div
  class="flex items-center gap-3 px-4 py-3 border-b border-base-300 bg-base-100"
>
  <button
    type="button"
    class="btn btn-ghost btn-sm btn-square"
    onclick={() => onBack?.()}
    title="Back"
  >
    <ArrowLeft class="size-4" />
  </button>

  {#if coverArtId}
    {#if onCoverArtClick}
      <button
        type="button"
        class="group h-12 w-12 shrink-0 overflow-hidden rounded p-0 transition-opacity hover:opacity-80 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary"
        onclick={() => onCoverArtClick()}
        title="View cover art"
        aria-label="View cover art for {title}"
      >
        <LazyImage
          {coverArtId}
          size={64}
          alt={title}
          class="h-full w-full rounded"
        />
      </button>
    {:else}
      <LazyImage {coverArtId} size={64} alt={title} class="w-12 h-12 rounded" />
    {/if}
  {/if}

  <div class="min-w-0 flex-1">
    <h2 class="font-semibold text-base truncate">{title}</h2>
    {#if subtitle}
      <p class="text-sm opacity-60 truncate">{subtitle}</p>
    {/if}
  </div>

  {#if onAction && actionLabel}
    <button
      type="button"
      class="btn btn-sm"
      disabled={actionDisabled}
      onclick={() => onAction?.()}
      title={actionTitle ?? actionLabel}
    >
      {#if ActionIcon}
        <ActionIcon class="size-4" />
      {/if}
      <span>{actionLabel}</span>
    </button>
  {/if}
</div>
