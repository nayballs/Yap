// Runs once before the suite: fail fast without a test build, and clear the
// previous run's screenshots and app folders.
import fs from 'node:fs';
import { EXE, RUNS, SCREENSHOTS } from './yap.js';

export default function globalSetup() {
  if (!fs.existsSync(EXE)) {
    throw new Error(`No test build at ${EXE}. Run \`npm run test:app:build\` (or \`npm run test:app\`).`);
  }
  for (const dir of [SCREENSHOTS, RUNS]) {
    fs.rmSync(dir, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
}
