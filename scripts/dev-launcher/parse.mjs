// What the dev launcher reads in `tauri dev`'s output (and dev-update.ps1's):
// lines and cargo's progress redraws, Vite, the stages, build errors, hot
// reloads. Pure: no I/O, the time always passed in, so the tests can feed it
// recorded output (scripts/dev-launcher.test.mjs).

/** Escape sequences and control characters out; tabs kept. */
export function stripAnsi(s) {
  return s
    .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, '') // OSC (titles, links)
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '') // CSI (colours, erase line)
    .replace(/\x1b[@-Z\\-_]/g, '') // other two-byte escapes
    .replace(/\uFEFF/g, '') // a BOM (PowerShell's UTF-8 output starts with one)
    .replace(/[\x00-\x08\x0b-\x1f\x7f]/g, '');
}

/**
 * Splits a stream into segments. A segment ended by a lone `\r` is
 * `transient`: the program redraws over it (cargo's progress bar). `\r\n`
 * is one line end, even split across two chunks: a `\r` ending a chunk is
 * passed on as transient straight away (cargo prints `659/660: yap(bin)\r`
 * and then says nothing for the whole link), and if the next chunk starts
 * with `\n` the same text comes again as a finished line.
 */
export class LineSplitter {
  constructor() {
    this.buf = '';
    this.crText = null; // the text a chunk-ending `\r` closed, in case a `\n` follows
  }

  push(chunk) {
    const out = [];
    let s = chunk;
    if (this.crText != null) {
      if (s.startsWith('\n')) {
        out.push({ text: this.crText, transient: false });
        s = s.slice(1);
      }
      if (s) this.crText = null;
    }
    const re = /\r\n|\r|\n/g;
    let i = 0;
    let m;
    while ((m = re.exec(s))) {
      const text = this.buf + s.slice(i, m.index);
      this.buf = '';
      out.push({ text, transient: m[0] === '\r' });
      if (m[0] === '\r' && m.index === s.length - 1) this.crText = text;
      i = m.index + m[0].length;
    }
    if (i < s.length) this.buf += s.slice(i);
    return out;
  }

  /** The rest at the end of the stream. */
  flush() {
    const out = [];
    if (this.buf) out.push({ text: this.buf, transient: false });
    this.buf = '';
    this.crText = null;
    return out;
  }
}

const RX = {
  progress: /^\s*Building \[[ =>-]*\]\s+(\d+)\/(\d+)(?::\s*(.*?))?\s*$/,
  compiling: /^\s*(Compiling|Checking)\s+([\w.+-]+)\s+v(\S+)/,
  finished: /^\s*Finished\s+(.*?)\s+in\s+([\dhms. ]+?)\s*$/,
  runExe: /^\s*Running\s+`(.+?)`\s*$/,
  tauriRun: /^\s*Running\s+(BeforeDevCommand|DevCommand)\b/,
  rebuild: /\bFile\s+(.+?)\s+changed\.\s+Rebuilding application/i,
  viteReady: /\bVITE\s+v(\S+)\s+ready in\s+([\d.]+)\s*ms/,
  viteLocal: /^\s*(?:\S\s+)?Local:\s+(https?:\/\/\S+)/,
  viteHmr: /\[vite\]\s+(?:\([^)]*\)\s+)?(hmr update|page reload)\s+(.+?)(?:\s+\(x\d+\))?\s*$/,
  viteError: /\[vite\].*(?:Internal server error|\berror\b)|\[plugin:[^\]]+\]/i,
  waitingFrontend: /Waiting for your frontend dev server/i,
  blocking: /Blocking waiting for file lock/,
  errorSummary: /^error: could not compile `([^`]+)`(.*)$/,
  error: /^error(?:\[(E\d+)\])?:\s*(.*)$/,
  warningSummary: /^warning: `([^`]+)` \(([^)]+)\) generated (\d+) warnings?/,
  warning: /^warning(?:\[[^\]]*\])?:/,
  panic: /\bpanicked at\b/,
  appLog: /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z?\s+(TRACE|DEBUG|INFO|WARN|ERROR)\s+(.*)$/,
  portInUse: /\bPort (\d+) is already in use/i,
  accessDenied: /Access is denied|\(os error 5\)/i,
  npmError: /^npm (?:error|ERR!)/,
  tauriLog: /^\s{2,}(Info|Warn|Error)\s+(.+)$/,
};

