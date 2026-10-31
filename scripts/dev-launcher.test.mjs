// node --test scripts/dev-launcher.test.mjs
// The dev launcher's reading of tauri dev's output (and its screens, as smoke).
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { DevState, LineSplitter, classify, fmtDuration, parseUpdate, stripAnsi } from './dev-launcher/parse.mjs';
import { drawLog, drawMain, drawSummary, logoPixels, strWidth, truncate, wrapSegs } from './dev-launcher/draw.mjs';

const G = '\x1b[1m\x1b[92m';
const Z = '\x1b[0m';
const bar = (n, m, crates) => `\x1b[1m\x1b[96m    Building\x1b[0m [=======================> ] ${n}/${m}: ${crates}   \r`;

/** Feeds [ms, text] pairs through a splitter into a DevState; returns the stages it went through. */
function feed(state, script, splitter = new LineSplitter()) {
  const stages = [state.stage];
  for (const [t, text] of script) {
    for (const seg of splitter.push(text)) {
      state.apply(seg, t);
      if (state.stage !== stages[stages.length - 1]) stages.push(state.stage);
    }
    state.tick(t);
    if (state.stage !== stages[stages.length - 1]) stages.push(state.stage);
  }
  return stages;
}

test('stripAnsi takes colours, erase-line, OSC and a BOM out', () => {
  assert.equal(stripAnsi(`\uFEFF${G}   Compiling${Z} serde v1.0.0\x1b[K`), '   Compiling serde v1.0.0');
  assert.equal(stripAnsi('\x1b]0;title\x07plain'), 'plain');
});

test('LineSplitter: redraws are transient, CRLF is one line end even across chunks', () => {
  const s = new LineSplitter();
  assert.deepEqual(s.push('a\r\nb\rc\n'), [
    { text: 'a', transient: false },
    { text: 'b', transient: true },
    { text: 'c', transient: false },
  ]);
  // cargo's last progress line ends a chunk with \r and then nothing comes for a minute
  assert.deepEqual(s.push('  659/660: yap(bin)\r'), [{ text: '  659/660: yap(bin)', transient: true }]);
  // ...but when it was half of a CRLF, the line comes again as a finished one
  const t = new LineSplitter();
  assert.deepEqual(t.push('done\r'), [{ text: 'done', transient: true }]);
  assert.deepEqual(t.push('\nnext\n'), [
    { text: 'done', transient: false },
    { text: 'next', transient: false },
  ]);
  const u = new LineSplitter();
  u.push('par');
  assert.deepEqual(u.push('tial\n'), [{ text: 'partial', transient: false }]);
  u.push('end');
  assert.deepEqual(u.flush(), [{ text: 'end', transient: false }]);
});

test('classify knows cargo, tauri, Vite and the app', () => {
  assert.deepEqual(classify('    Building [=====>      ] 412/660: plist, windows, icu_normali…'), {
    kind: 'progress',
    n: 412,
    m: 660,
    crates: ['plist', 'windows', 'icu_normali…'],
  });
  assert.equal(classify('   Compiling yap v0.1.1 (C:\\src\\yap\\src-tauri)').name, 'yap');
  assert.equal(classify('    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2m 03s').took, '2m 03s');
  assert.equal(classify('     Running `C:\\t\\debug\\yap.exe`').kind, 'run-exe');
  assert.equal(classify('     Running DevCommand (`cargo  run --no-default-features --color always --`)').kind, 'tauri-run');
  assert.equal(classify('        Info File src-tauri\\src\\lib.rs changed. Rebuilding application...').file, 'src-tauri\\src\\lib.rs');
  assert.equal(classify('  VITE v6.4.5  ready in 412 ms').ms, 412);
  assert.equal(classify('  ➜  Local:   http://localhost:51437/').url, 'http://localhost:51437/');
  assert.equal(classify('12:04:11 PM [vite] (client) hmr update /src/app.css').what, '/src/app.css');
  assert.equal(classify('12:04:11 PM [vite] hmr update /src/App.svelte (x2)').what, '/src/App.svelte');
  assert.equal(classify('2026-10-31T12:00:01.123456Z  WARN yap_lib::calendar: no calendar').level, 'WARN');
  assert.equal(classify('error[E0425]: cannot find value `x` in this scope').code, 'E0425');
  assert.equal(classify('error: could not compile `yap` (lib) due to 1 previous error').kind, 'error-summary');
  assert.equal(classify('error: failed to remove file `C:\\t\\debug\\yap.exe`').kind, 'error');
  assert.equal(classify('  Access is denied. (os error 5)').kind, 'access-denied');
  assert.equal(classify("thread 'main' panicked at src\\main.rs:4:5:").kind, 'panic');
  assert.equal(classify('Error: Port 51437 is already in use').port, 51437);
  assert.equal(classify('warning: unused variable: `x`').kind, 'warning');
  assert.equal(classify('        Warn Waiting for your frontend dev server to start on http://localhost:51437/...').kind, 'waiting-frontend');
});

