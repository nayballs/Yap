// Runs once before the robot suite: the hands-off guard first (support/
// guard.js: the PC idle, the input watcher up — else nothing runs and
// nothing is touched), then check there's a build, say how long the
// keyboard focus is taken, clear the last run, and remember the window in
// front so it gets the focus back at the end.
import fs from 'node:fs';
import path from 'node:path';
import { EXE } from '../../e2e/support/yap.js';
import { startGuard } from './guard.js';
import { OUT, RUNS, foreground, focus } from './targets.js';

export default async function globalSetup() {
  if (process.platform !== 'win32') throw new Error('The robot tests drive real Windows apps: Windows only.');
  const guard = await startGuard();
  let before;
  try {
    if (!fs.existsSync(EXE)) {
      throw new Error(`No test build at ${EXE}. Run \`npm run test:real:build\` (or \`npm run test:real\`).`);
    }
    for (const dir of [path.join(OUT, 'screenshots'), RUNS]) {
      fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
    }
    before = foreground();
  } catch (e) {
    await guard.stop();
    throw e;
  }
  console.log(
    [
      '',
      '  ┌───────────────────────────────────────────────────────────────────┐',
      '  │ Yap robot tests: real pastes into real windows.                   │',
      '  │ They take the keyboard focus for about 3 minutes: a test pad,     │',
      '  │ Windows Terminal, Git Bash, Edge windows and (if closed now)      │',
      '  │ Notepad open and close. Any key, click or mouse move of yours     │',
      "  │ stops the run. Yap only ever pastes into the robot's windows.     │",
      '  └───────────────────────────────────────────────────────────────────┘',
      '',
    ].join('\n'),
  );
  return async () => {
    // The window that was in front gets the focus back.
    try {
      if (before) await focus(before);
    } catch {
      /* it may be gone */
    }
    await guard.stop();
  };
}