/** What a line (escape codes already stripped) is. */
export function classify(text) {
  let m;
  if (!text.trim()) return { kind: 'blank' };
  if ((m = RX.progress.exec(text))) {
    const crates = (m[3] || '')
      .split(',')
      .map((c) => c.trim())
      .filter(Boolean);
    return { kind: 'progress', n: +m[1], m: +m[2], crates };
  }
  if ((m = RX.compiling.exec(text))) return { kind: 'compiling', name: m[2], version: m[3] };
  if ((m = RX.finished.exec(text))) return { kind: 'finished', profile: m[1], took: m[2] };
  if ((m = RX.runExe.exec(text))) return { kind: 'run-exe', exe: m[1] };
  if ((m = RX.tauriRun.exec(text))) return { kind: 'tauri-run', what: m[1] };
  if ((m = RX.rebuild.exec(text))) return { kind: 'rebuild', file: m[1] };
  if ((m = RX.viteReady.exec(text))) return { kind: 'vite-ready', version: m[1], ms: +m[2] };
  if ((m = RX.viteLocal.exec(text))) return { kind: 'vite-local', url: m[1] };
  if ((m = RX.viteHmr.exec(text))) return { kind: 'vite-hmr', how: m[1], what: m[2] };
  if ((m = RX.appLog.exec(text))) return { kind: 'app-log', level: m[1], rest: m[2] };
  if (RX.panic.test(text)) return { kind: 'panic' };
  if ((m = RX.portInUse.exec(text))) return { kind: 'port-in-use', port: +m[1] };
  if ((m = RX.errorSummary.exec(text))) return { kind: 'error-summary', crate: m[1] };
  if ((m = RX.error.exec(text))) return { kind: 'error', code: m[1] || null, accessDenied: RX.accessDenied.test(text) };
  if ((m = RX.warningSummary.exec(text))) return { kind: 'warning-summary', crate: m[1], count: +m[3] };
  if (RX.warning.test(text)) return { kind: 'warning' };
  if (RX.viteError.test(text)) return { kind: 'vite-error' };
  if (RX.npmError.test(text)) return { kind: 'npm-error' };
  if (RX.waitingFrontend.test(text)) return { kind: 'waiting-frontend' };
  if (RX.blocking.test(text)) return { kind: 'blocking' };
  if ((m = RX.tauriLog.exec(text))) {
    return { kind: m[1] === 'Error' ? 'tauri-error' : 'tauri-log', level: m[1], msg: m[2] };
  }
  if (RX.accessDenied.test(text)) return { kind: 'access-denied' };
  return { kind: 'other' };
}

// Until a build has been timed on this PC: the two slow last steps of a
// debug build (yap's own crate with its DLL, then linking yap.exe), and how
// long a dependency takes on average when many build at once.
export const DEFAULT_LIB_MS = 60_000;
export const DEFAULT_LINK_MS = 45_000;
export const DEFAULT_MS_PER_UNIT = 350;
const YAP_UNITS = 2; // the lib and the bin, last in cargo's count

const isYapLib = (c) => c === 'yap' || c === 'yap_lib';
const isYapBin = (c) => c === 'yap(bin)';
const BUILD_STAGES = new Set(['vite', 'compiling', 'launching']);
// Lines that go on being what they are while an error shows (not its detail).
const NOT_PART_OF_AN_ERROR = new Set(['progress', 'rebuild', 'vite-hmr', 'vite-ready', 'vite-local', 'app-log', 'tauri-log']);

/**
 * The launcher's model of a `tauri dev` run: which stage it's in, the build's
 * progress, hot reloads, errors. `apply` takes each segment, `tick` the time.
 */
