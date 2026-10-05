// Release notes from conventional commit subjects: latest.json's `notes` (the
// in-app "What's new" in the update toast and Settings → About) and the
// GitHub release body. Used by .github/workflows/nightly.yml + release.yml.
//
//   node scripts/release-notes.mjs [<since-ref>]
//
// Lists the feat/fix/perf commits in <since-ref>..HEAD (no merges), or the
// last 25 commits when <since-ref> is missing or not an ancestor of HEAD.
// Skips what doesn't ship in the app: the account server (cloud), the
// website, CI, docs and test tooling. Prints markdown (lib/markdown.js
// renders it).
import { execFileSync } from 'node:child_process';

const git = (...args) => execFileSync('git', args, { encoding: 'utf8' });

const since = (process.argv[2] || '').trim();
let range = ['-n', '25', 'HEAD'];
if (since) {
  try {
    git('merge-base', '--is-ancestor', since, 'HEAD');
    range = [`${since}..HEAD`];
  } catch {
    console.error(`release-notes: ${since} isn't an ancestor of HEAD; using recent history`);
  }
}

const SKIP_SCOPES = new Set(['cloud', 'website', 'ci', 'docs', 'deps', 'e2e', 'test', 'tests', 'lint']);
const groups = { feat: [], fix: [] };
const seen = new Set();
for (const subject of git('log', '--no-merges', '--format=%s', ...range).split('\n')) {
  const m = /^(feat|fix|perf)(?:\(([^)]*)\))?!?:\s*(.+)$/i.exec(subject.trim());
  if (!m) continue;
  const scopes = (m[2] || '').toLowerCase().split(/[\s,/]+/);
  if (scopes.some((s) => SKIP_SCOPES.has(s))) continue;
  const text = m[3].trim().replace(/\.$/, '');
  const line = text.charAt(0).toUpperCase() + text.slice(1);
  if (seen.has(line)) continue;
  seen.add(line);
  groups[m[1].toLowerCase() === 'feat' ? 'feat' : 'fix'].push(line);
}

const MAX = 8;
const list = (items) =>
  [
    ...items.slice(0, MAX).map((t) => `- ${t}`),
    ...(items.length > MAX ? [`- …and ${items.length - MAX} more`] : []),
  ].join('\n');

const parts = [];
if (groups.feat.length) parts.push(`### New\n${list(groups.feat)}`);
if (groups.fix.length) parts.push(`### Fixes\n${list(groups.fix)}`);
process.stdout.write(parts.length ? `${parts.join('\n\n')}\n` : 'Behind-the-scenes improvements and polish.\n');
