// End-to-end UI tests: Playwright drives a real (stub-STT) Yap build over
// CDP. Run with `npm run test:app`; see docs/e2e-tests.md.
import { defineConfig } from '@playwright/test';

export default defineConfig({
  testDir: '.',
  testMatch: '*.spec.js',
  outputDir: '../test-results/app/playwright',
  globalSetup: './support/global-setup.js',
  // One app instance at a time: specs share nothing, but parallel instances
  // would fight over the screen and slow CI runners down.
  workers: 1,
  fullyParallel: false,
  timeout: 60_000,
  expect: { timeout: 10_000 },
  retries: process.env.CI ? 1 : 0,
  // On CI, a broken environment (say, no DevTools port) should fail in
  // minutes rather than retrying every test until the job times out.
  maxFailures: process.env.CI ? 4 : 0,
  forbidOnly: !!process.env.CI,
  reporter: [
    ['list'],
    ['html', { outputFolder: '../test-results/app/report', open: 'never' }],
    ...(process.env.CI ? [['github']] : []),
  ],
});