export class DevState {
  constructor({ now = Date.now(), history = {} } = {}) {
    this.history = { ...history }; // { libMs, linkMs, buildMs, msPerUnit } from the last build here
    this.startedAt = now;
    this.stage = 'vite';
    this.stageSince = now;
    this.vite = { done: false, ms: null, url: null, version: null, skipped: false };
    this.build = null;
    this.builds = 0;
    this.lastBuild = null; // { ms, libMs, linkMs, units, took, rebuild }
    this.launchAt = null;
    this.runningSince = null;
    this.firstRunningAt = null;
    this.hot = { count: 0, recent: [] }; // { at, kind: 'ui' | 'rust', what, ms? }
    this.rustRebuilds = 0;
    this.appLog = { warn: 0, error: 0 };
    this.warnings = 0;
    this.errors = null; // { stage, lines: [], accessDenied, port, at }
    this.viteError = null; // { text, at }
    this.panic = null; // { text, at }
    this.exit = null; // { code, at, byUser }
    this.events = []; // what the launcher writes in its log: { at, text }
    this.pendingRebuild = null;
  }

  setStage(stage, now) {
    if (this.stage === stage) return;
    this.stage = stage;
    this.stageSince = now;
  }

  note(now, text) {
    this.events.push({ at: now, text });
  }

  startBuild(now, { rebuild = false, file = null } = {}) {
    this.build = {
      start: now,
      rebuild,
      file,
      n0: null,
      n: null,
      m: null,
      crates: [],
      firstProgressAt: null,
      compiled: 0,
      lastCrate: null,
      libStart: null,
      nAtLib: null,
      linkStart: null,
      finishedAt: null,
      took: null,
      waitingLock: false,
    };
    this.setStage('compiling', now);
  }

  /** One segment from the child: `{ text, transient }`, escape codes still in. */
  apply(seg, now) {
    const text = stripAnsi(seg.text);
    const c = classify(text);
    // a redrawn line only matters as cargo's progress bar (the rest comes again finished)
    if (!seg.transient || c.kind === 'progress') this.applyClassified(c, text, now, seg.transient);
    return { text, c };
  }

  applyClassified(c, text, now, transient = false) {
    if (this.stage === 'error' && this.errors) {
      if (c.kind === 'finished' || c.kind === 'run-exe') {
        // the build went on after all: that error wasn't the end of it
        this.note(now, 'the build carried on after the error');
        this.errors = null;
        this.setStage('compiling', now);
      } else if (!transient && !NOT_PART_OF_AN_ERROR.has(c.kind)) {
        // while a build error is showing, its following lines belong to it
        if (this.errors.lines.length < 400) this.errors.lines.push(text);
        if (c.accessDenied || c.kind === 'access-denied') this.errors.accessDenied = true;
        return;
      }
    }

    switch (c.kind) {
      case 'vite-ready':
        this.vite.done = true;
        this.vite.ms = c.ms;
        this.vite.version = c.version;
        if (this.stage === 'vite') this.startBuild(now);
        break;
      case 'vite-local':
        if (!this.vite.url) this.vite.url = c.url;
        break;
      case 'tauri-run':
        if (c.what === 'DevCommand' && (this.stage === 'vite' || !this.build)) {
          if (!this.vite.done) this.vite.skipped = true;
          this.startBuild(now);
        }
        break;
      case 'rebuild':
        this.rustRebuilds += 1;
        this.pendingRebuild = { at: now, file: c.file };
        this.errors = null;
        this.startBuild(now, { rebuild: true, file: c.file });
        this.note(now, `${c.file} changed: rebuilding`);
        break;
      case 'progress':
      case 'compiling':
      case 'blocking':
        if (this.stage === 'vite' || (this.stage !== 'compiling' && this.stage !== 'error' && !this.build)) {
          if (!this.vite.done) this.vite.skipped = true;
          this.startBuild(now);
        } else if (this.stage === 'running' || this.stage === 'launching' || !this.build) {
          // cargo again without the rebuild line we know (another watcher's wording)
          this.startBuild(now, { rebuild: this.builds > 0 });
        }
        this.onCargo(c, now);
        break;
      case 'finished':
        if (this.build) this.finishBuild(c, now);
        break;
      case 'run-exe':
        if (/yap(\.exe)?$/i.test(c.exe) || this.build) {
          if (this.build && !this.build.finishedAt) this.finishBuild({ took: null }, now);
          this.launchAt = now;
          this.setStage('launching', now);
          this.note(now, `starting ${c.exe}`);
        }
        break;
      case 'app-log':
        if (c.level === 'WARN') this.appLog.warn += 1;
        if (c.level === 'ERROR') this.appLog.error += 1;
        if (this.stage === 'launching') this.toRunning(now);
        break;
      case 'vite-hmr': {
        this.hot.count += 1;
        this.pushRecent({ at: now, kind: 'ui', what: c.what });
        this.viteError = null;
        break;
      }
      case 'vite-error':
        if (this.stage === 'vite') this.fail(text, now);
        else this.viteError = { text, at: now };
        break;
      case 'warning':
        this.warnings += 1;
        break;
      case 'panic':
        if (BUILD_STAGES.has(this.stage)) this.fail(text, now);
        else this.panic = { text, at: now };
        break;
      case 'port-in-use':
        if (BUILD_STAGES.has(this.stage)) this.fail(text, now, { port: c.port });
        break;
      case 'error':
      case 'error-summary':
      case 'npm-error':
      case 'tauri-error':
        if (BUILD_STAGES.has(this.stage)) this.fail(text, now, { accessDenied: !!c.accessDenied });
        break;
      case 'access-denied':
        if (this.errors) this.errors.accessDenied = true;
        break;
      default:
        break;
    }
  }

