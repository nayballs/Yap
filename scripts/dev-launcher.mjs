#!/usr/bin/env node
// Yap - Dev's screen: runs `tauri dev` (or dev-update.ps1, with --update)
// behind an animated console screen instead of a wall of log lines. The Yap
// mark yaps while it works; the stages show one by one (GitHub → Vite → Rust,
// with a real build bar from cargo's own progress → launching → running);
// L flips to the raw log, O opens the log file, Ctrl+C stops Yap and every
// helper under it. Build errors stop the animation and show in red.
//
// Called by scripts/dev.bat; plain Node, no dependencies.
//
//   node scripts/dev-launcher.mjs [options] -- <command...>
//   node scripts/dev-launcher.mjs [options] --cmd "<command>"
//   node scripts/dev-launcher.mjs [options] --replay <recording.jsonl|.txt> [--speed N]
//
//   --update         run the command as the "Updating from GitHub" step and hand its
//                    result to the next run (dev.bat's first pass; YAP_DEV_RUN pairs them)
//   --stub           the stub build (no Vulkan SDK): say so on screen
//   --bat <path>     dev.bat itself, for R (restart without administrator rights)
//   --record <file>  also save the child's output with its timing, for --replay
//   --plain          pass the output straight through (same as YAP_DEV_PLAIN=1)
//
// Environment: YAP_DEV_PLAIN=1 (straight through, as before the launcher),
// YAP_DEV_ALLOW_ADMIN=1 (no "running as administrator" notice),
// YAP_DEV_FRAMES=<dir> (also save each screen as plain text, for checking it),
// YAP_DEV_LAUNCHER_HOME=<dir> (where logs and timings go; default
// %LOCALAPPDATA%\com.yap.dictation\dev-launcher).

