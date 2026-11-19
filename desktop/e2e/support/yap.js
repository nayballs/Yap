// Launches an isolated Yap test instance and connects Playwright to its
// webviews over CDP. See docs/e2e-tests.md.
//
// Isolation, so a run never touches the developer's own Yap:
// - portable mode: the exe is copied next to a `portable` marker, so config,
//   notes, history and logs live in <run>/app/Data (never %APPDATA%\yap);
// - its own WebView2 profile (WEBVIEW2_USER_DATA_FOLDER) and CDP port;
// - YAP_E2E=1 (debug builds only, desktop/src-tauri/src/shell/e2e.rs): no global keyboard
//   hook, no paste into other apps, no focus stealing, and it quits when we
//   close its stdin;
// - YAP_AUTH_URL points at a closed local port: the account service is
//   unreachable and the saved-session slot is one nobody uses;
// - YAP_BRIDGE_FILE and YAP_MCP_CLIENT_ROOT (debug builds only) keep the
//   local API's discovery file and the AI apps' config files ("Add to
//   Claude"…) inside the run folder, never in ~/.yap or a real AI app.
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { createRequire } from 'node:module';
import net from 'node:net';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

/** The desktop app's folder, `desktop/`: its src/, desktop/src-tauri/ and these tests. */
export const APP = fileURLToPath(new URL('../..', import.meta.url));
/** The repository's root: test-results/ is there, beside the phone's. */
export const REPO = path.resolve(APP, '..');
export const OUT = path.join(REPO, 'test-results', 'app');

/**
 * A file inside an npm package (`vite`, `bin/vite.js`), wherever npm put the
 * package: the repo is an npm workspace, which keeps them in the root's
 * node_modules.
 */
export function packageFile(name, file) {
  const require = createRequire(import.meta.url);
  return path.join(path.dirname(require.resolve(`${name}/package.json`)), file);
}
export const SCREENSHOTS = path.join(OUT, 'screenshots');
export const RUNS = path.join(OUT, 'runs');

/**
 * WebView2's own arguments in a test run (a spec adding one, say
 * `--force-device-scale-factor=1.5`, passes these and its own in `env`).
 * Port 0: WebView2 picks a free port and writes it to DevToolsActivePort in
 * the profile.
 */
export const WEBVIEW2_ARGS = '--remote-debugging-port=0 --disable-features=CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows';

/**
 * The environment for a `cargo build` given its target dir as `--target-dir`:
 * this one's, without CARGO_TARGET_DIR. sccache (the shared compile cache,
 * docs/architecture/build-and-test.md) keys every crate on each CARGO_*
 * variable rustc sees, so a target dir of each task's own named there made
 * every crate a miss in every other task's build (0 hits in 946, 16 Nov 2026).
 */
export function cargoEnv(extra = {}) {
  const env = { ...extra, ...process.env };
  delete env.CARGO_TARGET_DIR;
  delete env.CARGO_BUILD_TARGET_DIR;
  return env;
}

/** Where `npm run test:app:build` puts the binary (see desktop/e2e/build.mjs). */
export const TARGET_DIR =
  process.env.YAP_E2E_TARGET_DIR || path.join(APP, 'src-tauri', 'target', 'e2e');
export const EXE = process.env.YAP_E2E_EXE || path.join(TARGET_DIR, 'debug', 'yap.exe');

