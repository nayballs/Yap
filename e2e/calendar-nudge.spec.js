// Before any calendar is connected (src-tauri/src/calendar.rs): the
// Meetings view's empty state and "Connect your calendar" card, a build
// without a Google client explaining itself, the one-time nudge after a
// meeting ends (where "Not now" means never), and the Meetings view's past
// meeting notes, search and Ask bar, whose question opens Chat grounded in
// the meetings (the AI is a local fake, support/fake-llm.js).
import {
  test as base,
  expect,
  openView,
  closeSettings,
  expectStore,
  settingsDialog,
  sidebar,
} from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'calendar-nudge',
        config: {
          llmScopes: {
            chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });

/** A meeting note that recorded `lines` (You/Them), finished without a summary. */
async function recordedMeeting(yap, title, lines) {
  const note = await yap.invoke('note_create', { title, folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  let ts = Math.floor(Date.now() / 1000) - 600;
  const segments = lines.map(([source, text]) => ({ source, text, ts: (ts += 20) }));
  await yap.invoke('e2e_meeting_feed', { segments });
  await yap.invoke('meeting_pause');
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  return note;
}

test('no calendar yet: the Meetings view offers one, and a build without Google says so', async ({ main, shot }) => {
  await openView(main, 'Meetings');
  const nudge = main.getByRole('region', { name: 'Connect your calendar' });
  await expect(nudge).toBeVisible();
  const today = main.getByRole('region', { name: 'Today' });
  await expect(today.getByText('No meetings found')).toBeVisible();
  await shot(main, '01-meetings-no-calendar');

  await today.getByRole('button', { name: 'Connect calendar' }).click();
  const dialog = settingsDialog(main);
  await expect(dialog.getByRole('heading', { level: 1, name: 'Connectors' })).toBeVisible();
  // This build has no Google OAuth client: its button explains instead.
  const google = dialog.getByRole('group', { name: 'Google Calendar' });
  await google.getByRole('button', { name: 'Connect' }).click();
  await expect(google).toContainText("This build of Yap can't connect to Google yet");
  await shot(main, '02-google-not-in-this-build');
  await closeSettings(main);
});

test('after a meeting ends, the nudge comes once, and "Not now" means never', async ({ yap, main, shot }) => {
  const note = await yap.invoke('note_create', { title: 'Quick sync', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(() => recording(yap)).toBe(true);
  // Enough talk that it isn't "Started by mistake?".
  const ts = Math.floor(Date.now() / 1000);
  await yap.invoke('e2e_meeting_feed', {
    segments: [
      { source: 'them', text: 'Thanks everyone, let us go through the roadmap for next quarter and pick the three things we ship first.', ts },
      { source: 'you', text: 'Sounds good, I will start with onboarding and then the billing page.', ts: ts + 8 },
    ],
  });
  await yap.invoke('meeting_end', {});
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);

  const nudge = main.getByRole('status').filter({ hasText: 'Connect your calendar' });
  await expect(nudge).toBeVisible({ timeout: 20_000 });
  await expect(nudge.getByRole('button', { name: 'Connect calendar' })).toBeVisible();
  await shot(main, '03-nudge-after-a-meeting');
  await nudge.getByRole('button', { name: 'Not now' }).click();
  await expectStore(yap, 'calendar.json', (c) => c.nudge?.dismissed === true);

  // Not in the Meetings view either, and not after the next meeting.
  await openView(main, 'Meetings');
  await expect(main.getByRole('region', { name: 'Connect your calendar' })).toHaveCount(0);
  await expect(main.getByRole('region', { name: 'Today' }).getByText('No meetings found')).toBeVisible();
  if (await visible(yap, 'notepad')) await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
});

test('past meeting notes with search, and the Ask bar asks Chat about your meetings', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await recordedMeeting(yap, 'Launch review', [
    ['them', 'We agreed the launch moves to March third so the pricing page can ship with it.'],
    ['you', 'Great. I will update the pricing page copy by Friday.'],
  ]);
  await recordedMeeting(yap, 'Hiring sync', [['them', 'Two candidates are in the final round for the design role.']]);
  if (await visible(yap, 'notepad')) await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();

  await openView(main, 'Meetings');
  const past = main.getByRole('region', { name: 'Past meeting notes' });
  const item = (title) => past.getByRole('listitem').filter({ hasText: title });
  await expect(item('Launch review')).toBeVisible();
  await expect(item('Hiring sync')).toBeVisible();
  // Search looks through transcripts too.
  const search = past.getByRole('searchbox', { name: 'Search meetings' });
  await search.fill('pricing page');
  await expect(past.getByRole('listitem')).toHaveCount(1);
  await expect(past.getByRole('listitem')).toContainText('Launch review');
  await shot(main, '04-past-notes-search');
  await search.fill('nothing like this');
  await expect(past.getByText('No meeting notes match that.')).toBeVisible();
  await search.fill('');

  // The Ask bar: example questions in the placeholder; a question opens Chat.
  const ask = main.getByRole('textbox', { name: 'Ask about your meetings' });
  await expect(ask).toHaveAttribute('placeholder', /\?$/);
  await expect(main.getByRole('button', { name: /Past chats/ })).toBeVisible();
  await shot(main, '05-ask-bar');
  await ask.fill('When does the launch happen?');
  await ask.press('Enter');
  await expect(sidebar(main).getByRole('button', { name: 'Chat', exact: true })).toHaveAttribute('aria-current', 'page');
  await expect(main.getByText('When does the launch happen?').first()).toBeVisible();
  // Grounded in the meetings: the latest ones are in the request.
  await expect
    .poll(() => fakeLlm.requests.filter((r) => r.system.includes('The user is asking about their meetings')).length)
    .toBeGreaterThan(0);
  const request = fakeLlm.requests.find((r) => r.system.includes('The user is asking about their meetings'));
  expect(request.system).toContain('title="Launch review"');
  expect(request.system).toContain('March third');
  await expect(main.locator('.chat')).toContainText('OK');
  await shot(main, '06-chat-from-the-ask-bar');

  // Past chats opens Chat too.
  await openView(main, 'Meetings');
  await main.getByRole('button', { name: /Past chats/ }).click();
  await expect(sidebar(main).getByRole('button', { name: 'Chat', exact: true })).toHaveAttribute('aria-current', 'page');
});
