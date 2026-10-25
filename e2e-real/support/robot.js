// Playwright fixtures for the real-app robot tests (e2e-real/, `npm run
// test:real`; docs/e2e-tests.md "Real-app robot tests"). Every spec imports
// `test` and `expect` from here.
//
// - `robot` (one per worker): the test pad, then an isolated Yap in robot
//   mode (`launchYap` from the UI suite plus YAP_REALTEST=1, the run's nonce
//   and the test pad's process id — src-tauri/src/shell/realtest.rs), its
//   clipboard snapshot taken so the developer's clipboard comes back at the
//   end. Other target windows (`robot.terminal()`, `robot.chat()`…) open on
//   first use and close with it.
// - A spec that needs Yap started differently sets `robotOptions`:
//   `test.use({ robotOptions: { name, config, exe, prepare } })`.
import { execFileSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect } from '@playwright/test';
import { launchYap, EXE } from '../../e2e/support/yap.js';
import {
  RUNS,
  OUT,
  compileHelper,
  makeNonce,
  startTestPad,
  startPowerShell,
  startFakeClaudeCode,
  startConsoleCli,
  startChatPage,
  startEditorPage,
  startEditorTerminal,
  startMediaPage,
  startMintty,
  startNotepad,
  startPage,
  startReadingPage,
} from './targets.js';

export { expect };

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Wait for a line in a Yap instance's log (stdout.log). */
export async function waitForLog(yap, needle, { timeout = 15_000, after = 0 } = {}) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    const log = fs.readFileSync(yap.logPath, 'utf8').slice(after);
    if (typeof needle === 'string' ? log.includes(needle) : needle.test(log)) return;
    await sleep(100);
  }
  throw new Error(`Yap's log never said ${needle}`);
}

export class Robot {
  constructor({ yap, pad, nonce, name }) {
    Object.assign(this, { yap, pad, nonce, name });
    this.targets = [];
    this.dir = path.join(RUNS, name, 'targets');
  }

  /** How far Yap's log has got (for `logSince`). */
  logMark() {
    return fs.readFileSync(this.yap.logPath, 'utf8').length;
  }

  logSince(mark) {
    return fs.readFileSync(this.yap.logPath, 'utf8').slice(mark);
  }

  /**
   * A dictation of `text` (as if the STT had heard it) into `target`
   * (brought to the front first). Returns what went to the paste:
   * `{ text, raw, kind, submit, error }`.
   *
   * Another app taking the focus mid-dictation makes Yap's guard refuse
   * the paste (nothing is sent); then it's tried again, up to twice, unless
   * `retry: false` (a test that wants the refusal). A refusal after the
   * paste (the submit key) isn't: the text is already there.
   */
  async dictate(target, text, { retry = true } = {}) {
    return this.withFocus(target, retry, () =>
      this.yap.invoke('realtest_dictate_text', { text, hwnd: target.hwnd }),
    );
  }

  /**
   * A dictation of a WAV file into `target`, through the real start/stop
   * path. `cancel`: cancelled at the end instead of stopped (the bar's ✕),
   * so it's kept for Undo (`null` comes back: nothing was transcribed).
   */
  async dictateWav(target, file, { retry = true, cancel = false } = {}) {
    return this.withFocus(target, retry, () =>
      this.yap.invoke('realtest_dictate_wav', { path: file, hwnd: target.hwnd, cancel }),
    );
  }

  async withFocus(target, retry, dictation) {
    const focusLost = /isn't in front|lost the focus|refused to (paste|type the text): the test didn't open it/;
    for (let attempt = 1; ; attempt++) {
      await target.focus();
      let r;
      try {
        r = await dictation();
      } catch (e) {
        if (retry && attempt < 3 && focusLost.test(e.message)) {
          console.warn(`robot: another window took the focus (${e.message}); trying again`);
          continue;
        }
        throw e;
      }
      if (retry && attempt < 3 && r?.error && focusLost.test(r.error)) {
        console.warn(`robot: another window took the focus (${r.error}); trying again`);
        continue;
      }
      return r;
    }
  }

  clipboard(action, text) {
    return this.yap.invoke('realtest_clipboard', { action, text });
  }

  /** Change Yap's settings the way Settings does (`save_config`). */
  async configure(patch) {
    const cfg = await this.yap.invoke('get_config');
    await this.yap.invoke('save_config', { cfg: { ...cfg, ...patch } });
  }

  async track(target) {
    this.targets.push(target);
    return target;
  }

  /** A PowerShell prompt in a new Windows Terminal window. */
  async shell(label = 'shell') {
    return this.track(await startPowerShell({ nonce: this.nonce, cwd: path.join(this.dir, `${label}-${this.targets.length}`) }));
  }

