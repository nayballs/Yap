// The windows the robot dictates into, all opened by the harness and all
// titled with the run's nonce (src-tauri/src/shell/realtest.rs only lets Yap
// paste, press keys or read in such windows, or in processes the harness
// started): the test pad, Windows Terminal (a PowerShell prompt, and a
// stand-in for Claude Code), an Edge app window showing a "WhatsApp" page,
// and Notepad. Each one's `close()` takes down exactly what was opened.
//
// Windows, PowerShell, Notepad and Edge are only ever started here with
// their own throwaway state (a temp folder, a fresh Edge profile), and
// nothing reads a window the harness didn't open (`yaprobot find` prints
// only titles with the nonce).
import { spawn, execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

export const REPO = fileURLToPath(new URL('../..', import.meta.url));
export const OUT = path.join(REPO, 'test-results', 'real');
export const RUNS = path.join(OUT, 'runs');
export const CACHE = path.join(OUT, 'cache');
const BIN = path.join(OUT, 'bin');
// The helper's sources: its commands, and the hands-off guard's (guard.js).
const HELPER_SRCS = ['./yaprobot.cs', './inputwatch.cs'].map((f) => fileURLToPath(new URL(f, import.meta.url)));
export const HELPER = path.join(BIN, 'yaprobot.exe');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** A fresh nonce for a run's window titles (≥ 12 characters, as Yap wants). */
export function makeNonce() {
  return `yaprobot-${crypto.randomBytes(6).toString('hex')}`;
}

/** Compile yaprobot.exe with the .NET Framework's C# compiler (every Windows has one). */
export function compileHelper() {
  const fresh =
    fs.existsSync(HELPER) &&
    HELPER_SRCS.every((src) => fs.statSync(HELPER).mtimeMs >= fs.statSync(src).mtimeMs);
  if (fresh) return HELPER;
  const fw = path.join(process.env.WINDIR || 'C:\\Windows', 'Microsoft.NET', 'Framework64', 'v4.0.30319');
  const csc = path.join(fw, 'csc.exe');
  if (!fs.existsSync(csc)) throw new Error(`No C# compiler at ${csc} (the .NET Framework 4)`);
  fs.mkdirSync(BIN, { recursive: true });
  const wpf = (dll) => `-r:${path.join(fw, 'WPF', dll)}`;
  try {
    execFileSync(
      csc,
      [
        '-nologo',
        '-target:winexe',
        `-out:${HELPER}`,
        '-r:System.Windows.Forms.dll',
        '-r:System.Drawing.dll',
        '-r:System.Web.Extensions.dll',
        wpf('UIAutomationClient.dll'),
        wpf('UIAutomationTypes.dll'),
        wpf('WindowsBase.dll'),
        ...HELPER_SRCS,
      ],
      { encoding: 'utf8', windowsHide: true },
    );
  } catch (e) {
    throw new Error(`Couldn't compile the robot's helper:\n${e.stdout || ''}${e.stderr || e.message}`);
  }
  return HELPER;
}

/** Run a one-shot helper command; its JSON answer. */
export function helper(...args) {
  let out;
  try {
    out = execFileSync(HELPER, args.map(String), { encoding: 'utf8', windowsHide: true, timeout: 30_000 });
  } catch (e) {
    out = e.stdout || '';
    if (!out.trim()) throw new Error(`yaprobot ${args.join(' ')} failed: ${e.message}`);
  }
  const r = JSON.parse(out.trim().split('\n').pop());
  if (r && r.error && !('ok' in r)) throw new Error(`yaprobot ${args[0]}: ${r.error}`);
  return r;
}

/** The top-level window in front (its handle). */
export function foreground() {
  return helper('foreground').hwnd;
}

/** Bring a window to the front (throws if Windows won't). */
export async function focus(hwnd) {
  for (let i = 0; i < 3; i++) {
    const r = helper('focus', hwnd);
    if (r.ok) return;
    await sleep(200);
  }
  throw new Error(`Couldn't bring window ${hwnd} to the front`);
}

/** The visible windows titled with the nonce. */
export function findWindows(nonce) {
  return helper('find', nonce);
}

/** Wait for a window titled with `nonce` that `match` accepts. */
export async function waitForWindow(nonce, match = () => true, { timeout = 20_000, what = 'window' } = {}) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const w = findWindows(nonce).find(match);
    if (w) return w;
    await sleep(200);
  }
  throw new Error(`The ${what} never showed up (no window titled with the run's nonce)`);
}

/** A window's text over UI Automation. */
export function windowText(hwnd) {
  const r = helper('text', hwnd);
  if (r.error) throw new Error(`Couldn't read the window's text: ${r.error}`);
  return r.text;
}

function killTree(pid) {
  try {
    execFileSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true });
  } catch {
    /* already gone */
  }
}

/**
 * Processes named one of `names` whose command line has `marker` (a temp
 * path of the harness's own). Never Windows Terminal itself: it may be
 * hosting your own terminal windows too.
 */
function processesWith(marker, names) {
  const list = names.map((n) => `'${n}'`).join(',');
  const ps = `Get-CimInstance Win32_Process | Where-Object { @(${list}) -contains $_.Name -and $_.CommandLine -and $_.CommandLine.Contains('${marker.replace(/'/g, "''")}') } | ForEach-Object { $_.ProcessId }`;
  try {
    const out = execFileSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command', ps], {
      encoding: 'utf8',
      windowsHide: true,
    });
    return out.split(/\s+/).map(Number).filter((n) => n > 0 && n !== process.pid);
  } catch {
    return [];
  }
}

