// The update flow against a local feed announcing the next patch version: a
// manual check finds it, the signed download is verified, the "ready" toast
// and Settings → About say so, and "Restart to update" stops short of running
// an installer (debug builds never do — it would replace the installed Yap).
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings } from './support/fixtures.js';
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
      const feed = await startUpdateFeed(path.join(RUNS, `update-feed-w${workerInfo.workerIndex}`), {
        version: VERSION,
        notes: '- Faster startup\n- A new end-to-end test suite',
      });
      await use(feed);
      await feed.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ updateFeed }, use) => {
      await use({
        name: 'updates',
        env: {
          YAP_UPDATE_TEST_ENDPOINT: updateFeed.endpoint,
          YAP_UPDATE_TEST_PUBKEY: updateFeed.pubkey,
          // Portable test instances would otherwise take the "update by hand"
          // path; force the installed-app path with a background download.
          YAP_UPDATE_TEST_PORTABLE: '0',
          YAP_UPDATE_TEST_METERED: '0',
        },
      });
    },
    { scope: 'worker' },
  ],
});

const toast = (main, title) => main.getByRole('status').filter({ hasText: title });

test('an update is found, downloaded, announced, and never installed by a debug build', async ({
  yap,
  main,
  updateFeed,
  shot,
}) => {
  const dialog = await openSettings(main, 'About');
  const updatesGroup = dialog.getByRole('group', { name: 'Updates' });
  await updatesGroup.getByRole('button', { name: 'Check for updates', exact: true }).click();

  // Found → downloaded and signature-checked in the background → "ready".
  const ready = toast(main, `Yap ${VERSION} is ready`);
  await expect(ready).toBeVisible({ timeout: 30_000 });
  await expect(ready.getByRole('button', { name: 'Restart to update' })).toBeVisible();
  expect(updateFeed.requests).toContain(`/Yap_${VERSION}_x64-setup.exe`);
  await expect(updatesGroup.getByText(`Yap ${VERSION} is ready to install`)).toBeVisible();
  await expect(updatesGroup.getByText(`What’s new in ${VERSION}`)).toBeVisible();
  await expect(updatesGroup.getByRole('button', { name: 'Restart to update' })).toBeVisible();
  expect(await yap.invoke('update_status')).toMatchObject({
    status: 'ready',
    version: VERSION,
    currentVersion: CURRENT,
  });
  await shot(main, '01-ready-toast-and-about');

  // "Restart to update" from the toast: everything up to the installer runs,
  // then the debug build refuses and the toast says why.
  await closeSettings(main);
  await ready.getByRole('button', { name: 'Restart to update' }).click();
  const failed = toast(main, "Couldn't update Yap");
  await expect(failed).toBeVisible({ timeout: 15_000 });
  await expect(failed).toContainText("the installer wasn't run");
  await expect.poll(async () => (await yap.invoke('update_status')).status).toBe('ready');
  await shot(main, '02-install-refused-in-debug');
});
