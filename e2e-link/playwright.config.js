// Yap Link's phone page (src-tauri/link-page/) in a real Chromium, with the
// PC mocked: the page's files from a made-up HTTPS origin (page.route), Yap
// on the PC as a WebSocket mock (page.routeWebSocket), the mic faked from
// WAV files (support/audio.js). No Yap build, nothing on the network, no
// window on screen. Run with `npm run test:link`; see the page's README.md.
import fs from 'node:fs';
import { defineConfig, chromium } from '@playwright/test';
import { TONE_WAV, SPEECH_WAV } from './support/audio.js';

// Playwright's own Chromium when it's installed (`npx playwright install
// chromium`), else Edge, which every Windows PC has (Chromium too). Or say:
// YAP_LINK_CHANNEL=chromium|msedge|chrome.
function channel() {
  if (process.env.YAP_LINK_CHANNEL) return process.env.YAP_LINK_CHANNEL;
  try {
    return fs.existsSync(chromium.executablePath()) ? 'chromium' : 'msedge';
  } catch {
    return 'msedge';
  }
}

const media = ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'];

export default defineConfig({
  testDir: '.',
  testMatch: '*.spec.js',
  outputDir: '../test-results/link/playwright',
  globalSetup: './support/global-setup.js',
  // Real-time audio: two at a time is plenty, and keeps the PC calm.
  workers: 2,
  timeout: 45_000,
  expect: { timeout: 8_000 },
  retries: process.env.CI ? 1 : 0,
  forbidOnly: !!process.env.CI,
  reporter: [['list'], ['html', { outputFolder: '../test-results/link/report', open: 'never' }]],
  use: {
    channel: channel(),
    headless: true,
    // A phone-sized page.
    viewport: { width: 390, height: 844 },
    hasTouch: false,
    permissions: ['microphone'],
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects: [
    {
      name: 'link',
      testIgnore: /handsfree\.spec\.js/,
      // The mic: a steady 440 Hz tone, looped.
      use: { launchOptions: { args: [...media, `--use-file-for-fake-audio-capture=${TONE_WAV}`] } },
    },
    {
      name: 'handsfree',
      testMatch: /handsfree\.spec\.js/,
      // The mic: quiet, a moment of speech, then quiet (played once).
      use: { launchOptions: { args: [...media, `--use-file-for-fake-audio-capture=${SPEECH_WAV}%noloop`] } },
    },
  ],
});
