// The meeting notepad (src-tauri/src/notepad.rs, src/lib/Notepad.svelte): a
// window docked to the right edge of the screen that opens when a meeting
// starts recording — My thoughts (synced with the Notes view), the live
// You/Them transcript, the summary Rust writes when the meeting ends
// (meeting_end.rs, with steps and a Retry; also after a stop Yap makes
// itself, meeting_guard.rs), "What did I miss?" and the AI meeting title
// (meeting_assist.rs), "Started by mistake?", hiding from screen capture
// (capture.rs), and the dictation key and meeting shortcut caught in-page.
//
// No audio device is opened (test mode records silence); transcript lines
// are handed to the recorder with `e2e_meeting_feed`. The AI is a local fake
// (support/fake-llm.js) that records every request. Split screen moves
// another app's window, which a test run never does: its window picking is
// unit-tested in notepad.rs.
import {
  test as base,
  expect,
  openView,
  openSettings,
  closeSettings,
  closeToasts,
  expectStore,
  settingsDialog,
  pressHotkey,
} from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { newMeetingNote, meetingNote } from './support/meetings.js';

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
        name: 'notepad',
        config: {
          llmScopes: {
            noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
/** The notepad's display affinity (debug-only `capture_affinity`). */
const affinity = (yap) => yap.invoke('capture_affinity').then((a) => a.windows.notepad);
/** WDA_EXCLUDEFROMCAPTURE: on screen, left out of every capture. */
const EXCLUDED_FROM_CAPTURE = 17;
const toast = (page, title) => page.getByRole('status').filter({ hasText: title });
const tab = (yap, name) => yap.notepad.getByRole('tab', { name });

/** A new meeting note (typed title unless `title` is null), recording. */
async function startMeeting(yap, title) {
  const note = await yap.invoke('note_create', { title: title ?? '', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  return note;
}

let clockTs = Math.floor(Date.now() / 1000);
/** Hand the recorder transcript lines (`[source, text, echo?]`), as if just transcribed. */
async function say(yap, ...lines) {
  const segments = lines.map(([source, text, echo]) => ({
    source,
    text,
    ts: (clockTs += 6),
    ...(echo ? { echo: true } : {}),
  }));
  expect(await yap.invoke('e2e_meeting_feed', { segments })).toBe(segments.length);
}

/** Close the notepad (as its ✕ does: hidden, the recording untouched). */
async function closeNotepad(yap) {
  if (await visible(yap, 'notepad')) {
    await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
    await expect.poll(() => visible(yap, 'notepad')).toBe(false);
  }
}

/** Pause whatever records (no summary) and wait for the last chunk. */
async function pauseAll(yap) {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
}

const BUDGET_TALK = [
  ['you', 'Thanks for joining, Alice. Today I want to go through the Q3 budget line by line, starting with travel and the offsite, because those two moved the most since last quarter and finance wants an answer.'],
  ['them', 'Sure. I have the numbers ready. Travel is up twelve percent, mostly flights for the offsite, and the venue deposit landed in this quarter instead of the next one, which makes the budget look worse than it is.'],
  ['you', 'Okay, so the budget is fine overall, it is the timing. Can you send the revised budget to finance by Friday so they see the deposit moved?'],
  ['them', 'Yes, I will send the revised budget by Friday and copy you on it. I will also flag the deposit so nobody panics when they read the totals.'],
  ['you', 'Perfect. Then the only open item is catering for the offsite, which nobody owns yet, so let us pick that up next week once the venue confirms the final headcount for both days.'],
]; // ~160 words: a little over a minute of talk, enough for the AI title

test.afterEach(async ({ yap }) => {
  await pauseAll(yap);
  await closeToasts(yap.notepad);
});

test('a meeting starting opens the notepad docked to the right, without taking the focus', async ({
  yap,
  main,
  shot,
}) => {
  await closeNotepad(yap);
  const note = await startMeeting(yap, 'Design review');
  const pad = yap.notepad;

  // Docked: the right edge of the work area, full height, about a third of
  // the screen. (Measured on what shows: the window rect reaches a few px
  // past it, the invisible resize borders of an undecorated window.)
  const geo = await pad.evaluate(async () => {
    const i = window.__TAURI_INTERNALS__;
    const [pos, size, mon] = await Promise.all([
      i.invoke('plugin:window|inner_position', { label: 'notepad' }),
      i.invoke('plugin:window|inner_size', { label: 'notepad' }),
      i.invoke('plugin:window|current_monitor'),
    ]);
    return { pos, size, work: mon.workArea };
  });
  const { pos, size, work } = geo;
  expect(Math.abs(pos.x + size.width - (work.position.x + work.size.width))).toBeLessThanOrEqual(2);
  expect(Math.abs(pos.y - work.position.y)).toBeLessThanOrEqual(2);
  expect(Math.abs(size.height - work.size.height)).toBeLessThanOrEqual(2);
  expect(size.width).toBeGreaterThanOrEqual(Math.min(400, work.size.width / 2) - 2);
  expect(size.width).toBeLessThanOrEqual(work.size.width / 2 + 2);
  // It never took the focus (the call stays in front)…
  expect(await yap.invoke('plugin:window|is_focused', { label: 'notepad' })).toBe(false);
  // …and stays out of screen shares and screenshots while the meeting
  // records (capture.rs: WDA_EXCLUDEFROMCAPTURE).
  await expect.poll(() => affinity(yap)).toBe(EXCLUDED_FROM_CAPTURE);

  // Notes first: the title (serif), the date, "My thoughts", and the footer.
  await expect(pad.getByRole('textbox', { name: 'Meeting title' })).toHaveValue('Design review');
  await expect(tab(yap, 'My thoughts')).toHaveAttribute('aria-selected', 'true');
  await expect(pad.getByText('Always get consent when transcribing others.')).toBeVisible();
  await expect(pad.getByRole('button', { name: 'Stop', exact: true })).toBeVisible();
  await expect(pad.getByRole('button', { name: 'What did I miss?' })).toBeVisible();
  await expect(pad.locator('.meta .live')).toContainText(/Recording \d+:\d\d/);
  await shot(pad, '01-opens-on-record');

  // Transcript: the elapsed time, the tip, and a friendly empty state.
  await tab(yap, 'Transcript').click();
  await expect(pad.locator('.trow .elapsed')).toContainText(/\d+:\d\d/);
  await expect(pad.getByText('Yap is listening')).toBeVisible();
  const hint = pad.locator('.hint');
  await expect(hint).toContainText('tidies the transcript into one paragraph per speaker');
  await shot(pad, '02-transcript-listening');

  // Live lines, with coloured speaker labels, following the newest one.
  await say(yap, ['you', 'Morning! Shall we start with the onboarding flow?'], ['them', 'Yes, I have the new mocks open.']);
  const log = pad.getByRole('log', { name: 'Transcript' });
  await expect(log.locator('.turn.you .who')).toHaveText('You');
  await expect(log.locator('.turn.them .who')).toHaveText('Them');
  await expect(log).toContainText('I have the new mocks open.');
  const [you, them] = await Promise.all([
    log.locator('.turn.you .who').evaluate((el) => getComputedStyle(el).color),
    log.locator('.turn.them .who').evaluate((el) => getComputedStyle(el).color),
  ]);
  expect(you).not.toBe(them);
  // Echo (the call through the speakers) stays hidden, as in the Notes view.
  await say(
    yap,
    ['them', 'The first screen asks for a workspace name.'],
    ['you', 'ECHO the first screen asks for a workspace name', true]
  );
  const echo = pad.getByRole('button', { name: /Show 1 line your mic picked up from the speakers/ });
  await expect(echo).toBeVisible();
  await expect(log).not.toContainText('ECHO');
  await shot(pad, '03-transcript-live-lines');
  await echo.click();
  await expect(log.locator('.turn.echo')).toContainText('from the speakers');
  await pad.getByRole('button', { name: /Hide 1 line/ }).click();
  expect((await yap.invoke('note_get', { id: note.id })).transcript.filter((s) => s.echo)).toHaveLength(1);

  // The tip goes for good.
  await hint.getByRole('button', { name: 'Dismiss the tip' }).click();
  await expect(hint).toHaveCount(0);

  // Closing it never stops the recording; the Notes view brings it back.
  await closeNotepad(yap);
  expect(await recording(yap)).toBe(true);
  await openView(main, 'Notes');
  await main.locator('.folder').filter({ hasText: 'Meetings' }).click();
  await main.locator('.item').filter({ hasText: 'Design review' }).click();
  await main.getByRole('button', { name: 'Notepad' }).click();
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  await expect(pad.getByRole('textbox', { name: 'Meeting title' })).toHaveValue('Design review');
  // Shareable again once the meeting stops.
  await pauseAll(yap);
  await expect.poll(() => affinity(yap)).toBe(0);
});

test('My thoughts sync with the Notes view both ways', async ({ yap, main, shot }) => {
  await newMeetingNote(main, 'Sync check', ['Priya']);
  await main.getByRole('button', { name: 'Record' }).click();
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  await expect(pad.getByRole('textbox', { name: 'Meeting title' })).toHaveValue('Sync check');

  // Typed in the notepad → the Notes view.
  const thoughts = pad.getByRole('textbox', { name: 'My thoughts' });
  await thoughts.fill('Ask Priya about the launch date');
  const raw = main.locator('textarea.raw');
  await expect(raw).toHaveValue('Ask Priya about the launch date');
  await shot(pad, '04-thoughts-typed-in-notepad');

  // Typed in the Notes view → the notepad.
  await raw.fill('Ask Priya about the launch date\nPress release needs legal review');
  await expect(thoughts).toHaveValue('Ask Priya about the launch date\nPress release needs legal review');
  await expectStore(yap, 'notes.json', (s) =>
    meetingNote(s, 'Sync check')?.content?.includes('legal review')
  );
  await shot(main, '05-thoughts-synced-in-notes');

  // The title too, from the notepad.
  await pad.getByRole('textbox', { name: 'Meeting title' }).fill('Launch sync with Priya');
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Launch sync with Priya');

  // Pause (Notes view): no summary, Resume in both.
  await say(yap, ['them', 'Legal wants a week with it.']);
  await main.getByRole('button', { name: 'Pause recording' }).click();
  await expect(pad.getByRole('button', { name: 'Resume' })).toBeVisible({ timeout: 20_000 });
  await expect(main.getByRole('button', { name: 'Resume' })).toBeVisible();
  await shot(pad, '06-paused-resume');
});

test('"What did I miss?": nothing new, then what was said since you looked', async ({
  yap,
  fakeLlm,
  shot,
}) => {
  await startMeeting(yap, 'Weekly sync');
  const pad = yap.notepad;
  await tab(yap, 'Transcript').click();
  await say(yap, ['you', 'Let us start with the roadmap.'], ['them', 'Sure, the roadmap is on track.'], ['you', 'Great, next item.']);
  await expect(pad.getByRole('log', { name: 'Transcript' })).toContainText('next item');

  // Everything was on screen: nothing new, and no AI call for it.
  const before = fakeLlm.requests.length;
  await pad.getByRole('button', { name: 'What did I miss?' }).click();
  const chat = pad.getByRole('region', { name: 'What did I miss?' });
  await expect(chat).toContainText('Nothing new since you last looked.');
  expect(fakeLlm.requests.length).toBe(before);
  await shot(pad, '07-what-did-i-miss-nothing-new');

  // Looking away (My thoughts), the meeting goes on.
  await tab(yap, 'My thoughts').click();
  await say(
    yap,
    ['them', 'MISSED-1 Can you send the slides by Friday? The board meets Monday.'],
    ['them', 'MISSED-2 And we moved the demo to Thursday afternoon.']
  );
  await pad.getByRole('button', { name: 'What did I miss?' }).click();
  await expect(chat).toContainText('2 new lines since you looked, opening with "MISSED-1 Can you send the"');
  await expect(chat).toContainText('Them asked You to send the slides by Friday.');
  const [catchUp] = fakeLlm.requests.filter((r) => r.kind === 'catchUp');
  const since = catchUp.user.split('Since they last looked')[1];
  expect(since).toContain('MISSED-1');
  expect(since).toContain('MISSED-2');
  expect(since).not.toContain('roadmap'); // seen already: context only
  await shot(pad, '08-what-did-i-miss-answer');

  // The answer counts as looking.
  await pad.getByRole('button', { name: 'What did I miss?' }).click();
  await expect(chat.locator('.ma').last()).toContainText('Nothing new since you last looked.');

  // A follow-up question, answered from the meeting.
  await chat.getByRole('textbox', { name: 'Ask about this meeting' }).fill('Who owns the budget?');
  await chat.getByRole('button', { name: 'Ask' }).click();
  await expect(chat).toContainText('Alice owns the budget');
  expect(fakeLlm.requests.filter((r) => r.kind === 'meetingAsk')).toHaveLength(1);
  await chat.getByRole('button', { name: 'Close catch-up' }).click();
  await expect(chat).toHaveCount(0);
});

test('the AI names a meeting with a made-up title, never one the person typed', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  // Call detection's "Record notes": the note is "Zoom call · 5 Oct, 14:30".
  await yap.invoke('meeting_detect_simulate', { appId: 'zoom', active: true });
  await toast(main, 'Zoom call detected').getByRole('button', { name: 'Record notes' }).click();
  await expect.poll(() => visible(yap, 'notepad'), { timeout: 15_000 }).toBe(true);
  const pad = yap.notepad;
  const title = pad.getByRole('textbox', { name: 'Meeting title' });
  await expect(title).toHaveValue(/^Zoom call · \d{1,2} [A-Z][a-z]{2}, \d{2}:\d{2}$/);
  await expect(title).toHaveClass(/auto/); // muted until named
  await shot(pad, '09-made-up-title');

  // About a minute of talk: the meeting gets a real title, everywhere.
  const asked = fakeLlm.requests.filter((r) => r.kind === 'title').length;
  await say(yap, ...BUDGET_TALK);
  await expect(title).toHaveValue('Q3 Budget Review with Alice', { timeout: 15_000 });
  await expect(title).not.toHaveClass(/auto/);
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Q3 Budget Review with Alice');
  const titleCalls = fakeLlm.requests.filter((r) => r.kind === 'title');
  expect(titleCalls).toHaveLength(asked + 1);
  expect(titleCalls.at(-1).user).toContain('Transcript (start):');
  await expectStore(yap, 'notes.json', (s) => !!meetingNote(s, 'Q3 Budget Review with Alice'));
  await shot(pad, '10-ai-title');
  await shot(main, '11-ai-title-in-notes');
  await pauseAll(yap);
  await yap.invoke('meeting_detect_simulate', { appId: 'zoom', active: false });

  // A typed title stays, however much is said.
  await startMeeting(yap, 'Board prep');
  await say(yap, ...BUDGET_TALK);
  await pad.waitForTimeout(1_500);
  expect(fakeLlm.requests.filter((r) => r.kind === 'title')).toHaveLength(asked + 1);
  await expect(title).toHaveValue('Board prep');
});

test('"Started by mistake?": Keep keeps the meeting, Discard deletes it', async ({ yap, shot }) => {
  const pad = yap.notepad;
  const kept = await startMeeting(yap, 'Kept by mistake');
  await say(yap, ['you', 'Hi, Yap.']);
  await pad.getByRole('button', { name: 'Stop', exact: true }).click();
  const ask = toast(pad, 'Started by mistake?');
  await expect(ask).toBeVisible({ timeout: 20_000 });
  await expect(ask).toContainText('Only a few words were captured. Keep this meeting or discard it.');
  await expect(ask.getByRole('button', { name: 'Discard' })).toBeVisible();
  await shot(pad, '12-started-by-mistake');

  // Keep: nothing written, nothing deleted; Resume and Generate summary.
  await ask.getByRole('button', { name: 'Keep' }).click();
  await expect(ask).toHaveCount(0);
  await expect(pad.getByRole('button', { name: 'Resume' })).toBeVisible();
  await expect(pad.getByRole('button', { name: 'Generate summary' })).toBeVisible();
  expect(meetingNote(yap.readJson('notes.json'), 'Kept by mistake')?.id).toBe(kept.id);
  expect((await yap.invoke('note_get', { id: kept.id })).enhancedContent ?? '').toBe('');
  await shot(pad, '13-kept-resume-generate');

  // Generate summary, when wanted after all.
  await pad.getByRole('button', { name: 'Generate summary' }).click();
  await expect(pad.getByRole('region', { name: 'Summary' }).locator('.rendered')).toBeVisible({ timeout: 15_000 });
  await expect(pad.getByRole('button', { name: 'Generate summary' })).toHaveCount(0);

  // Discard: the note goes, and the notepad with it.
  const gone = await startMeeting(yap, 'Discarded by mistake');
  await say(yap, ['you', 'Oops.']);
  await pad.getByRole('button', { name: 'Stop', exact: true }).click();
  await toast(pad, 'Started by mistake?').getByRole('button', { name: 'Discard' }).click();
  await expectStore(yap, 'notes.json', (s) => !s.notes.some((n) => n.id === gone.id));
  await expect.poll(() => visible(yap, 'notepad')).toBe(false);
});

test('Stop writes the summary in steps; a failure says so and Retry works', async ({
  yap,
  fakeLlm,
  shot,
}) => {
  const pad = yap.notepad;
  await startMeeting(yap, 'Budget sync');
  await say(yap, ...BUDGET_TALK.slice(0, 2));
  await tab(yap, 'Summary').click();
  await expect(pad.locator('.sumempty')).toHaveText('Your summary is written when you stop');

  // The model answers slowly, then fails.
  fakeLlm.delay('actionPlan', 2_500);
  fakeLlm.failNext('actionPlan', 1);
  await pad.getByRole('button', { name: 'Stop', exact: true }).click();
  const summary = pad.getByRole('region', { name: 'Summary' });
  await expect(summary.locator('.steps')).toContainText('Step 2 of 3', { timeout: 20_000 });
  await expect(summary.locator('.stepline')).toContainText('into an action plan');
  await shot(pad, '14-summary-step-2-of-3');

  const failed = summary.getByRole('alert');
  await expect(failed).toContainText("The summary didn't come through", { timeout: 15_000 });
  await expect(failed).toContainText('HTTP 500');
  await expect(pad.getByRole('button', { name: 'Generate summary' })).toBeVisible();
  await shot(pad, '15-summary-error-retry');

  // Retry: the action plan.
  fakeLlm.delay('actionPlan', 0);
  await failed.getByRole('button', { name: 'Retry' }).click();
  await expect(summary.locator('.rendered').getByRole('heading', { name: 'Action plan' })).toBeVisible({
    timeout: 15_000,
  });
  await expect(failed).toHaveCount(0);
  await expect(pad.getByRole('button', { name: 'Generate summary' })).toHaveCount(0);
  await expect(pad.getByRole('button', { name: 'Resume' })).toBeVisible();
  await shot(pad, '16-summary-done');
});

test('a stop Yap makes itself writes the plan in the notepad, without bringing up Notes', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  // The length limit in seconds instead of hours (debug-only): stop 4 s in.
  await openView(main, 'Home');
  await yap.invoke('e2e_meeting_limit', { limitMs: 4_000, warnMs: 2_000 });
  try {
    const plans = fakeLlm.requests.filter((r) => r.kind === 'actionPlan').length;
    const note = await startMeeting(yap, 'Ops review');
    await say(yap, ...BUDGET_TALK.slice(0, 2));
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);

    // The plan, written by Rust, shows in the notepad…
    const pad = yap.notepad;
    await expect(tab(yap, 'Summary')).toHaveAttribute('aria-selected', 'true');
    await expect(pad.getByRole('region', { name: 'Summary' }).locator('.rendered')).toBeVisible({ timeout: 15_000 });
    expect(fakeLlm.requests.filter((r) => r.kind === 'actionPlan')).toHaveLength(plans + 1);
    await expectStore(yap, 'notes.json', (s) => !!s.notes.find((n) => n.id === note.id)?.enhancedContent);
    await shot(pad, '18-auto-stop-plan-in-notepad');
    // …and the main window stayed where it was (no jump to Notes).
    await expect(
      main.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Home', exact: true })
    ).toHaveAttribute('aria-current', 'page');
  } finally {
    await yap.invoke('e2e_meeting_limit', {});
  }
});

test('the meeting shortcut works with the notepad focused (in-page fallback)', async ({ yap, shot }) => {
  // Win+Alt+M in the notepad: notes in a new meeting note…
  await closeNotepad(yap);
  await startMeeting(yap, 'Shortcut warm-up');
  await pauseAll(yap);
  const pad = yap.notepad;
  await pad.keyboard.press('Meta+Alt+KeyM');
  await expect.poll(() => recording(yap), { timeout: 15_000 }).toBe(true);
  await expect(pad.getByRole('textbox', { name: 'Meeting title' })).toHaveValue(
    /^Meeting · \d{1,2} [A-Z][a-z]{2}, \d{2}:\d{2}$/
  );
  // …and again (a second later): stopped, asked about right here.
  await pad.waitForTimeout(1_100);
  await pad.keyboard.press('Meta+Alt+KeyM');
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expect(toast(pad, 'Started by mistake?')).toBeVisible({ timeout: 20_000 });
  await shot(pad, '19-shortcut-stopped-in-notepad');
});

test('Settings → General → Meetings: open the notepad, split the screen', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'General');
  const meetings = dialog.getByRole('group', { name: 'Meetings' });
  const open = meetings.getByRole('button', { name: 'Open the notepad when a meeting starts' });
  const split = meetings.getByRole('button', { name: 'Split the screen when joining' });
  await expect(open).toHaveAttribute('aria-pressed', 'true');
  await expect(split).toHaveAttribute('aria-pressed', 'false');
  await split.scrollIntoViewIfNeeded();
  await shot(main, '17-settings-notepad-rows');

  await split.click();
  await expectStore(yap, 'config.json', (c) => c.meetingSplitScreen === true);
  await split.click();
  await expectStore(yap, 'config.json', (c) => c.meetingSplitScreen === false);

  // Off: a meeting starts without the notepad (and split waits for it).
  await open.click();
  await expectStore(yap, 'config.json', (c) => c.meetingOpenNotepad === false);
  await expect(split).toBeDisabled();
  await closeSettings(main);
  await closeNotepad(yap);
  const note = await yap.invoke('note_create', { title: 'No notepad', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await main.waitForTimeout(1_000);
  expect(await visible(yap, 'notepad')).toBe(false);
  await pauseAll(yap);

  await openSettings(main, 'General');
  await open.click();
  await expectStore(yap, 'config.json', (c) => c.meetingOpenNotepad === true);

  // "Show live transcript" off: the Transcript tab stays quiet until the
  // meeting stops (it's still transcribed).
  const live = settingsDialog(main)
    .getByRole('group', { name: 'Meetings' })
    .getByRole('button', { name: 'Show live transcript' });
  await expect(live).toHaveAttribute('aria-pressed', 'true');
  await live.click();
  await expectStore(yap, 'config.json', (c) => c.meetingLiveTranscript === false);
  await closeSettings(main);
  await startMeeting(yap, 'Quiet transcript');
  await tab(yap, 'Transcript').click();
  await say(yap, ['them', 'QUIET-LINE nobody sees this until the end.']);
  const pad = yap.notepad;
  await expect(pad.getByText('Live transcript is off')).toBeVisible();
  await expect(pad.getByRole('log', { name: 'Transcript' })).toHaveCount(0);
  await shot(pad, '20-live-transcript-off');
  await pauseAll(yap);
  await expect(pad.getByRole('log', { name: 'Transcript' })).toContainText('QUIET-LINE');

  await openSettings(main, 'General');
  await live.click();
  await expectStore(yap, 'config.json', (c) => c.meetingLiveTranscript === true);
  await expect(settingsDialog(main)).toBeVisible();
});

test('the dictation hotkey works with the notepad focused (in-page fallback)', async ({ yap, main }) => {
  const mics = await yap.invoke('list_audio_devices');
  test.skip(mics.length === 0, 'no microphone here; no-mic.spec.js covers that path');
  await openView(main, 'Home');
  await pressHotkey(yap.notepad);
  await expect(yap.overlay.locator('.capsule')).toBeVisible();
  await main.waitForTimeout(1_200);
  await pressHotkey(yap.notepad);
  await expectStore(yap, 'history.json', (h) => JSON.stringify(h).includes('STT stub'));
});