/** Seeded into every test instance's config.json (a test can add to it). */
export const BASE_CONFIG = {
  hotkey: 'kb:135', // F24: nobody's keyboard has one
  autostart: false,
  updateChecksEnabled: false,
  bridgeEnabled: false, // off unless a spec needs it (mcp.spec.js)
  soundEnabled: false,
  startHidden: true, // no first-run onboarding pop-up (tests open it themselves)
  // Light, whatever this PC's Windows app mode (the theme's default is
  // System): screenshots and measured colours stay the same everywhere.
  // theme.spec.js covers Dark and System.
  theme: 'light',
  // A spoken question isn't cleaned up unless a spec asks (on by default:
  // clean-speech.spec.js turns them on), so a spec with a cleanup model
  // set up sees its questions' calls as before.
  chatCleanSpeech: false,
  agentsCleanSpeech: false,
  // A Code chat on the plan hands its requests to an agent through Chat's
  // model unless a spec asks otherwise (on by default: agents-direct.spec.js
  // turns it on), so the specs of that hand-off see it as before. Yap Lab's
  // test Yap starts from this too: its scripts say which they test.
  agentsDirect: false,
  // A new document is written straight away unless a spec asks for its
  // outline first (on by default: chat-outline.spec.js turns it on), so the
  // specs of Chat's documents see their turns as before. Yap Lab too: `set
  // '{"config":{"chatOutlineReview":true}}'` for the outline card.
  chatOutlineReview: false,
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

/** Whether process `pid` still runs. */
function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

function rmrf(dir) {
  // WebView2's processes can hold their profile for a moment after Yap exits.
  try {
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 });
  } catch (e) {
    console.warn(`e2e: couldn't remove ${dir}: ${e.message}`);
  }
}

/**
 * Copy the build into a run folder, waiting out a lock. A restart copies
 * into the folder the last instance just quit from, and Windows keeps a
 * deleted exe "delete pending" until the last handle on it (the exited
 * process's image, an antivirus scan) closes: a copy then gets EPERM (CI,
 * quick-note.spec.js's restart, 2026-11-03). Retried as rimraf does, for up
 * to 20 s; the same build already there is left as it is.
 */
async function copyExe(src, dest) {
  const same = (a, b) => a.size === b.size && Math.trunc(a.mtimeMs) === Math.trunc(b.mtimeMs);
  for (let waited = 0; ; waited += 250) {
    try {
      if (fs.existsSync(dest) && same(fs.statSync(src), fs.statSync(dest))) return;
      fs.copyFileSync(src, dest);
      return;
    } catch (e) {
      if (!['EPERM', 'EBUSY', 'EACCES'].includes(e.code) || waited >= 20_000) throw e;
      await sleep(250);
    }
  }
}

/** `rmrf` without blocking the runner while it retries a locked file. */
async function rmrfAsync(dir) {
  try {
    await fs.promises.rm(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 });
  } catch (e) {
    console.warn(`e2e: couldn't remove ${dir}: ${e.message}`);
  }
}

/**
 * The instance running in each run folder now. A restart
 * (`launchYap({ restart: true })`) runs in the last one's folder, from its
 * copy of the exe and with a WebView2 profile there: the older instance's
 * `stop()` must leave them be. (A test's `finally` stopping the first
 * instance after the second had started deleted the second's running exe
 * and profile: on CI, `rmSync`'s retries on the locked files froze the
 * runner for 5 minutes — notes-summary's Ask-thread test, 2026-10-26.)
 */
const LIVE = new Map();

/**
 * Start a fresh test instance.
 * @param {object} opts
 * @param {string} opts.name   run folder name (test-results/app/runs/<name>)
 * @param {object} [opts.config]  extra config.json fields
 * @param {Record<string,string>} [opts.env]  extra environment for the app
 * @param {Record<string,object>} [opts.data]  files to seed in Data/ before
 *   launch, e.g. `{ 'notes.json': {...} }` (written as JSON)
 * @param {string} [opts.runsDir]  where the run folder goes (the real-app
 *   robot tests keep theirs in test-results/real/runs)
 * @param {string} [opts.exe]  the build to run (copied into the run folder)
 * @param {(dataDir: string) => void} [opts.prepare]  last touches to Data/
 *   before launch (the robot's STT spec links a model in)
 * @param {boolean} [opts.restart]  start again on the run folder's Data/ as
 *   the last instance left it (after `kill()`: a crash), config included
 * @param {string[]} [opts.args]  the launch's arguments (a Markdown file to
 *   open, `md-file.spec.js`); started in the app's folder, so a relative
 *   path is from there
 */
