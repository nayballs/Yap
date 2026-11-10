// Yap Lab's options, folders and the test Yap's config (docs/lab.md). Pure
// but for the folders' defaults (the environment, what's on disk) and the
// build's freshness (git).
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
export const APP = path.join(REPO, 'desktop');

/**
 * The lab's own folder: the running lab's control file and record, each
 * turn's transcript, the shots, the reports, the scratch workspace and the
 * test Yap's run folder (its data, its log). `YAP_LAB_DIR`, else
 * test-results/lab in this checkout (git-ignored, beside the e2e suite's).
 */
export function labDir(env = process.env) {
  return env.YAP_LAB_DIR ? path.resolve(env.YAP_LAB_DIR) : path.join(REPO, 'test-results', 'lab');
}

/**
 * Where `lab build` puts the test Yap: `YAP_LAB_TARGET_DIR`, else
 * `yap-lab` in this PC's builds folder (`E:\Builds` when there is one: the
 * maintainer's PC keeps every build there, never in the checkout), else
 * desktop/src-tauri/target/lab. A Cargo target dir of its own, so it never
 * fights `tauri dev`, the e2e suite's or the evaluations' builds.
 */
export function targetDir(env = process.env, exists = fs.existsSync) {
  if (env.YAP_LAB_TARGET_DIR) return path.resolve(env.YAP_LAB_TARGET_DIR);
  if (process.platform === 'win32' && exists('E:\\Builds')) return 'E:\\Builds\\yap-lab';
  return path.join(APP, 'src-tauri', 'target', 'lab');
}

export const exeIn = (target) => path.join(target, 'debug', process.platform === 'win32' ? 'yap.exe' : 'yap');
export const stampIn = (target) => path.join(target, 'lab-build.json');

/** The checkout's last commit: `{ commit, time }` (time in Unix seconds). */
export function headCommit(cwd = REPO) {
  const out = execFileSync('git', ['log', '-1', '--format=%H %ct'], { cwd, encoding: 'utf8' }).trim();
  const [commit, time] = out.split(' ');
  return { commit, time: Number(time) };
}

/**
 * Is the build from the checkout's last commit or later? `stamp`: what
 * `lab build` wrote (`{ commit, builtAt }`, builtAt in Unix seconds), or
 * null. Pure.
 */
export function buildFresh(stamp, head) {
  if (!stamp) return { fresh: false, why: 'no lab build yet: run `npm run lab -- build`' };
  if (stamp.commit === head.commit) return { fresh: true };
  if (Number(stamp.builtAt) >= head.time) return { fresh: true };
  return { fresh: false, why: `the lab build (${String(stamp.commit).slice(0, 9)}) is older than the checkout's last commit (${head.commit.slice(0, 9)}): run \`npm run lab -- build\`, or pass --stale-ok` };
}

// ---------------------------------------------------------------- arguments

/** Flags that take no value. */
const SWITCHES = new Set(['spoken', 'fake', 'live-web', 'stale-ok', 'no-wait', 'agents', 'keep-workspace', 'no-shots']);

/**
 * `argv` (after the command) → `{ _: [positional…], flag: value }`.
 * `--name value`, `--name=value`, and the switches above alone. Pure.
 */
export function parseArgs(argv) {
  const out = { _: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith('--')) {
      out._.push(a);
      continue;
    }
    const eq = a.indexOf('=');
    const name = (eq > 0 ? a.slice(2, eq) : a.slice(2)).trim();
    if (eq > 0) out[name] = a.slice(eq + 1);
    else if (SWITCHES.has(name)) out[name] = true;
    else if (i + 1 < argv.length) out[name] = argv[++i];
    else throw new Error(`--${name} needs a value`);
  }
  return out;
}

/** The main window's size in a lab run (`--size`): the same every run, so shots compare. */
export const SIZE = '1280x860';

/**
 * `--size WIDTHxHEIGHT` → "1280x860": at least the main window's minimum
 * (860 × 600), at most an 8K screen. Pure.
 */
export function parseSize(v, fallback = SIZE) {
  const s = String(v ?? fallback).trim();
  const m = /^(\d{3,4})\s*[x×]\s*(\d{3,4})$/i.exec(s);
  if (!m) throw new Error(`--size is WIDTHxHEIGHT (like ${SIZE}), not "${v}"`);
  const [w, h] = [Number(m[1]), Number(m[2])];
  if (w < 860 || h < 600 || w > 7680 || h > 4320) throw new Error("--size is at least 860x600 (the main window's minimum) and at most 7680x4320");
  return `${w}x${h}`;
}

/** "on" / "off" (or true / false) → a boolean; `fallback` when not given. Pure. */
export function onOff(v, fallback, name) {
  if (v === undefined) return fallback;
  if (v === true || v === 'on' || v === 'true' || v === '1') return true;
  if (v === false || v === 'off' || v === 'false' || v === '0') return false;
  throw new Error(`--${name} is on or off, not "${v}"`);
}

