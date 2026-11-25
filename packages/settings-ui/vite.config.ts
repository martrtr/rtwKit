import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
export default defineConfig({ base: './', plugins: [react()], build: { sourcemap: false, rollupOptions: { input: { settings: 'index.html', appearance: 'appearance.html' } } } });