export async function launchYap({ name, config = {}, env = {}, data = {}, runsDir = RUNS, exe = EXE, prepare, restart = false, args = [] }) {
  if (!fs.existsSync(exe)) {
    throw new Error(`No test build at ${exe} — run \`npm run test:app:build\` first.`);
  }
  const runDir = path.join(runsDir, name);
  if (restart) {
    // A fresh WebView2 profile (the last one's DevToolsActivePort is stale).
    rmrf(path.join(runDir, 'webview2'));
  } else {
    rmrf(runDir);
  }
  const appDir = path.join(runDir, 'app');
  const dataDir = path.join(appDir, 'Data');
  fs.mkdirSync(dataDir, { recursive: true });
  await copyExe(exe, path.join(appDir, 'yap.exe'));
  fs.writeFileSync(path.join(appDir, 'portable'), 'Yap Portable Mode');
  if (!restart) {
    fs.writeFileSync(
      path.join(dataDir, 'config.json'),
      JSON.stringify({ ...BASE_CONFIG, ...config }, null, 2)
    );
  }
  for (const [file, value] of Object.entries(data)) {
    fs.writeFileSync(path.join(dataDir, file), JSON.stringify(value, null, 2));
  }
  prepare?.(dataDir);

  const closedPort = await freePort(); // nothing listens here
  const webview2Dir = path.join(runDir, 'webview2');
  const bridgeFile = path.join(runDir, 'cli-bridge.json');
  const childEnv = Object.fromEntries(Object.entries(process.env).filter(([k]) => !SCRUB.test(k)));
  Object.assign(childEnv, {
    YAP_E2E: '1',
    YAP_AUTH_URL: `http://127.0.0.1:${closedPort}`,
    WEBVIEW2_USER_DATA_FOLDER: webview2Dir,
    // Port 0: WebView2 picks a free port and writes it to DevToolsActivePort
    // in the profile. (An elevated process ignores WEBVIEW2_* variables; CI
    // runners are elevated, so e2e.yml sets the same flag as a machine policy.)
    // Test windows sit behind the developer's apps (shell/window.rs
    // `behind_in_test_runs`): with occlusion tracking off they still draw.
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: WEBVIEW2_ARGS,
    NO_COLOR: '1',
    YAP_BRIDGE_FILE: bridgeFile,
    // A Yap that restarts itself (webview2-exit.spec.js) has no harness on
    // its stdin: it goes once this process does.
    YAP_E2E_HARNESS_PID: String(process.pid),
    YAP_MCP_CLIENT_ROOT: path.join(runDir, 'ai-apps'),
    ...env,
  });

  const logPath = path.join(runDir, 'stdout.log');
  const log = fs.openSync(logPath, 'w');
  const child = spawn(path.join(appDir, 'yap.exe'), args, {
    cwd: appDir,
    env: childEnv,
    // stdin stays open: closing it is how we ask the app to quit (shell/e2e.rs).
    stdio: ['pipe', log, log],
  });
  fs.closeSync(log);
  child.stdin.on('error', () => {}); // EPIPE if it exits before we close it
  let exited = null;
  child.on('exit', (code, signal) => (exited = { code, signal }));
  child.on('error', (e) => (exited = { error: e }));

  const app = new YapApp({ name, child, runDir, appDir, dataDir, logPath, bridgeFile: childEnv.YAP_BRIDGE_FILE });
  LIVE.set(runDir, app);
  try {
    app.browser = await connect(webview2Dir, () => exited, logPath);
    await assertTestMode(logPath, exe);
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
async function assertTestMode(logPath, exe) {
  // (45 s: a PC busy compiling took 16 s to say so, and the run failed.)
  const deadline = Date.now() + 45_000;
  while (Date.now() < deadline) {
    if (fs.readFileSync(logPath, 'utf8').includes('e2e: test mode on')) return;
    await sleep(200);
  }
  throw new Error(
    `${exe} didn't start in test mode. The suite needs the debug build from \`npm run test:app:build\`.`
  );
}

/** Wait for WebView2's DevTools port (from DevToolsActivePort) and connect. */
async function connect(webview2Dir, exited, logPath) {
  const portFile = path.join(webview2Dir, 'EBWebView', 'DevToolsActivePort');
  const deadline = Date.now() + 30_000;
  let last = 'no DevToolsActivePort yet';
  while (Date.now() < deadline) {
    if (exited()) {
      const tail = fs.readFileSync(logPath, 'utf8').split('\n').slice(-20).join('\n');
      throw new Error(`Yap exited during startup (${JSON.stringify(exited())}):\n${tail}`);
    }
    let port = '';
    try {
      port = fs.existsSync(portFile) ? fs.readFileSync(portFile, 'utf8').split('\n')[0].trim() : '';
    } catch (e) {
      // EBUSY: WebView2 is still writing the file. Try again next round.
      last = e;
    }
    if (port) {
      try {
        return await chromium.connectOverCDP(`http://127.0.0.1:${port}`, { timeout: 5_000 });
      } catch (e) {
        last = e;
      }
    }
    await sleep(250);
  }
  throw new Error(`Couldn't connect to Yap's webviews: ${last}\n${describeWebView2(webview2Dir)}`);
}

/** What WebView2 is doing with our profile (for a failed connect). */
function describeWebView2(webview2Dir) {
  const lines = [];
  try {
    const ps = `Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like '*${webview2Dir}*' -and $_.CommandLine -notlike '*--type=*' } | ForEach-Object { $_.ProcessId.ToString() + ': ' + $_.CommandLine }`;
    const out = execFileSync('powershell.exe', ['-NoProfile', '-Command', ps], { encoding: 'utf8' });
    lines.push(`WebView2 browser process: ${out.trim() || '(none)'}`);
  } catch (e) {
    lines.push(`(couldn't list WebView2 processes: ${e.message})`);
  }
  return lines.join('\n');
}

/** The webview's Tauri window label ("settings", "onboarding", "notepad", "overlay", "quick-note"). */
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
  /** The meeting notepad (docked beside a call while a meeting records). */
  get notepad() {
    return this.pages.notepad;
  }
  /** The quick note (notes/quick_note.rs: its shortcut, the tray, Notes). */
  get quickNote() {
    return this.pages['quick-note'];
  }

  /**
   * Find the webviews, start listening for errors, then reload each one so
   * its whole boot (which ran before we connected) happens under watch.
   */
  async attachPages() {
    await this.findPages();
    for (const [label, page] of Object.entries(this.pages)) {
      // The embedded frontend, not a dev server (which would be someone's Vite).
      if (!page.url().startsWith('http://tauri.localhost/')) {
        throw new Error(`The test build loads ${page.url()}, not its embedded frontend (${label})`);
      }
    }
    this.watchPages();
    await Promise.all(Object.values(this.pages).map((p) => p.reload({ waitUntil: 'load' })));
    await this.main.getByRole('navigation', { name: 'Main' }).waitFor();
  }

  /** Each window's webview, by its label, on this connection. */
  async findPages() {
    const context = this.browser.contexts()[0];
    const labels = ['settings', 'onboarding', 'notepad', 'overlay', 'quick-note'];
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      for (const page of context.pages()) {
        if (Object.values(this.pages).includes(page)) continue;
        const label = await labelOf(page);
        if (label) this.pages[label] = page;
      }
      if (labels.every((l) => this.pages[l])) break;
      await sleep(250);
    }
    for (const label of labels) {
      if (!this.pages[label]) throw new Error(`Yap's "${label}" webview never showed up`);
    }
  }

  /** Every webview's uncaught errors and console errors, kept for the fixtures. */
  watchPages() {
    for (const [label, page] of Object.entries(this.pages)) {
      page.on('pageerror', (err) => this.pageErrors.push({ label, message: err.stack || String(err) }));
      page.on('console', (msg) => {
        if (msg.type() === 'error') this.consoleErrors.push({ label, message: msg.text() });
      });
    }
  }

  /**
   * Connect to the webviews again, as at launch but without reloading
   * them. Playwright is done with a page whose renderer crashed ("Page
   * crashed") even once WebView2 has loaded it again
   * (never-blank.spec.js): a new connection finds it, and every other
   * window, afresh. The old connection stays open until `stop()`.
   *
   * `restarted`: Yap restarted itself (webview2-exit.spec.js): the process
   * this harness started is gone, and the new one — started by it, from the
   * run folder's copy, on the same stdout and WebView2 profile — is found by
   * its executable's path, so `stop()` and `kill()` end it too (it goes by
   * itself once this process does: `YAP_E2E_HARNESS_PID`).
   */
  async reattach({ restarted = false } = {}) {
    this.retired = [...(this.retired ?? []), this.browser];
    let exited = () => (this.child.exitCode !== null ? { code: this.child.exitCode } : null);
    if (restarted) {
      this.restartedPid = await this.findRestarted();
      exited = () => (alive(this.restartedPid) ? null : { restartedPid: this.restartedPid, gone: true });
    }
    this.browser = await connect(path.join(this.runDir, 'webview2'), exited, this.logPath);
    this.pages = {};
    await this.findPages();
    this.watchPages();
  }

  /** The Yap running from this run folder's copy that isn't the one this harness started. */
  async findRestarted() {
    const exe = path.join(this.appDir, 'yap.exe');
    const ps = `Get-CimInstance Win32_Process -Filter "Name='yap.exe'" | Where-Object { $_.ExecutablePath -eq '${exe.replace(/'/g, "''")}' } | ForEach-Object { $_.ProcessId }`;
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      const pids = execFileSync('powershell.exe', ['-NoProfile', '-Command', ps], { encoding: 'utf8' })
        .split(/\s+/)
        .map(Number)
        .filter((pid) => pid && pid !== this.child.pid);
      if (pids.length) return pids[0];
      await sleep(250);
    }
    throw new Error(`No restarted Yap running from ${exe}`);
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

  /**
   * A crash: this instance's process tree killed outright (no exit hooks),
   * its Data/ left as it was for `launchYap({ restart: true })`. Only ever
   * the process this harness started, in its own portable run folder.
   */
  async kill() {
    const marker = path.join(this.appDir, 'portable');
    if (!fs.existsSync(marker) || fs.readFileSync(marker, 'utf8') !== 'Yap Portable Mode' || !this.dataDir.startsWith(this.runDir)) {
      throw new Error(`Not killing ${this.child.pid}: ${this.appDir} isn't a portable test copy`);
    }
    await this.browser?.close().catch(() => {});
    const { child } = this;
    if (child.exitCode === null && child.signalCode === null) {
      const gone = new Promise((r) => child.once('exit', r));
      execFileSync('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
      await Promise.race([gone, sleep(10_000)]);
    }
    if (this.restartedPid && alive(this.restartedPid)) {
      execFileSync('taskkill', ['/PID', String(this.restartedPid), '/T', '/F'], { stdio: 'ignore' });
    }
    await sleep(500);
  }

  /**
   * Quit (by closing stdin), falling back to killing the process tree. Once
   * only; and an instance a restart replaced leaves the run folder's files
   * to the newer one (`LIVE`).
   */
  async stop() {
    if (this.stopped) return;
    this.stopped = true;
    // A Yap that restarted itself has no harness on its stdin: quit from
    // its tray, as a person would (ended below if it doesn't).
    if (this.restartedPid) {
      this.main?.evaluate(() => window.__TAURI_INTERNALS__.invoke('e2e_tray', { click: 'quit' })).catch(() => {});
      await sleep(300);
    }
    for (const old of this.retired ?? []) await old.close().catch(() => {});
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
    if (this.restartedPid) {
      const deadline = Date.now() + 10_000;
      while (alive(this.restartedPid) && Date.now() < deadline) await sleep(200);
      if (alive(this.restartedPid)) {
        try {
          execFileSync('taskkill', ['/PID', String(this.restartedPid), '/T', '/F'], { stdio: 'ignore' });
        } catch {
          /* already gone */
        }
      }
    }
    // Keep config, data and logs for the report; drop the big stuff —
    // unless a newer instance runs from them.
    if (LIVE.get(this.runDir) !== this) return;
    LIVE.delete(this.runDir);
    await sleep(500);
    await rmrfAsync(path.join(this.appDir, 'yap.exe'));
    await rmrfAsync(path.join(this.runDir, 'webview2'));
  }
}
