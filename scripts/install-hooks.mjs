// Installs Yap's pre-push privacy guard where every checkout of this repo runs
// it: the repository's shared hooks folder (`<git common dir>/hooks`), which
// all worktrees use. A `core.hooksPath` setting is removed, since a path
// that's missing in one checkout silently turned the guard off (22 Oct 2026).
// A copy of the guard also goes to `%USERPROFILE%\.yap-private\`, beside the
// private terms file, for checkouts made before the guard existed; the hook
// refuses a push when it can find neither.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const git = (...args) => spawnSync('git', args, { cwd: root, encoding: 'utf8' });

const common = git('rev-parse', '--path-format=absolute', '--git-common-dir').stdout.trim();
if (!common) {
  console.error('hooks:install: not in a git repository');
  process.exit(1);
}

const hooks = path.join(common, 'hooks');
fs.mkdirSync(hooks, { recursive: true });
const hook = path.join(hooks, 'pre-push');
fs.copyFileSync(path.join(root, '.githooks', 'pre-push'), hook);
fs.chmodSync(hook, 0o755);

const privateDir = path.join(os.homedir(), '.yap-private');
fs.mkdirSync(privateDir, { recursive: true });
fs.copyFileSync(path.join(root, 'scripts', 'privacy-guard.mjs'), path.join(privateDir, 'privacy-guard.mjs'));

git('config', '--unset', 'core.hooksPath');

console.log(`hooks:install: pre-push guard in ${hook}`);
console.log(`hooks:install: fallback copy in ${path.join(privateDir, 'privacy-guard.mjs')}`);
