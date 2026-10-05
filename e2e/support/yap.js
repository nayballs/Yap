// Launches an isolated Yap test instance and connects Playwright to its
// webviews over CDP. See docs/e2e-tests.md.
//
// Isolation, so a run never touches the developer's own Yap:
// - portable mode: the exe is copied next to a `portable` marker, so config,
//   notes, history and logs live in <run>/app/Data (never %APPDATA%\yap);
// - its own WebView2 profile (WEBVIEW2_USER_DATA_FOLDER) and CDP port;
// - YAP_E2E=1 (debug builds only, src-tauri/src/e2e.rs): no global keyboard
//   hook, no paste into other apps, no focus stealing, and it quits when we
//   close its stdin;
// - YAP_AUTH_URL points at a closed local port: the account service is
//   unreachable and the saved-session slot is one nobody uses.
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

export const REPO = fileURLToPath(new URL('../..', import.meta.url));
export const OUT = path.join(REPO, 'test-results', 'app');
export const SCREENSHOTS = path.join(OUT, 'screenshots');
export const RUNS = path.join(OUT, 'runs');

/** Where `npm run test:app:build` puts the binary (see e2e/build.mjs). */
export const TARGET_DIR =
  process.env.YAP_E2E_TARGET_DIR || path.join(REPO, 'src-tauri', 'target', 'e2e');
export const EXE = process.env.YAP_E2E_EXE || path.join(TARGET_DIR, 'debug', 'yap.exe');

/** Seeded into every test instance's config.json (a test can add to it). */
export const BASE_CONFIG = {
  hotkey: 'kb:135', // F24: nobody's keyboard has one
  autostart: false,
  updateChecksEnabled: false,
  bridgeEnabled: false, // its discovery file lives in ~/.yap, outside Data
  soundEnabled: false,
  startHidden: true, // no first-run onboarding pop-up (tests open it themselves)
  configVersion: 1,
};

/** Variables from the developer's shell that must not reach a test instance. */
const SCRUB = /^(YAP_|WEBVIEW2_|TAURI_SIGNING_)/;

function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.unref();
    srv.on('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address();
      srv.close(() => resolve(port));
    });
  });
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function rmrf(dir) {
  // WebView2's processes can hold their profile for a moment after Yap exits.
  try {
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 });
  } catch (e) {
    console.warn(`e2e: couldn't remove ${dir}: ${e.message}`);
  }
}

/**
 * Start a fresh test instance.
 * @param {object} opts
 * @param {string} opts.name   run folder name (test-results/app/runs/<name>)
 * @param {object} [opts.config]  extra config.json fields
 * @param {Record<string,string>} [opts.env]  extra environment for the app
 */
export async function launchYap({ name, config = {}, env = {} }) {
  if (!fs.existsSync(EXE)) {
    throw new Error(`No test build at ${EXE} — run \`npm run test:app:build\` first.`);
  }
  const runDir = path.join(RUNS, name);
  rmrf(runDir);
  const appDir = path.join(runDir, 'app');
  const dataDir = path.join(appDir, 'Data');
  fs.mkdirSync(dataDir, { recursive: true });
  fs.copyFileSync(EXE, path.join(appDir, 'yap.exe'));
  fs.writeFileSync(path.join(appDir, 'portable'), 'Yap Portable Mode');
  fs.writeFileSync(
    path.join(dataDir, 'config.json'),
    JSON.stringify({ ...BASE_CONFIG, ...config }, null, 2)
  );

  const cdpPort = await freePort();
  const closedPort = await freePort(); // nothing listens here
  const childEnv = Object.fromEntries(Object.entries(process.env).filter(([k]) => !SCRUB.test(k)));
  Object.assign(childEnv, {
    YAP_E2E: '1',
    YAP_AUTH_URL: `http://127.0.0.1:${closedPort}`,
    WEBVIEW2_USER_DATA_FOLDER: path.join(runDir, 'webview2'),
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${cdpPort}`,
    NO_COLOR: '1',
    ...env,
  });

  const logPath = path.join(runDir, 'stdout.log');
  const log = fs.openSync(logPath, 'w');
  const child = spawn(path.join(appDir, 'yap.exe'), [], {
    cwd: appDir,
    env: childEnv,
    // stdin stays open: closing it is how we ask the app to quit (e2e.rs).
    stdio: ['pipe', log, log],
  });
  fs.closeSync(log);
  child.stdin.on('error', () => {}); // EPIPE if it exits before we close it
  let exited = null;
  child.on('exit', (code, signal) => (exited = { code, signal }));
  child.on('error', (e) => (exited = { error: e }));

  const app = new YapApp({ name, child, runDir, appDir, dataDir, logPath });
  try {
    app.browser = await connect(cdpPort, () => exited, logPath);
    await assertTestMode(logPath);
    await app.attachPages();
  } catch (e) {
    await app.stop();
    throw e;
  }
  return app;
}

/**
 * Refuse to drive a build without test mode (a release build ignores
 * YAP_E2E, so it would hook the keyboard and paste into other apps).
 */
async function assertTestMode(logPath) {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    if (fs.readFileSync(logPath, 'utf8').includes('e2e: test mode on')) return;
    await sleep(200);
  }
  throw new Error(
    `${EXE} didn't start in test mode. The suite needs the debug build from \`npm run test:app:build\`.`
  );
}

async function connect(port, exited, logPath) {
  const deadline = Date.now() + 45_000;
  let last;
  while (Date.now() < deadline) {
    if (exited()) {
      const tail = fs.readFileSync(logPath, 'utf8').split('\n').slice(-20).join('\n');
      throw new Error(`Yap exited during startup (${JSON.stringify(exited())}):\n${tail}`);
    }
    try {
      return await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 5_000 });
    } catch (e) {
      last = e;
      await sleep(250);
    }
  }
  throw new Error(`Couldn't connect to Yap's webviews on port ${port}: ${last}`);
}

