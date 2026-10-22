// Tests for scripts/privacy-guard.mjs (`npm run test:privacy`): a throwaway
// git repo with a bare "remote", a throwaway terms file, and fake commits.
// Every value that the guard should block is put together at run time, so
// this file itself never trips the guard.
import { test, before, beforeEach, after } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { looksFake, emailAllowed, ukPhone, termsRegex, scanLine, MASK } from './privacy-guard.mjs';

const GUARD = fileURLToPath(new URL('./privacy-guard.mjs', import.meta.url));
const HOOK = fileURLToPath(new URL('../.githooks/pre-push', import.meta.url));
const TERM = 'Zebulon'; // the stand-in private term
const at = (user, domain) => [user, domain].join('@');
const ALNUM = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789';
const randomKey = (n) => Array.from(crypto.randomBytes(n), (b) => ALNUM[b % ALNUM.length]).join('');
// A key-shaped value with no runs or test words in it (random could, rarely).
function realLookingKey(prefix, n) {
  for (;;) {
    const body = randomKey(n);
    if (!looksFake(body) && /\d/.test(body)) return prefix + body;
  }
}

let tmp;
let repo;
let terms;

function git(args, opts = {}) {
  const r = spawnSync('git', args, { cwd: repo, encoding: 'utf8', ...opts });
  assert.equal(r.status, 0, `git ${args.join(' ')}: ${r.stderr}`);
  return r.stdout.trim();
}

function commit(file, text, message, author) {
  fs.writeFileSync(path.join(repo, file), text);
  git(['add', file]);
  const env = author ? { ...process.env, GIT_AUTHOR_NAME: author.name, GIT_AUTHOR_EMAIL: author.email } : process.env;
  git(['commit', '-q', '-m', message], { env });
  return git(['rev-parse', 'HEAD']);
}

/** Runs the guard as git's pre-push hook would, for pushing HEAD to origin's main. */
function prePush({ termsFile = terms, remoteSha } = {}) {
  const local = git(['rev-parse', 'HEAD']);
  const remote = remoteSha ?? git(['rev-parse', 'origin/main']);
  const r = spawnSync(process.execPath, [GUARD, 'origin', 'unused-url'], {
    cwd: repo,
    encoding: 'utf8',
    input: `refs/heads/main ${local} refs/heads/main ${remote}\n`,
    env: { ...process.env, YAP_PRIVATE_TERMS: termsFile },
  });
  return { code: r.status, out: r.stdout + r.stderr };
}

/** Throws away the commits since origin/main. */
function reset() {
  git(['reset', '-q', '--hard', 'origin/main']);
}

before(() => {
  tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'yap-privacy-guard-'));
  repo = path.join(tmp, 'repo');
  const remote = path.join(tmp, 'remote.git');
  terms = path.join(tmp, 'terms.txt');
  fs.writeFileSync(terms, `# private words, one per line\n${TERM}\nPlum Orchard Lane\n`);
  spawnSync('git', ['init', '-q', '--bare', remote]);
  fs.mkdirSync(repo);
  git(['init', '-q', '-b', 'main']);
  git(['config', 'user.name', 'Test Person']);
  git(['config', 'user.email', at('test', 'example.com')]);
  git(['config', 'core.autocrlf', 'false']);
  git(['config', 'core.hooksPath', 'no-hooks-here']);
  git(['remote', 'add', 'origin', remote]);
  commit('README.md', '# A repo\n', 'chore: start');
  git(['push', '-q', 'origin', 'main']);
});

after(() => {
  fs.rmSync(tmp, { recursive: true, force: true });
});

// Every test starts on main as origin has it, whatever the one before left.
beforeEach(() => {
  git(['checkout', '-q', '-f', 'main']);
  reset();
});

test('a private term in a diff blocks the push, and is never printed', () => {
  commit('notes.md', `Ask ${TERM.toLowerCase()} about it.\n`, 'docs: notes');
  const r = prePush();
  assert.equal(r.code, 1, r.out);
  assert.match(r.out, /notes\.md:1 {2}\[private term\]/);
  assert.match(r.out, /Ask \[private term\] about it\./);
  assert.doesNotMatch(r.out, new RegExp(TERM, 'i'));
  reset();
});

test('a private phrase matches across its spaces, as a whole word', () => {
  commit('a.md', 'We met on plum   orchard lane.\nPlum orchards and lanes.\n', 'docs: a');
  const r = prePush();
  assert.equal(r.code, 1, r.out);
  assert.match(r.out, /a\.md:1 /);
  assert.doesNotMatch(r.out, /a\.md:2 /);
  reset();
});

