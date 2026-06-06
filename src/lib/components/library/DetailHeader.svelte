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
  }: Props = $props();
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

  {#if onAction && actionLabel}
    <button
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