import { spawn, execFile, execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { StringDecoder } from 'node:string_decoder';
import { DevState, LineSplitter, parseUpdate, stripAnsi } from './dev-launcher/parse.mjs';
import { chrome, drawLog, drawMain, drawSummary, waveSample } from './dev-launcher/draw.mjs';

const RESTARTED = 75; // dev.bat closes its window on this: R started a fresh one

// ── arguments ───────────────────────────────────────────────────────────────

function quoteArgs(args) {
  return args.map((a) => (a === '' || /[\s"&|<>^()]/.test(a) ? `"${a.replace(/"/g, '""')}"` : a)).join(' ');
}

function parseArgs(argv) {
  const o = { update: false, stub: false, plain: false, record: null, replay: null, speed: 1, cmd: null, bat: null, pretendAdmin: false, headless: null };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--') {
      o.cmd = quoteArgs(argv.slice(i + 1));
      break;
    } else if (a === '--update') o.update = true;
    else if (a === '--stub') o.stub = true;
    else if (a === '--plain') o.plain = true;
    else if (a === '--cmd') o.cmd = argv[++i];
    else if (a === '--replay') o.replay = argv[++i];
    else if (a === '--speed') o.speed = Math.max(0.05, Number(argv[++i]) || 1);
    else if (a === '--record') o.record = argv[++i];
    else if (a === '--bat') o.bat = argv[++i];
    else if (a === '--pretend-admin') o.pretendAdmin = true; // to see the notice without elevating
    else if (a === '--headless') {
      // draw at this size without a console (with YAP_DEV_FRAMES, to check the screens)
      const m = /^(\d+)x(\d+)$/.exec(argv[++i] || '');
      o.headless = m ? [+m[1], +m[2]] : [120, 30];
    } else {
      process.stderr.write(`dev-launcher: unknown option ${a}\n`);
      process.exit(2);
    }
  }
  if (!o.cmd && !o.replay) {
    process.stderr.write('dev-launcher: nothing to run (-- <command>, --cmd or --replay)\n');
    process.exit(2);
  }
  return o;
}

const opts = parseArgs(process.argv.slice(2));
const headless = opts.headless;
const plain = !headless && (opts.plain || process.env.YAP_DEV_PLAIN === '1' || !process.stdout.isTTY);

// ── plain: straight through, as dev.bat did before ──────────────────────────

if (plain) runPlain();
else runScreen();

function runPlain() {
  if (opts.replay) {
    const items = loadRecording(opts.replay);
    let i = 0;
    const start = Date.now();
    const step = () => {
      const el = (Date.now() - start) * opts.speed;
      while (i < items.length && items[i].t <= el) {
        const it = items[i++];
        if ('exit' in it) process.exit(it.exit);
        if (it.d) process.stdout.write(it.d);
      }
      if (i < items.length) setTimeout(step, Math.min(250, Math.max(0, (items[i].t - el) / opts.speed)));
    };
    step();
    return;
  }
  const child = spawn(opts.cmd, { shell: true, stdio: 'inherit' });
  // Ctrl+C reaches every process in the console by itself, as before: wait for them.
  process.on('SIGINT', () => {});
  process.on('SIGBREAK', () => {});
  child.on('error', (e) => {
    process.stderr.write(`dev-launcher: couldn't start ${opts.cmd}: ${e.message}\n`);
    process.exit(1);
  });
  child.on('exit', (code) => process.exit(opts.update ? 0 : exitCodeFor(code)));
}

function exitCodeFor(code) {
  if (code == null) return 1;
  return code === RESTARTED ? 1 : code;
}

/** A recording: JSON lines `{t, d, s?}` / `{t, exit}` / `{t, key}`, or plain text (a line every 40 ms). */
function loadRecording(file) {
  const raw = fs.readFileSync(file, 'utf8');
  if (/\.jsonl$/i.test(file)) {
    return raw
      .split(/\r?\n/)
      .filter((l) => l.trim())
      .map((l) => JSON.parse(l));
  }
  const lines = raw.split(/\r?\n/);
  return [...lines.map((l, i) => ({ t: i * 40, d: l + '\n' })), { t: lines.length * 40, exit: 0 }];
}

// ── the screen ──────────────────────────────────────────────────────────────

function runScreen() {
  const home =
    process.env.YAP_DEV_LAUNCHER_HOME ||
    (opts.replay
      ? path.join(os.tmpdir(), 'yap-dev-launcher-replay')
      : path.join(process.env.LOCALAPPDATA || path.join(os.homedir(), 'AppData', 'Local'), 'com.yap.dictation', 'dev-launcher'));
  try {
    fs.mkdirSync(path.join(home, 'logs'), { recursive: true });
  } catch {}

  const t0 = Date.now();
  const speed = opts.replay ? opts.speed : 1;
  const now = () => t0 + (Date.now() - t0) * speed; // a replay's clock runs at its speed
  const animT = () => (Date.now() - t0) / 1000;

  const profile = opts.replay ? 'replay' : opts.stub ? 'stub' : /--features\s+\S*engines/.test(opts.cmd || '') ? 'engines' : 'default';
  const timingsFile = path.join(home, 'timings.json');
  const timings = readJson(timingsFile) || { version: 1, profiles: {} };
  if (!timings.profiles) timings.profiles = {};
  const state = new DevState({ now: now(), history: timings.profiles[profile] || {} });

  const handoffFile = path.join(home, 'last-update.json');
  const run = process.env.YAP_DEV_RUN || null;
  let handed = null;
  if (!opts.update && run) {
    const h = readJson(handoffFile);
    if (h && h.run === run) handed = h;
  }
  if (handed?.aborted) {
    // Ctrl+C while updating: don't start Yap
    process.stdout.write('\x1b[0m\x1b[?25h\x1b[?1049l\n  Stopped before Yap started.\n\n', () => process.exit(0));
    return;
  }

  const ui = {
    mode: opts.update ? 'update' : 'dev',
    view: 'main',
    commit: currentCommit(),
    update: opts.update ? null : handed?.result || null,
    updateDone: false,
    updateTail: null,
    stub: opts.stub,
    admin: false,
    logShown: null,
    logFull: null,
    logLines: [],
    logScroll: 0,
    wave: [],
    waveKick: 0,
    yapUntil: 0,
    message: null,
    messageUntil: 0,
  };
  if (ui.update?.running) ui.commit = ui.update.running;

  // Elevated? (Windows then refuses drags from Explorer into Yap: UIPI.)
  if (!opts.update && process.env.YAP_DEV_ALLOW_ADMIN !== '1') {
    if (opts.pretendAdmin) ui.admin = true;
    else if (process.platform === 'win32') {
      execFile('whoami', ['/groups'], { windowsHide: true, timeout: 5000 }, (err, out) => {
        if (!err && /S-1-16-12288|S-1-16-16384/.test(String(out))) {
          ui.admin = true;
          state.note(now(), 'running as administrator');
        }
      });
    }
  }

  // ── the log file ──
  let logFd = null;
  let logBuf = '';
  if (!opts.update) {
    pruneLogs(path.join(home, 'logs'), 20);
    const logFile = path.join(home, 'logs', `dev-${stamp(new Date())}.log`);
    try {
      logFd = fs.openSync(logFile, 'a');
      ui.logFull = logFile;
      const la = process.env.LOCALAPPDATA;
      ui.logShown = la && logFile.toLowerCase().startsWith(la.toLowerCase()) ? '%LOCALAPPDATA%' + logFile.slice(la.length) : logFile;
    } catch {}
    const head = [
      `Yap - Dev launcher log, ${new Date().toString()}`,
      `command  ${opts.replay ? `replay of ${opts.replay}` : opts.cmd}`,
      `folder   ${process.cwd()}`,
      `commit   ${ui.commit ? `${ui.commit.hash} ${ui.commit.title}` : 'unknown'}`,
      '',
    ];
    if (handed?.raw) head.push('---- dev-update.ps1 ----', handed.raw.trimEnd(), '');
    head.push('---- tauri dev ----', '');
    logBuf += head.join('\n') + '\n';
  }
  const flushLog = () => {
    if (logFd == null || !logBuf) return;
    try {
      fs.writeSync(logFd, logBuf);
    } catch {}
    logBuf = '';
  };
  const pushLog = (text, c) => {
    logBuf += text + '\n';
    ui.logLines.push({ text, kind: c.kind, level: c.level });
    if (ui.logLines.length > 4000) ui.logLines.splice(0, ui.logLines.length - 3000);
    if (ui.logScroll) ui.logScroll = Math.min(ui.logScroll + 1, ui.logLines.length);
  };
  const drainEvents = () => {
    while (state.events.length) {
      const e = state.events.shift();
      const line = `[launcher ${clockOf(e.at)}] ${e.text}`;
      logBuf += line + '\n';
      ui.logLines.push({ text: line, kind: 'launcher' });
    }
  };

  let recordFd = null;
  if (opts.record) {
    try {
      recordFd = fs.openSync(opts.record, 'w');
    } catch {}
  }

  // ── terminal ──
  let restored = false;
  let fallen = false; // the screen broke: raw output from here on
  const out = (s) => {
    if (!restored && !headless) process.stdout.write(s);
  };
  out('\x1b[?1049h\x1b[?25l\x1b[2J\x1b[H');
  const restoreSeq = '\x1b[0m\x1b]9;4;0;0\x07\x1b]0;Yap Dev (stopped)\x07\x1b[?25h\x1b[?1049l';
  const restore = () => {
    if (restored) return;
    restored = true;
    if (headless) return;
    try {
      if (process.stdin.isTTY) process.stdin.setRawMode(false);
      process.stdin.pause();
    } catch {}
    process.stdout.write(restoreSeq);
  };
  process.on('exit', () => {
    killTree(true);
    if (!restored && !handoffKeepsScreen && !headless) {
      try {
        fs.writeSync(1, restoreSeq);
      } catch {}
    }
  });
  let handoffKeepsScreen = false;

  // ── rendering ──
  let prevRows = [];
  let prevSize = '';
  let prevTitle = '';
  let prevProgress = '';
  let lastCanvas = null;
  const model = () => ({ state, ui, t: animT(), now: now() });
  const size = () => {
    if (headless) return headless;
    try {
      const [w, h] = process.stdout.getWindowSize();
      return [w, h];
    } catch {
      return [process.stdout.columns || 80, process.stdout.rows || 25];
    }
  };
  function render() {
    if (restored || fallen) return;
    // a console paused mid-selection (QuickEdit) stops taking writes: don't pile frames up
    if (process.stdout.writableLength > 256 * 1024) return;
    const [W, H] = size();
    let s = '';
    const key = `${W}x${H}`;
    if (key !== prevSize) {
      s += '\x1b[0m\x1b[2J';
      prevRows = [];
      prevSize = key;
    }
    if (W < 24 || H < 8) {
      if (prevRows[0] !== 'tiny') {
        s += `\x1b[1;1HYap Dev: ${state.stage} (make the window bigger)`;
        prevRows = ['tiny'];
      }
      out(s);
      return;
    }
    const m = model();
    const cv =ui.view === 'log' && ui.mode === 'dev' ? drawLog(m, W, H) : drawMain(m, W, H);
    lastCanvas = cv;
    for (let y = 0; y < H; y++) {
      const row = cv.rowAnsi(y, { skipLastCell: y === H - 1 });
      if (row !== prevRows[y]) {
        s += `\x1b[${y + 1};1H${row}`;
        prevRows[y] = row;
      }
    }
    const ch = chrome(m);
    if (ch.title !== prevTitle) {
      s += `\x1b]0;${ch.title}\x07`;
      prevTitle = ch.title;
    }
    const pr = ch.progress.join(';');
    if (pr !== prevProgress) {
      s += `\x1b]9;4;${pr}\x07`;
      prevProgress = pr;
    }
    if (s) out(`\x1b[?2026h${s}\x1b[?2026l`);
  }

  // YAP_DEV_FRAMES: each screen as plain text (on a stage change, else at most once a second)
  const framesDir = process.env.YAP_DEV_FRAMES || null;
  let frameSeq = 0;
  let lastFrame = '';
  let lastFrameAt = 0;
  let lastFrameKey = '';
  if (framesDir) fs.mkdirSync(framesDir, { recursive: true });
  function saveFrame(cv, name, force = false) {
    if (!framesDir || !cv) return;
    const key = name;
    if (!force && key === lastFrameKey && Date.now() - lastFrameAt < 1000) return;
    const rows = [];
    for (let y = 0; y < cv.h; y++) rows.push(cv.rowPlain(y));
    const text = rows.join('\n').replace(/\s+$/, '') + '\n';
    if (text === lastFrame) return;
    lastFrame = text;
    lastFrameAt = Date.now();
    lastFrameKey = key;
    frameSeq += 1;
    fs.writeFileSync(path.join(framesDir, `${String(frameSeq).padStart(4, '0')}-${name}.txt`), text);
  }

  // ── the child (or a recording) ──
  const env = {
    ...process.env,
    CARGO_TERM_PROGRESS_WHEN: 'always',
    CARGO_TERM_PROGRESS_WIDTH: process.env.CARGO_TERM_PROGRESS_WIDTH || '100',
  };
  const streams = {
    out: { dec: new StringDecoder('utf8'), split: new LineSplitter() },
    err: { dec: new StringDecoder('utf8'), split: new LineSplitter() },
  };
  let updateRaw = '';
  let savedFor = null;

  function onChunk(which, data) {
    if (recordFd != null) {
      try {
        fs.writeSync(recordFd, JSON.stringify({ t: Date.now() - t0, s: which, d: String(data) }) + '\n');
      } catch {}
    }
    if (fallen) {
      process.stdout.write(data);
      return;
    }
    const st = streams[which];
    const text = typeof data === 'string' ? data : st.dec.write(data);
    for (const seg of st.split.push(text)) onSegment(seg);
  }

  function onSegment(seg) {
    const t = now();
    if (opts.update) {
      if (seg.transient) return;
      const line = stripAnsi(seg.text);
      updateRaw += line + '\n';
      ui.update = parseUpdate(updateRaw);
      if (ui.update.running) ui.commit = ui.update.running;
      if (ui.update.npm && line.trim() && !/^Installing updated dependencies/.test(line.trim())) ui.updateTail = line.trim();
      return;
    }
    const before = state.stage;
    const { text, c } = state.apply(seg, t);
    if (!seg.transient && c.kind !== 'progress') pushLog(text, c);
    if (c.kind === 'vite-hmr') {
      ui.yapUntil = animT() + 1.3; // a hot reload gets a little yap
      ui.waveKick = 0.7;
    }
    if (state.stage === 'error' && before !== 'error') ui.view = 'main';
    // a finished build: remember its timings for the next one's estimate
    const b = state.build;
    if (b?.finishedAt && savedFor !== b) {
      savedFor = b;
      const measured = state.measured();
      if (measured) {
        Object.assign(state.history, measured);
        timings.profiles[profile] = { ...(timings.profiles[profile] || {}), ...measured, at: new Date().toISOString() };
        writeJson(timingsFile, timings);
      }
    }
    drainEvents();
  }

  let child = null;
  let replay = null;
  let exitCode = null;
  let finished = false;
  let stopping = false;
  let restarting = false;

  if (opts.replay) {
    replay = startReplay(loadRecording(opts.replay));
  } else {
    child = spawn(opts.cmd, { shell: true, env, stdio: ['ignore', 'pipe', 'pipe'] });
    child.stdout.on('data', (d) => onChunk('out', d));
    child.stderr.on('data', (d) => onChunk('err', d));
    child.on('error', (e) => {
      onSegment({ text: `error: couldn't start ${opts.cmd}: ${e.message}`, transient: false });
      finish(1, false);
    });
    let closed = false;
    child.on('close', () => {
      closed = true;
      finish(exitCode ?? child.exitCode, stopping);
    });
    child.on('exit', (code) => {
      exitCode = code;
      // its helpers may hold the pipes a moment longer: don't wait on them for ever
      setTimeout(() => {
        if (!closed) finish(code, stopping);
      }, 1500).unref();
    });
  }

  function startReplay(items) {
    let i = 0;
    let timer = null;
    const start = Date.now();
    const step = () => {
      const el = (Date.now() - start) * speed;
      while (i < items.length && items[i].t <= el) {
        const it = items[i++];
        if ('exit' in it) {
          finish(it.exit, false);
          return;
        }
        if (it.key) onKey(it.key);
        if (it.d) onChunk(it.s === 'err' ? 'err' : 'out', it.d);
      }
      if (i < items.length) timer = setTimeout(step, Math.min(250, Math.max(0, (items[i].t - el) / speed)));
      else finish(0, false);
    };
    timer = setTimeout(step, 0);
    return {
      stop() {
        clearTimeout(timer);
      },
    };
  }

  function killTree(sync = false) {
    if (!child || child.exitCode != null || child.signalCode != null) return;
    if (process.platform === 'win32') {
      const args = ['/PID', String(child.pid), '/T', '/F'];
      if (sync) spawnSync('taskkill', args, { stdio: 'ignore', windowsHide: true });
      else spawn('taskkill', args, { stdio: 'ignore', windowsHide: true }).on('error', () => {});
    } else {
      try {
        child.kill('SIGTERM');
      } catch {}
    }
  }

  function stop() {
    if (finished) return;
    if (stopping) {
      // a second Ctrl+C: don't wait any more
      killTree(true);
      finish(exitCode, true);
      return;
    }
    stopping = true;
    state.stopping(now());
    ui.view = 'main';
    render();
    if (replay) {
      replay.stop();
      finish(null, true);
      return;
    }
    killTree();
    setTimeout(() => finish(exitCode, true), 8000).unref();
  }

  function restartAsUser() {
    if (!ui.admin || stopping) return;
    if (!opts.bat) {
      say("R needs dev.bat's path (--bat): close this window and double-click Yap - Dev instead.");
      return;
    }
    restarting = true;
    say('Restarting Yap - Dev without administrator rights…');
    stop();
  }

  function say(text, ms = 4000) {
    ui.message = text;
    ui.messageUntil = Date.now() + ms;
    render();
  }

  function openLog() {
    if (!ui.logFull) return;
    flushLog();
    try {
      spawn('notepad.exe', [ui.logFull], { detached: true, stdio: 'ignore' }).on('error', () => {}).unref();
      say('Opened the log in Notepad (it shows the log as it was just now).');
    } catch {}
  }

  function finish(code, byUser) {
    if (finished) return;
    finished = true;
    clearInterval(timer);
    for (const which of ['out', 'err']) {
      const st = streams[which];
      const rest = st.dec.end();
      for (const seg of [...st.split.push(rest), ...st.split.flush()]) onSegment(seg);
    }
    if (recordFd != null) {
      try {
        fs.writeSync(recordFd, JSON.stringify({ t: Date.now() - t0, exit: code ?? 1 }) + '\n');
        fs.closeSync(recordFd);
      } catch {}
    }
    if (fallen) {
      flushLog();
      process.exit(opts.update ? 0 : exitCodeFor(code));
    }

    if (opts.update) {
      const result = parseUpdate(updateRaw);
      writeJson(handoffFile, { run, at: new Date().toISOString(), aborted: !!byUser, result, raw: updateRaw });
      if (byUser) {
        restore();
        process.stdout.write('\n  Stopped while updating.\n', () => process.exit(0));
        return;
      }
      // leave the screen as it is: the launcher dev.bat starts next takes over from here
      ui.updateDone = true;
      ui.update = result;
      render();
      saveFrame(lastCanvas, 'update-done', true);
      handoffKeepsScreen = true;
      try {
        if (process.stdin.isTTY) process.stdin.setRawMode(false);
        process.stdin.pause();
      } catch {}
      process.stdout.write('', () => process.exit(0));
      return;
    }

    state.exited(code, now(), byUser);
    drainEvents();
    flushLog();
    if (framesDir) {
      const [W, H] = size();
      saveFrame(drawMain(model(), W, H), 'exited', true);
    }
    if (restarting && opts.bat) {
      // through the shell, so it starts as the signed-in user, not elevated
      try {
        spawn('explorer.exe', [opts.bat], { detached: true, stdio: 'ignore' }).on('error', () => {}).unref();
      } catch {}
      restore();
      if (headless) process.exit(RESTARTED);
      process.stdout.write('\n  Yap - Dev is starting again without administrator rights, in a new window.\n', () =>
        process.exit(RESTARTED),
      );
      return;
    }
    restore();
    const [W] = size();
    const cv = drawSummary(model(), Math.max(40, Math.min(W, 140)));
    if (framesDir) saveFrame(cv, 'summary', true);
    let s = '\n';
    for (let y = 0; y < cv.h; y++) s += cv.rowAnsi(y, { transparent: true }) + '\n';
    if (logFd != null) {
      try {
        fs.closeSync(logFd);
      } catch {}
    }
    if (headless) process.exit(exitCodeFor(code));
    process.stdout.write(s, () => process.exit(exitCodeFor(code)));
  }

  // ── keys ──
  function onKey(k) {
    if (k === '\x03') return stop();
    if (finished) return;
    if (ui.view === 'log') {
      const page = Math.max(1, size()[1] - 4);
      const max = Math.max(0, ui.logLines.length - 1);
      const moves = { '\x1b[A': 1, '\x1b[B': -1, '\x1b[5~': page, '\x1b[6~': -page };
      if (k in moves) {
        ui.logScroll = Math.max(0, Math.min(max, ui.logScroll + moves[k]));
        return render();
      }
      if (k === '\x1b[F' || k === '\x1b[4~' || k === '\x1bOF') {
        ui.logScroll = 0;
        return render();
      }
      if (k === '\x1b[H' || k === '\x1b[1~' || k === '\x1bOH') {
        ui.logScroll = max;
        return render();
      }
    }
    if (k.startsWith('\x1b')) return;
    for (const ch of k) {
      const c = ch.toLowerCase();
      if (c === 'l' && ui.mode === 'dev') {
        ui.view = ui.view === 'log' ? 'main' : 'log';
        ui.logScroll = 0;
        render();
      } else if (c === 'o' && ui.mode === 'dev') openLog();
      else if (c === 'r' && ui.mode === 'dev') restartAsUser();
    }
  }
  if (process.stdin.isTTY && !headless) {
    try {
      process.stdin.setRawMode(true);
      process.stdin.setEncoding('utf8');
      process.stdin.on('data', onKey);
      process.stdin.resume();
    } catch {}
  } else if (headless && !process.stdin.isTTY) {
    // a test drives the keys through a pipe
    process.stdin.setEncoding('utf8');
    process.stdin.on('data', onKey);
    process.stdin.on('error', () => {});
  }
  process.on('SIGINT', stop);
  process.on('SIGBREAK', stop);
  // the console window closing: take Yap and its helpers down with it, quickly
  process.on('SIGHUP', () => {
    killTree(true);
    process.exit(1);
  });

  // ── a UI bug never takes Yap down: fall back to the raw output ──
  process.on('uncaughtException', (err) => {
    if (fallen) {
      process.stderr.write(`dev-launcher: ${err?.stack || err}\n`);
      return;
    }
    fallen = true;
    clearInterval(timer);
    try {
      restore();
    } catch {}
    process.stderr.write(`\ndev-launcher: the screen broke (${err?.message || err}); showing the raw output instead.\n\n`);
    for (const l of ui.logLines.slice(-30)) process.stdout.write(l.text + '\n');
    if (opts.update) {
      try {
        writeJson(handoffFile, { run, at: new Date().toISOString(), aborted: false, result: parseUpdate(updateRaw), raw: updateRaw });
      } catch {}
    }
    process.removeAllListeners('SIGINT');
    process.on('SIGINT', () => {
      killTree(true);
      process.exit(130);
    });
  });

  // ── the clock ──
  const timer = setInterval(() => {
    if (finished || fallen) return;
    const t = now();
    state.tick(t);
    drainEvents();
    const m = model();
    ui.wave.push(waveSample(m));
    if (ui.wave.length > 80) ui.wave.splice(0, ui.wave.length - 80);
    ui.waveKick *= 0.82;
    if (ui.message && Date.now() > ui.messageUntil) ui.message = null;
    render();
    flushLog();
    const name = ui.mode === 'update' ? 'update' : `${state.stage}${ui.view === 'log' ? '-log' : ''}`;
    saveFrame(lastCanvas, name);
  }, 90);
  render();
}

// ── small helpers ───────────────────────────────────────────────────────────

function readJson(file) {
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8').replace(/^﻿/, ''));
  } catch {
    return null;
  }
}

function writeJson(file, value) {
  try {
    const tmp = `${file}.${process.pid}.tmp`;
    fs.writeFileSync(tmp, JSON.stringify(value, null, 2));
    fs.renameSync(tmp, file);
  } catch {}
}

function currentCommit() {
  try {
    const out = execFileSync('git', ['log', '-1', '--format=%h%x09%s'], { encoding: 'utf8', timeout: 4000, windowsHide: true, stdio: ['ignore', 'pipe', 'ignore'] });
    const [hash, ...rest] = out.trim().split('\t');
    return hash ? { hash, title: rest.join('\t') } : null;
  } catch {
    return null;
  }
}

function stamp(d) {
  const p = (n) => String(n).padStart(2, '0');
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}_${p(d.getHours())}-${p(d.getMinutes())}-${p(d.getSeconds())}`;
}

function clockOf(ms) {
  const d = new Date(ms);
  const p = (n) => String(n).padStart(2, '0');
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

function pruneLogs(dir, keep) {
  try {
    const files = fs
      .readdirSync(dir)
      .filter((f) => /^dev-.*\.log$/.test(f))
      .sort();
    for (const f of files.slice(0, Math.max(0, files.length - keep + 1))) fs.unlinkSync(path.join(dir, f));
  } catch {}
}
