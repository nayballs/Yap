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
const TAURI_DIR = path.resolve(here, '../../src-tauri');

/**
 * The desktop's own files the PC serves beside the page, as build.rs embeds
 * them (`LINK_SHARED`: Chat's picker code at core/, the logos at
 * providers/): read from build.rs itself, so a module the page imports that
 * isn't embedded fails here as it would on a phone.
 */
export const SHARED = Object.fromEntries(
  [...fs.readFileSync(path.join(TAURI_DIR, 'build.rs'), 'utf8').matchAll(/\("((?:core|providers)\/[^"]+)",\s*"([^"]+)"\)/g)].map(
    ([, served, src]) => [served, path.resolve(TAURI_DIR, src)],
  ),
);
export const ORIGIN = 'https://yap-pc.test:51443';
export const WS_URL = 'wss://yap-pc.test:51443/ws';

/** Fictional, as everything in this public repo. */
export const PC = 'Office PC';
export const DEVICE = { id: 'dev-test-1', secret: 'test-secret-1' };
export const MODELS = [
  { id: 'ondevice|', label: 'Qwen3.5 4B', onThisPc: true },
  { id: 'anthropic|claude-haiku-5-5', label: 'Claude Haiku 5.5 · Anthropic', onThisPc: false },
];

/**
 * What a PC that answers typed questions, has Chat's picker, keeps the
 * phone's own conversation, takes its pictures and gives Talk's words back
 * for the box says it can (`ready.can`).
 */
export const CAN = ['ask', 'read_aloud', 'picker', 'effort', 'chats', 'pictures', 'words'];
/**
 * A PC from the afternoon of 3 Nov, before words in the box: Talk's Done
 * sends the question and Yap answers out loud — as hands-free does on a PC
 * today. The specs about a spoken question's answer and its voice use it,
 * so a tap of Done asks (the fake mic's steady tone isn't speech for
 * hands-free to end on; handsfree.spec.js has recorded speech).
 */
export const CAN_BEFORE_WORDS = ['ask', 'read_aloud', 'picker', 'effort', 'chats', 'pictures'];
/** A PC from the morning of 3 Nov: typing and the picker, no drawer, no pictures. */
export const CAN_BEFORE_CHATS = ['ask', 'read_aloud', 'picker', 'effort'];

/** Chat's conversations on the PC, as `chats` lists them (fictional, newest first). */
export const RECENTS = [
  { id: 41, title: 'Trip packing list', ts: 1_793_700_000 },
  { id: 38, title: 'Sourdough starter schedule', ts: 1_793_600_000 },
  { id: 35, title: 'Bike gear ratios', ts: 1_793_500_000 },
  { id: 30, title: 'Allotment watering rota', ts: 1_793_400_000 },
  { id: 22, title: 'Birthday card wording', ts: 1_793_300_000 },
];

/** A conversation's messages as `open_chat` returns them (fictional). */
export const THREADS = {
  41: {
    title: 'Trip packing list',
    messages: [
      { role: 'user', text: 'What should I pack for three days of hiking in the rain?' },
      {
        role: 'assistant',
        text: 'Pack **layers** you can add and take off:\n\n- A waterproof jacket and overtrousers\n- Two merino base layers\n- Spare socks in a dry bag\n\nA small towel and a head torch help too.',
        id: 'chat:41:1',
      },
      { role: 'user', text: 'Is this rucksack big enough?', voice: true, pictures: 1 },
      { role: 'assistant', text: 'A 35-litre pack is plenty for three days if you hut-hop.', id: 'chat:41:3' },
    ],
  },
  38: {
    title: 'Sourdough starter schedule',
    messages: [
      { role: 'user', text: 'How often do I feed a starter kept in the fridge?' },
      { role: 'assistant', text: 'Once a week: discard half, then feed it equal weights of flour and water.', id: 'chat:38:1' },
    ],
  },
};

/**
 * Chat's picker as the PC sends it (`ready.picker`, link/models.rs): Chat on
 * the on-device Qwen, keys for Groq (with a live list) and Anthropic
 * (Yap's curated list), the Claude models reading images. Fictional.
 */
