// The robot tests' hands-off guard (docs/e2e-tests.md, "Hands off: the idle
// check and the input watcher"). The robot drives the real keyboard, mouse
// and focus, so it must never run while someone is using the PC, whoever
// starts it and whatever the time:
//
// - the idle check: a run starts only once the PC has had none of the
//   person's input for YAP_ROBOT_IDLE_MIN minutes (10); YAP_ROBOT_FORCE=1
//   skips it, only when the person at the PC says so;
// - the input watcher (`yaprobot watch`, support/inputwatch.cs): its hooks
//   go in before the check, so nothing slips between, and it's armed once
//   the run goes ahead. From then any key, click, wheel or mouse move of
//   the person's own (input without the injected flags the robot's
//   carries; any since the hooks went in too) ends the run at once: the
//   runner's whole process tree killed, the robot lock deleted,
//   test-results/real/stopped-by-input.txt saying why. FORCE never turns
//   it off.
//
// Windows' idle time (GetLastInputInfo) counts the robot's own injected
// input too, so a run right after another would wait 10 minutes for
// nothing. When a watch ends cleanly it leaves its end behind (the last
// input's tick then, and when the person's input was last seen): if no
// input at all has come since, the person's last input is still that one.
import { spawn, execFileSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import readline from 'node:readline';
import { HELPER, OUT, REPO, compileHelper } from './targets.js';

export const IDLE_MIN_DEFAULT = 10;
/** One line saying why the watcher stopped a run. */
export const NOTE = path.join(OUT, 'stopped-by-input.txt');
/** The last clean watch's end: one per Windows user, so every checkout on the PC shares it. */
const LAST_WATCH = path.join(os.tmpdir(), 'yap-robot-last-watch.json');

/** Minutes of no input a run waits for (YAP_ROBOT_IDLE_MIN). */
export function idleMinutes() {
  const raw = process.env.YAP_ROBOT_IDLE_MIN;
  if (raw === undefined || raw === '') return IDLE_MIN_DEFAULT;
  const n = Number(raw);
  if (!Number.isFinite(n) || n < 0) {
    throw refusal(`YAP_ROBOT_IDLE_MIN must be a number of minutes, not "${raw}".`);
  }
  return n;
}

/**
 * The robot lock: YAP_ROBOT_LOCK, else `.yap-robot-lock` in the folder that
 * holds the main checkout (a worktree's main one too), where agents sharing
 * the PC take it before a run.
 */
export function robotLock() {
  if (process.env.YAP_ROBOT_LOCK) return process.env.YAP_ROBOT_LOCK;
  let main = REPO;
  try {
    const common = execFileSync('git', ['rev-parse', '--git-common-dir'], {
      cwd: REPO,
      encoding: 'utf8',
      windowsHide: true,
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
    main = path.dirname(path.resolve(REPO, common));
  } catch {
    /* not a git checkout: the folder itself */
  }
  return path.join(path.dirname(main), '.yap-robot-lock');
}

/** "12 s", "4 min", "2 h 5 min". */
export function span(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}

/**
 * When the person's input was last seen: Windows' last input (any), or —
 * when nothing at all has come since the last clean watch ended — what that
 * watch knew.
 */
export function lastRealInput({ idleMs, tick }, now = Date.now(), last = readLastWatch()) {
  const any = now - idleMs;
  if (last && last.tick === tick && last.until <= now && last.lastReal <= any) return last.lastReal;
  return any;
}

function readLastWatch() {
  try {
    const w = JSON.parse(fs.readFileSync(LAST_WATCH, 'utf8'));
    return [w.tick, w.until, w.lastReal].every(Number.isFinite) ? w : null;
  } catch {
    return null;
  }
}

function writeLastWatch(w) {
  try {
    const tmp = `${LAST_WATCH}.${process.pid}.tmp`;
    fs.writeFileSync(tmp, JSON.stringify(w));
    fs.renameSync(tmp, LAST_WATCH);
  } catch (e) {
    console.warn(`robot guard: couldn't note the watch's end (${e.message}); the next run waits for the PC to be idle`);
  }
}

/** An error Playwright shows as its one line. */
function refusal(message) {
  const e = new Error(message);
  e.stack = message;
  return e;
}

/**
 * Start the input watcher, then check the PC has been idle. Resolves to
 * `{ stop }` (call it when the run ends) or throws the refusal, having
 * touched no key, pointer or window.
 */
export async function startGuard() {
  const minutes = idleMinutes();
  compileHelper();
  fs.rmSync(NOTE, { force: true });
  const lock = robotLock();
  const child = spawn(HELPER, ['watch', String(process.pid), lock, NOTE], {
    stdio: ['pipe', 'pipe', 'inherit'],
    windowsHide: true,
    // Out of the runner's kill-on-close job (Node puts every non-detached
    // child in one): it must outlive the runner it kills, to say why. It
    // still quits as soon as the runner is gone.
    detached: true,
  });
  child.stdin.on('error', () => {});
  const lines = readline.createInterface({ input: child.stdout });
  const waiting = [];
  lines.on('line', (line) => waiting.shift()?.(line));
  let exited = false;
  child.on('exit', () => {
    exited = true;
    for (const w of waiting.splice(0)) w(null);
  });
  const next = (ms) =>
    new Promise((resolve) => {
      const timer = setTimeout(() => resolve(null), ms);
      waiting.push((line) => {
        clearTimeout(timer);
        resolve(line);
      });
    });
  const end = async () => {
    if (exited) return null;
    const reply = next(5_000);
    child.stdin.end('stop\n');
    const line = await reply;
    if (!exited) child.kill();
    try {
      return line ? JSON.parse(line) : null;
    } catch {
      return null;
    }
  };

  let first = null;
  try {
    first = JSON.parse((await next(15_000)) ?? 'null');
  } catch {
    /* said below */
  }
  if (!first?.watching) {
    await end();
    throw refusal(
      `The robot tests' input watcher couldn't start (${first?.error ?? 'no answer'}): they never run without it.`,
    );
  }

  const now = Date.now();
  const lastReal = lastRealInput(first, now);
  const idle = now - lastReal;
  if (process.env.YAP_ROBOT_FORCE === '1') {
    console.log(
      `robot guard: YAP_ROBOT_FORCE=1, not waiting for the PC to be idle (last input ${span(idle)} ago). Any key, click or mouse move still stops the run.`,
    );
  } else if (idle < minutes * 60_000) {
    await end();
    throw refusal(
      `The PC is in use (last input ${span(idle)} ago): robot tests wait until it's been idle ${minutes} min. Set YAP_ROBOT_FORCE=1 only when the person at the PC says so.`,
    );
  }

  // Armed: from here the person's input — any since its hooks went in
  // too — ends the run.
  const armed = next(5_000);
  child.stdin.write('arm\n');
  let ack = null;
  try {
    ack = JSON.parse((await armed) ?? 'null');
  } catch {
    /* said below */
  }
  if (!ack?.armed) {
    await end();
    throw refusal("The robot tests' input watcher didn't arm: they never run without it.");
  }
  // The robot fixture checks this (`assertWatched`) before it takes the keyboard.
  process.env.YAP_ROBOT_WATCHER = String(child.pid);
  // Never what keeps the runner alive (stopping it sets a timer of its own).
  child.unref();
  child.stdin.unref?.();
  child.stdout.unref?.();
  return {
    async stop() {
      const done = await end();
      delete process.env.YAP_ROBOT_WATCHER;
      if (done?.stopped && Number.isFinite(done.tick)) {
        writeLastWatch({ tick: done.tick, until: Date.now(), lastReal });
      }
    },
  };
}

/**
 * Throws unless the input watcher is up: the robot takes no key, pointer or
 * focus without it (a spec run under another config, or the watcher gone).
 */
export function assertWatched() {
  const pid = Number(process.env.YAP_ROBOT_WATCHER);
  let alive = false;
  if (pid > 0) {
    try {
      process.kill(pid, 0);
      alive = true;
    } catch {
      /* gone */
    }
  }
  if (!alive) {
    throw refusal(
      "The robot tests' input watcher isn't running: robot tests run only under e2e-real's config (`npx playwright test -c e2e-real`), which starts it.",
    );
  }
}
