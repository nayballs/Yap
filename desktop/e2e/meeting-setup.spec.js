// "Set up meeting notes" in the main window's sidebar (2026-10-29;
// views/MeetingSetup.svelte, shell/guide.rs): connect a calendar, record a
// first meeting (or try one), add Yap to an AI app (MCP) — each ticked by
// itself as it's done; ✕ dismisses it for good; all done, it goes. A local
// iCal feed stands in for a calendar, a made-up Cursor install for the AI
// app (YAP_MCP_CLIENT_ROOT), and the meeting is fed as a test run feeds one.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, expectStore, openView, closeSettings, settingsDialog, connectDialog } from './support/fixtures.js';
import { startCalendarFeed, calendarFile, vevent } from './support/calendar-feed.js';
import { RUNS } from './support/yap.js';

const test = base.extend({
  profile: [
    async ({}, use) => {
      const root = path.join(RUNS, 'meeting-setup-profile');
      fs.rmSync(root, { recursive: true, force: true });
      // Cursor installed, with no config yet.
      fs.mkdirSync(path.join(root, '.cursor'), { recursive: true });
      await use(root);
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ profile }, use) => {
      await use({ name: 'meeting-setup', env: { YAP_MCP_CLIENT_ROOT: profile } });
    },
    { scope: 'worker' },
  ],
});

const card = (main) => main.getByTestId('meeting-setup');
const item = (main, id) => card(main).locator(`[data-item="${id}"]`);
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);

test('the checklist ticks itself as each is done, and goes once all are', async ({ yap, main, shot }) => {
  await yap.invoke('e2e_guide_reset');
  await main.setViewportSize({ width: 1200, height: 800 });
  await openView(main, 'Insights');
  await openView(main, 'Home');
  await expect(card(main)).toBeVisible();
  await expect(card(main).getByRole('heading', { name: 'Set up meeting notes' })).toBeVisible();
  for (const id of ['calendar', 'meeting', 'mcp']) await expect(item(main, id)).toHaveAttribute('aria-pressed', 'false');
  await expect(card(main).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '0');
  await shot(main, '01-checklist');

  // Connect a calendar: the dialog, over Meetings.
  await item(main, 'calendar').click();
  await expect(main.getByRole('heading', { level: 1, name: 'Meetings' })).toBeVisible();
  const dialog = connectDialog(main);
  await expect(dialog).toBeVisible();
  const feed = await startCalendarFeed();
  const soon = new Date(Date.now() + 3 * 3600_000);
  feed.set(calendarFile('Work', [vevent({ uid: 'review', summary: 'Design review', start: soon, end: new Date(soon.getTime() + 1800_000) })]));
  try {
    await dialog.getByRole('button', { name: 'iCloud or another calendar' }).click();
    await dialog.locator('input').fill(feed.url);
    await dialog.locator('input').press('Enter');
    await expect(dialog).toBeHidden({ timeout: 10_000 });
    await expect(item(main, 'calendar')).toHaveAttribute('aria-pressed', 'true');
    await expect(card(main).getByRole('progressbar')).toHaveAttribute('aria-valuenow', '1');

    // Record your first meeting: Meetings; a meeting recorded ticks it.
    await openView(main, 'Home');
    await item(main, 'meeting').click();
    await expect(main.getByRole('heading', { level: 1, name: 'Meetings' })).toBeVisible();
    const note = await yap.invoke('note_create', { title: 'Weekly sync', folder: 'Meetings' });
    await yap.invoke('meeting_start', { noteId: note.id });
    const ts = Math.floor(Date.now() / 1000) - 300;
    await yap.invoke('e2e_meeting_feed', { segments: [{ source: 'you', text: 'Shall we start with the budget?', ts }] });
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
    await openView(main, 'Home');
    await expect(item(main, 'meeting')).toHaveAttribute('aria-pressed', 'true');
    await shot(main, '02-checklist-two-done');

    // Add Yap to an AI app: Settings → AI apps (MCP); Cursor added ticks it,
    // and with all three done the checklist goes.
    await item(main, 'mcp').click();
    await expect(settingsDialog(main).getByRole('heading', { level: 1, name: /AI apps/ })).toBeVisible();
    await yap.invoke('mcp_client_add', { id: 'cursor' });
    await closeSettings(main);
    await openView(main, 'Insights');
    await expect(card(main)).toBeHidden();
    expect(await yap.invoke('meeting_setup_status')).toMatchObject({ calendar: true, meeting: true, mcp: true, shown: false });
  } finally {
    const s = await yap.invoke('calendar_status');
    for (const c of s.connections) await yap.invoke('calendar_disconnect', { id: c.id });
    await yap.invoke('mcp_client_remove', { id: 'cursor' }).catch(() => {});
    await feed.close();
  }
});

test('✕ dismisses the checklist for good', async ({ yap, main }) => {
  await yap.invoke('e2e_guide_reset');
  await main.setViewportSize({ width: 1200, height: 800 });
  await openView(main, 'Insights');
  await expect(card(main)).toBeVisible();
  await card(main).getByRole('button', { name: 'Dismiss meeting notes setup' }).click();
  await expect(card(main)).toBeHidden();
  await expectStore(yap, 'onboarding.json', (g) => g.checklistDismissed === true);
  await openView(main, 'Home');
  await expect(card(main)).toBeHidden();
  expect((await yap.invoke('meeting_setup_status')).shown).toBe(false);
  await yap.invoke('e2e_guide_reset');
});
