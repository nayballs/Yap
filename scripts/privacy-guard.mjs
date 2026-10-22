#!/usr/bin/env node
// Yap's privacy guard: nothing personal leaves this PC in a push.
//
// Yap's repo is public. Before a push, this checks every commit about to go
// out (the added lines of its diff, its message and the paths it touches —
// never the author/committer headers) for:
//
//  (a) the maintainer's private terms: names and details that must never be
//      in the repo, listed one per line in a file OUTSIDE every repo,
//      `%USERPROFILE%\.yap-private\terms.txt` (or `YAP_PRIVATE_TERMS`).
//      Matched case-insensitively as whole words. No file: this part is
//      skipped with one warning (CI has none). A hit is printed with the
//      term masked as "[private term]", never the term itself: terminal
//      output ends up in logs.
//  (b) generic leaks: API-key shapes (Groq, OpenAI, Anthropic, xAI, Tavily,
//      ElevenLabs, AWS, GitHub, Google, Stripe, Slack, Resend, private-key
//      PEM headers) unless plainly a test fake; email addresses outside an
//      allowlist (example.com and friends, the product's own addresses);
//      UK phone numbers outside Ofcom's ranges reserved for drama.
//
// Usage:
//   node scripts/privacy-guard.mjs              pre-push hook: reads git's
//                                               "<local ref> <sha> <remote ref> <sha>" lines
//   node scripts/privacy-guard.mjs --scan-tree  every tracked file as it is now
//   node scripts/privacy-guard.mjs --range A..B the commits in a range (CI)
//   --no-private                                skip (a), the private terms
//
// Exit code 1 = something was found (the push is blocked), 0 = clean.
// Installed by `npm run hooks:install` (core.hooksPath = .githooks). A false
// positive — and only after checking it really is one — can be pushed with
// `git push --no-verify`.

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const MASK = '[private term]';
const ZERO = /^0+$/;

// ---------------------------------------------------------------- (a) terms

/** Where the private terms live: `YAP_PRIVATE_TERMS`, else the user's profile. */
export function termsPath(env = process.env) {
  if (env.YAP_PRIVATE_TERMS) return env.YAP_PRIVATE_TERMS;
  const home = env.USERPROFILE || env.HOME || os.homedir();
  return path.join(home, '.yap-private', 'terms.txt');
}

/** The terms in `file` (blank lines and `#` notes left out), or null when there's no file. */
export function loadTerms(file) {
  let text;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch {
    return null;
  }
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter((l) => l && !l.startsWith('#'));
}

