import { reactRouter } from "@react-router/dev/vite";
import tailwindcss from "@tailwindcss/vite";
import { defineConfig } from "vite";

const TREX_URL = process.env.TREX_URL ?? "http://127.0.0.1:8080";

export default defineConfig({
  plugins: [tailwindcss(), reactRouter()],
  resolve: {
    tsconfigPaths: true,
  },
  server: {
    // the browser talks to trex through the dev server, so trex needs no cors
    proxy: {
      "/api": { target: TREX_URL, changeOrigin: true, rewrite: (path) => path.replace(/^\/api/, "") },
    },
  },
});
