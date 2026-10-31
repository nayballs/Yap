#!/usr/bin/env node
// Runs the Astro CLI with its anonymous usage telemetry switched off: a site
// that promises "nothing leaves your PC" shouldn't phone home while it builds.
// (`ASTRO_TELEMETRY_DISABLED` is read by @astrojs/telemetry; nothing is
// written to the user's global Astro config.)

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

// The package's own bin (it isn't in astro's "exports", so not require.resolve).
const cli = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../node_modules/astro/bin/astro.mjs');

const run = spawnSync(process.execPath, [cli, ...process.argv.slice(2)], {
  stdio: 'inherit',
  env: { ...process.env, ASTRO_TELEMETRY_DISABLED: '1' },
});
process.exit(run.status ?? 1);
