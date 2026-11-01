import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const daemon = process.env.BANTU_DAEMON_URL ?? "http://127.0.0.1:47800";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    port: 5173,
    proxy: { "/v1": { target: daemon, changeOrigin: false } },
  },
});