// ---------------------------------------------------------------- the model

/** The plan's model when none is named (`--model claude-plan`). */
export const PLAN_DEFAULT = 'claude-sonnet-5-5';
/** Short names: the plan's models (ai/claude_plan.rs MODELS). */
const SHORT = { haiku: 'claude-haiku-5-5', sonnet: 'claude-sonnet-5-5', opus: 'claude-opus-5-5', fable: 'claude-fable-5-2' };
/** The fake AI's strong model (desktop/e2e/support/fake-llm.js: every tool, the browser's too). */
export const FAKE_MODEL = 'fake-gpt-5';
export const EFFORTS = ['low', 'medium', 'high', 'xhigh', 'max'];

/**
 * `--model` → who answers. Pure.
 * - `claude-plan` (the default), `claude-plan:<model id>`, or `haiku`,
 *   `sonnet`, `opus`, `fable`: the person's Claude plan, through their own
 *   Claude Code (its usage, never money);
 * - `anthropic:<model id>`: the Anthropic API on the key in
 *   `YAP_LAB_ANTHROPIC_KEY` (money: only when the person set that key);
 * - with `--fake`: the plan's ones play Yap's stand-in for Claude Code
 *   (`yap.exe fake-claude`), anything else the e2e suite's fake AI.
 */
export function parseModel(spec, { fake = false } = {}) {
  const s = String(spec ?? '').trim();
  if (fake && (!s || s === 'fake')) return { kind: 'fake', provider: 'custom', model: FAKE_MODEL };
  if (!s || s === 'claude-plan') return { kind: 'plan', provider: 'claude-plan', model: PLAN_DEFAULT };
  if (SHORT[s]) return { kind: 'plan', provider: 'claude-plan', model: SHORT[s] };
  const [head, ...rest] = s.split(':');
  const id = rest.join(':').trim();
  if (head === 'claude-plan' && id) return { kind: 'plan', provider: 'claude-plan', model: id };
  if (head === 'anthropic' && id) return { kind: 'api', provider: 'anthropic', model: id };
  if (fake) return { kind: 'fake', provider: 'custom', model: s };
  throw new Error(`--model is claude-plan, claude-plan:<model>, haiku, sonnet, opus, fable or anthropic:<model> — not "${s}"`);
}

/**
 * The test Yap's config (laid over the e2e suite's BASE_CONFIG): Chat on
 * `model`, the switches asked for, and nothing that spends the model's time
 * behind the lab's back (memory learning, cleanup). `key`: the Anthropic
 * key for an API model (from YAP_LAB_ANTHROPIC_KEY only); `fakeBase`: the
 * fake AI's address. Pure.
 */
export function labConfig({ model, effort, web = true, yapCode = false, connectors = false, key = '', fakeBase = '' }) {
  let chat;
  const keys = {};
  if (model.kind === 'plan') {
    chat = { enabled: true, provider: 'claude-plan', baseUrl: 'claude-code://plan', model: model.model, apiKey: 'claude-plan', apiKeys: {}, prompt: '', disableThinking: false };
    keys['claude-plan'] = 'claude-plan';
  } else if (model.kind === 'api') {
    if (!key) throw new Error('An Anthropic API model needs YAP_LAB_ANTHROPIC_KEY set (the lab never reads a key from anywhere else)');
    chat = { enabled: true, provider: 'anthropic', baseUrl: 'https://api.anthropic.com/v1', model: model.model, apiKey: key, apiKeys: {}, prompt: '', disableThinking: false };
    keys.anthropic = key;
  } else {
    chat = { enabled: true, provider: 'custom', baseUrl: fakeBase, model: model.model, apiKey: '', apiKeys: {}, prompt: '', disableThinking: false };
  }
  if (effort !== undefined && !EFFORTS.includes(effort)) throw new Error(`--effort is one of ${EFFORTS.join(', ')}`);
  return {
    llmScopes: { chat },
    ppApiKeys: keys,
    ...(effort ? { chatEffort: { [model.model]: effort } } : {}),
    webSearch: web,
    agentsEnabled: yapCode,
    ...(yapCode ? { agentsPay: model.kind === 'api' ? 'key' : 'plan', agentsSpeak: 'never' } : {}),
    connectorsPreview: connectors,
    // Nothing the lab didn't ask for: no cleanup, no learning from its
    // chats (each would be a model call on the plan), no reminders, no voice.
    postProcessEnabled: false,
    ppProvider: 'ondevice',
    memoryLearnChats: false,
    memoryLearnMeetings: false,
    meetingReminder: 'never',
    speakReplies: false,
    // The test Yap's log at debug (its own data folder): `lab logs`.
    debugLogging: true,
  };
}
