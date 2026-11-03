// Yap Link's phone page (src-tauri/link-page/) in a real Chromium, with the
// PC mocked: the page's files from a made-up HTTPS origin (page.route), Yap
// on the PC as a WebSocket mock (page.routeWebSocket), the mic faked from
// WAV files (support/audio.js). No Yap build, nothing on the network, no
// window on screen. Run with `npm run test:link`; see the page's README.md.
//
// And in Firefox (the "firefox" project), when Playwright's own Firefox is
// installed (`npx playwright install firefox`): the page's specs that don't
// need a recorded mic, Firefox's fake mic being a tone (its prefs below).
// YAP_LINK_FIREFOX=0 leaves it out, =1 insists on it.
import fs from 'node:fs';
import { defineConfig, chromium, firefox } from '@playwright/test';
import { TONE_WAV, SPEECH_WAV, TONE_HZ } from './support/audio.js';

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

function withFirefox() {
  if (process.env.YAP_LINK_FIREFOX === '0') return false;
  if (process.env.YAP_LINK_FIREFOX === '1') return true;
  try {
    return fs.existsSync(firefox.executablePath());
  } catch {
    return false;
  }
}

const media = ['--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream'];

const projects = [
  {
    name: 'link',
    testIgnore: /handsfree\.spec\.js/,
    // The mic: a steady 440 Hz tone, looped.
    use: {
      channel: channel(),
      permissions: ['microphone'],
      launchOptions: { args: [...media, `--use-file-for-fake-audio-capture=${TONE_WAV}`] },
    },
  },
  {
    name: 'handsfree',
    testMatch: /handsfree\.spec\.js/,
    // The mic: quiet, a moment of speech, then quiet (played once).
    use: {
      channel: channel(),
      permissions: ['microphone'],
      launchOptions: { args: [...media, `--use-file-for-fake-audio-capture=${SPEECH_WAV}%noloop`] },
    },
  },
];

if (withFirefox()) {
  projects.push({
    name: 'firefox',
    // Hands-free needs the recorded speech, and the screenshots are Chromium's.
    testMatch: /(link-page|sound|mic-open|typing|picker|chats|pictures|words)\.spec\.js/,
    use: {
      browserName: 'firefox',
      // Firefox has no microphone permission to grant here: its prefs let
      // the page have the fake mic without asking.
      permissions: [],
      launchOptions: {
        firefoxUserPrefs: {
          'media.navigator.streams.fake': true,
          'media.navigator.permission.disabled': true,
          // The fake mic: a steady tone, at the same pitch as Chromium's.
          'media.navigator.audio.fake_frequency': TONE_HZ,
          // Nothing heard on the PC's speakers.
          'media.volume_scale': '0.0',
        },
      },
    },
  });
}

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
    headless: true,
    // A phone-sized page.
    viewport: { width: 390, height: 844 },
    hasTouch: false,
    trace: 'retain-on-failure',
    screenshot: 'only-on-failure',
  },
  projects,
});