export function pickerData({ model = 'ondevice', effort = null } = {}) {
  const now = Math.floor(Date.now() / 1000);
  const models = {
    ondevice: { provider: 'ondevice', id: 'qwen3.5-4b-q4_k_m', name: 'Qwen3.5 4B', onThisPc: true },
    opus: { provider: 'anthropic', id: 'claude-opus-5-5' },
    'opus-4-8': { provider: 'anthropic', id: 'claude-opus-4-8' },
  };
  return {
    status: {
      scope: 'chat',
      ready: true,
      error: null,
      model: models[model],
      host: model === 'ondevice' ? '127.0.0.1:8080' : 'api.anthropic.com',
      tools: true,
      fromScope: true,
      provider: models[model].provider,
      keyed: ['groq', 'anthropic'],
      servers: [],
      onDevice: { name: 'Qwen3.5 4B', tools: true },
      context: null,
      images: model === 'ondevice' ? false : true,
      effort,
    },
    catalog: {
      groq: {
        provider: 'groq',
        ok: true,
        fetchedAt: now - 120,
        checkedAt: now - 120,
        error: null,
        checking: false,
        models: [
          { id: 'openai/gpt-oss-20b', name: 'GPT-OSS 20B', context: 131072 },
          { id: 'openai/gpt-oss-120b', name: 'GPT-OSS 120B', context: 131072 },
          { id: 'qwen/qwen3.8-27b', name: 'Qwen3.8 27B', context: 131072 },
        ],
      },
    },
    local: {
      installed: true,
      running: true,
      activeFile: 'Qwen3.5-4B-Q4_K_M.gguf',
      modelFile: 'Qwen2.5-1.5B-Instruct-Q4_K_M.gguf',
      gpu: { name: 'Example Graphics 8 GB' },
      gpuCapable: true,
      placement: { kind: 'gpu', gpuLayers: 33, layers: 33 },
      slowOnThisPc: false,
      sharedOnDevice: false,
      own: [],
      curated: [
        {
          id: 'qwen3.5-4b',
          display: 'Qwen3.5 4B',
          blurb: 'Balanced: quick answers that call tools well',
          filename: 'Qwen3.5-4B-Q4_K_M.gguf',
          sizeMb: 2740,
          recommended: true,
          isDefault: false,
          tools: true,
          family: 'qwen',
          installed: true,
          maxContext: 262144,
          readsImages: false,
        },
        {
          id: 'gemma-4-12b',
          display: 'Gemma 4 12B',
          blurb: 'Larger and slower; reads images',
          filename: 'gemma-4-12b-it-Q4_K_M.gguf',
          sizeMb: 7300,
          recommended: false,
          isDefault: false,
          tools: true,
          family: 'gemma',
          installed: false,
          maxContext: 131072,
          readsImages: false,
        },
      ],
    },
    images: {
      groq: ['qwen/qwen3.8-27b'],
      anthropic: ['claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5', 'claude-fable-5-1', 'claude-opus-4-8'],
    },
  };
}

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.webmanifest': 'application/manifest+json',
  '.woff2': 'font/woff2',
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
    const file = SHARED[rel] || path.join(PAGE_DIR, rel);
    if ((!SHARED[rel] && !file.startsWith(PAGE_DIR)) || !fs.existsSync(file) || rel.endsWith('.md')) {
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
    // A PC from before typed questions and Chat's picker (`old`): v1's `ready` only.
    can: opts.old ? null : opts.can || CAN,
    picker: opts.old ? null : opts.picker || pickerData(),
    // The drawer's conversations and their messages: `chats`, `open_chat`
    // and `new_chat` answered as the PC does (`autoChats` false: by the test).
    recents: [...(opts.recents || RECENTS)],
    threads: { ...(opts.threads || THREADS) },
    autoChats: opts.autoChats !== false,
    // Pictures as they came: `{n, size, bytes}` (a `picture` and its 0x03 frames).
    pictures: [],
  };
  pc.ready = (conn = pc.last()) =>
    pc.sendTo(conn, {
      type: 'ready',
      pc: PC,
      model: pc.model,
      models: pc.models,
      ...(pc.can ? { can: pc.can } : {}),
      ...(pc.picker ? { picker: pc.picker } : {}),
    });
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
        else if (msg.type === 'picture') pc.pictures.push({ n: msg.n, size: msg.size, bytes: Buffer.alloc(0), pieces: 0 });
        else if (pc.autoChats && msg.type === 'chats') pc.sendTo(conn, { type: 'chats', items: pc.recents });
        else if (pc.autoChats && msg.type === 'open_chat') {
          const t = pc.threads[msg.id];
          if (t) pc.sendTo(conn, { type: 'thread', id: msg.id, title: t.title, messages: t.messages, more: false });
          else {
            pc.sendTo(conn, { type: 'error', message: "That chat isn't in Chat on your PC any more. Your next question starts a new one." });
            pc.sendTo(conn, { type: 'thread', id: null, title: '', messages: [], more: false });
          }
        } else if (pc.autoChats && msg.type === 'new_chat') {
          pc.sendTo(conn, { type: 'thread', id: null, title: '', messages: [], more: false });
        }
      } else if (Buffer.from(m)[0] === 0x03) {
        // A piece of the picture announced last.
        const b = Buffer.from(m);
        const p = pc.pictures[pc.pictures.length - 1];
        if (p) {
          expect(b.readUInt32LE(1)).toBe(p.pieces);
          p.pieces++;
          p.bytes = Buffer.concat([p.bytes, b.subarray(5)]);
        }
        pc.log.push({ picture: b.length });
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

/**
 * A made-up photo, drawn in the page (a sky, a sun, hills; `hue` sets its
 * colours), as a file to choose: `{name, mimeType, buffer}`.
 */
export async function photo(page, { w = 1200, h = 900, hue = 200, name = 'photo.png' } = {}) {
  const bytes = await page.evaluate(
    async ([w, h, hue]) => {
      const c = new OffscreenCanvas(w, h);
      const g = c.getContext('2d');
      const sky = g.createLinearGradient(0, 0, 0, h);
      sky.addColorStop(0, `hsl(${hue} 70% 72%)`);
      sky.addColorStop(1, `hsl(${hue + 30} 60% 90%)`);
      g.fillStyle = sky;
      g.fillRect(0, 0, w, h);
      g.fillStyle = 'hsl(45 95% 62%)';
      g.beginPath();
      g.arc(w * 0.72, h * 0.3, Math.min(w, h) * 0.11, 0, Math.PI * 2);
      g.fill();
      g.fillStyle = `hsl(${hue - 80} 35% 45%)`;
      g.beginPath();
      g.moveTo(0, h * 0.7);
      g.quadraticCurveTo(w * 0.3, h * 0.45, w * 0.6, h * 0.68);
      g.quadraticCurveTo(w * 0.85, h * 0.85, w, h * 0.6);
      g.lineTo(w, h);
      g.lineTo(0, h);
      g.fill();
      g.fillStyle = `hsl(${hue - 90} 40% 30%)`;
      g.beginPath();
      g.moveTo(0, h * 0.86);
      g.quadraticCurveTo(w * 0.5, h * 0.7, w, h * 0.9);
      g.lineTo(w, h);
      g.lineTo(0, h);
      g.fill();
      const blob = await c.convertToBlob({ type: 'image/png' });
      return [...new Uint8Array(await blob.arrayBuffer())];
    },
    [w, h, hue],
  );
  return { name, mimeType: 'image/png', buffer: Buffer.from(bytes) };
}

/** A JPEG's size from its frame header, or null when it isn't one. */
export function jpegSize(buf) {
  if (buf[0] !== 0xff || buf[1] !== 0xd8) return null;
  let i = 2;
  while (i + 9 < buf.length) {
    if (buf[i] !== 0xff) return null;
    const marker = buf[i + 1];
    const len = buf.readUInt16BE(i + 2);
    if (marker >= 0xc0 && marker <= 0xcf && ![0xc4, 0xc8, 0xcc].includes(marker)) {
      return { h: buf.readUInt16BE(i + 5), w: buf.readUInt16BE(i + 7) };
    }
    i += 2 + len;
  }
  return null;
}

/** Pictures chosen with + → Choose photos. */
export async function choosePhotos(page, files) {
  await page.getByTestId('plus').click();
  await expect(page.getByTestId('plus-menu')).toBeVisible();
  const chooser = page.waitForEvent('filechooser');
  await page.getByTestId('choose-photos').click();
  await (await chooser).setFiles(files);
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

/** The settings sheet (⋯): Hands-free and its wait, Clearer sound, the mic, Sound details. */
export async function openSettings(page) {
  await page.getByTestId('settings').click();
  await expect(page.getByTestId('settings-sheet')).toBeVisible();
}

export async function closeSettings(page) {
  await page.getByTestId('settings-sheet').getByRole('button', { name: 'Done' }).click();
  await expect(page.getByTestId('settings-sheet')).toBeHidden();
}

/** A question typed in the composer and sent with Enter. */
export async function type(page, pc, text) {
  const asks = pc.of('ask').length;
  await page.getByTestId('input').fill(text);
  await page.getByTestId('input').press('Enter');
  await expect.poll(() => pc.of('ask').length).toBe(asks + 1);
}
