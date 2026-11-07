import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri expects a fixed dev port and must not watch the Rust sources.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    // WKWebView on macOS 11+ and WebKitGTK on Linux and the BSDs.
    target: 'safari15',
    sourcemap: false,
    outDir: 'dist',
    emptyOutDir: true,
    // Vite minifies CSS with lightningcss, which has no OpenBSD build.
    // There the stylesheet is left unminified (a few kilobytes larger).
    cssMinify: process.platform !== 'openbsd',
  },
});