// ---- The test pad ----

/**
 * The test pad (yaprobot.exe testpad): a multi-line text box ("box"), a
 * rich text box ("rich") and a password box ("password"), titled "Yap robot
 * test pad <label>". `call({cmd, …})` → its answer: `set {field, text, caret?}`,
 * `select {field, start, length}`, `focus {field}`, `get {field}` →
 * `{text, caret}`, `pastes` → what the clipboard held at each Ctrl+V since
 * the last ask (format names only).
 */
export async function startTestPad(label) {
  compileHelper();
  const child = spawn(HELPER, ['testpad', label],{ stdio: ['pipe', 'pipe', 'inherit'], windowsHide: false });
  child.stdin.on('error', () => {});
  const lines = readline.createInterface({ input: child.stdout });
  const waiting = [];
  lines.on('line', (line) => waiting.shift()?.(line));
  let exited = false;
  child.on('exit', () => {
    exited = true;
    for (const w of waiting.splice(0)) w(JSON.stringify({ error: 'the test pad exited' }));
  });
  const call = (req) =>
    new Promise((resolve, reject) => {
      if (exited) return reject(new Error('the test pad has exited'));
      waiting.push((line) => {
        const r = JSON.parse(line);
        if (r.error) reject(new Error(`test pad: ${r.error}`));
        else resolve(r);
      });
      child.stdin.write(`${JSON.stringify(req)}\n`);
    });
  const { hwnd, pid } = await call({ cmd: 'hwnd' });
  // The field a dictation goes into: the last one set or focused.
  let current = 'box';
  return {
    hwnd,
    pid,
    /** In the pad's title: what the helper checks before sending it keys. */
    label,
    call,
    /** Bring the pad to the front with `field` (default: the last one used) focused. */
    async focus(field = current) {
      current = field;
      await focus(hwnd);
      await call({ cmd: 'focus', field });
    },
    /** Put `text` in `field` with the caret at `caret` (default: the end). */
    async set(field, text, caret) {
      current = field;
      await call({ cmd: 'set', field, text, caret });
    },
    async text(field = 'box') {
      return (await call({ cmd: 'get', field })).text;
    },
    async pastes() {
      return (await call({ cmd: 'pastes' })).pastes;
    },
    /** Select `length` characters of `field` from `start`. */
    async select(field, start, length) {
      current = field;
      await call({ cmd: 'select', field, start, length });
    },
    /** Hold the clipboard open for `ms` (a busy clipboard manager). */
    async holdClipboard(ms) {
      return (await call({ cmd: 'holdClipboard', ms })).held;
    },
    /** Once the pad's own next copy has landed, hold the clipboard `ms`. */
    async holdAfterCopy(ms) {
      return (await call({ cmd: 'holdAfterCopy', ms })).listening;
    },
    async clipboardHeld() {
      return (await call({ cmd: 'clipboardHeld' })).held;
    },
    /** The slow box's Ctrl+V reads the clipboard `ms` later (never when `ms` < 0). */
    async slowPaste(ms) {
      await call({ cmd: 'slowPaste', ms });
    },
    /** Play a WAV on a loop from the pad's own process (its own audio session). */
    async play(file) {
      await call({ cmd: 'play', path: file });
    },
    async stopPlaying() {
      await call({ cmd: 'stop' });
    },
    async close() {
      child.stdin.end();
      const done = await Promise.race([new Promise((r) => child.once('exit', () => r(true))), sleep(5_000)]);
      if (!done && !exited) killTree(child.pid);
    },
  };
}

// ---- Windows Terminal ----

/** wt.exe (the App Execution Alias in WindowsApps, or on PATH). */
const WINDOWS_APPS = path.join(process.env.LOCALAPPDATA || '', 'Microsoft', 'WindowsApps');

/** wt.exe, an App Execution Alias (a reparse point `fs.existsSync` can't stat). */
function wtExe() {
  return path.join(WINDOWS_APPS, 'wt.exe');
}

export function hasWindowsTerminal() {
  try {
    return fs.readdirSync(WINDOWS_APPS).some((f) => f.toLowerCase() === 'wt.exe');
  } catch {
    return false;
  }
}

/**
 * A new Windows Terminal window (`-w new`, so never a tab in yours) titled
 * `title` (which must contain the nonce; the shell can't change it) and
 * running `command` in a temp folder. Windows Terminal may host it in the
 * same process as your own terminal windows, which is why Yap's guard
 * knows it by its title, never its process. `close()` asks just this
 * window to close, then ends anything left of what it ran (found by
 * `marker` in its command line). (wt.exe reads ";" as its own separator:
 * keep it out of `command`.)
 */
export async function startTerminal({ nonce, title, cwd, command, marker }) {
  fs.mkdirSync(cwd, { recursive: true });
  const before = new Set(findWindows(nonce).map((w) => w.hwnd));
  const child = spawn(wtExe(), ['-w', 'new', '--title', title, '--suppressApplicationTitle', '-d', cwd, ...command], {
    stdio: 'ignore',
    detached: true,
    windowsHide: false,
  });
  child.unref();
  const ours = (w) => !before.has(w.hwnd) && w.cls === 'CASCADIA_HOSTING_WINDOW_CLASS' && w.title === title;
  const cleanup = async (hwnd) => {
    for (const w of hwnd ? [{ hwnd }] : findWindows(nonce).filter(ours)) helper('close', w.hwnd, nonce);
    await sleep(300);
    for (const pid of processesWith(marker, ['powershell.exe', 'node.exe'])) killTree(pid);
  };
  let win;
  try {
    win = await waitForWindow(nonce, ours, { what: 'Windows Terminal window' });
  } catch (e) {
    await cleanup(null);
    throw e;
  }
  return {
    hwnd: win.hwnd,
    title,
    text: () => windowText(win.hwnd),
    focus: () => focus(win.hwnd),
    close: () => cleanup(win.hwnd),
  };
}

