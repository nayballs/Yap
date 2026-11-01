// The downloads Yap pins at a version in its own code — GitHub release
// assets such as the speaker engine, sherpa-onnx, eSpeak NG and the on-device
// engine's floor — against each project's latest release. Run it locally
// (`npm run pins:check`) for a report; the weekly workflow
// (.github/workflows/pins.yml) runs it with `--issue` to keep one issue open
// while anything is behind, and close it once nothing is.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const TITLE = 'Pinned downloads behind upstream';
// Projects that release several times a day: their pin is a floor the app
// moves past by itself, so it's only behind once it's this many days older
// than their latest.
const FLOOR_DAYS = { 'ggml-org/llama.cpp': 30 };

/** Every GitHub release download pinned in the Rust code: repo → tags, and where. Pure but for the files. */
export function pinsIn(files) {
  const re = /https:\/\/github\.com\/([\w.-]+)\/([\w.-]+)\/releases\/download\/([^/"\s]+)\//g;
  const pins = new Map();
  for (const [file, text] of files) {
    const lines = text.split('\n');
    lines.forEach((line, i) => {
      for (const m of line.matchAll(re)) {
        const [, owner, repo, tag] = m;
        if (!isVersion(tag)) continue;
        const key = `${owner}/${repo}`;
        const pin = pins.get(key) ?? { repo: key, tags: new Set(), where: `${file}:${i + 1}` };
        pin.tags.add(tag);
        pins.set(key, pin);
      }
    });
  }
  return [...pins.values()].map((p) => ({ repo: p.repo, tag: [...p.tags].sort(compareVersions).at(-1), where: p.where }));
}

/** A release tag that's a version ("v0.2.0", "1.52.0", "b11960"), not a folder of assets ("speaker-segmentation-models"). Pure. */
export function isVersion(tag) {
  return /^(v?\d+(\.\d+)*([-.+][\w.]+)?|b\d+)$/i.test(tag);
}

/** Order two version tags by their numbers ("v1.13.8" < "1.13.10", "b10441" < "b11960"). Pure. */
export function compareVersions(a, b) {
  const nums = (t) => t.replace(/^[vb]/i, '').split(/[.\-+]/).map((p) => (/^\d+$/.test(p) ? Number(p) : -1));
  const x = nums(a);
  const y = nums(b);
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    const d = (x[i] ?? 0) - (y[i] ?? 0);
    if (d) return d;
  }
  return 0;
}

/** Whether a pin is behind its project's latest release (a fast project's floor only past FLOOR_DAYS). Pure. */
export function behind(pin, latest, pinnedAt, latestAt) {
  if (!latest || compareVersions(latest, pin.tag) <= 0) return false;
  const floor = FLOOR_DAYS[pin.repo];
  if (floor == null) return true;
  if (!pinnedAt || !latestAt) return true;
  return (Date.parse(latestAt) - Date.parse(pinnedAt)) / 86_400_000 > floor;
}

function rustFiles(dir) {
  const out = [];
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name);
    if (e.isDirectory()) out.push(...rustFiles(p));
    else if (e.name.endsWith('.rs')) out.push([path.relative(ROOT, p).replaceAll('\\', '/'), fs.readFileSync(p, 'utf8')]);
  }
  return out;
}

async function gh(url, init = {}) {
  const headers = { Accept: 'application/vnd.github+json', 'User-Agent': 'yap-pins-check', ...(init.headers || {}) };
  if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
  const r = await fetch(`https://api.github.com${url}`, { ...init, headers });
  if (!r.ok) throw new Error(`${r.status} ${url}`);
  return r.status === 204 ? null : r.json();
}

async function check(pin) {
  try {
    const latest = await gh(`/repos/${pin.repo}/releases/latest`);
    const pinned = await gh(`/repos/${pin.repo}/releases/tags/${encodeURIComponent(pin.tag)}`).catch(() => null);
    return { ...pin, latest: latest.tag_name, latestAt: latest.published_at, pinnedAt: pinned?.published_at ?? null, behind: behind(pin, latest.tag_name, pinned?.published_at, latest.published_at) };
  } catch (e) {
    return { ...pin, error: String(e.message || e) };
  }
}

function report(rows) {
  const day = (iso) => (iso ? iso.slice(0, 10) : '?');
  const lines = [
    'The downloads Yap pins at a version, against each project\'s latest release (`npm run pins:check`).',
    '',
    '| Download | Pinned | Latest | Behind? | Where |',
    '|---|---|---|---|---|',
    ...rows.map((r) =>
      r.error
        ? `| ${r.repo} | ${r.tag} | couldn't check (${r.error}) | ? | \`${r.where}\` |`
        : `| ${r.repo} | ${r.tag} (${day(r.pinnedAt)}) | ${r.latest} (${day(r.latestAt)}) | ${r.behind ? '**yes**' : 'no'} | \`${r.where}\` |`
    ),
    '',
    'Update a pin with its new URL and the SHA-256 the release publishes, then check it on a real PC before it ships.',
  ];
  return lines.join('\n');
}

async function syncIssue(rows, text) {
  const repo = process.env.GITHUB_REPOSITORY;
  if (!repo || !process.env.GITHUB_TOKEN) throw new Error('--issue needs GITHUB_REPOSITORY and GITHUB_TOKEN');
  const open = await gh(`/repos/${repo}/issues?state=open&per_page=100`);
  const issue = open.find((i) => i.title === TITLE && !i.pull_request);
  const anyBehind = rows.some((r) => r.behind);
  const post = (url, method, body) => gh(url, { method, body: JSON.stringify(body), headers: { 'Content-Type': 'application/json' } });
  if (anyBehind && issue) await post(`/repos/${repo}/issues/${issue.number}`, 'PATCH', { body: text });
  else if (anyBehind) await post(`/repos/${repo}/issues`, 'POST', { title: TITLE, body: text });
  else if (issue) {
    await post(`/repos/${repo}/issues/${issue.number}/comments`, 'POST', { body: 'Every pinned download is current now.' });
    await post(`/repos/${repo}/issues/${issue.number}`, 'PATCH', { state: 'closed' });
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const pins = pinsIn(rustFiles(path.join(ROOT, 'src-tauri', 'src')));
  const rows = [];
  for (const p of pins) rows.push(await check(p));
  const text = report(rows);
  console.log(text);
  if (process.argv.includes('--issue')) await syncIssue(rows, text);
}
