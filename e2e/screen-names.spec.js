// "Spell names the way they're written on screen" (dictionary/screen_terms.rs): the
// setting in Settings → Dictation → Typing into apps, and what it says about
// what leaves the PC there and in Privacy & data's table. A test run never
// reads another app's window (shell/e2e.rs), so the read, the terms and their use
// are unit-tested in Rust and robot-tested for real (e2e-real/
// screen-names.real.js).
import { test, expect, openSettings, expectStore } from './support/fixtures.js';

// AI cleanup on a cloud provider: the names a dictation seems to contain go
// with it, and the settings say so.
test.use({ yapOptions: { name: 'screen-names', config: { postProcessEnabled: true, ppProvider: 'groq', ppApiKey: '' } } });

test('on by default; says what goes to the cleanup provider; off is saved and shown in Privacy & data', async ({ yap, main, shot }) => {
  let dialog = await openSettings(main, 'Dictation');
  const typing = dialog.getByRole('group', { name: 'Typing into apps' });
  const names = typing.getByRole('button', { name: "Spell names the way they're written on screen" });
  await expect(names).toHaveAttribute('aria-pressed', 'true');
  await expect(
    typing.getByText('Names in the window come out spelled as shown. The ones a dictation seems to contain go to Groq with it, for cleanup'),
  ).toBeVisible();
  await names.scrollIntoViewIfNeeded();
  await shot(main, '01-dictation-screen-names');

  // Privacy & data: its row in "What leaves this PC".
  dialog = await openSettings(main, 'Privacy & data');
  const table = dialog.getByRole('table', { name: 'What leaves this PC' });
  const row = table.getByRole('row').filter({ has: main.getByRole('rowheader', { name: 'Names on screen' }) });
  await expect(row).toContainText('Read on this PC for each dictation. The names a dictation seems to contain go to Groq with it, for cleanup');
  await shot(main, '02-privacy-screen-names');

  // Off: saved, and the table says so.
  dialog = await openSettings(main, 'Dictation');
  await dialog.getByRole('button', { name: "Spell names the way they're written on screen" }).click();
  await expectStore(yap, 'config.json', (c) => c.screenNames === false);
  dialog = await openSettings(main, 'Privacy & data');
  await expect(
    dialog.getByRole('table', { name: 'What leaves this PC' }).getByRole('row').filter({ has: main.getByRole('rowheader', { name: 'Names on screen' }) }),
  ).toContainText('Off');

  // Back on.
  dialog = await openSettings(main, 'Dictation');
  await dialog.getByRole('button', { name: "Spell names the way they're written on screen" }).click();
  await expectStore(yap, 'config.json', (c) => c.screenNames === true);
});
