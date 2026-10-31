#!/usr/bin/env node
// The name lives in ONE place: src/brand.ts. This fails the build when the
// product's current name, or one on the rename shortlist, is spelled anywhere
// else in the site's source, so the rename stays a one-line change and no
// candidate name leaks into a preview.

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const allowed = new Set([path.join(root, 'src', 'brand.ts')]);
const scan = ['src', 'public', 'astro.config.ts'];
const NAMES = /\b(?:yap|kiku|yatter|kagami)\b/i;
const TEXT = /\.(astro|ts|js|mjs|css|svg|txt|json|md|html)$/i;

const hits = [];
function walk(p) {
  const stat = fs.statSync(p);
  if (stat.isDirectory()) {
    for (const f of fs.readdirSync(p)) walk(path.join(p, f));
    return;
  }
  if (allowed.has(p) || !TEXT.test(p)) return;
  fs.readFileSync(p, 'utf8')
    .split(/\r?\n/)
    .forEach((line, i) => {
      if (NAMES.test(line)) hits.push(`${path.relative(root, p)}:${i + 1}: ${line.trim().slice(0, 120)}`);
    });
}
for (const s of scan) {
  const p = path.join(root, s);
  if (fs.existsSync(p)) walk(p);
}

if (hits.length) {
  console.error('check-name: the product name is spelled outside src/brand.ts. Use NAME from brand.ts instead:');
  for (const h of hits) console.error(`  ${h}`);
  process.exit(1);
}
console.log('check-name: the name appears only in src/brand.ts');
