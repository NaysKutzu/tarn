import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { resolve } from 'node:path';

/** Standalone phone-remote page: one HTML file Rust can `include_str!`. */
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  build: {
    outDir: 'dist-remote',
    emptyOutDir: true,
    target: ['es2021', 'chrome105', 'safari14'],
    minify: 'oxc',
    assetsInlineLimit: 64_000,
    cssCodeSplit: false,
    modulePreload: false,
    codeSplitting: false,
    rollupOptions: {
      input: resolve(import.meta.dirname, 'remote.html'),
      output: {
        entryFileNames: 'remote.js',
        assetFileNames: 'remote[extname]',
      },
    },
  },
});
