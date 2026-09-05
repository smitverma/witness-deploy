import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [sveltekit()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,

  build: {
    rollupOptions: {
      output: {
        /** @param {string} id */
        manualChunks(id) {
          const normalizedId = id.replaceAll("\\", "/");
          if (normalizedId.includes("/src/lib/components/")) {
            if (/(Intruder|IntruderResults|PayloadWarehouse)\.svelte$/.test(normalizedId)) return "workspace-fuzz";
            if (/(SettingsPanel|AiSettings)\.svelte$/.test(normalizedId)) return "workspace-settings";
            return "workspace-components";
          }
          if (normalizedId.includes("/src/lib/")) return "workspace-lib";
          if (normalizedId.includes("/node_modules/@codemirror/")) return "vendor-codemirror";
          if (normalizedId.includes("/node_modules/driver.js/")) return "vendor-tutorial";
          if (normalizedId.includes("/node_modules/@tauri-apps/")) return "vendor-tauri";
          if (normalizedId.includes("/node_modules/")) return "vendor";
          return undefined;
        },
      },
    },
  },


  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
