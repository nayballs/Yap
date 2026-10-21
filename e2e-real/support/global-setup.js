// Runs once before the robot suite: check there's a build, say how long
// the keyboard focus is taken, clear the last run, and remember the window
// in front so it gets the focus back at the end.
import fs from 'node:fs';
import path from 'node:path';
import { EXE } from '../../e2e/support/yap.js';
import { OUT, RUNS, compileHelper, foreground, focus } from './targets.js';

export default async function globalSetup() {
  if (process.platform !== 'win32') throw new Error('The robot tests drive real Windows apps: Windows only.');
  if (!fs.existsSync(EXE)) {
    throw new Error(`No test build at ${EXE}. Run \`npm run test:real:build\` (or \`npm run test:real\`).`);
  }
  for (const dir of [path.join(OUT, 'screenshots'), RUNS]) {
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
  compileHelper();
  const before = foreground();
  console.log(
    [
      '',
      '  ┌───────────────────────────────────────────────────────────────────┐',
      '  │ Yap robot tests: real pastes into real windows.                   │',
      '  │ They take the keyboard focus for about 3 minutes: a test pad,     │',
      '  │ Windows Terminal, Git Bash, Edge windows and (if closed now)      │',
      '  │ Notepad open and close. Please leave the keyboard and mouse       │',
      "  │ alone; Yap only ever pastes into the windows the robot opened.    │",
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
  };
}
