import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const dist = join(root, 'dist-remote');
const out = join(root, 'src-tauri', 'src', 'remote', 'index.html');

let html = readFileSync(join(dist, 'remote.html'), 'utf8');

html = html.replace(/<link[^>]+href="([^"]+\.css)"[^>]*>/g, (_m, href) => {
  const file = join(dist, href.replace(/^\.\//, '').replace(/^\//, ''));
  return `<style>${readFileSync(file, 'utf8')}</style>`;
});

html = html.replace(/<script[^>]+src="([^"]+\.js)"[^>]*><\/script>/g, (_m, src) => {
  const file = join(dist, src.replace(/^\.\//, '').replace(/^\//, ''));
  return `<script type="module">${readFileSync(file, 'utf8')}</script>`;
});

mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, html);
console.log(`embedded remote -> ${out} (${(Buffer.byteLength(html) / 1024).toFixed(1)} KiB)`);
