// `npm run -s lab -- <command>` (or `node scripts/lab/lab.mjs <command>`):
// Yap Lab, a test Yap whose Chat a Claude Code session can question and
// read back (docs/lab.md). Every command prints one line of JSON on stdout
// and exits (0: ok).
//
//   build                                  the test Yap from this checkout
//   start [--model …] [--effort …] [--web on|off] [--yap-code on|off]
//         [--connectors on|off] [--workspace <dir>] [--build <exe>]
//         [--stale-ok] [--fake [--live-web]] [--idle <min>]
//         [--size 1280x860] [--no-shots] [--theme light|dark]
//   check                                  is Claude Code here and signed in?
//   ask "<text>" [--spoken] [--chat new|same|<id>|code:<workspace>]
//       [--timeout 600] [--no-shots] [--theme light|dark]
//   wait [--timeout 600] [--agents [--cards allow|deny]]
//   answer <card-id> allow|deny|always [--timeout 600] [--no-wait]
//   cancel | state | agents | logs [--since N]
//   shot [--window thread|main|bar|panel|page] [--theme light|dark|both]
//        [--name <n>] [--selector <css>]
//   probe --window <w> [--css <selector>] [--text <words>]
//   set '<json>' | set [--web on|off] [--yap-code on|off] [--connectors on|off]
//   report <prompt> [--timeout 540] [--cards deny|allow] [--no-wait]
//   sheet [--turns 1-3] [--out <png>]      a contact sheet of the run's shots
//   run <script.json|.mjs> [--no-shots] [--theme light|dark]
//   fake '<json steps>'                    (start --fake: script the fake AI)
//   stop
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { APP, REPO, buildFresh, exeIn, headCommit, labDir, onOff, parseArgs, parseModel, parseSize, stampIn, targetDir } from './options.mjs';
import { loadScript, runScript, scriptFile } from './script.mjs';
import { makeSheet, runShots } from './sheet.mjs';
import { themesOf } from './shots.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const LAB = labDir();
const CONTROL = path.join(LAB, 'control.json');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function print(out) {
  process.stdout.write(`${JSON.stringify(out)}\n`);
}

function readControl() {
  try {
    return JSON.parse(fs.readFileSync(CONTROL, 'utf8'));
  } catch {
    return null;
  }
}

/** Send `cmd` to the running lab: its JSON answer. */
function send(cmd, args, control = readControl()) {
  if (!control?.port || !control?.token || !['ready', 'starting'].includes(control.state)) {
    return Promise.resolve({ ok: false, error: `The lab isn't running${control?.state ? ` (${control.state}${control.why ? `: ${control.why}` : ''})` : ''}: npm run lab -- start` });
  }
  const body = JSON.stringify({ cmd, args });
  return new Promise((resolve) => {
    const req = http.request(
      { host: '127.0.0.1', port: control.port, path: '/cmd', method: 'POST', headers: { authorization: `Bearer ${control.token}`, 'content-type': 'application/json', 'content-length': Buffer.byteLength(body) } },
      (res) => {
        let raw = '';
        res.on('data', (c) => (raw += c));
        res.on('end', () => {
          try {
            resolve(JSON.parse(raw));
          } catch {
            resolve({ ok: false, error: `The lab answered ${res.statusCode}` });
          }
        });
      }
    );
    req.on('error', (e) => resolve({ ok: false, error: `The lab didn't answer (${e.code ?? e.message}): is it running? npm run lab -- state` }));
    req.end(body);
  });
}

// ---------------------------------------------------------------- build

function build(args) {
  const target = args.target ? path.resolve(args.target) : targetDir();
  fs.mkdirSync(LAB, { recursive: true });
  const logFile = path.join(LAB, 'build.log');
  const logFd = fs.openSync(logFile, 'w');
  const started = Date.now();
  const head = headCommit();
  // The e2e suite's build (stub, debug, frontend embedded), into the lab's target dir.
  const r = spawnSync(process.execPath, [path.join(APP, 'e2e', 'build.mjs')], {
    cwd: REPO,
    stdio: ['ignore', logFd, logFd],
    env: { ...process.env, YAP_E2E_TARGET_DIR: target },
  });
  fs.closeSync(logFd);
  const ms = Date.now() - started;
  if (r.status !== 0) {
    const tail = fs.readFileSync(logFile, 'utf8').split(/\r?\n/).filter(Boolean).slice(-25);
    return { ok: false, error: `The build failed (exit ${r.status ?? r.error?.message})`, log: logFile, tail, ms };
  }
  const dirty = spawnSync('git', ['status', '--porcelain', '--untracked-files=no'], { cwd: REPO, encoding: 'utf8' }).stdout.trim().length > 0;
  const stamp = { commit: head.commit, commitTime: head.time, builtAt: Math.floor(Date.now() / 1000), dirty };
  fs.writeFileSync(stampIn(target), JSON.stringify(stamp, null, 2));
  return { ok: true, exe: exeIn(target), commit: head.commit, dirty, log: logFile, ms };
}

