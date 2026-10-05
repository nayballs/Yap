// The automatic update path: with the main window on screen, a background
// check finds the next patch version on its own, downloads and verifies it,
// and announces it with the "ready" toast; "Restart to update" there stops
// short of running an installer (debug builds never do). A check the person
// starts by hand shows in About instead: updates.spec.js.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect } from './support/fixtures.js';
import { startUpdateFeed } from './support/update-feed.js';
import { REPO, RUNS } from './support/yap.js';

// The build's own version (CARGO_PKG_VERSION), and the one the feed offers.
const CURRENT = fs
  .readFileSync(path.join(REPO, 'src-tauri', 'Cargo.toml'), 'utf8')
  .match(/^version\s*=\s*"([^"]+)"/m)[1];
const [major, minor, patch] = CURRENT.split('-')[0].split('.').map(Number);
const VERSION = `${major}.${minor}.${patch + 1}`;

const test = base.extend({
  updateFeed: [
    async ({}, use, workerInfo) => {
      const feed = await startUpdateFeed(path.join(RUNS, `update-feed-auto-w${workerInfo.workerIndex}`), {
        version: VERSION,
        notes: '- Faster startup',
      });
      await use(feed);
      await feed.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ updateFeed }, use) => {
      await use({
        name: 'updates-auto',
        env: {
          YAP_UPDATE_TEST_ENDPOINT: updateFeed.endpoint,
          YAP_UPDATE_TEST_PUBKEY: updateFeed.pubkey,
          YAP_UPDATE_TEST_PORTABLE: '0',
          YAP_UPDATE_TEST_METERED: '0',
          // The first automatic check, a few seconds after launch.
          YAP_UPDATE_TEST_FIRST_CHECK: '3',
        },
      });
    },
    { scope: 'worker' },
  ],
});

const toast = (main, title) => main.getByRole('status').filter({ hasText: title });

test('a background check announces the update with a toast', async ({ yap, main, updateFeed, shot }) => {
  // The harness keeps Yap's window hidden (an announcement then goes to a
  // Windows notification, which test mode never posts). Open it the way the
  // tray does; the update itself is still found without anyone asking.
  await yap.invoke('open_settings');
  // Nobody clicks anything: the scheduler finds it, downloads it, announces it.
  const ready = toast(main, `Yap ${VERSION} is ready`);
  await expect(ready).toBeVisible({ timeout: 30_000 });
  await expect(ready.getByRole('button', { name: 'Restart to update' })).toBeVisible();
  expect(updateFeed.requests).toContain(`/Yap_${VERSION}_x64-setup.exe`);
  expect(await yap.invoke('update_status')).toMatchObject({ status: 'ready', version: VERSION });
  await shot(main, '01-ready-toast');

  // "Restart to update" from the toast: everything up to the installer runs,
  // then the debug build refuses and the toast says why.
  await ready.getByRole('button', { name: 'Restart to update' }).click();
  const failed = toast(main, "Couldn't update Yap");
  await expect(failed).toBeVisible({ timeout: 15_000 });
  await expect(failed).toContainText("the installer wasn't run");
  await expect.poll(async () => (await yap.invoke('update_status')).status).toBe('ready');
  await shot(main, '02-install-refused-in-debug');
});