/** A PowerShell prompt in Windows Terminal, in `cwd`. */
export function startPowerShell({ nonce, cwd }) {
  // A short prompt ("PS> "), so a line never wraps; the nonce in a comment
  // makes the encoded command this shell's own (close() finds it by that).
  const script = `function global:prompt { 'PS> ' } # ${nonce} ${crypto.randomBytes(4).toString('hex')}`;
  const encoded = Buffer.from(script, 'utf16le').toString('base64');
  return startTerminal({
    nonce,
    title: `Yap robot shell ${nonce}`,
    cwd,
    command: ['powershell.exe', '-NoLogo', '-NoProfile', '-NoExit', '-EncodedCommand', encoded],
    marker: encoded,
  });
}

/**
 * A stand-in for Claude Code in Windows Terminal: the title Claude Code
 * shows when idle ("✳ <session>"), and a small program that, as Claude
 * Code does, turns on bracketed paste and reads the keyboard raw. It runs
 * nothing: it logs what arrives (`received()`) and echoes it.
 */
export async function startFakeClaudeCode({ nonce, cwd, title = `✳ ${nonce}`, banner = '' }) {
  fs.mkdirSync(cwd, { recursive: true });
  const log = path.join(cwd, 'received.jsonl');
  fs.writeFileSync(log, '');
  // `banner`: what the CLI showed before (a file it read), on its screen.
  if (banner) fs.writeFileSync(path.join(cwd, 'banner.txt'), banner);
  const script = fileURLToPath(new URL('./fake-cli.mjs', import.meta.url));
  const term = await startTerminal({
    nonce,
    title,
    cwd,
    command: [process.execPath, script, log],
    marker: log,
  });
  await cliReady(log);
  return { ...term, received: () => cliReceived(log) };
}

/** Once the stand-in CLI has turned bracketed paste on. */
async function cliReady(log) {
  const deadline = Date.now() + 15_000;
  while (!fs.readFileSync(log, 'utf8').includes('"ready"') && Date.now() < deadline) await sleep(100);
}

/** Everything typed or pasted into the stand-in CLI so far, as one string. */
function cliReceived(log) {
  return fs
    .readFileSync(log, 'utf8')
    .split('\n')
    .filter(Boolean)
    .map((l) => JSON.parse(l))
    .filter((e) => e.data !== undefined)
    .map((e) => e.data)
    .join('');
}

/**
 * The stand-in CLI in a classic console window (conhost, whatever this
 * PC's default terminal is), titled `title` (with the nonce: "✳ <nonce>"
 * is Claude Code's idle title). `close()` closes it and ends what it ran.
 */
