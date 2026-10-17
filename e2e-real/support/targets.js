// The windows the robot dictates into, all opened by the harness and all
// titled with the run's nonce (src-tauri/src/realtest.rs only lets Yap
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
import path from 'node:path';
import readline from 'node:readline';
import { fileURLToPath } from 'node:url';
import { chromium } from '@playwright/test';

export const REPO = fileURLToPath(new URL('../..', import.meta.url));
export const OUT = path.join(REPO, 'test-results', 'real');
export const RUNS = path.join(OUT, 'runs');
export const CACHE = path.join(OUT, 'cache');
const BIN = path.join(OUT, 'bin');
const HELPER_SRC = fileURLToPath(new URL('./yaprobot.cs', import.meta.url));
export const HELPER = path.join(BIN, 'yaprobot.exe');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** A fresh nonce for a run's window titles (≥ 12 characters, as Yap wants). */
export function makeNonce() {
  return `yaprobot-${crypto.randomBytes(6).toString('hex')}`;
}

/** Compile yaprobot.exe with the .NET Framework's C# compiler (every Windows has one). */
export function compileHelper() {
  const fresh =
    fs.existsSync(HELPER) && fs.statSync(HELPER).mtimeMs >= fs.statSync(HELPER_SRC).mtimeMs;
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
        HELPER_SRC,
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
export async function startFakeClaudeCode({ nonce, cwd }) {
  fs.mkdirSync(cwd, { recursive: true });
  const log = path.join(cwd, 'received.jsonl');
  fs.writeFileSync(log, '');
  const script = fileURLToPath(new URL('./fake-cli.mjs', import.meta.url));
  const term = await startTerminal({
    nonce,
    title: `✳ ${nonce}`,
    cwd,
    command: [process.execPath, script, log],
    marker: log,
  });
  // Ready once it has turned bracketed paste on.
  const deadline = Date.now() + 15_000;
  while (!fs.readFileSync(log, 'utf8').includes('"ready"') && Date.now() < deadline) await sleep(100);
  return {
    ...term,
    /** Everything typed or pasted into it so far, as one string. */
    received() {
      return fs
        .readFileSync(log, 'utf8')
        .split('\n')
        .filter(Boolean)
        .map((l) => JSON.parse(l))
        .filter((e) => e.data !== undefined)
        .map((e) => e.data)
        .join('');
    },
  };
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
 * `set(text, caret?)` fills it, over the DevTools protocol.
 */
export async function startPage({ nonce, dir, title, note = '', attrs = 'aria-label="Message"' }) {
  const exe = edgeExe();
  if (!exe) throw new Error('Microsoft Edge not found');
  fs.mkdirSync(dir, { recursive: true });
  const page = path.join(dir, 'page.html');
  const before = new Set(findWindows(nonce).map((w) => w.hwnd));
  fs.writeFileSync(
    page,
    `<!doctype html><meta charset="utf-8"><title>${title}</title>
<style>body{font:15px Segoe UI,sans-serif;margin:16px;background:#f4f1ea}textarea{width:100%;height:120px;font:inherit}</style>
${note ? `<p>${note}</p>` : ''}
<textarea id="box" ${attrs} autofocus></textarea>
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
    value: () => tab.evaluate(() => document.getElementById('box').value),
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
          box.value = t;
          box.focus();
          const at = c ?? t.length;
          box.setSelectionRange(at, e ?? at);
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
 * A web page's text box ("Yap robot editor - <nonce>": an ordinary app to
 * Yap); `note`: text on the page above the box (none by default).
 */
export function startEditorPage({ nonce, dir, note }) {
  return startPage({ nonce, dir, title: `Yap robot editor - ${nonce}`, note });
}

/** A page titled "WhatsApp - <nonce>": a chat app to Yap (nothing is sent anywhere). */
export function startChatPage({ nonce, dir, note }) {
  return startPage({ nonce, dir, title: `WhatsApp - ${nonce}`, note, attrs: 'aria-label="Type a message"' });
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