// ---------------------------------------------------------------- start

async function start(args, { check = false } = {}) {
  const fake = !!args.fake;
  const modelSpec = check ? 'claude-plan' : args.model;
  const model = parseModel(modelSpec, { fake });
  if (model.kind === 'api' && !process.env.YAP_LAB_ANTHROPIC_KEY) {
    return { ok: false, error: 'An Anthropic API model needs YAP_LAB_ANTHROPIC_KEY set by the person (it costs money; the lab never reads a key from anywhere else)' };
  }
  const web = onOff(args.web, true, 'web');
  const yapCode = onOff(args['yap-code'], false, 'yap-code');
  const connectors = onOff(args.connectors, false, 'connectors');
  const size = parseSize(args.size);
  if (args.theme !== undefined) themesOf(args.theme);
  // The build: the lab's own (fresh), or the one named.
  const target = targetDir();
  const exe = args.build ? path.resolve(args.build) : exeIn(target);
  if (!fs.existsSync(exe)) return { ok: false, error: `No test Yap at ${exe}: npm run lab -- build` };
  let stamp = null;
  const stampFile = args.build ? path.join(path.dirname(path.dirname(exe)), 'lab-build.json') : stampIn(target);
  try {
    stamp = JSON.parse(fs.readFileSync(stampFile, 'utf8'));
  } catch {
    // A build `lab build` didn't make: its file's time stands in.
    if (args.build) stamp = { commit: '', builtAt: Math.floor(fs.statSync(exe).mtimeMs / 1000) };
  }
  if (!args['stale-ok']) {
    const fresh = buildFresh(stamp, headCommit());
    if (!fresh.fresh) return { ok: false, error: fresh.why };
  }
  // One lab at a time.
  const running = readControl();
  if (running && ['ready', 'starting'].includes(running.state)) {
    const ping = await send('ping', {}, running);
    if (ping.ok) return { ok: false, error: `A lab is already running (pid ${running.pid}): npm run lab -- stop`, running: running.result ?? null };
  }
  fs.mkdirSync(LAB, { recursive: true });
  fs.rmSync(CONTROL, { force: true });
  const options = {
    modelSpec: modelSpec ?? (fake ? 'fake' : 'claude-plan'),
    effort: args.effort,
    web,
    yapCode,
    connectors,
    workspace: args.workspace ?? null,
    keepWorkspace: !!args['keep-workspace'],
    fake,
    liveWeb: fake && !!args['live-web'],
    fakeClaudeScript: args['fake-claude-script'] ? path.resolve(args['fake-claude-script']) : null,
    exe,
    build: stamp,
    idle: Number(args.idle ?? (check ? 5 : 120)),
    // Screenshots after every question (and while a card waits): on unless
    // --no-shots, in both themes unless --theme; the main window at `size`.
    size,
    shots: !args['no-shots'],
    theme: args.theme ?? 'both',
  };
  fs.writeFileSync(path.join(LAB, 'start.json'), JSON.stringify(options, null, 2));
  const logFd = fs.openSync(path.join(LAB, 'daemon.log'), 'a');
  const child = spawn(process.execPath, [path.join(HERE, 'daemon.mjs'), LAB], {
    cwd: REPO,
    detached: true,
    stdio: ['ignore', logFd, logFd],
    windowsHide: true,
    // The key only for an API model, and only from the person's own variable.
    env: { ...process.env, ...(model.kind === 'api' ? {} : { YAP_LAB_ANTHROPIC_KEY: '' }) },
  });
  child.unref();
  fs.closeSync(logFd);
  const deadline = Date.now() + 180_000;
  while (Date.now() < deadline) {
    await sleep(500);
    const c = readControl();
    if (!c || c.pid !== child.pid) continue;
    if (c.state === 'ready') return { ok: true, pid: c.pid, ...c.result, lab: LAB };
    if (['failed', 'exited', 'stopped'].includes(c.state)) return { ok: false, error: c.error ?? c.why ?? c.state, ...(c.result ?? {}), log: path.join(LAB, 'daemon.log') };
  }
  return { ok: false, error: `The lab didn't start in 3 minutes: see ${path.join(LAB, 'daemon.log')}` };
}

