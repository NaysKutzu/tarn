import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Set by `tauri android/ios dev` so the phone can reach the dev server.
const host = process.env.TAURI_DEV_HOST;

// Touch screens keep :hover "stuck" after a tap. On phone builds only, move
// every :hover rule under @media (hover: hover). Desktop output is untouched.
const platform = process.env.TAURI_ENV_PLATFORM;
const phone = platform === 'android' || platform === 'ios';
const DONE = Symbol('hover-done');
const hoverOnlyWhenHoverable = {
  postcssPlugin: 'hover-only-when-hoverable',
  Rule(rule: any, { AtRule }: any) {
    if (rule[DONE]) return;
    if (rule.parent?.type === 'atrule' && /keyframes/i.test(rule.parent.name)) return;
    const sels: string[] = rule.selectors ?? [];
    const hov = sels.filter((x) => x.includes(':hover'));
    if (!hov.length) return;
    const rest = sels.filter((x) => !x.includes(':hover'));
    const moved = rule.clone({ selectors: hov });
    moved[DONE] = true;
    const media = new AtRule({ name: 'media', params: '(hover: hover)' });
    media.append(moved);
    rule.after(media);
    if (rest.length) rule.selectors = rest;
    else rule.remove();
  },
};

export default defineConfig({
  plugins: [svelte()],
  css: phone ? { postcss: { plugins: [hoverOnlyWhenHoverable] } } : undefined,
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: {
      ignored: ['**/src-tauri/**'],
    },
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: ['es2021', 'chrome105', 'safari14'],
    minify: 'esbuild',
  },
});
