import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
export default defineConfig({ base: "./", plugins: [react()], build: { sourcemap: false, rollupOptions: { input: { shell: "index.html", home: "home.html" } } } });