async function stopRunning() {
  const c = readControl();
  if (!c || !['ready', 'starting'].includes(c.state)) return { ok: true, stopped: false, note: 'The lab wasn’t running', lab: LAB };
  const out = await send('stop', {}, c);
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    const now = readControl();
    if (now && !['ready', 'starting', 'stopping'].includes(now.state)) break;
    await sleep(300);
  }
  return { ...out, ok: true };
}

// ---------------------------------------------------------------- main

async function main() {
  const [command, ...rest] = process.argv.slice(2);
  const args = parseArgs(rest);
  switch (command) {
    case 'build':
      return build(args);
    case 'start':
      return start(args);
    case 'check': {
      // Is Claude Code on this PC and signed in, as Yap sees it? A lab
      // started on the plan and stopped again: nothing is asked of it.
      const out = await start({ ...args, model: 'claude-plan' }, { check: true });
      if (out.ok) await stopRunning();
      return { ...out, stopped: true };
    }
    case 'stop':
      return stopRunning();
    case 'ask':
      return send('ask', { ...args, text: args._.join(' ') });
    case 'fake':
      return send('fake', { script: args._.join(' ') || undefined });
    case 'set':
      // A JSON object (`set '{"web": true, "files": [...]}'`), or flags.
      return send('set', args._.length ? JSON.parse(args._.join(' ')) : args);
    case 'sheet':
      return { ok: true, ...(await makeSheet({ lab: LAB, turns: args.turns, out: args.out, title: 'Yap Lab', subtitle: subtitle() })) };
    case 'run':
      return run(args);
    case 'wait':
    case 'answer':
    case 'cancel':
    case 'shot':
    case 'probe':
    case 'state':
    case 'agents':
    case 'logs':
    case 'report':
      return send(command, args);
    default:
      return { ok: false, error: `Commands: build, start, check, ask, wait, answer, cancel, shot, probe, set, state, agents, logs, report, sheet, run, fake, stop (docs/lab.md)` };
  }
}

/** The sheet's subtitle: the run's model and build, from what `start` wrote. */
function subtitle() {
  try {
    const s = JSON.parse(fs.readFileSync(path.join(LAB, 'start.json'), 'utf8'));
    return [s.modelSpec, s.fake ? 'fake' : '', s.build?.commit ? `build ${String(s.build.commit).slice(0, 9)}` : '', new Date().toISOString().slice(0, 16).replace('T', ' ')].filter(Boolean).join(' · ');
  } catch {
    return '';
  }
}

// ---------------------------------------------------------------- run

/**
 * `run <script>`: a feature script's steps against the running lab, a
 * contact sheet of its turns, and one JSON summary: each step passed or
 * failed, the failures' details, the sheet. Exit 1 on any failed expect.
 */
async function run(args) {
  const started = Date.now();
  const file = scriptFile(args._[0]);
  const script = await loadScript(file);
  const state = await send('state', {});
  if (!state.ok) return { ok: false, script: file, error: state.error };
  if (args.theme !== undefined) themesOf(args.theme);
  const out = await runScript(script, { send, state, options: { shots: args['no-shots'] ? false : undefined, theme: args.theme } });
  let sheet = null;
  // A contact sheet of the script's turns, when they have shots.
  if (runShots(LAB, (n) => out.turns.includes(n)).length) {
    const stamp = new Date().toISOString().replace(/[:.]/g, '-').slice(0, 19);
    sheet = await makeSheet({ lab: LAB, turns: (n) => out.turns.includes(n), out: path.join(LAB, 'sheets', `${script.name}-${stamp}.png`), title: `Yap Lab script: ${script.name}`, subtitle: subtitle() }).catch((e) => ({ error: e.message.split('\n')[0] }));
  }
  return { ok: out.ok, script: file, name: script.name, ...(script.about ? { about: script.about } : {}), passed: out.steps.filter((s) => s.ok).length, failed: out.steps.filter((s) => !s.ok).length, failures: out.failures, warnings: out.warnings, ...(out.stopped ? { stopped: out.stopped } : {}), turns: out.turns, sheet, steps: out.steps, ms: Date.now() - started };
}

main()
  .then((out) => {
    print(out);
    process.exitCode = out?.ok === false ? 1 : 0;
  })
  .catch((e) => {
    print({ ok: false, error: e.message.split('\n')[0] });
    process.exitCode = 1;
  });
