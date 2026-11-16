// End-to-end UI tests: Playwright drives a real (stub-STT) Yap build over
// CDP. Run with `npm run test:app`; see docs/e2e-tests.md.
import { defineConfig } from '@playwright/test';

// Specs that need something only one test Yap on a PC can hold at a time:
// the connectors' sign-in listener on its fixed port (127.0.0.1:51439,
// integrations/connectors/oauth/loopback.rs), which these specs expect to
// get. They run one at a time ("solo", workers: 1), beside the others.
const SOLO = ['connectors-signin.spec.js', 'connectors-discover.spec.js', 'connectors.spec.js'];

// Several test Yaps at once (2026-11-16): each spec file has its own,
// isolated in its own run folder, WebView2 profile, DevTools port and ports
// for its stand-ins, its windows behind the person's (docs/e2e-tests.md,
// "Several at once"). Locally 4 (YAP_E2E_WORKERS or --workers to change);
// on CI one a runner, and the suite split into shards (e2e.yml).
const WORKERS = Number(process.env.YAP_E2E_WORKERS) || (process.env.CI ? 1 : 4);

export default defineConfig({
  testDir: '.',
  testMatch: '*.spec.js',
  outputDir: '../../test-results/app/playwright',
  globalSetup: './support/global-setup.js',
  workers: WORKERS,
  // A spec file's tests run in order in one worker: they share its Yap.
  fullyParallel: false,
  projects: [
    { name: 'app', testIgnore: SOLO },
    { name: 'solo', testMatch: SOLO, workers: 1 },
  ],
  timeout: 60_000,
  expect: { timeout: 10_000 },
  // One retry, as on CI (2026-11-16): a test that fails and then passes is
  // reported as flaky, by name, in the summary; one failing twice fails.
  // Some fail now and then on a busy PC, one Yap at a time too (meeting-crash,
  // chat-browse's tabs, the bar's menu: 16 Nov 2026); `--retries=0` for none.
  retries: 1,
  // On CI, a broken environment (say, no DevTools port) should fail in
  // minutes rather than retrying every test until the job times out.
  maxFailures: process.env.CI ? 4 : 0,
  forbidOnly: !!process.env.CI,
  reporter: [
    ['list'],
    ['html', { outputFolder: '../../test-results/app/report', open: 'never' }],
    // Each spec's time, for CI's shards (shards.mjs --record).
    ['json', { outputFile: '../../test-results/app/results.json' }],
    ...(process.env.CI ? [['github']] : []),
  ],
});
