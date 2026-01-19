<script lang="ts">
  import { connection } from "$lib/stores/connection.svelte";

  let url = $state("");
  let username = $state("");
  let password = $state("");

  async function handleSubmit(e: Event) {
    e.preventDefault();
    await connection.connect({ url, username, password });
  }
</script>

<div
  class="flex h-full w-full flex-col items-center justify-center bg-base-200 p-8"
>
  <!-- Logo/Title -->
  <div class="mb-8 text-center">
    <h1 class="text-4xl font-bold tracking-tight text-base-content">
      Stereodrome
    </h1>
    <p class="mt-1 text-sm text-base-content/60">Your music, everywhere</p>
  </div>

  <!-- Login Card -->
  <div
    class="w-full max-w-sm rounded-lg border border-base-300 bg-base-100 p-6 shadow-lg"
  >
    <h2 class="text-xl font-semibold text-base-content">Connect to Server</h2>
    <p class="mt-1 text-sm text-base-content/60">
      Enter your Subsonic server credentials
    </p>

    <form onsubmit={handleSubmit} class="mt-6 space-y-4">
      <div>
        <label
          class="mb-1.5 block text-xs font-medium text-base-content/70"
          for="url"
        >
          Server URL
        </label>
        <input
          type="url"
          id="url"
          bind:value={url}
          placeholder="https://your-server.com"
          class="w-full rounded-md border border-base-300 bg-base-100 px-3 py-2 text-sm text-base-content placeholder-base-content/40 transition-colors focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
          required
        />
      </div>

      <div>
        <label
          class="mb-1.5 block text-xs font-medium text-base-content/70"
          for="username"
        >
          Username
        </label>
        <input
          type="text"
          id="username"
          bind:value={username}
          placeholder="Username"
          class="w-full rounded-md border border-base-300 bg-base-100 px-3 py-2 text-sm text-base-content placeholder-base-content/40 transition-colors focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
          required
        />
      </div>

      <div>
        <label
          class="mb-1.5 block text-xs font-medium text-base-content/70"
          for="password"
        >
          Password
        </label>
        <input
          type="password"
          id="password"
          bind:value={password}
          placeholder="Password"
          class="w-full rounded-md border border-base-300 bg-base-100 px-3 py-2 text-sm text-base-content placeholder-base-content/40 transition-colors focus:border-primary focus:outline-none focus:ring-1 focus:ring-primary"
          required
        />
      </div>

      {#if connection.error}
        <div
          class="flex items-center gap-2 rounded-md border border-error/30 bg-error/10 px-3 py-2 text-sm text-error"
        >
          <svg
            xmlns="http://www.w3.org/2000/svg"
            class="h-4 w-4 shrink-0"
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
            stroke-width="2"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M12 8v4m0 4h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z"
            />
          </svg>
          <span>{connection.error}</span>
        </div>
      {/if}

      <button
        type="submit"
        class="mt-2 w-full rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-content shadow-sm transition-all hover:bg-primary/90 focus:outline-none focus:ring-2 focus:ring-primary focus:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-60"
        disabled={connection.isConnecting}
      >
        {#if connection.isConnecting}
          <span class="inline-flex items-center gap-2">
            <svg
              class="h-4 w-4 animate-spin"
              xmlns="http://www.w3.org/2000/svg"
              fill="none"
              viewBox="0 0 24 24"
            >
              <circle
                class="opacity-25"
                cx="12"
                cy="12"
                r="10"
                stroke="currentColor"
                stroke-width="4"
              ></circle>
              <path
                class="opacity-75"
                fill="currentColor"
                d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
              ></path>
            </svg>
            Connecting...
          </span>
        {:else}
          Connect
        {/if}
      </button>
    </form>
  </div>
</div>
