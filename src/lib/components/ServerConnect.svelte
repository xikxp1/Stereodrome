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

<div class="card bg-base-200 w-full max-w-md shadow-xl">
  <div class="card-body">
    <h2 class="card-title text-2xl font-bold">Connect to Server</h2>
    <p class="text-base-content/70">Enter your Subsonic server credentials</p>

    <form onsubmit={handleSubmit} class="mt-4 space-y-4">
      <div class="form-control">
        <label class="label" for="url">
          <span class="label-text">Server URL</span>
        </label>
        <input
          type="url"
          id="url"
          bind:value={url}
          placeholder="https://your-server.com"
          class="input input-bordered w-full"
          required
        />
      </div>

      <div class="form-control">
        <label class="label" for="username">
          <span class="label-text">Username</span>
        </label>
        <input
          type="text"
          id="username"
          bind:value={username}
          placeholder="Username"
          class="input input-bordered w-full"
          required
        />
      </div>

      <div class="form-control">
        <label class="label" for="password">
          <span class="label-text">Password</span>
        </label>
        <input
          type="password"
          id="password"
          bind:value={password}
          placeholder="Password"
          class="input input-bordered w-full"
          required
        />
      </div>

      {#if connection.error}
        <div class="alert alert-error">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            class="stroke-current shrink-0 h-6 w-6"
            fill="none"
            viewBox="0 0 24 24"
          >
            <path
              stroke-linecap="round"
              stroke-linejoin="round"
              stroke-width="2"
              d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z"
            />
          </svg>
          <span>{connection.error}</span>
        </div>
      {/if}

      <div class="card-actions justify-end mt-6">
        <button
          type="submit"
          class="btn btn-primary"
          disabled={connection.isConnecting}
        >
          {#if connection.isConnecting}
            <span class="loading loading-spinner loading-sm"></span>
            Connecting...
          {:else}
            Connect
          {/if}
        </button>
      </div>
    </form>
  </div>
</div>
