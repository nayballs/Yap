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