  /**
   * The stand-in for Claude Code in a new Windows Terminal window (`title`:
   * another title, with the nonce — "Yap robot raw <nonce>" is a plain
   * program, no coding CLI).
   */
  async claudeCode(label = 'cli', { title } = {}) {
    return this.track(
      await startFakeClaudeCode({ nonce: this.nonce, cwd: path.join(this.dir, `${label}-${this.targets.length}`), title }),
    );
  }

  /** The stand-in CLI in a classic console window (conhost), titled `title` (default Claude Code's "✳ <nonce>"). */
  async consoleCli(label = 'conhost', { title } = {}) {
    return this.track(await startConsoleCli({ nonce: this.nonce, cwd: path.join(this.dir, `${label}-${this.targets.length}`), title }));
  }

  /**
   * An Edge app window on a "WhatsApp" page (`note`: text above its box;
   * `{ editable }`: the box a contenteditable, as WhatsApp Web's is;
   * `{ attrs }`: the box's attributes instead of its aria-label).
   */
  async chat(note, { editable = false, attrs } = {}) {
    return this.track(
      await startChatPage({ nonce: this.nonce, dir: path.join(this.dir, `chat-${this.targets.length}`), note, editable, attrs }),
    );
  }

  /**
   * An Edge app window on a page with a text box, an ordinary app (`note`:
   * text above it; `beside`: `[before, after]`, page text on its line).
   */
  async editor(note, beside) {
    return this.track(
      await startEditorPage({ nonce: this.nonce, dir: path.join(this.dir, `editor-${this.targets.length}`), note, beside }),
    );
  }

  /**
   * A stand-in for VS Code's integrated terminal (xterm.js's input, labelled
   * "Terminal 1, <terminalTitle>") in an Edge app window; the spec's Yap
   * must count Edge as an editor (`robotOptions.env.YAP_REALTEST_EDITORS`).
   */
  async editorTerminal(terminalTitle) {
    return this.track(
      await startEditorTerminal({ nonce: this.nonce, dir: path.join(this.dir, `vscode-${this.targets.length}`), terminalTitle }),
    );
  }

  /**
   * An Edge app window on a page of `body`'s markup ("Yap robot page -
   * <nonce>": an ordinary app), its element `#box` focused.
   */
  async page(body, label = 'page') {
    return this.track(
      await startPage({ nonce: this.nonce, dir: path.join(this.dir, `${label}-${this.targets.length}`), title: `Yap robot page - ${this.nonce}`, body }),
    );
  }

  /** Git Bash's mintty running a reader that logs what arrives (runs nothing). */
  async mintty() {
    return this.track(await startMintty({ nonce: this.nonce, cwd: path.join(this.dir, `mintty-${this.targets.length}`) }));
  }

  /** An Edge app window playing `wav` on a loop, its title (with the nonce) in Windows' media controls. */
  async mediaPage(wav) {
    return this.track(
      await startMediaPage({ nonce: this.nonce, dir: path.join(this.dir, `media-${this.targets.length}`), wav }),
    );
  }

  /** Other apps' sound as Yap's robot mode may see it (only the test's own). */
  otherApps() {
    return this.yap.invoke('realtest_other_apps');
  }

  /** An Edge app window on a page of text to select and read aloud. */
  async readingPage(paragraphs) {
    return this.track(
      await startReadingPage({ nonce: this.nonce, dir: path.join(this.dir, `reading-${this.targets.length}`), paragraphs }),
    );
  }

  /**
   * What the Read aloud shortcut does with `target` in front: its
   * selection read and read aloud (`realtest_read_selection`). Returns
   * `{ via, chars, terminal, copyAllowed, outcome, text }`.
   */
  async readSelection(target, { retry = true } = {}) {
    return this.withFocus(target, retry, () => this.yap.invoke('realtest_read_selection', { hwnd: target.hwnd }));
  }

  /** Notepad on an empty temp file. */
  async notepad() {
    return this.track(await startNotepad({ nonce: this.nonce, dir: path.join(this.dir, `notepad-${this.targets.length}`) }));
  }

  async closeTargets() {
    for (const t of this.targets.splice(0).reverse()) {
      await t.close().catch((e) => console.warn(`robot: couldn't close a test window: ${e.message}`));
    }
  }
}