const escapeRe = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** One case-insensitive, whole-word regex for every term (a phrase's spaces match any whitespace). */
export function termsRegex(terms) {
  if (!terms || terms.length === 0) return null;
  const alts = [...terms].sort((a, b) => b.length - a.length).map((t) => escapeRe(t).replace(/\s+/g, '\\s+'));
  return new RegExp(`(?<![\\p{L}\\p{N}_])(?:${alts.join('|')})(?![\\p{L}\\p{N}_])`, 'giu');
}

// ------------------------------------------------------------- (b) generic

// API keys by their published shapes. `body` is what follows the prefix.
const KEY_RULES = [
  { rule: 'anthropic-key', re: /\bsk-ant-[a-z]+\d*-([A-Za-z0-9_-]{20,})/g },
  { rule: 'openai-key', re: /\bsk-(?!ant-)(?:proj-|svcacct-|admin-)?([A-Za-z0-9_-]{20,})/g },
  { rule: 'groq-key', re: /\bgsk_([A-Za-z0-9]{20,})/g },
  { rule: 'xai-key', re: /\bxai-([A-Za-z0-9]{20,})/g },
  { rule: 'tavily-key', re: /\btvly-(?:dev-|prod-)?([A-Za-z0-9]{20,})/g },
  { rule: 'elevenlabs-key', re: /\bsk_([a-f0-9]{40,})\b/g },
  { rule: 'aws-key', re: /\b(?:AKIA|ASIA)([0-9A-Z]{16})\b/g },
  { rule: 'github-token', re: /\b(?:gh[pousr]_|github_pat_)([A-Za-z0-9_]{30,})/g },
  { rule: 'google-api-key', re: /\bAIza([0-9A-Za-z_-]{35})\b/g },
  { rule: 'stripe-key', re: /\b(?:sk|rk)_live_([A-Za-z0-9]{20,})/g },
  { rule: 'slack-token', re: /\bxox[abposr]-([A-Za-z0-9-]{20,})/g },
  { rule: 'resend-key', re: /\bre_([A-Za-z0-9]{8}_[A-Za-z0-9]{24})\b/g },
];
const PEM = /-----BEGIN (?:[A-Z0-9]+ )*PRIVATE KEY(?: BLOCK)?-----/g;

const FAKE_WORDS = /fake|test|example|dummy|sample|placeholder|notreal|not_a_real|redacted|your|xxxx|demo|invalid|e2e_|_e2e/i;

function hasRun(s, len) {
  // A run of one character, or an ascending run of letters or digits ("abcdef", "123456").
  let same = 1;
  let up = 1;
  for (let i = 1; i < s.length; i++) {
    same = s[i] === s[i - 1] ? same + 1 : 1;
    const a = s.charCodeAt(i - 1);
    const b = s.charCodeAt(i);
    const alnum = (c) => (c >= 48 && c <= 57) || (c >= 97 && c <= 122);
    up = b === a + 1 && alnum(a) && alnum(b) ? up + 1 : 1;
    if (same >= len || up >= len) return true;
  }
  return false;
}

/** A key-shaped string that's plainly made up: a test word, a run, too few distinct characters. */
export function looksFake(body) {
  if (FAKE_WORDS.test(body)) return true;
  if (hasRun(body.toLowerCase(), 6)) return true;
  if (new Set(body).size < 10) return true;
  // Real keys mix letters and digits; an identifier-like word salad isn't one.
  return !(/[0-9]/.test(body) && /[A-Za-z]/.test(body));
}

// Email addresses: allowed are documentation domains (RFC 2606/6761), GitHub's
// noreply, Google Calendar ids, and the addresses the product itself uses.
const EMAIL = /[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}(?![A-Za-z0-9-])/g;
const EMAIL_EXACT = new Set([
  'noreply@anthropic.com',
  'noreply@github.com',
  'git@github.com',
  'contextmirror@proton.me',
  'no-reply@mail.contextmirror.com',
]);
const FILE_EXTENSIONS = new Set(['png', 'jpg', 'jpeg', 'gif', 'svg', 'webp', 'ico', 'avif', 'js', 'mjs', 'cjs', 'ts', 'css', 'json', 'md', 'rs', 'html', 'svelte', 'wasm', 'map']);

export function emailAllowed(address) {
  const a = address.toLowerCase();
  if (EMAIL_EXACT.has(a)) return true;
  const domain = a.slice(a.lastIndexOf('@') + 1);
  const tld = domain.slice(domain.lastIndexOf('.') + 1);
  if (FILE_EXTENSIONS.has(tld)) return true; // "icon@2x.png" is a file name
  if (/(^|\.)example\.[a-z.]+$/.test(domain)) return true; // example.com/.org/.net/.co.uk, host.example.org
  if (/\.(test|example|invalid|localhost|local)$/.test(domain)) return true;
  if (domain === 'users.noreply.github.com') return true;
  if (/(^|\.)calendar\.google\.com$/.test(domain)) return true; // calendar ids, not people
  return false;
}

// UK phone numbers: candidates, then judged on their digits.
const PHONE = /(?<![\w+.#/=-])(?:\+44[\s-]?(?:\(0\)[\s-]?)?|\(?0)\d[\d\s()-]{7,14}\d(?![\w@/-]|\.\w)/g;
// Ofcom's numbers for drama and examples ("07700 900123"), by national prefix.
const DRAMA = [
  /^07700900\d{3}$/,
  /^01632960\d{3}$/,
  /^02079460\d{3}$/,
  /^01(1[3-8]|21|31|41|51|61)4960\d{3}$/,
  /^01914980\d{3}$/,
  /^02896496\d{3}$/,
  /^02920180\d{3}$/,
  /^08081570\d{3}$/,
  /^09098790\d{3}$/,
  /^03069990\d{3}$/,
];

/** The UK national number (0…) a candidate spells, or null if it isn't one worth flagging. */
export function ukPhone(candidate) {
  let digits = candidate.replace(/\D/g, '');
  if (candidate.trim().startsWith('+44')) digits = '0' + digits.slice(2).replace(/^0/, '');
  if (!/^0[1-37-9]/.test(digits) || digits.length < 10 || digits.length > 11) return null;
  // Separators must look like a phone number's: one kind, in a few groups.
  const groups = candidate.trim().split(/[\s-]+/).filter(Boolean);
  if (groups.length > 5) return null;
  if (DRAMA.some((re) => re.test(digits))) return null;
  if (hasRun(digits, 6)) return null; // 01234567890, 07777777777
  return digits;
}

// --------------------------------------------------------------- scanning

/**
 * Every finding in one line of text. `where` says where it is (shown with it).
 * Returns [{ rule, text }] with `text` already masked for printing.
 */
export function scanLine(line, termsRe, { generic = true } = {}) {
  const found = [];
  if (termsRe) {
    termsRe.lastIndex = 0;
    if (termsRe.test(line)) found.push({ rule: 'private term' });
  }
  if (generic) {
    for (const { rule, re } of KEY_RULES) {
      re.lastIndex = 0;
      for (const m of line.matchAll(re)) {
        if (!looksFake(m[1])) found.push({ rule, secret: m[0] });
      }
    }
    PEM.lastIndex = 0;
    if (PEM.test(line)) found.push({ rule: 'private-key' });
    for (const m of line.matchAll(EMAIL)) {
      if (!emailAllowed(m[0])) found.push({ rule: 'email' });
    }
    for (const m of line.matchAll(PHONE)) {
      if (ukPhone(m[0])) found.push({ rule: 'uk-phone' });
    }
  }
  if (found.length === 0) return found;
  let shown = line.trim();
  if (termsRe) shown = shown.replace(termsRe, MASK);
  for (const f of found) {
    if (f.secret) shown = shown.split(f.secret).join(`${f.secret.slice(0, 8)}…[redacted]`);
  }
  if (shown.length > 160) shown = shown.slice(0, 157) + '…';
  // One finding per rule per line is enough to act on.
  const seen = new Set();
  return found.filter((f) => !seen.has(f.rule) && seen.add(f.rule)).map((f) => ({ rule: f.rule, text: shown }));
}

function git(args, opts = {}) {
  const r = spawnSync('git', ['-c', 'core.quotepath=off', ...args], {
    encoding: 'utf8',
    maxBuffer: 1 << 30,
    windowsHide: true,
    ...opts,
  });
  return r;
}

function gitOk(args, opts) {
  const r = git(args, opts);
  if (r.error) throw r.error;
  if (r.status !== 0) throw new Error(`git ${args.join(' ')} failed: ${r.stderr.trim()}`);
  return r.stdout;
}

const RS = '\x1e';
const US = '\x1f';

/**
 * Scans `git log` output for a set of revisions: each commit's message, the
 * paths it touches and the lines its diff adds (a merge: the lines its
 * resolution adds, `--cc`). Calls `report(where, finding)`.
 */
export function scanCommits(revArgs, termsRe, report, { cwd, generic = true, seen = new Set() } = {}) {
  const out = gitOk(
    ['log', '-p', '--cc', '-U0', '-M', '--no-color', '--no-ext-diff', '--no-textconv', '--src-prefix=a/', '--dst-prefix=b/', `--format=${RS}%H${US}%B${US}`, ...revArgs],
    { cwd },
  );
  let count = 0;
  for (const record of out.split(RS)) {
    if (!record) continue;
    const [sha, message, patch = ''] = record.split(US);
    if (seen.has(sha)) continue;
    seen.add(sha);
    count++;
    const short = sha.slice(0, 8);
    message.split('\n').forEach((line, i) => {
      for (const f of scanLine(line, termsRe, { generic })) report(`${short}  (commit message, line ${i + 1})`, f);
    });
    let file = null;
    let next = 0;
    let cols = 1;
    for (const line of patch.split('\n')) {
      if (line.startsWith('diff --git ') || line.startsWith('diff --cc ') || line.startsWith('diff --combined ')) {
        file = null;
        continue;
      }
      if (line.startsWith('+++ ')) {
        file = line === '+++ /dev/null' ? null : line.slice(4).replace(/^b\//, '');
        if (file && termsRe) {
          termsRe.lastIndex = 0;
          if (termsRe.test(file)) report(`${short}  ${file.replace(termsRe, MASK)}`, { rule: 'private term', text: '(in the file path)' });
        }
        continue;
      }
      if (line.startsWith('--- ') || line.startsWith('rename ') || line.startsWith('Binary files ')) continue;
      const hunk = /^(@@+) .*\+(\d+)(?:,\d+)? @@+/.exec(line);
      if (hunk) {
        cols = hunk[1].length - 1;
        next = Number(hunk[2]);
        continue;
      }
      if (!file || line.startsWith('\\')) continue;
      const prefix = line.slice(0, cols);
      if (prefix.length < cols || /[^ +-]/.test(prefix)) continue;
      if (prefix.includes('-')) continue; // not in the result
      if (prefix.includes('+')) {
        for (const f of scanLine(line.slice(cols), termsRe, { generic })) report(`${short}  ${termsRe ? file.replace(termsRe, MASK) : file}:${next}`, f);
      }
      next++;
    }
  }
  return count;
}

/** The revisions a pre-push hook's stdin lines put on the remote. */
export function pushRevisions(stdin, cwd) {
  const sets = [];
  for (const raw of stdin.split('\n')) {
    const parts = raw.trim().split(/\s+/);
    if (parts.length < 4) continue;
    const [, localSha, , remoteSha] = parts;
    if (ZERO.test(localSha)) continue; // a deletion pushes nothing
    const known = !ZERO.test(remoteSha) && git(['cat-file', '-e', `${remoteSha}^{commit}`], { cwd }).status === 0;
    // A new branch (or a remote tip we don't have): whatever no remote branch has yet.
    sets.push(known ? [`${remoteSha}..${localSha}`] : [localSha, '--not', '--remotes']);
  }
  return sets;
}

/** Every tracked file as it is in the working tree now. */
export function scanTree(termsRe, report, { cwd, generic = true } = {}) {
  const root = gitOk(['rev-parse', '--show-toplevel'], { cwd }).trim();
  const files = gitOk(['ls-files', '-z'], { cwd: root }).split('\0').filter(Boolean);
  let count = 0;
  for (const file of files) {
    const shownPath = termsRe ? file.replace(termsRe, MASK) : file;
    if (termsRe) {
      termsRe.lastIndex = 0;
      if (termsRe.test(file)) report(shownPath, { rule: 'private term', text: '(in the file path)' });
    }
    let buf;
    try {
      buf = fs.readFileSync(path.join(root, file));
    } catch {
      continue; // deleted in the working tree
    }
    if (buf.length > 20 * 1024 * 1024 || buf.subarray(0, 8000).includes(0)) continue; // binary
    count++;
    buf
      .toString('utf8')
      .split(/\r?\n/)
      .forEach((line, i) => {
        for (const f of scanLine(line, termsRe, { generic })) report(`${shownPath}:${i + 1}`, f);
      });
  }
  return count;
}

// -------------------------------------------------------------------- main

function main(argv) {
  const args = argv.slice(2);
  const tree = args.includes('--scan-tree');
  const rangeAt = args.indexOf('--range');
  const range = rangeAt >= 0 ? args[rangeAt + 1] : null;
  const usePrivate = !args.includes('--no-private');
  if (!tree && !range && process.stdin.isTTY) {
    console.error('Usage: node scripts/privacy-guard.mjs --scan-tree | --range A..B [--no-private]');
    console.error('(with no arguments it runs as the pre-push hook and reads the refs from stdin)');
    return 2;
  }

  let termsRe = null;
  if (usePrivate) {
    const file = termsPath();
    const terms = loadTerms(file);
    if (terms === null) console.warn(`privacy-guard: no private terms file (${process.env.YAP_PRIVATE_TERMS ? 'YAP_PRIVATE_TERMS' : '%USERPROFILE%\\.yap-private\\terms.txt'}): checking keys, emails and phone numbers only.`);
    else termsRe = termsRegex(terms);
  }

  const findings = [];
  const report = (where, f) => findings.push({ where, ...f });
  let what;
  if (tree) {
    const n = scanTree(termsRe, report);
    what = `${n} tracked files`;
  } else {
    const sets = range ? [[range]] : pushRevisions(fs.readFileSync(0, 'utf8'), undefined);
    const seen = new Set();
    let n = 0;
    for (const revs of sets) n += scanCommits(revs, termsRe, report, { seen });
    what = `${n} commit${n === 1 ? '' : 's'}${range ? ` in ${range}` : ' about to be pushed'}`;
  }

  if (findings.length === 0) {
    console.log(`privacy-guard: ${what}: clean.`);
    return 0;
  }
  console.error(`privacy-guard: ${findings.length} finding${findings.length === 1 ? '' : 's'} in ${what}:`);
  for (const f of findings) console.error(`  ${f.where}  [${f.rule}]  ${f.text}`);
  console.error('');
  console.error('Fix them before this goes public: fictional names and details, example.com');
  console.error("addresses, Ofcom's drama numbers (07700 900xxx), no real keys. Amend or rebase the");
  console.error('commits, then push again. Only for a false positive, after checking it really is');
  console.error('one: git push --no-verify');
  return 1;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = main(process.argv);
  } catch (e) {
    console.error(`privacy-guard: ${e.message}`);
    process.exitCode = 2;
  }
}
