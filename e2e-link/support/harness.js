// The phone's page without a PC: its files served from a made-up HTTPS
// origin through `page.route` (nothing listens anywhere), and Yap on the PC
// played by `page.routeWebSocket` at wss://<origin>/ws, keeping to the v1
// protocol (docs/architecture/link.md; src-tauri/link-page/README.md).
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect } from '@playwright/test';

const here = path.dirname(fileURLToPath(import.meta.url));
export const PAGE_DIR = path.resolve(here, '../../src-tauri/link-page');
export const ORIGIN = 'https://yap-pc.test:51443';
export const WS_URL = 'wss://yap-pc.test:51443/ws';

/** Fictional, as everything in this public repo. */
export const PC = 'Office PC';
export const DEVICE = { id: 'dev-test-1', secret: 'test-secret-1' };
export const MODELS = [
  { id: 'local:qwen3.5-4b', label: 'Qwen3.5 4B', onThisPc: true },
  { id: 'anthropic:claude-haiku', label: 'Claude Haiku', onThisPc: false },
];

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.webmanifest': 'application/manifest+json',
};

/**
 * Playwright can't route a worklet's own requests (Chromium makes them
 * outside the page's interception), so `addModule('…/mic-worklet.js')` would
 * go to the network and fail. Here the page fetches the worklet and what it
 * imports itself (routed like any request), inlined into one module from a
 * blob: the same code runs on the audio thread. On a phone the worklet loads
 * mic-worklet.js and dsp.js the ordinary way.
 */
function inlineWorklets() {
  const IMPORT = /^import\s*\{[^}]*\}\s*from\s*['"]([^'"]+)['"];?[ \t]*$/gm;
  const real = AudioWorklet.prototype.addModule;
  AudioWorklet.prototype.addModule = async function (url, options) {
    const seen = new Set();
    async function inline(u) {
      const res = await fetch(u);
      if (!res.ok) throw new DOMException(`Failed to load ${u}`, 'AbortError');
      const src = await res.text();
      let deps = '';
      for (const m of src.matchAll(IMPORT)) {
        const dep = new URL(m[1], u).href;
        if (seen.has(dep)) continue;
        seen.add(dep);
        deps += (await inline(dep)).replace(/^export\s+/gm, '') + '\n';
      }
      return deps + src.replace(IMPORT, '');
    }
    const code = await inline(new URL(url, location.href).href);
    window.__workletsInlined = (window.__workletsInlined || 0) + 1;
    return real.call(this, URL.createObjectURL(new Blob([code], { type: 'text/javascript' })), options);
  };
}

/** Serves src-tauri/link-page/ at ORIGIN, as Yap does; anything else is refused. */
export async function servePage(page) {
  await page.addInitScript(inlineWorklets);
  const served = [];
  await page.route('**/*', async (route) => {
    const url = new URL(route.request().url());
    if (url.origin !== ORIGIN) {
      // The page asks for nothing outside its own origin.
      served.push({ outside: url.href });
      return route.abort();
    }
    const rel = url.pathname === '/' ? 'index.html' : decodeURIComponent(url.pathname.slice(1));
    const file = path.join(PAGE_DIR, rel);
    if (!file.startsWith(PAGE_DIR) || !fs.existsSync(file) || rel.endsWith('.md')) {
      served.push({ missing: rel });
      return route.fulfill({ status: 404, body: 'Not found' });
    }
    served.push({ file: rel });
    return route.fulfill({
      status: 200,
      body: fs.readFileSync(file),
      headers: { 'content-type': TYPES[path.extname(rel)] || 'application/octet-stream', 'cache-control': 'no-store' },
    });
  });
  return served;
}

/** Puts a paired device (and the PC's name) in the page's storage before it loads. */
export async function seedDevice(page, device = DEVICE) {
  await page.addInitScript(
    ([d, pc]) => {
      if (!sessionStorage.getItem('seeded')) {
        localStorage.setItem('yap.link.device', JSON.stringify(d));
        localStorage.setItem('yap.link.pc', JSON.stringify(pc));
        sessionStorage.setItem('seeded', '1');
      }
    },
    [device, PC],
  );
}

/**
 * Yap on the PC, mocked. By default a hello with the right device gets
 * `ready`, and a ping a pong. Every message and frame is logged in order
 * (`log`), each connection kept (`conns`).
 */