  onCargo(c, now) {
    const b = this.build;
    if (c.kind === 'blocking') {
      b.waitingLock = true;
      return;
    }
    b.waitingLock = false;
    if (c.kind === 'compiling') {
      b.compiled += 1;
      b.lastCrate = c.name;
      if (c.name === 'yap' && b.m == null && !b.libStart) {
        b.libStart = now; // no progress bar: the "Compiling yap" line is all there is
      }
      return;
    }
    // progress
    if (b.n0 == null) {
      b.n0 = c.n;
      b.firstProgressAt = now;
    }
    b.n = c.n;
    b.m = c.m;
    if (c.crates.length) b.crates = c.crates;
    if (!b.libStart && c.crates.some(isYapLib)) {
      b.libStart = now;
      b.nAtLib = c.n;
    }
    if (!b.linkStart && c.crates.some(isYapBin)) {
      b.linkStart = now;
      if (!b.libStart) b.libStart = now;
    }
  }

  finishBuild(c, now) {
    const b = this.build;
    b.finishedAt = now;
    b.took = c.took || null;
    this.builds += 1;
    const libMs = b.libStart && b.linkStart ? b.linkStart - b.libStart : null;
    const linkMs = b.linkStart ? now - b.linkStart : null;
    this.lastBuild = {
      ms: now - b.start,
      libMs,
      linkMs,
      units: b.m,
      took: b.took,
      rebuild: b.rebuild,
    };
    if (this.pendingRebuild) {
      this.pushRecent({ at: this.pendingRebuild.at, kind: 'rust', what: this.pendingRebuild.file, ms: now - b.start });
      this.pendingRebuild = null;
    }
    const parts = [`build finished in ${fmtDuration(now - b.start)}`];
    if (b.m) parts.push(`${b.m} units`);
    if (libMs != null) parts.push(`yap ${fmtDuration(libMs)}`);
    if (linkMs != null) parts.push(`link ${fmtDuration(linkMs)}`);
    this.note(now, parts.join(' · '));
  }

