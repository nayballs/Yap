#!/usr/bin/env node
// Full-page screenshots of every page, desktop and phone, light and dark,
// into site/screenshots/ (git-ignored), from the built site served by
// `astro preview`. Run `npm run build` first.
//
// It also checks the site's promises while it's there, and exits 1 if one
// breaks: no request to any other host, no cookie set, no console error (a
// CSP violation is one), no sideways scroll on a phone.
//
// Uses the Playwright library with the installed Chrome or Edge (or
// SITE_BROWSER=<channel>), so no browser download is needed.

import { spawn } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium, devices } from 'playwright';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const outDir = path.join(root, 'screenshots');
const PORT = Number(process.env.SITE_PORT ?? 4329);
// IPv4 loopback on purpose: `astro preview` alone listens on ::1 only, which
// some tools' "localhost" doesn't reach.
const HOST = '127.0.0.1';
const BASE = `http://${HOST}:${PORT}`;

const PAGES = [
  ['home', '/'],
  ['dictation', '/dictation/'],
  ['meetings', '/meetings/'],
  ['assistant', '/assistant/'],
  ['privacy', '/privacy/'],
  ['download', '/download/'],
  ['pricing', '/pricing/'],
  ['faq', '/faq/'],
  ['privacy-policy', '/legal/privacy-policy/'],
  ['not-found', '/no-such-page/'],
];

const VIEWS = [
  { name: 'desktop', options: { viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 } },
  {
    name: 'phone',
    options: { ...devices['Pixel 7'], viewport: { width: 390, height: 844 }, deviceScaleFactor: 1 },
  },
];

const SCHEMES = ['light', 'dark'];

function startPreview() {
  // Astro's own CLI, not scripts/astro.mjs: killing the wrapper on Windows
  // would leave its child serving. Telemetry off the same way.
  const cli = path.join(root, 'node_modules', 'astro', 'bin', 'astro.mjs');
  const child = spawn(process.execPath, [cli, 'preview', '--host', HOST, '--port', String(PORT)], {
    cwd: root,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, ASTRO_TELEMETRY_DISABLED: '1' },
  });
  child.stdout.on('data', () => {});
  child.stderr.on('data', (d) => process.stderr.write(d));
  child.on('exit', (code) => code && console.error(`preview exited with ${code}`));
  return child;
}

async function waitForServer(ms = 60000) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    try {
      const res = await fetch(BASE);
      if (res.ok) return;
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`preview server didn't start on ${BASE}`);
}

async function settle(page) {
  // Load every lazy picture, then go back to the top.
  await page.evaluate(async () => {
    for (let y = 0; y < document.body.scrollHeight; y += 600) {
      window.scrollTo(0, y);
      await new Promise((r) => setTimeout(r, 40));
    }
    window.scrollTo(0, 0);
    await Promise.all(
      [...document.images].map((img) => (img.complete ? null : new Promise((r) => img.addEventListener('load', r, { once: true })))),
    );
    await document.fonts.ready;
  });
  await page.waitForTimeout(150);
}

const problems = [];
const shots = [];

fs.mkdirSync(outDir, { recursive: true });
const preview = startPreview();
let browser;
try {
  await waitForServer();
  // The installed Chrome or Edge first (no browser download), else Playwright's own.
  const channels = [process.env.SITE_BROWSER, 'chrome', 'msedge'].filter(Boolean);
  for (const channel of channels) {
    try {
      browser = await chromium.launch({ channel });
      break;
    } catch {
      /* not installed: try the next */
    }
  }
  browser ??= await chromium.launch();
  for (const view of VIEWS) {
    for (const scheme of SCHEMES) {
      const context = await browser.newContext({ ...view.options, colorScheme: scheme, reducedMotion: 'no-preference' });
      const page = await context.newPage();
      page.on('request', (req) => {
        const host = new URL(req.url()).hostname;
        if (!['localhost', '127.0.0.1', '[::1]'].includes(host) && !req.url().startsWith('data:')) {
          problems.push(`${view.name}/${scheme}: a request to another host: ${req.url()}`);
        }
      });
      page.on('response', async (res) => {
        const cookie = (await res.allHeaders())['set-cookie'];
        if (cookie) problems.push(`${view.name}/${scheme}: a cookie was set by ${res.url()}`);
      });
      page.on('console', (msg) => {
        if (msg.type() === 'error' && !/404 \(Not Found\)/.test(msg.text())) {
          problems.push(`${view.name}/${scheme} ${page.url()}: console error: ${msg.text()}`);
        }
      });
      page.on('pageerror', (err) => problems.push(`${view.name}/${scheme} ${page.url()}: ${err.message}`));

      for (const [name, url] of PAGES) {
        await page.goto(BASE + url, { waitUntil: 'networkidle' });
        await settle(page);
        if (view.name === 'phone') {
          const sideways = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
          if (sideways > 0) problems.push(`phone/${scheme} ${url}: scrolls sideways by ${sideways}px`);
        }
        const file = path.join(outDir, `${name}-${view.name}-${scheme}.png`);
        await page.screenshot({ path: file, fullPage: true });
        shots.push(file);
      }

      // The hero demo mid-dictation, one viewport.
      await page.goto(BASE + '/', { waitUntil: 'networkidle' });
      await page.click('[data-key]');
      await page.waitForTimeout(2200);
      const listening = path.join(outDir, `home-demo-listening-${view.name}-${scheme}.png`);
      await page.screenshot({ path: listening });
      shots.push(listening);
      await page.waitForTimeout(3600);
      const pasted = path.join(outDir, `home-demo-pasted-${view.name}-${scheme}.png`);
      await page.screenshot({ path: pasted });
      shots.push(pasted);

      await context.close();
    }
  }
} finally {
  await browser?.close();
  preview.kill();
}

console.log(`screenshots: ${shots.length} files in ${path.relative(process.cwd(), outDir) || '.'}`);
if (problems.length) {
  console.error('screenshots: the site broke a promise:');
  for (const p of [...new Set(problems)]) console.error(`  ${p}`);
  process.exit(1);
}
console.log('screenshots: no other hosts, no cookies, no console errors, no sideways scroll on a phone');
