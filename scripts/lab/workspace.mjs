// The lab's scratch workspace for Yap Code (`--yap-code on` without
// `--workspace`): a tiny made-up project an agent can read and change
// without touching anything of the person's. Made afresh at every `start`
// unless `--keep-workspace`.
import fs from 'node:fs';
import path from 'node:path';

/** The workspace's name in Yap (its folder's). */
export const NAME = 'lab-sandbox';

const FILES = {
  'README.md': `# Unit Shelf

A tiny made-up library that converts kitchen units (cups, grams, millilitres).
It exists for Yap Lab: agents may read and change anything here.

- \`src/convert.js\`: the conversions.
- \`src/convert.test.js\`: its tests (\`npm test\`).
- \`TODO.md\`: what's left to do.
`,
  'TODO.md': `# To do

- [ ] Add tablespoons (1 tbsp = 15 ml).
- [ ] \`toGrams\` should refuse an unknown ingredient instead of returning NaN.
- [x] Cups to millilitres.
`,
  'package.json': `${JSON.stringify({ name: 'unit-shelf', version: '0.1.0', private: true, type: 'module', scripts: { test: 'node --test' } }, null, 2)}\n`,
  'src/convert.js': `// Kitchen conversions. Densities in grams per millilitre.
const DENSITY = { water: 1, flour: 0.53, sugar: 0.85, butter: 0.96 };

/** Cups to millilitres (a metric cup). */
export function cupsToMl(cups) {
  return cups * 250;
}

/** Millilitres of an ingredient to grams. */
export function toGrams(ml, ingredient) {
  return ml * DENSITY[ingredient];
}
`,
  'src/convert.test.js': `import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cupsToMl, toGrams } from './convert.js';

test('a cup is 250 ml', () => {
  assert.equal(cupsToMl(1), 250);
});

test('a cup of flour weighs about 132 g', () => {
  assert.equal(Math.round(toGrams(cupsToMl(1), 'flour')), 133);
});
`,
};

/** The one remote the scratch workspace gets (`set --git <branch>`): Yap's own public repository. */
export const LAB_REMOTE = 'https://github.com/nayballs/Yap.git';

/**
 * **The scratch workspace as a git repository on `branch`** (`set --git`,
 * 16 Nov 2026), its one remote Yap's own public repository — so a Code chat's
 * summary has a branch whose pull requests the lab's read-only `gh` can list
 * (`set --gh on`). No commit is made (the branch is new and empty: Yap reads
 * it from HEAD), nothing is fetched or pushed. Only a branch name of letters,
 * digits and `._/-`. `{ branch, remote }`.
 */
export function makeGitRepo(dir, branch, { run }) {
  if (!/^[A-Za-z0-9._/-]{1,200}$/.test(branch) || branch.startsWith('-')) throw new Error(`Not a branch name: ${branch}`);
  const git = (...args) => run('git', ['-C', dir, ...args]);
  if (!fs.existsSync(path.join(dir, '.git'))) git('init', '-q', '-b', branch);
  else git('symbolic-ref', 'HEAD', `refs/heads/${branch}`);
  const remotes = String(git('remote') ?? '').split(/\s+/).filter(Boolean);
  if (remotes.includes('origin')) git('remote', 'set-url', 'origin', LAB_REMOTE);
  else git('remote', 'add', 'origin', LAB_REMOTE);
  return { branch, remote: LAB_REMOTE };
}

/** Make the workspace in `dir` (emptied first unless `keep`); its path. */
export function makeWorkspace(dir, { keep = false } = {}) {
  if (!keep) fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  for (const [file, text] of Object.entries(FILES)) {
    const p = path.join(dir, file);
    if (keep && fs.existsSync(p)) continue;
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, text);
  }
  return dir;
}
