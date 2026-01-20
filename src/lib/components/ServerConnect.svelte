<script lang="ts">
  import { connection } from "$lib/stores/connection.svelte";
  import { AlertCircle } from "lucide-svelte";

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
  <div class="mb-8 text-center">
    <h1 class="text-4xl font-bold tracking-tight">Stereodrome</h1>
    <p class="mt-1 text-sm opacity-60">Your music, everywhere</p>
  </div>

  <div class="card bg-base-100 w-full max-w-sm shadow-lg">
    <div class="card-body">
      <h2 class="card-title">Connect to Server</h2>
      <p class="text-sm opacity-60">Enter your Subsonic server credentials</p>

      <form onsubmit={handleSubmit} class="mt-4 space-y-4">
        <label class="floating-label">
          <span>Server URL</span>
          <input
            type="url"
            bind:value={url}
            placeholder="https://your-server.com"
            class="input input-bordered w-full"
            required
          />
        </label>

        <label class="floating-label">
          <span>Username</span>
          <input
            type="text"
            bind:value={username}
            placeholder="Username"
            class="input input-bordered w-full"
            required
          />
        </label>

        <label class="floating-label">
          <span>Password</span>
          <input
            type="password"
            bind:value={password}
            placeholder="Password"
            class="input input-bordered w-full"
            required
          />
        </label>

        {#if connection.error}
          <div role="alert" class="alert alert-error alert-sm">
            <AlertCircle class="h-4 w-4" />
            <span>{connection.error}</span>
          </div>
        {/if}

        <button
          type="submit"
          class="btn btn-primary w-full"
          disabled={connection.isConnecting}
        >
          {#if connection.isConnecting}
            <span class="loading loading-spinner loading-sm"></span>
            Connecting...
          {:else}
            Connect
          {/if}
        </button>
      </form>
    </div>
  </div>
</div>