test('a whole dev session: stages, the build phases, timings, hot reloads', () => {
  const s = new DevState({ now: 0 });
  const stages = feed(s, [
    [10, `${G}     Running${Z} BeforeDevCommand (\`npm run dev\`)\n`],
    [900, '\n  VITE v6.4.5  ready in 412 ms\n\n  ➜  Local:   http://localhost:51437/\n'],
    [1000, `${G}     Running${Z} DevCommand (\`cargo  run --color always --\`)\n`],
    [1500, bar(10, 600, 'serde, tokio')],
    [1600, `                     \r${G}   Compiling${Z} serde v1.0.0\n`],
    [40_000, bar(400, 600, 'tauri, windows')],
    [90_000, bar(598, 600, 'yap')],
    [150_000, bar(599, 600, 'yap(bin)')],
    [200_000, `                     \r${G}    Finished${Z} \`dev\` profile [unoptimized + debuginfo] target(s) in 3m 19s\n`],
    [200_050, `${G}     Running${Z} \`C:\\t\\debug\\yap.exe\`\n`],
    [201_000, '2026-10-31T12:00:01.123456Z  INFO yap_lib::shell::window: main window shown\n'],
    [205_000, '12:04:11 PM [vite] (client) hmr update /src/lib/views/home/HomeView.svelte\n'],
  ]);
  assert.deepEqual(stages, ['vite', 'compiling', 'launching', 'running']);
  assert.equal(s.vite.url, 'http://localhost:51437/');
  assert.equal(s.vite.ms, 412);
  assert.equal(s.lastBuild.units, 600);
  const m = s.measured();
  assert.equal(m.libMs, 60_000);
  assert.equal(m.linkMs, 50_000);
  assert.equal(m.msPerUnit, Math.round((90_000 - 1500) / (598 - 10)));
  assert.equal(s.hot.count, 1);
  assert.equal(s.hot.recent[0].kind, 'ui');

  // a Rust edit: back to the bar, then running again, the rebuild in the recent list
  const again = feed(s, [
    [300_000, '        Info File src-tauri\\src\\lib.rs changed. Rebuilding application...\n'],
    [300_100, bar(598, 600, 'yap')],
    [320_000, bar(599, 600, 'yap(bin)')],
    [330_000, `${G}    Finished${Z} \`dev\` profile [unoptimized + debuginfo] target(s) in 30.00s\n`],
    [330_010, `${G}     Running${Z} \`C:\\t\\debug\\yap.exe\`\n`],
    [334_000, ''],
  ]);
  assert.deepEqual(again, ['running', 'compiling', 'launching', 'running']);
  assert.equal(s.rustRebuilds, 1);
  assert.equal(s.hot.recent[0].kind, 'rust');
  assert.equal(s.hot.recent[0].ms, 30_000);
});

test("the link step keeps moving against the last build's timing, and never reads done before it is", () => {
  const s = new DevState({ now: 0, history: { libMs: 40_000, linkMs: 60_000, msPerUnit: 300 } });
  feed(s, [
    [0, '        Info File src-tauri\\src\\lib.rs changed. Rebuilding application...\n'],
    [100, bar(598, 600, 'yap')],
    [40_100, bar(599, 600, 'yap(bin)')],
  ]);
  const a = s.progress(50_000);
  const b = s.progress(80_000);
  const late = s.progress(500_000);
  assert.equal(a.phase, 'link');
  assert.ok(b.frac > a.frac, 'the bar moves with time while cargo says nothing');
  assert.ok(late.frac < 1, 'a long link still is not "done"');
  assert.equal(a.step.label, 'Linking yap.exe');
  assert.ok(a.step.known);
  assert.equal(Math.round(a.eta / 1000), 50);
  // without an earlier build there's no estimate, but the step still says what it is
  const fresh = new DevState({ now: 0 });
  feed(fresh, [[0, '  VITE v6.4.5  ready in 1 ms\n'], [10, bar(599, 600, 'yap(bin)')]]);
  const p = fresh.progress(5000);
  assert.equal(p.step.known, false);
  assert.equal(p.eta, null);
});

