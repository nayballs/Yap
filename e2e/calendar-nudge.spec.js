// Before any calendar is connected (src-tauri/src/calendar.rs): the
// Meetings view's one "No meetings found" card and the "Connect your
// calendar" dialog it opens, a build without a Google client explaining
// itself (there and in Settings → Connectors), the one-time nudge after a
// meeting ends (where "Not now" means never), the Meetings view's past
// meeting notes by day (search, Generate summary) and Ask bar, whose
// question opens Chat grounded in the meetings (the AI is a local fake,
// support/fake-llm.js), and its ◉ Take notes, the meeting shortcut as a
// button.
import {
  test as base,
  expect,
  openView,
  openSettings,
  closeSettings,
  closeToasts,
  expectStore,
  sidebar,
  connectDialog,
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
const toast = (main, text) => main.getByRole('status').filter({ hasText: text });

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

const NOT_IN_THIS_BUILD = 'Google sign-in is in the installed Yap. In this build, use a private iCal link below';

test('no calendar yet: one "No meetings found" card opens the dialog, and a build without Google says so', async ({ main, shot }) => {
  await openView(main, 'Meetings');
  const today = main.getByRole('region', { name: 'Today' });
  await expect(today.getByText('No meetings found')).toBeVisible();
  // One way in (no banner as well), and the header's Take notes.
  await expect(main.getByRole('button', { name: 'Connect calendar' })).toHaveCount(1);
  await expect(main.getByRole('button', { name: 'Take notes' })).toBeVisible();
  await expect(main.getByRole('button', { name: 'Meeting settings' })).toBeVisible();
  await shot(main, '01-meetings-no-calendar');

  const connect = today.getByRole('button', { name: 'Connect calendar' });
  await connect.click();
  const dialog = connectDialog(main);
  await expect(dialog.getByRole('heading', { name: 'Connect your calendar' })).toBeVisible();
  // This build has no Google OAuth client: that way in is off, and says why.
  await expect(dialog.getByRole('button', { name: 'Continue with Google' })).toBeDisabled();
  await expect(dialog).toContainText(`${NOT_IN_THIS_BUILD}.`);
  await expect(dialog.getByRole('button', { name: 'Continue with Outlook' })).toBeEnabled();
  await expect(dialog.getByRole('button', { name: 'iCloud or another calendar' })).toBeEnabled();
  // The focus starts on the first way in that works.
  await expect(dialog.getByRole('button', { name: 'Continue with Outlook' })).toBeFocused();
  await shot(main, '02-connect-dialog-without-google');
  await main.keyboard.press('Escape');
  await expect(dialog).toBeHidden();
  await expect(connect).toBeFocused();

  // Settings → Connectors says it the same way.
  const settings = await openSettings(main, 'Connectors');
  const google = settings.getByRole('group', { name: 'Google Calendar' });
  await google.getByRole('button', { name: 'Connect' }).click();
  await expect(google).toContainText(NOT_IN_THIS_BUILD);
  await shot(main, '03-google-not-in-this-build');
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
  await shot(main, '04-nudge-after-a-meeting');
  await nudge.getByRole('button', { name: 'Not now' }).click();
  await expectStore(yap, 'calendar.json', (c) => c.nudge?.dismissed === true);
  expect((await yap.invoke('calendar_status')).nudge.afterMeeting).toBe(false);

  // Not after the next meeting either; the Meetings view still has its
  // empty state's Connect calendar (that's not a nudge).
  await openView(main, 'Meetings');
  await expect(main.getByRole('region', { name: 'Today' }).getByRole('button', { name: 'Connect calendar' })).toBeVisible();
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
  const hiring = await recordedMeeting(yap, 'Hiring sync', [['them', 'Two candidates are in the final round for the design role.']]);
  if (await visible(yap, 'notepad')) await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();

  await openView(main, 'Meetings');
  const past = main.getByRole('region', { name: 'Past meeting notes' });
  const item = (title) => past.getByRole('listitem').filter({ hasText: title });
  await expect(item('Launch review')).toBeVisible();
  await expect(item('Hiring sync')).toBeVisible();
  // By day, as Wispr's, each with its time and, without a summary yet,
  // "Generate summary".
  const todayLabel = new Date().toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
  await expect(past.getByText(`Today, ${todayLabel}`)).toBeVisible();
  await expect(item('Launch review').getByRole('button', { name: 'Generate summary' })).toBeVisible();
  await expect(item('Launch review')).toContainText(/\d{2}:\d{2}/);
  // Search looks through transcripts too.
  const search = past.getByRole('searchbox', { name: 'Search meetings' });
  await search.fill('pricing page');
  await expect(past.getByRole('listitem')).toHaveCount(1);
  await expect(past.getByRole('listitem')).toContainText('Launch review');
  await shot(main, '05-past-notes-search');
  await search.fill('nothing like this');
  await expect(past.getByText('No meeting notes match that.')).toBeVisible();
  await search.fill('');

  // Generate summary writes the action plan, in the note.
  await item('Hiring sync').getByRole('button', { name: 'Generate summary' }).click();
  await expect(sidebar(main).getByRole('button', { name: 'Notes', exact: true })).toHaveAttribute('aria-current', 'page');
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Hiring sync');
  await expect.poll(() => yap.invoke('meeting_summary_status', { noteId: hiring.id })).not.toBeNull();
  await closeToasts(main);

  // The Ask bar: example questions in the placeholder; a question opens Chat.
  await openView(main, 'Meetings');
  const ask = main.getByRole('textbox', { name: 'Ask about your meetings' });
  await expect(ask).toHaveAttribute('placeholder', /\?$/);
  await expect(main.getByRole('button', { name: /Past chats/ })).toBeVisible();
  await shot(main, '06-ask-bar');
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
  await shot(main, '07-chat-from-the-ask-bar');

  // Past chats opens Chat too.
  await openView(main, 'Meetings');
  await main.getByRole('button', { name: /Past chats/ }).click();
  await expect(sidebar(main).getByRole('button', { name: 'Chat', exact: true })).toHaveAttribute('aria-current', 'page');
});

test('◉ Take notes in the Meetings header starts meeting notes, as the meeting shortcut does, and Stop and summarise ends them', async ({
  yap,
  main,
  shot,
}) => {
  await openView(main, 'Meetings');
  const take = main.getByRole('button', { name: 'Take notes' });
  await expect(take).toHaveAttribute('title', /Win \+ Alt \+ M/);
  await take.click();
  // No call on: a meeting note of its own, open in Notes, recording.
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue(/^Meeting · /, { timeout: 15_000 });
  await expect.poll(() => recording(yap)).toBe(true);

  await openView(main, 'Meetings');
  const stop = main.getByRole('button', { name: 'Stop and summarise' });
  await expect(stop).toBeVisible();
  await expect(main.getByRole('button', { name: 'Take notes' })).toHaveCount(0);
  await shot(main, '08-meetings-taking-notes');
  // The shortcut takes one press a second.
  await main.waitForTimeout(1_100);
  await stop.click();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expect(main.getByRole('button', { name: 'Take notes' })).toBeVisible();
  // A silent test recording: "Started by mistake?" (Discard tidies it away).
  const ended = toast(main, /Nothing to summarise yet|Started by mistake\?/);
  await expect(ended).toBeVisible({ timeout: 20_000 });
  const discard = ended.getByRole('button', { name: 'Discard' });
  if (await discard.count()) await discard.click();
  await closeToasts(main);
  if (await visible(yap, 'notepad')) await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
});
