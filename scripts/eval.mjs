// `npm run eval -- <options>`: build the debug binary the model evaluation
// harness lives in, then run `yap.exe eval <options>` from the repo root
// (docs/model-evals.md). `--no-build` skips the build.
//
// - A DEBUG build (the harness only exists with debug_assertions), the
//   default STUB build (no `engines`: no Vulkan SDK needed; Chat doesn't use
//   speech-to-text), into its own target dir ($YAP_EVAL_TARGET_DIR, default
//   src-tauri/target/eval), so it never fights `tauri dev` or the e2e build.
// - Run folders go to $YAP_EVAL_OUT (default test-results/evals, git-ignored)
//   unless --out is given.
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const TARGET_DIR = process.env.YAP_EVAL_TARGET_DIR
  ? path.resolve(process.env.YAP_EVAL_TARGET_DIR)
  : path.join(REPO, 'src-tauri', 'target', 'eval');
const exe = path.join(TARGET_DIR, 'debug', process.platform === 'win32' ? 'yap.exe' : 'yap');

let args = process.argv.slice(2);
const build = !args.includes('--no-build');
args = args.filter((a) => a !== '--no-build');

function run(cmd, cmdArgs, opts = {}) {
  const r = spawnSync(cmd, cmdArgs, { cwd: REPO, stdio: 'inherit', ...opts });
  if (r.error) throw r.error;
  return r.status ?? 1;
}

if (build) {
  // tauri's generate_context! needs dist/ to exist at compile time.
  if (!existsSync(path.join(REPO, 'dist', 'index.html'))) {
    const s = run(process.execPath, [path.join(REPO, 'node_modules', 'vite', 'bin', 'vite.js'), 'build']);
    if (s !== 0) process.exit(s);
  }
  const s = run('cargo', ['build', '--locked'], {
    cwd: path.join(REPO, 'src-tauri'),
    env: { CARGO_PROFILE_DEV_DEBUG: 'line-tables-only', ...process.env, CARGO_TARGET_DIR: TARGET_DIR },
  });
  if (s !== 0) process.exit(s);
}
process.exit(run(exe, ['eval', ...args]));