test('a build error stops at the error, keeps its lines, and a rebuild clears it', () => {
  const s = new DevState({ now: 0 });
  const stages = feed(s, [
    [0, '  VITE v6.4.5  ready in 388 ms\n'],
    [100, bar(659, 660, 'yap(bin)')],
    [
      9000,
      '                    \r\x1b[1m\x1b[91merror[E0425]\x1b[0m: cannot find value `widget_count` in this scope\n --> src\\main.rs:5:13\n  |\n5 |     let _ = widget_count;\n  |             ^^^^^^^^^^^^ not found in this scope\n\n',
    ],
    [9001, bar(659, 660, 'yap(bin)')],
    [9050, '\x1b[1m\x1b[91merror\x1b[0m: could not compile `yap` (bin "yap") due to 1 previous error\n'],
    [9100, '        Info Watching C:\\src\\yap\\src-tauri for changes...\n'],
  ]);
  assert.deepEqual(stages, ['vite', 'compiling', 'error']);
  assert.equal(s.errors.stage, 'compiling');
  assert.match(s.errors.lines[0], /^error\[E0425\]/);
  assert.ok(s.errors.lines.some((l) => /^error: could not compile/.test(l)));
  assert.ok(!s.errors.lines.some((l) => /Watching/.test(l)), "tauri's own info lines aren't part of the error");
  feed(s, [[20_000, '        Info File src-tauri\\src\\main.rs changed. Rebuilding application...\n']]);
  assert.equal(s.stage, 'compiling');
  assert.equal(s.errors, null);
});

test('the exe lock and a busy Vite port are recognised for their hints', () => {
  const s = new DevState({ now: 0 });
  feed(s, [
    [0, '  VITE v6.4.5  ready in 1 ms\n'],
    [10, bar(659, 660, 'yap(bin)')],
    [20, 'error: failed to remove file `C:\\t\\debug\\yap.exe`\n\nCaused by:\n  Access is denied. (os error 5)\n'],
  ]);
  assert.equal(s.stage, 'error');
  assert.equal(s.errors.accessDenied, true);
  const v = new DevState({ now: 0 });
  feed(v, [[0, 'error when starting dev server:\nError: Port 51437 is already in use\n']]);
  assert.equal(v.stage, 'error');
  assert.equal(v.errors.port, 51437);
});

test('a panic while running is noted, not treated as a failed build; the exit is recorded', () => {
  const s = new DevState({ now: 0 });
  feed(s, [
    [0, '  VITE v6.4.5  ready in 1 ms\n'],
    [10, `${G}    Finished${Z} \`dev\` profile [unoptimized + debuginfo] target(s) in 0.50s\n`],
    [20, `${G}     Running${Z} \`C:\\t\\debug\\yap.exe\`\n`],
    [5000, ''],
    [6000, "thread 'tokio-runtime-worker' panicked at src\\lib.rs:10:5:\n"],
  ]);
  assert.equal(s.stage, 'running');
  assert.match(s.panic.text, /panicked/);
  s.exited(0, 7000);
  assert.equal(s.stage, 'exited');
  assert.equal(s.exit.code, 0);
});

test("dev-update.ps1's output read back", () => {
  const updated = parseUpdate(
    '\uFEFF  Checking GitHub for the latest version...\n  Updated to the latest version. New since last time:\n    9f3e2a1 fix(chat): keep the effort\n    4b7c0d2 feat(notes): pin a note\n  Installing updated dependencies (npm install)...\n\nadded 3 packages in 4s\n  Running 9f3e2a1 fix(chat): keep the effort\n',
  );
  assert.equal(updated.status, 'updated');
  assert.deepEqual(
    updated.commits.map((c) => c.hash),
    ['9f3e2a1', '4b7c0d2'],
  );
  assert.equal(updated.npm, true);
  assert.deepEqual(updated.running, { hash: '9f3e2a1', title: 'fix(chat): keep the effort' });
  assert.equal(parseUpdate('  Checking GitHub for the latest version...\n  Already up to date.\n').status, 'uptodate');
  const branch = parseUpdate("  On branch 'feature-x', not main: not updating.\n");
  assert.equal(branch.status, 'skipped');
  assert.match(branch.reason, /feature-x/);
  const edits = parseUpdate('  This checkout has local edits, so it is not updated (running them as they are):\n     M src/app.css\n    ?? notes.txt\n');
  assert.equal(edits.status, 'skipped');
  assert.deepEqual(edits.files, ['src/app.css', 'notes.txt']);
  assert.equal(parseUpdate("  Checking GitHub for the latest version...\n  Couldn't reach GitHub: running the version already here.\n").reason, "couldn't reach GitHub");
  assert.equal(parseUpdate('  Checking GitHub for the latest version...\n').status, 'checking');
});

test('fmtDuration', () => {
  assert.equal(fmtDuration(42_000), '0:42');
  assert.equal(fmtDuration(123_000), '2:03');
  assert.equal(fmtDuration(3_723_000), '1:02:03');
});

