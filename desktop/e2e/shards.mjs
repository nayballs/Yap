// CI's e2e shards, split by time (.github/workflows/e2e.yml; docs/e2e-tests.md, CI).
//
//   node desktop/e2e/shards.mjs 3/8 > list.txt   shard 3 of 8: its spec files, one a line,
//                                                for `playwright test --test-list list.txt`
//   node desktop/e2e/shards.mjs --record <results.json>…
//                                                timings.json from runs' JSON reports
//                                                (test-results/app/results.json)
//
// Playwright's own `--shard` splits the files in name order by how many tests
// each has, so the long agents-* specs landed together: 8 shards of it would
// have run from 10.5 to 16.7 minutes. Here each spec file (whole: its tests
// share one worker-scoped Yap) goes, longest first, to the shard with the
// least time so far: 8 shards of about 12.4 minutes each from the same
// timings. A spec timings.json doesn't know yet (a new one) counts as the
// median; every spec file is in exactly one shard.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const TIMINGS = path.join(HERE, 'timings.json');
/** What starting a spec's test Yap costs on CI, beside its tests' own time. */
const START_S = 5;

/** The spec files, by name. */
export function specFiles(dir = HERE) {
  return fs.readdirSync(dir).filter((f) => f.endsWith('.spec.js')).sort();
}

/** `n` shards of `files`, each `{ files, seconds }`, balanced by `seconds[file]`. Pure. */
export function split(files, seconds, n) {
  const known = files.map((f) => seconds[f]).filter((s) => Number.isFinite(s)).sort((a, b) => a - b);
  const fallback = known.length ? known[Math.floor(known.length / 2)] : 30;
  const cost = (f) => (Number.isFinite(seconds[f]) ? seconds[f] : fallback);
  const shards = Array.from({ length: n }, () => ({ files: [], seconds: 0 }));
  for (const f of [...files].sort((a, b) => cost(b) - cost(a) || a.localeCompare(b))) {
    const lightest = shards.reduce((min, s) => (s.seconds < min.seconds ? s : min));
    lightest.files.push(f);
    lightest.seconds += cost(f);
  }
  for (const s of shards) s.files.sort();
  return shards;
}

/** Seconds per spec file in a Playwright JSON report: every attempt of its tests, and its start. */
export function secondsIn(report) {
  const out = {};
  const walk = (suite, file) => {
    const here = suite.file ? path.basename(suite.file) : file;
    for (const spec of suite.specs ?? []) {
      for (const test of spec.tests ?? []) {
        for (const r of test.results ?? []) out[here] = (out[here] ?? START_S) + (r.duration ?? 0) / 1000;
      }
    }
    for (const child of suite.suites ?? []) walk(child, here);
  };
  for (const suite of report.suites ?? []) walk(suite, null);
  return out;
}

function main(args) {
  const timings = JSON.parse(fs.readFileSync(TIMINGS, 'utf8'));
  if (args[0] === '--record') {
    if (args.length < 2) throw new Error('Usage: shards.mjs --record <results.json>…');
    for (const file of args.slice(1)) {
      for (const [spec, s] of Object.entries(secondsIn(JSON.parse(fs.readFileSync(file, 'utf8'))))) {
        timings.seconds[spec] = Math.round(s);
      }
    }
    const files = new Set(specFiles());
    timings.seconds = Object.fromEntries(Object.entries(timings.seconds).filter(([f]) => files.has(f)).sort(([a], [b]) => a.localeCompare(b)));
    fs.writeFileSync(TIMINGS, `${JSON.stringify(timings, null, 1)}\n`);
    console.log(`timings.json: ${Object.keys(timings.seconds).length} spec files`);
    return;
  }
  const m = /^(\d+)\/(\d+)$/.exec(args[0] ?? '');
  if (!m || Number(m[1]) < 1 || Number(m[1]) > Number(m[2])) throw new Error('Usage: shards.mjs <k>/<n> (1-based), or --record <results.json>…');
  const shard = split(specFiles(), timings.seconds, Number(m[2]))[Number(m[1]) - 1];
  console.error(`e2e shard ${m[1]}/${m[2]}: ${shard.files.length} spec files, about ${(shard.seconds / 60).toFixed(1)} min on CI`);
  process.stdout.write(`${shard.files.join('\n')}\n`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    main(process.argv.slice(2));
  } catch (e) {
    console.error(e.message);
    process.exit(1);
  }
}