  /** What to remember for the next build's estimate (only what this one measured). */
  measured() {
    const b = this.build;
    if (!b || !b.finishedAt) return null;
    const out = { buildMs: b.finishedAt - b.start };
    // (under a second it wasn't really that step: a check build, a cached one)
    if (b.libStart && b.linkStart && b.linkStart - b.libStart >= 1000) out.libMs = b.linkStart - b.libStart;
    if (b.linkStart && b.finishedAt - b.linkStart >= 1000) out.linkMs = b.finishedAt - b.linkStart;
    if (b.libStart && b.firstProgressAt != null && b.nAtLib != null) {
      const units = b.nAtLib - b.n0;
      if (units >= 30) out.msPerUnit = Math.round((b.libStart - b.firstProgressAt) / units);
    }
    if (b.m) out.units = b.m;
    return out;
  }

  pushRecent(item) {
    this.hot.recent.unshift(item);
    if (this.hot.recent.length > 8) this.hot.recent.length = 8;
  }

  toRunning(now) {
    this.runningSince = this.launchAt ?? now;
    if (!this.firstRunningAt) this.firstRunningAt = now;
    this.setStage('running', now);
    this.note(now, 'Yap is running');
  }

  fail(text, now, extra = {}) {
    if (this.stage === 'error') return;
    this.errors = {
      stage: this.stage,
      lines: [text],
      accessDenied: !!extra.accessDenied,
      port: extra.port ?? null,
      at: now,
    };
    this.setStage('error', now);
    this.note(now, `stopped at an error while ${this.errors.stage === 'vite' ? 'starting Vite' : this.errors.stage}`);
  }

  /** Time passing: a started yap.exe counts as running after a moment even if it logs nothing. */
  tick(now) {
    if (this.stage === 'launching' && now - this.launchAt >= 2500) this.toRunning(now);
  }

  stopping(now) {
    if (this.stage === 'exited') return;
    this.setStage('stopping', now);
    this.note(now, 'stopping (Ctrl+C)');
  }

  exited(code, now, byUser = false) {
    this.exit = { code, at: now, byUser, stage: this.stage };
    this.setStage('exited', now);
    this.note(now, byUser ? 'stopped by Ctrl+C' : `tauri dev exited with code ${code}`);
  }

  /**
   * How far the build is: a fraction for the bar (null = no idea yet), the
   * phase, and for the slow last steps their elapsed and expected times.
   * Dependencies count by cargo's N/M; yap's own crate and the link by time
   * against the last build here, so the bar keeps moving through them.
   */
  progress(now) {
    const b = this.build;
    if (!b) return null;
    const h = this.history;
    const libExp = h.libMs || DEFAULT_LIB_MS;
    const linkExp = h.linkMs || DEFAULT_LINK_MS;
    const phase = b.finishedAt ? 'done' : b.linkStart ? 'link' : b.libStart ? 'lib' : b.m != null || b.compiled ? 'deps' : 'waiting';
    const out = {
      phase,
      n: b.n,
      m: b.m,
      compiled: b.compiled,
      crates: b.crates,
      lastCrate: b.lastCrate,
      elapsed: now - b.start,
      waitingLock: b.waitingLock,
      frac: null,
      eta: null,
      step: null, // { label, elapsed, expected, known }
    };
    if (phase === 'done') {
      out.frac = 1;
      return out;
    }
    if (phase === 'lib') out.step = { label: 'Compiling yap', elapsed: now - b.libStart, expected: libExp, known: !!h.libMs };
    if (phase === 'link') out.step = { label: 'Linking yap.exe', elapsed: now - b.linkStart, expected: linkExp, known: !!h.linkMs };
    if (b.m == null) {
      if (b.libStart) {
        // no progress bar, but yap itself is compiling: time alone
        out.frac = Math.min(0.97, (now - b.libStart) / (libExp + linkExp));
      }
      return out;
    }
    const depsTodo = Math.max(0, b.m - b.n0 - YAP_UNITS);
    const depsDone = b.libStart ? depsTodo : Math.min(depsTodo, Math.max(0, b.n - b.n0));
    const msPerUnit = h.msPerUnit || DEFAULT_MS_PER_UNIT;
    const depsExp = depsTodo * msPerUnit;
    const depsP = depsTodo ? depsDone / depsTodo : 1;
    const libP = b.linkStart ? 1 : b.libStart ? Math.min(0.95, (now - b.libStart) / libExp) : 0;
    const linkP = b.linkStart ? Math.min(0.97, (now - b.linkStart) / linkExp) : 0;
    const total = depsExp + libExp + linkExp;
    out.frac = (depsP * depsExp + libP * libExp + linkP * linkExp) / total;
    // how long is left, while the build keeps to the last one's pace (once a step
    // runs over, the detail line says so instead of "0:00 left")
    if (h.libMs && h.linkMs && !(out.step && out.step.elapsed > out.step.expected)) {
      const libLeft = b.linkStart ? 0 : b.libStart ? Math.max(0, libExp - (now - b.libStart)) : libExp;
      const linkLeft = b.linkStart ? Math.max(0, linkExp - (now - b.linkStart)) : linkExp;
      const depsLeft = (depsTodo - depsDone) * msPerUnit;
      out.eta = depsLeft + libLeft + linkLeft;
    }
    return out;
  }
}

