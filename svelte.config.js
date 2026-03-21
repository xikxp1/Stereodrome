// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

const basePreprocess = vitePreprocess({ script: true });

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: {
    ...basePreprocess,
    async script(options) {
      const result = await basePreprocess.script?.(options);

      if (!result) {
        return result;
      }

      const nextAttributes = { ...(result.attributes ?? options.attributes) };
      delete nextAttributes.lang;

      return {
        ...result,
        attributes: nextAttributes,
      };
    },
  },
  kit: {
    adapter: adapter({
      fallback: "index.html",
    }),
  },
};

export default config;
