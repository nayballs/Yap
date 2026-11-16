// `node --test desktop/e2e/shards.test.mjs` (npm run test:shards): CI's e2e
// shards (shards.mjs) hold every spec file once, evenly.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { secondsIn, specFiles, split } from './shards.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));

test('every spec file is in exactly one shard, for any count', () => {
  const files = specFiles();
  const { seconds } = JSON.parse(fs.readFileSync(path.join(HERE, 'timings.json'), 'utf8'));
  for (const n of [1, 3, 8, 13]) {
    const all = split(files, seconds, n).flatMap((s) => s.files);
    assert.equal(all.length, files.length, `${n} shards`);
    assert.deepEqual([...all].sort(), files, `${n} shards`);
  }
});

test('longest first onto the lightest: even shards, the same every time', () => {
  const seconds = { 'a.spec.js': 60, 'b.spec.js': 50, 'c.spec.js': 40, 'd.spec.js': 30, 'e.spec.js': 20, 'f.spec.js': 10 };
  const shards = split(Object.keys(seconds), seconds, 3);
  assert.deepEqual(shards.map((s) => s.seconds), [70, 70, 70]);
  assert.deepEqual(shards.map((s) => s.files), [['a.spec.js', 'f.spec.js'], ['b.spec.js', 'e.spec.js'], ['c.spec.js', 'd.spec.js']]);
  assert.deepEqual(split(Object.keys(seconds), seconds, 3), shards);
});

test('a spec timings.json does not know counts as the median', () => {
  const seconds = { 'a.spec.js': 10, 'b.spec.js': 20, 'c.spec.js': 90 };
  const shards = split(['a.spec.js', 'b.spec.js', 'c.spec.js', 'new.spec.js'], seconds, 2);
  assert.deepEqual(shards.map((s) => s.seconds).sort((x, y) => x - y), [50, 90]);
});

test('a JSON report: every attempt of every test, by its file, and its start', () => {
  const report = {
    suites: [
      {
        file: 'bar.spec.js',
        specs: [{ tests: [{ results: [{ duration: 2000 }, { duration: 3000 }] }] }],
        suites: [{ title: 'a describe', specs: [{ tests: [{ results: [{ duration: 1000 }] }] }] }],
      },
      { file: 'sub/notes.spec.js', specs: [{ tests: [{ results: [{ duration: 500 }] }] }] },
    ],
  };
  assert.deepEqual(secondsIn(report), { 'bar.spec.js': 11, 'notes.spec.js': 5.5 });
});