/** 0:42, 2:03, 1:02:03 */
export function fmtDuration(ms) {
  const s = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}

// ── dev-update.ps1 ──────────────────────────────────────────────────────────

/**
 * dev-update.ps1's output read back: up to date, updated (its new commits),
 * or skipped and why; whether npm install ran; the commit it runs.
 */
export function parseUpdate(raw) {
  const r = {
    status: 'unknown', // checking | uptodate | updated | skipped | unknown
    reason: null,
    commits: [],
    files: [],
    npm: false,
    running: null, // { hash, title }
    notes: [],
    last: null, // the latest line it said, for the live screen
  };
  let list = null; // 'commits' | 'files'
  for (const line0 of stripAnsi(raw).split(/\r?\n|\r/)) {
    const line = line0.replace(/\s+$/, '');
    if (!line.trim()) continue;
    let m;
    if (list === 'commits' && (m = /^\s{3,}([0-9a-f]{7,12})\s+(.*)$/.exec(line))) {
      r.commits.push({ hash: m[1], title: m[2] });
      continue;
    }
    if (list === 'files' && (m = /^\s{3,}(\S{1,2})\s+(.+)$/.exec(line))) {
      r.files.push(m[2]);
      continue;
    }
    list = null;
    const t = line.trim();
    if (/^Not a git checkout/.test(t)) {
      r.status = 'skipped';
      r.reason = 'not a git checkout';
    } else if ((m = /^On branch '(.+)', not main/.exec(t))) {
      r.status = 'skipped';
      r.reason = `on branch '${m[1]}', not main`;
    } else if (/has local edits/.test(t)) {
      r.status = 'skipped';
      r.reason = 'this checkout has local edits';
      list = 'files';
    } else if (/^Checking GitHub/.test(t)) {
      r.status = 'checking';
    } else if (/^Couldn't reach GitHub/.test(t)) {
      r.status = 'skipped';
      r.reason = "couldn't reach GitHub";
    } else if (/^Couldn't move to the latest/.test(t)) {
      r.status = 'skipped';
      r.reason = "couldn't fast-forward to main";
    } else if (/^Already up to date/.test(t)) {
      r.status = 'uptodate';
    } else if (/^Updated to the latest version/.test(t)) {
      r.status = 'updated';
      list = 'commits';
    } else if (/^Installing updated dependencies/.test(t)) {
      r.npm = true;
    } else if ((m = /^Running ([0-9a-f]{7,12}) (.*)$/.exec(t))) {
      r.running = { hash: m[1], title: m[2] };
    } else if ((m = /^Update skipped: (.*)$/.exec(t))) {
      r.status = 'skipped';
      r.reason = m[1];
    } else if (/^Stopped \d+ AI-app helper/.test(t) || /^Couldn't check for AI-app helpers/.test(t)) {
      r.notes.push(t);
    }
    r.last = t;
  }
  return r;
}