export const test = base.extend({
  // { name, config, env, exe, prepare } for the robot's Yap.
  robotOptions: [{ name: 'robot' }, { scope: 'worker', option: true }],

  robot: [
    async ({ robotOptions }, use, workerInfo) => {
      compileHelper();
      const nonce = makeNonce();
      const name = `${robotOptions.name}-w${workerInfo.workerIndex}`;
      // The pad's title doesn't have the nonce: Yap knows it by its
      // process (YAP_REALTEST_PIDS), the other windows by their titles.
      const pad = await startTestPad(`pad-${crypto.randomBytes(4).toString('hex')}`);
      let yap;
      let robot;
      try {
        yap = await launchYap({
          name,
          runsDir: RUNS,
          exe: robotOptions.exe ?? EXE,
          prepare: robotOptions.prepare,
          // Debug logging: the log says when the text around the cursor was
          // read (lengths only) and what kind of app a dictation went into.
          config: { debugLogging: true, ...robotOptions.config },
          env: {
            YAP_REALTEST: '1',
            YAP_REALTEST_NONCE: nonce,
            YAP_REALTEST_PIDS: String(pad.pid),
            ...robotOptions.env,
          },
        });
        await waitForLog(yap, 'e2e: robot mode on');
        // The developer's clipboard, kept (in Yap's memory only) to put back.
        await yap.invoke('realtest_clipboard', { action: 'save' });
        robot = new Robot({ yap, pad, nonce, name });
        await use(robot);
      } finally {
        await robot?.closeTargets();
        await yap?.invoke('realtest_clipboard', { action: 'restore' }).catch(() => {});
        await yap?.stop();
        await pad.close();
      }
    },
    { scope: 'worker', timeout: 120_000 },
  ],

  // Before each test: no toast an earlier test left in Yap's main window
  // (`clearToasts`). After it: the windows it opened close (the test pad
  // stays), and if Yap saw the focus move while keys were on their way
  // (`realtest::check_after`: they may have reached another window), the
  // test fails, naming the process that took the focus.
  afterEachRobot: [
    async ({ robot }, use) => {
      await clearToasts(robot.yap.main);
      const seen = (await robot.yap.invoke('realtest_refusals')).length;
      await use();
      await robot.closeTargets();
      const stray = (await robot.yap.invoke('realtest_refusals')).slice(seen).filter((m) => /may have gone/.test(m));
      if (stray.length) {
        const named = stray.map((m) => {
          const pid = /process (\d+)/.exec(m)?.[1];
          return pid ? `${m} = ${processName(pid)}` : m;
        });
        throw new Error(
          `The focus moved while Yap's keys were on their way (someone clicked, or an app took the focus):\n${named.join('\n')}`,
        );
      }
    },
    { auto: true },
  ],

  /** A screenshot of one of Yap's webviews: test-results/real/screenshots/<spec>/<name>.png. */
  shot: async ({}, use, testInfo) => {
    const spec = path.basename(testInfo.file).replace(/\.real\.js$/, '');
    await use(async (page, name) => {
      const file = path.join(OUT, 'screenshots', spec, `${name}.png`);
      fs.mkdirSync(path.dirname(file), { recursive: true });
      await page.screenshot({ path: file, scale: 'css', animations: 'disabled' });
      await testInfo.attach(name, { path: file, contentType: 'image/png' });
    });
  },
});

/**
 * Close the toasts up in Yap's main window (their Close buttons, clicked
 * over CDP: no real input), then move the page's pointer off them.
 *
 * Why: one robot Yap serves every spec with the default `robotOptions`, one
 * after another (Playwright runs those files in one worker). A click over
 * CDP leaves the page's pointer where it clicked, and nothing moves it after
 * — learn.real.js clicks Add and Not now on its suggestion toast. From then
 * on every toast comes up under that pointer: hovered, its timer paused for
 * good, and no notice may take its place (noticePolicy.js `preempts`: never
 * from under the pointer). So rewrite-last.real.js' "Couldn't rewrite your
 * last dictation" stayed up, and testpad.real.js' guard test, later in the
 * run, never saw "Robot mode refused to paste": it waited in the queue
 * (failed in full runs on 2026-10-23 and 24, passed alone; learn,
 * rewrite-last then testpad fail every time without this).
 */
export async function clearToasts(main) {
  const toasts = main.getByRole('status');
  try {
    await expect(async () => {
      const close = toasts.getByRole('button', { name: 'Close', exact: true }).first();
      if ((await close.count()) > 0) await close.click({ timeout: 1_000 });
      await expect(toasts).toHaveCount(0, { timeout: 500 });
    }).toPass({ timeout: 20_000 });
    // The click left the page's pointer where the toasts come up.
    await main.mouse.move(0, 0);
  } catch (e) {
    console.warn(`robot: couldn't clear the toasts in Yap's main window: ${e.message}`);
  }
}

/** A process's name, for a message ("claude.exe"). */
function processName(pid) {
  try {
    const out = execFileSync('tasklist', ['/FI', `PID eq ${pid}`, '/NH', '/FO', 'CSV'], { encoding: 'utf8', windowsHide: true });
    return /^"([^"]+)"/.exec(out.trim())?.[1] ?? `process ${pid}`;
  } catch {
    return `process ${pid}`;
  }
}

/** Windows' line breaks as "\n" (a text box gives "\r\n"). */
export const lf = (s) => s.replace(/\r\n?/g, '\n');
