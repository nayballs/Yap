// The phone's page wears Yap's own colours: link.css copies src/app.css's
// `--yap-*` values (it can't import them: it's served on its own). This
// keeps the copy in step — every `--yap-*` link.css sets, light and dark,
// is app.css's value — and the browser's colours (the theme-color metas,
// the manifest) are the page's background in each theme. No browser.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { test, expect } from '@playwright/test';
import { PAGE_DIR } from './support/harness.js';

const here = path.dirname(fileURLToPath(import.meta.url));
const read = (f) => fs.readFileSync(f, 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
const APP = read(path.resolve(here, '../src/app.css'));
const LINK = read(path.join(PAGE_DIR, 'link.css'));

/** The `{…}` block that follows `at` in `css` (braces balanced). */
function block(css, at) {
  const start = css.indexOf('{', at);
  let depth = 0;
  for (let i = start; i < css.length; i++) {
    if (css[i] === '{') depth++;
    else if (css[i] === '}' && --depth === 0) return css.slice(start + 1, i);
  }
  throw new Error('unbalanced');
}

/** `--yap-*: value` pairs, whitespace evened out. */
function tokens(body) {
  const out = {};
  for (const m of body.matchAll(/(--yap-[\w-]+)\s*:\s*([^;]+);/g)) out[m[1]] = m[2].replace(/\s+/g, ' ').trim();
  return out;
}

/** The first `selector {` whose block sets `--yap-bg`. */
function rootWithBg(css, selector) {
  let at = 0;
  for (;;) {
    at = css.indexOf(selector, at);
    if (at === -1) throw new Error(`no ${selector} with --yap-bg`);
    const b = block(css, at);
    if (b.includes('--yap-bg:')) return tokens(b);
    at += selector.length;
  }
}

const APP_LIGHT = rootWithBg(APP, ':root {');
const APP_DARK = rootWithBg(APP, ":root[data-yap-theme='dark'] {");
const LINK_LIGHT = rootWithBg(LINK, ':root {');
const darkMedia = block(LINK, LINK.indexOf('@media (prefers-color-scheme: dark)'));
const LINK_DARK = rootWithBg(darkMedia, ':root {');

// The page's own font stacks add phone fallbacks; the faces are Yap's.
const OWN = new Set(['--yap-font-ui', '--yap-font-display']);

for (const [name, link, app] of [
  ['light', LINK_LIGHT, APP_LIGHT],
  ['dark', LINK_DARK, APP_DARK],
]) {
  test(`link.css's ${name} colours are app.css's`, () => {
    const names = Object.keys(link).filter((k) => !OWN.has(k));
    expect(names.length).toBeGreaterThan(30);
    const differ = names.filter((k) => link[k] !== app[k]).map((k) => `${k}: ${link[k]} (app.css: ${app[k]})`);
    expect(differ).toEqual([]);
  });
}

test("the browser's colours are the page's background, light and dark", () => {
  const html = fs.readFileSync(path.join(PAGE_DIR, 'index.html'), 'utf8');
  const meta = (scheme) => html.match(new RegExp(`theme-color" content="(#[0-9a-f]{6})" media="\\(prefers-color-scheme: ${scheme}\\)`))?.[1];
  expect(meta('light')).toBe(APP_LIGHT['--yap-s1']);
  expect(meta('dark')).toBe(APP_DARK['--yap-s1']);
  const manifest = JSON.parse(fs.readFileSync(path.join(PAGE_DIR, 'manifest.webmanifest'), 'utf8'));
  expect([manifest.theme_color, manifest.background_color]).toEqual([APP_LIGHT['--yap-s1'], APP_LIGHT['--yap-s1']]);
  const dark = manifest.user_preferences.color_scheme_dark;
  expect([dark.theme_color, dark.background_color]).toEqual([APP_DARK['--yap-s1'], APP_DARK['--yap-s1']]);
  // The icon is Yap's mark.
  expect(fs.readFileSync(path.join(PAGE_DIR, 'icon.svg'), 'utf8')).toBe(
    fs.readFileSync(path.resolve(here, '../src/assets/yap-logo.svg'), 'utf8'),
  );
});