export async function mockPc(page, opts = {}) {
  const pc = {
    conns: [],
    log: [],
    texts: [],
    frames: [],
    autoReady: opts.autoReady !== false,
    autoPong: opts.autoPong !== false,
    onHello: opts.onHello || null,
    models: opts.models || MODELS,
    model: opts.model || MODELS[0],
  };
  pc.ready = (conn = pc.last()) =>
    pc.sendTo(conn, { type: 'ready', pc: PC, model: pc.model, models: pc.models });
  pc.last = () => pc.conns[pc.conns.length - 1];
  pc.sendTo = (conn, msg) => conn.ws.send(JSON.stringify(msg));
  pc.send = (msg) => pc.sendTo(pc.last(), msg);
  pc.sendBinary = (buf) => pc.last().ws.send(buf);
  pc.close = () => pc.last().ws.close();
  pc.of = (type) => pc.texts.filter((m) => m.type === type);
  pc.types = () => pc.texts.map((m) => m.type);

  await page.routeWebSocket(WS_URL, (ws) => {
    const conn = { ws, texts: [], frames: [], closed: false };
    pc.conns.push(conn);
    ws.onMessage((m) => {
      if (typeof m === 'string') {
        const msg = JSON.parse(m);
        conn.texts.push(msg);
        pc.texts.push(msg);
        pc.log.push({ text: msg });
        if (msg.type === 'hello') {
          if (pc.onHello) pc.onHello(msg, conn, pc);
          else if (pc.autoReady && msg.device && msg.device.id === DEVICE.id) pc.ready(conn);
        } else if (msg.type === 'ping' && pc.autoPong) pc.sendTo(conn, { type: 'pong' });
      } else {
        const f = parseFrame(m);
        conn.frames.push(f);
        pc.frames.push(f);
        pc.log.push({ frame: f });
      }
    });
    ws.onClose(() => {
      conn.closed = true;
    });
  });
  return pc;
}

/** An up frame: {kind, seq, samples: Int16Array}. */
export function parseFrame(buf) {
  const b = Buffer.from(buf);
  const n = (b.length - 5) >> 1;
  const samples = new Int16Array(n);
  for (let i = 0; i < n; i++) samples[i] = b.readInt16LE(5 + i * 2);
  return { kind: b[0], seq: b.readUInt32LE(1), bytes: b.length, samples };
}

/** A down frame of Yap's voice: kind 0x02, seq, Int16 LE samples. */
export function downFrame(seq, samples) {
  const b = Buffer.alloc(5 + samples.length * 2);
  b[0] = 0x02;
  b.writeUInt32LE(seq, 1);
  for (let i = 0; i < samples.length; i++) b.writeInt16LE(samples[i], 5 + i * 2);
  return b;
}

/** `ms` of a soft tone at `rate`, as Int16. */
export function voice(rate, ms, hz = 220) {
  const n = Math.round((rate * ms) / 1000);
  const out = new Int16Array(n);
  for (let i = 0; i < n; i++) out[i] = Math.round(6000 * Math.sin((2 * Math.PI * hz * i) / rate));
  return out;
}

/** Opens the page (optionally at a fragment) and waits for its script to run. */
export async function open(page, hash = '') {
  await page.goto(`${ORIGIN}/${hash}`);
  await page.waitForFunction(() => !!window.__yapLink);
}

export const snapshot = (page) => page.evaluate(() => window.__yapLink.snapshot());

/** Paired, connected and ready, with the model chip up. */
export async function ready(page, pc) {
  await expect(page.getByTestId('conn')).toHaveText(`Connected to ${PC}`);
  await expect(page.getByTestId('talk')).toBeEnabled();
  return pc;
}

/** A question asked by tapping Talk, `ms` of it, and sent with a second tap. */
export async function ask(page, pc, ms = 600) {
  const frames = pc.frames.length;
  const starts = pc.of('start').length;
  const stops = pc.of('stop').length;
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('start').length).toBe(starts + 1);
  await expect.poll(() => pc.frames.length - frames, { timeout: 5000 }).toBeGreaterThan(Math.floor(ms / 20) - 5);
  await page.getByTestId('talk').click();
  await expect.poll(() => pc.of('stop').length).toBe(stops + 1);
}

/** Zero crossings → the pitch of Int16 samples at `rate`. */
export function pitch(samples, rate) {
  let crossings = 0;
  for (let i = 1; i < samples.length; i++) if ((samples[i - 1] < 0) !== (samples[i] < 0)) crossings++;
  return crossings / 2 / (samples.length / rate);
}