test('a private term in a commit message blocks the push', () => {
  commit('a.md', 'fine\n', `fix: what ${TERM} asked for`);
  const r = prePush();
  assert.equal(r.code, 1, r.out);
  assert.match(r.out, /\(commit message, line 1\) {2}\[private term\] {2}fix: what \[private term\] asked for/);
  reset();
});

test('a private term in the author header does not block the push', () => {
  commit('a.md', 'fine\n', 'docs: fine', { name: `${TERM} Quartz`, email: at(TERM.toLowerCase(), 'example.com') });
  const r = prePush();
  assert.equal(r.code, 0, r.out);
  assert.match(r.out, /1 commit about to be pushed: clean/);
  reset();
});

test('a whole word only: a longer word holding the term is fine', () => {
  commit('a.md', `${TERM}s and ${TERM}ville\n`, 'docs: fine');
  assert.equal(prePush().code, 0);
  reset();
});

test('no terms file: the terms are skipped with one warning, the rest still checked', () => {
  commit('a.md', `${TERM}\n`, 'docs: a');
  const r = prePush({ termsFile: path.join(tmp, 'missing.txt') });
  assert.equal(r.code, 0, r.out);
  assert.equal(r.out.match(/no private terms file/g)?.length, 1);
  commit('b.md', `key = "${realLookingKey('gsk_', 52)}"\n`, 'feat: b');
  assert.equal(prePush({ termsFile: path.join(tmp, 'missing.txt') }).code, 1);
  reset();
});

test('a real-looking API key blocks the push, and is printed cut short', () => {
  const key = realLookingKey('gsk_', 52);
  commit('config.js', `export const KEY = '${key}';\n`, 'feat: config');
  const r = prePush();
  assert.equal(r.code, 1, r.out);
  assert.match(r.out, /config\.js:1 {2}\[groq-key\]/);
  assert.ok(!r.out.includes(key), 'the whole key is never printed');
  reset();
  for (const k of [realLookingKey('sk-proj-', 48), realLookingKey('sk-ant-api03-', 40), realLookingKey('xai-', 40), realLookingKey('tvly-dev-', 32), realLookingKey('ghp_', 36)]) {
    commit('k.txt', `${k}\n`, 'feat: k');
    assert.equal(prePush().code, 1, k.slice(0, 8));
    reset();
  }
  const pem = ['-----BEGIN', 'RSA PRIVATE KEY-----'].join(' ');
  commit('k.pem', `${pem}\n`, 'feat: pem');
  assert.equal(prePush().code, 1);
  reset();
});

test('a test fake key does not block the push', () => {
  const fakes = [
    'gsk_abcdefghijklmnop1234567890',
    'gsk_abcdefghijklmnopqrstuvwxyz0123456789',
    'sk-proj-abcdefghijklmnopqrstuvwx',
    'sk-ant-api03-ABCDEFGHIJKLMNOPQRSTUV',
    'sk-ant-e2e-fake-1234567890',
    'gsk_' + 'a'.repeat(52),
    'sk_test_0123456789abcdef',
    'xai-' + 'x'.repeat(40),
  ];
  commit('fakes.rs', fakes.map((f) => `let k = "${f}";`).join('\n') + '\n', 'test: fakes');
  const r = prePush();
  assert.equal(r.code, 0, r.out);
  reset();
});

test('allowed emails do not block; a real-looking one does', () => {
  const allowed = [
    at('alex', 'example.com'),
    at('priya.shah', 'example.co.uk'),
    at('room4', 'resource.example.com'),
    at('team', 'group.calendar.google.com'),
    at('someone', 'users.noreply.github.com'),
    at('no-reply', 'mail.contextmirror.com'),
    at('contextmirror', 'proton.me'),
    at('dev', 'yap.test'),
    'icon' + '@2x.png',
  ];
  commit('a.md', allowed.join('\n') + '\n', `feat: a\n\nCo-Authored-By: Claude <${at('noreply', 'anthropic.com')}>`);
  let r = prePush();
  assert.equal(r.code, 0, r.out);
  commit('b.md', `Write to ${at('jo.bloggs', 'gmail.com')}\n`, 'docs: b');
  r = prePush();
  assert.equal(r.code, 1, r.out);
  assert.match(r.out, /b\.md:1 {2}\[email\]/);
  reset();
});