/** The webview's Tauri window label ("settings", "onboarding", "overlay"). */
async function labelOf(page) {
  return page
    .evaluate(() => window.__TAURI_INTERNALS__?.metadata?.currentWindow?.label ?? null)
    .catch(() => null);
}

export class YapApp {
  constructor(fields) {
    Object.assign(this, fields);
    /** @type {import('@playwright/test').Browser} */
    this.browser = null;
    /** @type {Record<string, import('@playwright/test').Page>} */
    this.pages = {};
    /** Uncaught exceptions / unhandled rejections, from any webview. */
    this.pageErrors = [];
    /** console.error calls, from any webview. */
    this.consoleErrors = [];
  }

  get main() {
    return this.pages.settings;
  }
  get onboarding() {
    return this.pages.onboarding;
  }
  get overlay() {
    return this.pages.overlay;
  }

  /**
   * Find the three webviews, start listening for errors, then reload each one
   * so its whole boot (which ran before we connected) happens under watch.
   */
  async attachPages() {
    const context = this.browser.contexts()[0];
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      for (const page of context.pages()) {
        if (Object.values(this.pages).includes(page)) continue;
        const label = await labelOf(page);
        if (label) this.pages[label] = page;
      }
      if (this.pages.settings && this.pages.onboarding && this.pages.overlay) break;
      await sleep(250);
    }
    for (const label of ['settings', 'onboarding', 'overlay']) {
      if (!this.pages[label]) throw new Error(`Yap's "${label}" webview never showed up`);
      // The embedded frontend, not a dev server (which would be someone's Vite).
      if (!this.pages[label].url().startsWith('http://tauri.localhost/')) {
        throw new Error(`The test build loads ${this.pages[label].url()}, not its embedded frontend`);
      }
    }
    for (const [label, page] of Object.entries(this.pages)) {
      page.on('pageerror', (err) => this.pageErrors.push({ label, message: err.stack || String(err) }));
      page.on('console', (msg) => {
        if (msg.type() === 'error') this.consoleErrors.push({ label, message: msg.text() });
      });
    }
    await Promise.all(Object.values(this.pages).map((p) => p.reload({ waitUntil: 'load' })));
    await this.main.getByRole('navigation', { name: 'Main' }).waitFor();
  }

  /** Call a Tauri command from the main window, like the frontend does. */
  invoke(cmd, args = {}) {
    return this.main.evaluate(([c, a]) => window.__TAURI_INTERNALS__.invoke(c, a), [cmd, args]);
  }

  /** A JSON store from the portable data dir (config.json, notes.json, …). */
  readJson(file) {
    const p = path.join(this.dataDir, file);
    return fs.existsSync(p) ? JSON.parse(fs.readFileSync(p, 'utf8')) : null;
  }

  readConfig() {
    return this.readJson('config.json');
  }

  /** Quit (by closing stdin), falling back to killing the process tree. */
  async stop() {
    await this.browser?.close().catch(() => {});
    const { child } = this;
    if (child.exitCode === null && child.signalCode === null) {
      child.stdin.end();
      const quit = await Promise.race([
        new Promise((r) => child.once('exit', () => r(true))),
        sleep(10_000).then(() => false),
      ]);
      if (!quit) {
        try {
          execFileSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
        } catch {
          /* already gone */
        }
      }
    }
    // Keep config, data and logs for the report; drop the big stuff.
    await sleep(500);
    rmrf(path.join(this.appDir, 'yap.exe'));
    rmrf(path.join(this.runDir, 'webview2'));
  }
}
