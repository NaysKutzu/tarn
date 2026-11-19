// Sanity check for UI translations: every `tr('…')` / `t('…')` key used in
// src/ should exist in each dictionary, and `{placeholders}` must match.
// Run: node scripts/check-i18n.mjs  (exits 1 on placeholder mismatches)
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';

const root = new URL('../src/', import.meta.url).pathname;
const files = [];
(function walk(d) {
  for (const n of readdirSync(d)) {
    const p = join(d, n);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(svelte|ts)$/.test(n)) files.push(p);
  }
})(root);

const keys = new Set();
const re = /\b(?:tr|t)\(\s*(?:'((?:[^'\\]|\\.)*)'|"((?:[^"\\]|\\.)*)")/g;
for (const f of files) {
  for (const m of readFileSync(f, 'utf8').matchAll(re)) {
    keys.add((m[1] ?? m[2]).replace(/\\(['"])/g, '$1'));
  }
}

const ph = (s) => (s.match(/\{\w+\}/g) ?? []).sort().join(',');
let bad = 0;
const dir = join(root, 'lib/i18n');
for (const n of readdirSync(dir).filter((n) => n.endsWith('.json'))) {
  const dict = JSON.parse(readFileSync(join(dir, n), 'utf8'));
  const missing = [...keys].filter((k) => !(k in dict) && /[a-z]{2}/.test(k));
  for (const [k, v] of Object.entries(dict)) {
    if (ph(k) !== ph(v)) { bad++; console.error(`${n}: placeholder mismatch for "${k}"`); }
  }
  console.log(`${n}: ${Object.keys(dict).length} strings, ${missing.length} used keys not translated (fall back to English)`);
  for (const k of missing) console.log(`  - ${k}`);
}
process.exit(bad ? 1 : 0);