test("UK phone numbers block, Ofcom's drama numbers don't", () => {
  const real = ['07', '911', ' 4', '83', '725'].join(''); // a mobile outside the drama range
  commit('a.md', `Call me on ${real}.\n`, 'docs: a');
  let r = prePush();
  assert.equal(r.code, 1, r.out);
  assert.match(r.out, /\[uk-phone\]/);
  reset();
  commit('b.md', 'Mobile 07700 900123, +44 7700 900456, 020 7946 0018, 0113 496 0000\n', 'docs: b');
  r = prePush();
  assert.equal(r.code, 0, r.out);
  reset();
});

test('a new branch: only the commits no remote branch has yet', () => {
  git(['checkout', '-q', '-b', 'topic']);
  commit('a.md', 'fine\n', 'feat: a');
  commit('b.md', 'fine too\n', 'feat: b');
  const r = prePush({ remoteSha: '0'.repeat(40) });
  assert.equal(r.code, 0, r.out);
  assert.match(r.out, /2 commits about to be pushed: clean/);
  git(['checkout', '-q', 'main']);
  git(['branch', '-q', '-D', 'topic']);
});

test('a deletion pushes nothing', () => {
  const r = spawnSync(process.execPath, [GUARD], {
    cwd: repo,
    encoding: 'utf8',
    input: `(delete) ${'0'.repeat(40)} refs/heads/old ${git(['rev-parse', 'HEAD'])}\n`,
    env: { ...process.env, YAP_PRIVATE_TERMS: terms },
  });
  assert.equal(r.status, 0, r.stderr);
});

test('--scan-tree checks every tracked file as it is now, with the term masked', () => {
  commit('tracked.md', `fine\nfrom ${TERM}\n`, 'docs: tracked');
  fs.writeFileSync(path.join(repo, 'untracked.md'), `${TERM}\n`);
  const r = spawnSync(process.execPath, [GUARD, '--scan-tree'], { cwd: repo, encoding: 'utf8', env: { ...process.env, YAP_PRIVATE_TERMS: terms } });
  assert.equal(r.status, 1, r.stdout + r.stderr);
  assert.match(r.stderr, /tracked\.md:2 {2}\[private term\] {2}from \[private term\]/);
  assert.doesNotMatch(r.stderr, /untracked\.md/);
  fs.rmSync(path.join(repo, 'untracked.md'));
  reset();
});

test('the real hook blocks `git push` and lets a clean one through', () => {
  // The hook finds the guard next to the checkout, as in Yap's repo.
  fs.mkdirSync(path.join(repo, 'scripts'), { recursive: true });
  fs.copyFileSync(GUARD, path.join(repo, 'scripts', 'privacy-guard.mjs'));
  fs.mkdirSync(path.join(repo, '.hooks'), { recursive: true });
  fs.copyFileSync(HOOK, path.join(repo, '.hooks', 'pre-push'));
  fs.chmodSync(path.join(repo, '.hooks', 'pre-push'), 0o755);
  git(['config', 'core.hooksPath', '.hooks']);
  const env = { ...process.env, YAP_PRIVATE_TERMS: terms };
  try {
    commit('a.md', `${TERM} again\n`, 'docs: a');
    let r = spawnSync('git', ['push', '-q', 'origin', 'main'], { cwd: repo, encoding: 'utf8', env });
    assert.notEqual(r.status, 0, 'blocked');
    assert.match(r.stderr, /\[private term\]/);
    reset();
    commit('a.md', 'nothing personal\n', 'docs: a');
    r = spawnSync('git', ['push', '-q', 'origin', 'main'], { cwd: repo, encoding: 'utf8', env });
    assert.equal(r.status, 0, r.stderr);
  } finally {
    git(['config', 'core.hooksPath', 'no-hooks-here']);
  }
});

test('the rules on their own', () => {
  const re = termsRegex([TERM, 'Plum Orchard Lane']);
  assert.deepEqual(scanLine(`hi ${TERM}!`, re), [{ rule: 'private term', text: `hi ${MASK}!` }]);
  assert.deepEqual(scanLine(`${TERM}_x and x${TERM}`, re), []);
  assert.equal(emailAllowed(at('a', 'example.org')), true);
  assert.equal(emailAllowed(at('a', 'gmail.com')), false);
  assert.equal(ukPhone('07700 900123'), null);
  assert.equal(ukPhone('01234567890'), null); // a run, not a number
  assert.equal(ukPhone(['+44', '20', '3555', '1987'].join(' ')), ['020', '3555', '1987'].join(''));
  assert.equal(looksFake('abcdefghijklmnop1234567890'), true);
  assert.equal(looksFake(randomKey(8) + 'FAKE' + randomKey(8)), true);
});
