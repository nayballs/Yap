// Real-app robot tests: a robot-mode Yap pastes into real windows the
// harness opens (a test pad, Windows Terminal, Edge, Notepad). Run with
// `npm run test:real`; see docs/e2e-tests.md, "Real-app robot tests".
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: '.',
  testMatch: '*.real.js',
  outputDir: '../test-results/real/playwright',
  globalSetup: './support/global-setup.js',
  // One robot at a time: there's one keyboard focus.
  workers: 1,
  fullyParallel: false,
  timeout: 90_000,
  expect: { timeout: 10_000 },
  retries: 0,
  reporter: [['list'], ['html', { outputFolder: '../test-results/real/report', open: 'never' }]],
});