export async function startConsoleCli({ nonce, cwd, title = `✳ ${nonce}` }) {
  // Now and then (3 of about a dozen launches on 2026-10-25, each the first
  // console window after another test's windows) conhost starts nothing
  // at all: no window, the program never runs. So once more.
  try {
    return await launchConsoleCli({ nonce, cwd, title, timeout: 20_000 });
  } catch (e) {
    if (!/never showed up.*didn't start/.test(e.message)) throw e;
    console.warn(`robot: ${e.message}; launching the console window again`);
    return launchConsoleCli({ nonce, cwd, title, timeout: 40_000 });
  }
}

async function launchConsoleCli({ nonce, cwd, title, timeout }) {
  fs.mkdirSync(cwd, { recursive: true });
  const log = path.join(cwd, 'received.jsonl');
  fs.writeFileSync(log, '');
  const script = fileURLToPath(new URL('./fake-cli.mjs', import.meta.url));
  const before = new Set(findWindows(nonce).map((w) => w.hwnd));
  const child = spawn(path.join(process.env.WINDIR || 'C:\\Windows', 'System32', 'conhost.exe'), [process.execPath, script, log, title], {
    cwd,
    stdio: 'ignore',
    detached: true,
    windowsHide: false,
  });
  child.unref();
  const cleanup = async (hwnd) => {
    if (hwnd && findWindows(nonce).some((w) => w.hwnd === hwnd)) helper('close', hwnd, nonce);
    await sleep(300);
    for (const pid of processesWith(log, ['node.exe'])) killTree(pid);
    killTree(child.pid);
  };
  let win;
  try {
    win = await waitForWindow(nonce, (w) => !before.has(w.hwnd) && w.cls === 'ConsoleWindowClass' && w.title === title, {
      what: 'console window',
      timeout,
    });
    await cliReady(log);
  } catch (e) {
    // What did come up (the run's own windows only): a delegated terminal?
    const seen = findWindows(nonce).filter((w) => !before.has(w.hwnd)).map((w) => `${w.cls} "${w.title}"`);
    const ready = fs.existsSync(log) && fs.readFileSync(log, 'utf8').includes('"ready"');
    await cleanup(win?.hwnd);
    throw new Error(`${e.message}; new windows: ${seen.join(', ') || 'none'}; the program ${ready ? 'started' : "didn't start"}`);
  }
  return {
    hwnd: win.hwnd,
    title,
    focus: () => focus(win.hwnd),
    received: () => cliReceived(log),
    close: () => cleanup(win.hwnd),
  };
}

/**
 * A careless clipboard manager's stand-in (`yaprobot clipreader`), for
 * `ms`: at each clipboard change, `delay` ms later, it opens the clipboard
 * with a window of its own process and reads the text, whatever the
 * privacy formats say. `result()` (once it's done): `{ updates, reads:
 * [{ open, got, len }] }` — never the text.
 */
export async function startClipReader({ ms = 6_000, delay = 0 } = {}) {
  compileHelper();
  // A program of its own name: the test pad is yaprobot.exe too, and Yap
  // takes a reader of the target's own program for the target.
  const exe = path.join(path.dirname(HELPER), 'yap-clip-reader.exe');
  if (!fs.existsSync(exe) || fs.statSync(exe).mtimeMs < fs.statSync(HELPER).mtimeMs) fs.copyFileSync(HELPER, exe);
  const child = spawn(exe, ['clipreader', String(ms), String(delay)], { stdio: ['ignore', 'pipe', 'inherit'], windowsHide: true });
  const lines = readline.createInterface({ input: child.stdout });
  const seen = [];
  let ready;
  const listening = new Promise((r) => (ready = r));
  lines.on('line', (l) => {
    seen.push(l);
    if (/"listening"|"error"/.test(l)) ready();
  });
  const exited = new Promise((r) => child.once('exit', r));
  await Promise.race([listening, sleep(10_000)]);
  if (seen.some((l) => /"error"/.test(l))) throw new Error(`The clipboard reader couldn't start: ${seen.join(' ')}`);
  return {
    async result() {
      await Promise.race([exited, sleep(ms + 10_000)]);
      const last = seen.filter((l) => /"updates"/.test(l)).pop();
      return last ? JSON.parse(last) : null;
    },
    stop() {
      try {
        child.kill();
      } catch {
        /* gone */
      }
    },
  };
}

/** Which modifier keys Windows says are down ("lalt", "lshift"…). */
export function keyState() {
  return helper('keystate').down;
}

/**
 * A modifier key pressed (`down`, only while the test's window is in
 * front: `owner` is the nonce or "pid:<n>") or let go (`up`), as an
 * injected key: down as far as Windows knows, no finger on it.
 */
export function modKey(hwnd, owner, key, how) {
  const r = helper('modkey', hwnd, owner, key, how);
  if (!r.ok) throw new Error(`Couldn't press ${key} ${how}: ${r.error}`);
}

// ---- Edge: a web page's text box, and a chat app ----

export function edgeExe() {
  const candidates = [
    path.join(process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)', 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
    path.join(process.env.ProgramFiles || 'C:\\Program Files', 'Microsoft', 'Edge', 'Application', 'msedge.exe'),
  ];
  return candidates.find((p) => fs.existsSync(p)) ?? null;
}

/**
 * An Edge app window (a fresh, throwaway profile: none of your Edge's
 * windows, tabs or sign-ins) on a local page titled `title` (which has the
 * nonce) with one text box that has the focus. `value()` reads the box and
 * `set(text, caret?)` fills it, over the DevTools protocol. `note`: a
 * paragraph above the box; `beside`: `[before, after]`, page text on the
 * box's own line, either side of it (an inline box); `editable`: the box is
 * a contenteditable (a paragraph, as WhatsApp Web's and Gmail's are), not a
 * textarea; `body`: the page's own markup instead (with an element
 * `#box` to focus). `evaluate(fn, arg)` runs `fn` in the page.
 */
export async function startPage({ nonce, dir, title, note = '', beside = null, editable = false, attrs = 'aria-label="Message"', body = null }) {
  const exe = edgeExe();
  if (!exe) throw new Error('Microsoft Edge not found');
  fs.mkdirSync(dir, { recursive: true });
  const page = path.join(dir, 'page.html');
  const before = new Set(findWindows(nonce).map((w) => w.hwnd));
  const box =
    body ??
    (editable
      ? `<div id="box" contenteditable="true" role="textbox" aria-multiline="true" ${attrs}><p><br></p></div>`
      : `<textarea id="box" ${attrs} autofocus></textarea>`);
  fs.writeFileSync(
    page,
    `<!doctype html><meta charset="utf-8"><title>${title}</title>
<style>body{font:15px Segoe UI,sans-serif;margin:16px;background:#f4f1ea}textarea,#box{width:100%;height:120px;font:inherit}#box[contenteditable]{background:#fff;border:1px solid #888;box-sizing:border-box;padding:4px}#box p{margin:0}.beside textarea{display:inline-block;width:50%;height:60px;vertical-align:middle}</style>
${note ? `<p>${note}</p>` : ''}
${beside ? `<div class="beside"><span>${beside[0]}</span> ${box} <span>${beside[1]}</span></div>` : box}
<script>document.getElementById('box').focus();</script>`,
  );
  const profile = path.join(dir, 'edge-profile');
  const child = spawn(
    exe,
    [
      `--user-data-dir=${profile}`,
      '--no-first-run',
      '--no-default-browser-check',
      '--disable-sync',
      '--remote-debugging-port=0',
      '--window-size=640,420',
      '--window-position=120,140',
      `--app=file:///${page.replace(/\\/g, '/')}`,
    ],
    { stdio: 'ignore', detached: true, windowsHide: false },
  );
  child.unref();
  // This page's window: titled `title`, and not one opened before it.
  const win = await waitForWindow(
    nonce,
    (w) => w.cls.startsWith('Chrome_WidgetWin') && w.title.startsWith(title) && !before.has(w.hwnd),
    { what: 'Edge window' },
  );
  // Its DevTools port, from the profile.
  const portFile = path.join(profile, 'DevToolsActivePort');
  const deadline = Date.now() + 15_000;
  while (!fs.existsSync(portFile) && Date.now() < deadline) await sleep(100);
  const port = fs.readFileSync(portFile, 'utf8').split('\n')[0].trim();
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const tab = browser
    .contexts()
    .flatMap((c) => c.pages())
    .find((p) => p.url().startsWith('file:'));
  if (!tab) throw new Error("Couldn't find the page over CDP");
  // Chromium builds its accessibility tree (what Yap reads) only once a UI
  // Automation client asks; a browser in use usually has, so ask once now
  // and let it settle, or the first dictation would find nothing to read.
  try {
    windowText(win.hwnd);
  } catch {
    /* nothing to read yet is fine */
  }
  await sleep(500);
  return {
    hwnd: win.hwnd,
    pid: win.pid,
    focus: () => focus(win.hwnd),
    evaluate: (fn, arg) => tab.evaluate(fn, arg),
    /**
     * The box's text (a contenteditable's as it reads: no final line break,
     * the no-break spaces editing puts in as spaces).
     */
    value: () =>
      tab.evaluate(() => {
        const box = document.getElementById('box');
        return 'value' in box ? box.value : box.innerText.replace(/\n+$/, '').replace(/ /g, ' ');
      }),
    /**
     * Fill the box: the caret at `caret` (default: the end), or `caret`…`end`
     * selected. Then a moment for Chromium's accessibility tree (what Yap
     * reads) to catch up with a value set from script — typing gets there
     * as it goes.
     */
    async set(text, caret, end) {
      await tab.evaluate(
        ([t, c, e]) => {
          const box = document.getElementById('box');
          const at = c ?? t.length;
          box.focus();
          if ('value' in box) {
            box.value = t;
            box.setSelectionRange(at, e ?? at);
            return;
          }
          // A contenteditable: one paragraph, empty as an editor leaves it.
          box.innerHTML = '<p><br></p>';
          const p = box.firstChild;
          if (t) p.textContent = t;
          const node = t ? p.firstChild : p;
          const range = document.createRange();
          range.setStart(node, t ? at : 0);
          range.setEnd(node, t ? (e ?? at) : 0);
          const selection = getSelection();
          selection.removeAllRanges();
          selection.addRange(range);
        },
        [text, caret ?? null, end ?? null],
      );
      await sleep(400);
    },
    async close() {
      await browser.close().catch(() => {});
      // Every process of this profile (and only this profile).
      for (const pid of processesWith(profile, ['msedge.exe'])) killTree(pid);
    },
  };
}

/**
 * An Edge app window (a fresh, throwaway profile) playing `wav` on a loop,
 * titled "Yap robot audio - <nonce>", with that title in Windows' media
 * controls too (`navigator.mediaSession`): the only media Yap may pause in
 * robot mode (shell/realtest.rs `media_permits`). `state()` reads the page's
 * player over the DevTools protocol: `{ paused, time }`.
 */
export async function startMediaPage({ nonce, dir, wav }) {
  const exe = edgeExe();
  if (!exe) throw new Error('Microsoft Edge not found');
  fs.mkdirSync(dir, { recursive: true });
  const title = `Yap robot audio - ${nonce}`;
  fs.copyFileSync(wav, path.join(dir, 'tone.wav'));
  const page = path.join(dir, 'audio.html');
  fs.writeFileSync(
    page,
    `<!doctype html><meta charset="utf-8"><title>${title}</title>
<style>body{font:15px Segoe UI,sans-serif;margin:16px;background:#f4f1ea}</style>
<p>Yap robot: a quiet tone on a loop.</p>
<audio id="a" src="tone.wav" loop></audio>
<script>
const a = document.getElementById('a');
if ('mediaSession' in navigator) {
  navigator.mediaSession.metadata = new MediaMetadata({ title: ${JSON.stringify(title)}, artist: 'Yap robot' });
}
a.play().catch((e) => (document.title = 'blocked ' + e.message));
</script>`,
  );
  const before = new Set(findWindows(nonce).map((w) => w.hwnd));
  const profile = path.join(dir, 'edge-profile');
  const child = spawn(
    exe,
    [
      `--user-data-dir=${profile}`,
      '--no-first-run',
      '--no-default-browser-check',
      '--disable-sync',
      '--autoplay-policy=no-user-gesture-required',
      '--remote-debugging-port=0',
      '--window-size=520,240',
      '--window-position=700,140',
      `--app=file:///${page.replace(/\\/g, '/')}`,
    ],
    { stdio: 'ignore', detached: true, windowsHide: false },
  );
  child.unref();
  const win = await waitForWindow(
    nonce,
    (w) => w.cls.startsWith('Chrome_WidgetWin') && w.title.startsWith(title) && !before.has(w.hwnd),
    { what: 'Edge audio window' },
  );
  const portFile = path.join(profile, 'DevToolsActivePort');
  const deadline = Date.now() + 15_000;
  while (!fs.existsSync(portFile) && Date.now() < deadline) await sleep(100);
  const port = fs.readFileSync(portFile, 'utf8').split('\n')[0].trim();
  const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const tab = browser
    .contexts()
    .flatMap((c) => c.pages())
    .find((p) => p.url().startsWith('file:'));
  if (!tab) throw new Error("Couldn't find the audio page over CDP");
  return {
    hwnd: win.hwnd,
    pid: win.pid,
    title,
    state: () => tab.evaluate(() => ({ paused: document.getElementById('a').paused, time: document.getElementById('a').currentTime })),
    async close() {
      await browser.close().catch(() => {});
      for (const pid of processesWith(profile, ['msedge.exe'])) killTree(pid);
    },
  };
}

/**
 * A web page's text box ("Yap robot editor - <nonce>": an ordinary app to
 * Yap); `note`: text on the page above the box (none by default); `beside`:
 * page text either side of it, on its line.
 */
export function startEditorPage({ nonce, dir, note, beside }) {
  return startPage({ nonce, dir, title: `Yap robot editor - ${nonce}`, note, beside });
}

/**
 * A page of text to read ("Yap robot reading - <nonce>"): `paragraphs` as
 * the page's own text (not a text box), in an element that has the focus
 * (`#box`, focusable): `selectText(i?)` selects paragraph `i` (or them all)
 * as a mouse would, over the DevTools protocol.
 */
export async function startReadingPage({ nonce, dir, paragraphs }) {
  const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');
  const body = `<div id="box" tabindex="-1">${paragraphs.map((p) => `<p>${esc(p)}</p>`).join('')}</div>`;
  const page = await startPage({ nonce, dir, title: `Yap robot reading - ${nonce}`, body });
  return {
    ...page,
    async selectText(i) {
      await page.evaluate((n) => {
        const box = document.getElementById('box');
        const ps = box.querySelectorAll('p');
        const range = document.createRange();
        if (n == null) range.selectNodeContents(box);
        else range.selectNodeContents(ps[n]);
        const sel = getSelection();
        sel.removeAllRanges();
        sel.addRange(range);
      }, i ?? null);
      await sleep(400);
    },
  };
}

/**
 * A page titled "WhatsApp - <nonce>": a chat app to Yap (nothing is sent
 * anywhere); `editable`: its message box a contenteditable, as WhatsApp
 * Web's is; `attrs`: the box's attributes (default: its aria-label).
 */
export function startChatPage({ nonce, dir, note, editable, attrs = 'aria-label="Type a message"' }) {
  return startPage({ nonce, dir, title: `WhatsApp - ${nonce}`, note, editable, attrs });
}

/**
 * A stand-in for VS Code's integrated terminal, in an Edge app window that
 * the robot's Yap counts as an editor (`YAP_REALTEST_EDITORS=msedge`, so
 * `app_kind` looks at what has the focus): xterm.js's input — a hidden
 * textarea of class "xterm-helper-textarea", labelled as VS Code labels it
 * ("Terminal 1, <terminalTitle>" and its help lines) — taking a paste and
 * Enter as xterm.js does and showing what arrived (`received()`: it runs
 * nothing). Beside it a textarea standing in for the editor itself
 * (Monaco's input): `focusEditor()` / `focusTerminal()`.
 */
export async function startEditorTerminal({ nonce, dir, terminalTitle = 'pwsh' }) {
  const help =
    'Run the command: Toggle Screen Reader Accessibility Mode for an optimized screen reader experience&#10;Use Alt+F1 for terminal accessibility help';
  const xtermInput =
    'position:absolute;opacity:0;left:-9999em;top:0;width:0;height:0;z-index:-5;white-space:nowrap;overflow:hidden;resize:none';
  const body = `<p>Yap robot: a stand-in for VS Code's terminal. It runs nothing; it shows what arrives.</p>
<textarea id="editor" class="inputarea monaco-mouse-cursor-text" aria-label="Editor content;Press Alt+F1 for Accessibility Options." style="height:50px"></textarea>
<div class="xterm" style="position:relative"><pre id="screen" style="background:#1e1e1e;color:#ddd;min-height:80px;padding:6px">$ </pre>
<textarea id="box" class="xterm-helper-textarea" aria-label="Terminal 1, ${terminalTitle}&#10;${help}" aria-multiline="false" autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck="false" tabindex="0" style="${xtermInput}"></textarea></div>
<script>
window.received = [];
const input = document.getElementById('box');
const screen = document.getElementById('screen');
const show = (s) => { window.received.push(s); screen.textContent += s.replace(/\\r/g, '⏎\\n$ '); };
input.addEventListener('paste', (e) => { e.preventDefault(); show(e.clipboardData.getData('text/plain')); });
input.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); show('\\r'); } });
input.addEventListener('input', () => { if (input.value) { show(input.value); input.value = ''; } });
</script>`;
  const page = await startPage({ nonce, dir, title: `Yap robot editor terminal - ${nonce}`, body });
  return {
    ...page,
    received: () => page.evaluate(() => window.received.join('')),
    focusEditor: () => page.evaluate(() => document.getElementById('editor').focus()),
    focusTerminal: () => page.evaluate(() => document.getElementById('box').focus()),
    editorText: () => page.evaluate(() => document.getElementById('editor').value),
  };
}

// ---- Git Bash (mintty) ----

/**
 * Git for Windows' mintty (`YAP_REAL_MINTTY` to name another), or null:
 * next to the `git` on PATH (…\Git\cmd\git.exe → …\Git\usr\bin), else the
 * usual install folders.
 */
export function minttyExe() {
  if (process.env.YAP_REAL_MINTTY) return fs.existsSync(process.env.YAP_REAL_MINTTY) ? process.env.YAP_REAL_MINTTY : null;
  const candidates = [];
  try {
    const out = execFileSync('where.exe', ['git'], { encoding: 'utf8', windowsHide: true });
    for (const git of out.split(/\r?\n/).filter(Boolean)) {
      candidates.push(path.join(path.dirname(path.dirname(git.trim())), 'usr', 'bin', 'mintty.exe'));
    }
  } catch {
    /* no git on PATH */
  }
  for (const base of [process.env.ProgramFiles, process.env['ProgramFiles(x86)'], path.join(process.env.LOCALAPPDATA || '', 'Programs')]) {
    if (base) candidates.push(path.join(base, 'Git', 'usr', 'bin', 'mintty.exe'));
  }
  return candidates.find((p) => fs.existsSync(p)) ?? null;
}

/**
 * Git Bash's terminal, mintty, titled with the nonce, running a reader
 * that runs nothing: bash puts the terminal in raw mode (no line editing,
 * no echo; Ctrl+C is a byte like any other) and `cat` writes every byte
 * that arrives to a log in `cwd` (`received()`). mintty's own settings
 * file may be yours: its two settings that matter are set on the command
 * line (Shift+Insert pastes — `ClipShortcuts`, its default —, and closing
 * never asks). `close()` closes the window, then ends what's left of it.
 */
export async function startMintty({ nonce, cwd }) {
  const exe = minttyExe();
  if (!exe) throw new Error('Git Bash (mintty) not found');
  fs.mkdirSync(cwd, { recursive: true });
  const log = path.join(cwd, 'received.log');
  fs.rmSync(log, { force: true });
  const title = `Yap robot Git Bash ${nonce}`;
  const before = new Set(findWindows(nonce).map((w) => w.hwnd));
  // A relative log name: bash starts in mintty's folder, `cwd` (no quoting
  // of a Windows path through mintty's command line).
  const script = '/usr/bin/stty raw -echo && exec /usr/bin/cat > received.log';
  const child = spawn(
    exe,
    ['--nodaemon', '-t', title, '-o', 'ConfirmExit=no', '-o', 'ClipShortcuts=yes', '-e', '/usr/bin/bash', '--noprofile', '--norc', '-c', script],
    { cwd, stdio: 'ignore', detached: true, windowsHide: false },
  );
  child.unref();
  const cleanup = async (hwnd) => {
    if (hwnd && findWindows(nonce).some((w) => w.hwnd === hwnd)) helper('close', hwnd, nonce);
    await sleep(300);
    killTree(child.pid);
  };
  let win;
  try {
    // (mintty pads its window title with spaces.)
    win = await waitForWindow(nonce, (w) => !before.has(w.hwnd) && w.cls === 'mintty' && w.title.trimEnd() === title, {
      what: 'Git Bash (mintty) window',
    });
    // Ready once `cat` has the log open (the terminal is raw by then).
    const deadline = Date.now() + 15_000;
    while (!fs.existsSync(log) && Date.now() < deadline) await sleep(100);
    if (!fs.existsSync(log)) throw new Error("Git Bash's reader never started");
  } catch (e) {
    await cleanup(win?.hwnd);
    throw e;
  }
  return {
    hwnd: win.hwnd,
    title,
    focus: () => focus(win.hwnd),
    /** Every byte that has arrived, as text. */
    received: () => (fs.existsSync(log) ? fs.readFileSync(log, 'utf8') : ''),
    close: () => cleanup(win.hwnd),
  };
}

// ---- Notepad ----

/**
 * Why Notepad can't be used now, or null. Windows 11's Notepad opens a file
 * as a new tab in a Notepad window that's already open — yours — so the
 * spec only runs while no Notepad is running.
 */
export function notepadBusy() {
  try {
    const out = execFileSync('tasklist', ['/FI', 'IMAGENAME eq Notepad.exe', '/NH', '/FO', 'CSV'], {
      encoding: 'utf8',
      windowsHide: true,
    });
    return /notepad\.exe/i.test(out)
      ? 'Notepad is open on this PC: Windows 11 would open the test file as a tab in your window'
      : null;
  } catch {
    return "Couldn't tell whether Notepad is running";
  }
}

/**
 * Notepad on an empty temp file named after the nonce (so its window title
 * has it). `close()` saves the file and closes its tab with the keyboard,
 * only while that window is in front and still shows the file (so it's
 * neither left in Notepad's restored session nor asks about saving).
 */
export async function startNotepad({ nonce, dir }) {
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${nonce}.txt`);
  fs.writeFileSync(file, '');
  spawn('notepad.exe', [file], { stdio: 'ignore', detached: true, windowsHide: false }).unref();
  const win = await waitForWindow(nonce, () => true, { what: 'Notepad window' });
  return {
    hwnd: win.hwnd,
    file,
    focus: () => focus(win.hwnd),
    text: () => windowText(win.hwnd),
    /** Select all its text (Ctrl+A, only while it's in front). */
    async selectAll() {
      await focus(win.hwnd);
      const r = helper('keys', win.hwnd, nonce, 'ctrl+a');
      if (!r.ok) throw new Error(`Couldn't select Notepad's text: ${r.error}`);
      await sleep(150);
    },
    async close() {
      try {
        await focus(win.hwnd);
        helper('keys', win.hwnd, nonce, 'ctrl+s');
        await sleep(400);
        helper('keys', win.hwnd, nonce, 'ctrl+w');
        await sleep(400);
      } catch {
        /* fall through to WM_CLOSE */
      }
      if (findWindows(nonce).some((w) => w.hwnd === win.hwnd)) helper('close', win.hwnd, nonce);
    },
  };
}

// ---- Electron and VS Code (when this PC has them) ----

/**
 * An Electron runtime: `YAP_ROBOT_ELECTRON` (an electron.exe), else the
 * `electron` package if it's installed (`npm i --no-save electron`).
 * `null` when there's none: the Electron specs skip.
 */
export function electronExe() {
  const env = process.env.YAP_ROBOT_ELECTRON;
  if (env && fs.existsSync(env)) return env;
  try {
    const exe = createRequire(import.meta.url)('electron');
    return typeof exe === 'string' && fs.existsSync(exe) ? exe : null;
  } catch {
    return null;
  }
}

/**
 * A tiny Electron app of the test's own (its own throwaway profile): a
 * window titled "Yap robot electron - <nonce>" with a text box `#box`,
 * focused, read over the DevTools protocol like `startPage`'s.
 */
export async function startElectronPage({ nonce, dir }) {
  const exe = electronExe();
  if (!exe) throw new Error('No Electron runtime (set YAP_ROBOT_ELECTRON or install the electron package)');
  fs.mkdirSync(dir, { recursive: true });
  const title = `Yap robot electron - ${nonce}`;
  fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: 'yap-robot-electron', main: 'main.js' }));
  fs.writeFileSync(
    path.join(dir, 'page.html'),
    `<!doctype html><meta charset="utf-8"><title>${title}</title>
<style>body{font:15px Segoe UI,sans-serif;margin:16px}textarea{width:100%;height:160px;font:inherit}</style>
<textarea id="box" aria-label="Message" autofocus></textarea>
<script>document.getElementById('box').focus();</script>`,
  );
  fs.writeFileSync(
    path.join(dir, 'main.js'),
    `const { app, BrowserWindow } = require('electron');
app.whenReady().then(() => {
  const w = new BrowserWindow({ width: 640, height: 420, x: 140, y: 160, title: ${JSON.stringify(title)} });
  w.loadFile('page.html');
});
app.on('window-all-closed', () => app.quit());
`,
  );
  const port = 20_000 + crypto.randomInt(20_000);
  const profile = path.join(dir, 'profile');
  const child = spawn(exe, [dir, `--user-data-dir=${profile}`, `--remote-debugging-port=${port}`], {
    stdio: 'ignore',
    detached: true,
    windowsHide: false,
  });
  child.unref();
  const win = await waitForWindow(nonce, (w) => w.title.startsWith(title), { what: 'Electron window' });
  let browser = null;
  const deadline = Date.now() + 15_000;
  while (!browser && Date.now() < deadline) {
    browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`).catch(() => null);
    if (!browser) await sleep(250);
  }
  if (!browser) throw new Error("Couldn't reach the Electron app over CDP");
  const tab = browser
    .contexts()
    .flatMap((c) => c.pages())
    .find((p) => p.url().startsWith('file:'));
  if (!tab) throw new Error("Couldn't find the Electron app's page over CDP");
  try {
    windowText(win.hwnd);
  } catch {
    /* nothing to read yet is fine */
  }
  await sleep(500);
  return {
    hwnd: win.hwnd,
    pid: win.pid,
    focus: () => focus(win.hwnd),
    evaluate: (fn, arg) => tab.evaluate(fn, arg),
    value: () => tab.evaluate(() => document.getElementById('box').value),
    async set(text) {
      await tab.evaluate((t) => {
        const box = document.getElementById('box');
        box.focus();
        box.value = t;
        box.setSelectionRange(t.length, t.length);
      }, text);
      await sleep(300);
    },
    async close() {
      await browser.close().catch(() => {});
      for (const pid of processesWith(profile, [path.basename(exe)])) killTree(pid);
    },
  };
}

/** VS Code's Code.exe (`YAP_ROBOT_VSCODE`, else where its installers put it), or `null`. */
export function vscodeExe() {
  const candidates = [
    process.env.YAP_ROBOT_VSCODE,
    path.join(process.env.LOCALAPPDATA || '', 'Programs', 'Microsoft VS Code', 'Code.exe'),
    path.join(process.env.ProgramFiles || 'C:\\Program Files', 'Microsoft VS Code', 'Code.exe'),
  ].filter(Boolean);
  return candidates.find((p) => fs.existsSync(p)) ?? null;
}

/**
 * VS Code on an empty file named after the nonce, with a throwaway profile
 * and extensions folder (none of your settings, extensions or recent
 * files). `text()` saves it (Ctrl+S, only while it's in front) and reads
 * the file.
 */
export async function startVsCode({ nonce, dir }) {
  const exe = vscodeExe();
  if (!exe) throw new Error('VS Code is not installed');
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, `${nonce}.txt`);
  fs.writeFileSync(file, '');
  const profile = path.join(dir, 'vscode-profile');
  const child = spawn(
    exe,
    [
      `--user-data-dir=${profile}`,
      `--extensions-dir=${path.join(dir, 'vscode-extensions')}`,
      '--new-window',
      '--disable-workspace-trust',
      '--skip-welcome',
      '--skip-release-notes',
      '--disable-extensions',
      file,
    ],
    { stdio: 'ignore', detached: true, windowsHide: false },
  );
  child.unref();
  const win = await waitForWindow(nonce, (w) => w.title.includes(nonce), { what: 'VS Code window', timeout: 40_000 });
  // Its editor up and focused.
  await sleep(2_500);
  return {
    hwnd: win.hwnd,
    file,
    focus: () => focus(win.hwnd),
    async text() {
      await focus(win.hwnd);
      const r = helper('keys', win.hwnd, nonce, 'ctrl+s');
      if (!r.ok) throw new Error(`Couldn't save in VS Code: ${r.error}`);
      await sleep(600);
      return fs.readFileSync(file, 'utf8');
    },
    async close() {
      if (findWindows(nonce).some((w) => w.hwnd === win.hwnd)) helper('close', win.hwnd, nonce);
      await sleep(1_000);
      for (const pid of processesWith(profile, ['Code.exe'])) killTree(pid);
    },
  };
}
