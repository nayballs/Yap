// Builds the binary the e2e suite drives (`npm run test:app:build`):
// - the default STUB build (no `engines`: transcription returns placeholder
//   text), so no Vulkan SDK or model is needed;
// - a DEBUG build, because the test-only hooks (YAP_E2E, YAP_UPDATE_TEST_*)
//   only exist with debug_assertions;
// - with the frontend embedded (`custom-protocol`, as `tauri build` would),
//   so no Vite server is involved and a running dev server is never touched.
// Same result as `tauri build --debug --no-bundle`, minus the CLI rewriting
// src-tauri/Cargo.toml. It uses its own Cargo target dir (YAP_E2E_TARGET_DIR,
// default src-tauri/target/e2e), so it never fights a running `tauri dev` over
// a target dir's lock or artifacts, and keeps only line-table debug info
// (enough for backtraces, a fraction of the size and build time).
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { REPO, TARGET_DIR } from './support/yap.js';

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { cwd: REPO, stdio: 'inherit', ...opts });
  if (r.error) throw r.error;
  if (r.status !== 0) process.exit(r.status ?? 1);
}

// The frontend first: tauri's generate_context! embeds dist/ at compile time.
run(process.execPath, [path.join(REPO, 'node_modules', 'vite', 'bin', 'vite.js'), 'build']);
run('cargo', ['build', '--locked', '--features', 'custom-protocol'], {
  cwd: path.join(REPO, 'src-tauri'),
  env: {
    CARGO_PROFILE_DEV_DEBUG: 'line-tables-only',
    ...process.env,
    CARGO_TARGET_DIR: TARGET_DIR,
  },
});
console.log(`\ne2e test build: ${path.join(TARGET_DIR, 'debug', 'yap.exe')}`);
