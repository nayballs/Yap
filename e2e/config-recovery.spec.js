// A config.json Yap can't read (config.rs, 2026-10-23): set aside as it
// was, never saved over. Here the kind a newer Yap could leave behind after
// a downgrade — JSON, but one setting of a type this build can't read. Yap
// keeps every other setting (the suite's own hotkey and switches, an API
// key), resets that one, writes the rest back as config.json, and says so
// once (an alert, kind "configReset"). Until then any of this was "the
// defaults", and the next save wrote them over the real file.
//
// A torn file (not JSON at all) starts from the defaults: config.rs's unit
// tests cover it, since a test instance on the defaults would take F9 and
// check for updates.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { BASE_CONFIG } from './support/yap.js';

const BROKEN = JSON.stringify({ ...BASE_CONFIG, ppApiKeys: { groq: 'example-key' }, dictionary: 'not a list' }, null, 2);

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'config-recovery',
        prepare: (dataDir) => fs.writeFileSync(path.join(dataDir, 'config.json'), BROKEN),
      });
    },
    { scope: 'worker' },
  ],
});

const setAside = (yap) => fs.readdirSync(yap.dataDir).filter((f) => f.startsWith('config.corrupt-'));
const log = (yap) => fs.readFileSync(yap.logPath, 'utf8');
const TOLD = /alert: Some of your settings were reset/g;

test("a settings file Yap can't read is set aside, what it can read kept, and the person told once", async ({ yap, main }) => {
  // Set aside as it was.
  const copies = setAside(yap);
  expect(copies).toHaveLength(1);
  expect(fs.readFileSync(path.join(yap.dataDir, copies[0]), 'utf8')).toBe(BROKEN);
  // The rest kept and written back; only the dictionary reset.
  const cfg = yap.readConfig();
  expect(cfg.hotkey).toBe(BASE_CONFIG.hotkey);
  expect(cfg.updateChecksEnabled).toBe(false);
  expect(cfg.ppApiKeys).toEqual({ groq: 'example-key' });
  expect(cfg.dictionary).toEqual([]);
  // Told once the windows were up; the log has the alert's title, never
  // what was in the file.
  await expect.poll(() => log(yap), { timeout: 15_000 }).toMatch(TOLD);
  expect(log(yap)).not.toContain('example-key');

  // A change in Settings saves over the repaired file, never the copy.
  const dialog = await openSettings(main, 'Dictation');
  const offer = dialog.getByRole('button', { name: 'Offer what you said after AI cleanup' });
  await offer.click();
  await expectStore(yap, 'config.json', (c) => c.asSaidOffer === true && c.ppApiKeys?.groq === 'example-key');
  await offer.click();
  await expectStore(yap, 'config.json', (c) => c.asSaidOffer === false);
  await closeSettings(main);
  expect(setAside(yap)).toEqual(copies);
  expect(fs.readFileSync(path.join(yap.dataDir, copies[0]), 'utf8')).toBe(BROKEN);
  expect(log(yap).match(TOLD)).toHaveLength(1);
});
