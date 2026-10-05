// The update flow against a local feed announcing the next patch version: a
// manual check finds it, the signed download is verified, Settings → About
// says so (and only About: a check the person started doesn't also pop a
// toast), and "Restart to update" stops short of running an installer (debug
// builds never do — it would replace the installed Yap). The automatic path,
// which announces with a toast, is updates-auto.spec.js.
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
          // This spec checks by hand: keep the automatic first check away.
          YAP_UPDATE_TEST_FIRST_CHECK: '3600',
        },
      });
    },
    { scope: 'worker' },
  ],
});

const toast = (main, title) => main.getByRole('status').filter({ hasText: title });

test('a manual check finds the update, shows it in About only, and a debug build never installs it', async ({
  yap,
  main,
  updateFeed,
  shot,
}) => {
  const dialog = await openSettings(main, 'About');
  const updatesGroup = dialog.getByRole('group', { name: 'Updates' });
  await updatesGroup.getByRole('button', { name: 'Check for updates', exact: true }).click();

  // Found → downloaded and signature-checked in the background → "ready",
  // right where the check was asked for.
  await expect(updatesGroup.getByText(`Yap ${VERSION} is ready to install`)).toBeVisible({ timeout: 30_000 });
  expect(updateFeed.requests).toContain(`/Yap_${VERSION}_x64-setup.exe`);
  await expect(updatesGroup.getByText(`What’s new in ${VERSION}`)).toBeVisible();
  await expect(updatesGroup.getByRole('button', { name: 'Restart to update' })).toBeVisible();
  expect(await yap.invoke('update_status')).toMatchObject({
    status: 'ready',
    version: VERSION,
    currentVersion: CURRENT,
  });
  // …and only there: no toast repeats what About already says.
  await main.waitForTimeout(1500);
  await expect(toast(main, `Yap ${VERSION} is ready`)).toHaveCount(0);
  await shot(main, '01-ready-in-about-no-toast');

  // "Restart to update" in About: everything up to the installer runs, then
  // the debug build refuses and About says why.
  await updatesGroup.getByRole('button', { name: 'Restart to update' }).click();
  await expect(updatesGroup.getByText("the installer wasn't run", { exact: false })).toBeVisible({
    timeout: 15_000,
  });
  await expect.poll(async () => (await yap.invoke('update_status')).status).toBe('ready');
  await shot(main, '02-install-refused-in-debug');
  await closeSettings(main);
});
