import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/resources": "http://127.0.0.1:3000",
      "/resource-maps": "http://127.0.0.1:3000",
    },
  },
});
