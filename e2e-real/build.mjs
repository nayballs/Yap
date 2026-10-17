// Builds what the real-app robot tests run (`npm run test:real:build`):
//
// - by default the UI suite's own test build (e2e/build.mjs): the STUB,
//   DEBUG build with the frontend embedded — robot mode is part of every
//   debug build, so the two suites share one binary;
// - with `--engines`, a debug build with the real STT (`engines`: whisper
//   on Vulkan, ONNX on DirectML; needs the Vulkan SDK) for the robot's
//   speech-to-text spec. It goes to a short target dir of its own
//   (YAP_REAL_ENGINES_TARGET_DIR, default `<drive>:\t-real`: whisper.cpp's
//   CMake build nests paths past Windows' MAX_PATH under src-tauri\target,
//   the same reason scripts/dev.bat builds into `<drive>:\t`).
import { spawnSync } from 'node:child_process';
import path from 'node:path';
import { REPO } from '../e2e/support/yap.js';
import { ENGINES_TARGET_DIR } from './support/engines.js';

function run(cmd, args, opts = {}) {
  const r = spawnSync(cmd, args, { cwd: REPO, stdio: 'inherit', ...opts });
  if (r.error) throw r.error;
  if (r.status !== 0) process.exit(r.status ?? 1);
}

if (!process.argv.includes('--engines')) {
  run(process.execPath, [path.join(REPO, 'e2e', 'build.mjs')]);
} else {
  if (!process.env.VULKAN_SDK) {
    console.error('The engines build needs the Vulkan SDK (VULKAN_SDK is not set): https://vulkan.lunarg.com');
    process.exit(1);
  }
  run(process.execPath, [path.join(REPO, 'node_modules', 'vite', 'bin', 'vite.js'), 'build']);
  run('cargo', ['build', '--locked', '--features', 'custom-protocol,engines'], {
    cwd: path.join(REPO, 'src-tauri'),
    env: {
      CARGO_PROFILE_DEV_DEBUG: 'line-tables-only',
      ...process.env,
      CARGO_TARGET_DIR: ENGINES_TARGET_DIR,
    },
  });
  console.log(`\nrobot engines build: ${path.join(ENGINES_TARGET_DIR, 'debug', 'yap.exe')}`);
}