test('text widths, truncation and wrapping', () => {
  assert.equal(strWidth('Yap'), 3);
  assert.equal(strWidth('日本'), 4);
  assert.equal(truncate('abcdefgh', 5), 'abcd…');
  assert.equal(truncate('abc', 5), 'abc');
  const rows = wrapSegs([['one two three four', 0, 0]], 0, 9, 2);
  assert.deepEqual(
    rows.map((r) => r.map(([w]) => w.trim()).join(' ')),
    ['one two', 'three', 'four'],
  );
});

test('the mark: see-through corners, an amber mouth, cream arcs when shown', () => {
  const N = 16;
  const shown = logoPixels(N, { open: 0.82, arcs: [1, 1, 1], blink: false });
  assert.equal(shown.col[0], -1, 'the rounded corner is see-through');
  const mouth = shown.cls[Math.round((505 / 1024) * N) * N + Math.round((250 / 1024) * N)];
  assert.equal(mouth, 2, 'left of the mouth centre is mouth');
  assert.ok(shown.cls.includes(4), 'arcs drawn');
  const hidden = logoPixels(N, { open: 0.06, arcs: [0, 0, 0], blink: false });
  assert.ok(!hidden.cls.includes(4), 'no arcs when they are hidden');
});

test('every screen draws at every size without throwing', () => {
  const s = new DevState({ now: 0 });
  const ui = { mode: 'dev', view: 'main', commit: { hash: 'abc1234', title: 'feat: a thing' }, update: parseUpdate('  Already up to date.\n'), stub: true, admin: true, logShown: '%LOCALAPPDATA%\\x.log', logLines: [{ text: 'hello', kind: 'other' }], logScroll: 0, wave: [0.1, 0.5, 0.9] };
  const steps = [
    [0, ''],
    [10, '  VITE v6.4.5  ready in 1 ms\n'],
    [20, bar(300, 600, 'serde')],
    [30, bar(599, 600, 'yap(bin)')],
    [40, 'error[E0425]: cannot find value `x`\n --> src\\main.rs:5:13\nerror: could not compile `yap`\n'],
  ];
  for (const [t, text] of steps) {
    feed(s, [[t, text]]);
    for (const [W, H] of [
      [120, 30],
      [80, 24],
      [60, 20],
      [200, 60],
      [30, 10],
    ]) {
      const m = { state: s, ui, t: t / 1000, now: t };
      const cv = drawMain(m, W, H);
      assert.equal(cv.w, W);
      for (let y = 0; y < H; y++) cv.rowAnsi(y);
      drawLog(m, W, H);
    }
  }
  s.exited(1, 100);
  const sum = drawSummary({ state: s, ui, t: 0, now: 100 }, 100);
  assert.ok(sum.h > 4);
});

const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};
const until = async (ok, ms) => {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (ok()) return true;
    await new Promise((r) => setTimeout(r, 100));
  }
  return ok();
};

test('Ctrl+C stops the child and everything under it, and the log has its output', { skip: process.platform !== 'win32', timeout: 30_000 }, async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'yap-dev-launcher-test-'));
  const pids = path.join(dir, 'pids.json');
  fs.writeFileSync(
    path.join(dir, 'tree.cjs'),
    `const { spawn } = require('child_process');
const g = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });
require('fs').writeFileSync(process.argv[2], JSON.stringify({ child: process.pid, grandchild: g.pid }));
console.log('  VITE v6.4.5  ready in 5 ms');
setInterval(() => console.log('tick'), 100);
`,
  );
  const launcher = spawn(
    process.execPath,
    [path.join(import.meta.dirname, 'dev-launcher.mjs'), '--headless', '100x30', '--cmd', `node "${path.join(dir, 'tree.cjs')}" "${pids}"`],
    { env: { ...process.env, YAP_DEV_LAUNCHER_HOME: dir, YAP_DEV_ALLOW_ADMIN: '1' }, stdio: ['pipe', 'ignore', 'inherit'] },
  );
  const exited = new Promise((r) => launcher.on('exit', r));
  try {
    assert.ok(await until(() => fs.existsSync(pids), 10_000), 'the child started');
    const { child, grandchild } = JSON.parse(fs.readFileSync(pids, 'utf8'));
    assert.ok(alive(child) && alive(grandchild));
    await new Promise((r) => setTimeout(r, 600));
    launcher.stdin.write('\x03');
    await exited;
    assert.ok(await until(() => !alive(child) && !alive(grandchild), 5000), 'nothing lingers');
    const logs = fs.readdirSync(path.join(dir, 'logs'));
    assert.equal(logs.length, 1);
    const log = fs.readFileSync(path.join(dir, 'logs', logs[0]), 'utf8');
    assert.match(log, /VITE v6\.4\.5 {2}ready in 5 ms/);
    assert.match(log, /tick/);
    assert.match(log, /stopped by Ctrl\+C/);
  } finally {
    launcher.kill();
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
