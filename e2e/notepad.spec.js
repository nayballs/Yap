// The meeting notepad (src-tauri/src/notepad.rs, src/lib/Notepad.svelte), in
// Wispr Flow's notepad layout: a window docked to the right edge of the
// screen that opens when a meeting starts recording — the header (back to
// Notes, the ⋯ menu, Share, split screen, delete), My thoughts (synced with
// the Notes view), the live transcript (grouped bubbles, paused and dictated
// dividers, search, copy), the summary Rust writes when the meeting ends
// (meeting_end.rs, with steps and a Retry; also after a stop Yap makes
// itself, meeting_guard.rs), the Ask sheet with "What did I miss?" and the AI
// meeting title (meeting_assist.rs), "Started by mistake?" (in the notepad,
// or on the Yap bar with no Yap window on screen), the consent message,
// hiding from screen capture (capture.rs), and the dictation key and meeting
// shortcut caught in-page.
//
// No audio device is opened (test mode records silence); transcript lines
// (and "You dictated here" markers) are handed to the recorder with
// `e2e_meeting_feed`. The AI is a local fake (support/fake-llm.js) that
// records every request. The clipboard is stubbed in the page. Split screen
// moves another app's window, which a test run never does (its window
// picking is unit-tested in notepad.rs); Save as .md and Email open a native
// dialog and the mail app, so they're checked through the text they'd use.
//
// The screenshots are named after the states in Wispr's own captures
// (E:\Projects\references\wispr-flow), for a side-by-side look.
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
import {
  noteMarkdown,
  noteText,
  mailtoUrl,
  transcriptItems,
  bubbleShape,
  highlightParts,
  consentMessage,
  consentToSave,
  DEFAULT_CONSENT_MESSAGE,
} from '../src/lib/notepadText.js';

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
const transcript = (pad) => pad.getByRole('log', { name: 'Transcript' });
const askSheet = (pad) => pad.getByRole('dialog', { name: 'Ask about this meeting' });
const askInput = (pad) => pad.getByRole('textbox', { name: 'Ask anything' });
const stopButton = (pad) => pad.getByRole('button', { name: 'Stop', exact: true });

/** A new meeting note (typed title unless `title` is null), recording. */
async function startMeeting(yap, title) {
  const note = await yap.invoke('note_create', { title: title ?? '', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  return note;
}

let clockTs = Math.floor(Date.now() / 1000);
/**
 * Hand the recorder transcript lines (`[source, text, echo?]`), as if just
 * transcribed; `'dictated'` is a "You dictated here" marker (meeting.rs).
 */
async function say(yap, ...lines) {
  const segments = lines.map((line) =>
    line === 'dictated'
      ? { source: 'you', text: '', ts: (clockTs += 6), dictated: true }
      : { source: line[0], text: line[1], ts: (clockTs += 6), ...(line[2] ? { echo: true } : {}) }
  );
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

/** The page's clipboard, stubbed (a test never touches the real one): returns what was copied last. */
async function stubClipboard(page) {
  await page.evaluate(() => {
    window.__copied = null;
    navigator.clipboard.writeText = async (t) => {
      window.__copied = t;
    };
  });
  return () => page.evaluate(() => window.__copied);
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
  // Popovers, the Ask sheet, a dialog, the search: Escape puts each away.
  for (let i = 0; i < 3; i += 1) await yap.notepad.keyboard.press('Escape');
  await closeToasts(yap.notepad);
});

test('the text it copies, saves and emails, and how the transcript is laid out', () => {
  const note = {
    title: 'Launch sync',
    participants: ['Priya'],
    content: 'Ask about legal',
    enhancedContent: '## Action plan\n\n### Priya\n- [ ] Book legal review',
    transcript: [
      { source: 'you', text: 'Morning.', ts: 1 },
      { source: 'them', text: 'Hi there.', ts: 2 },
      { source: 'you', text: 'Hi there.', ts: 3, echo: true },
      { source: 'you', text: '', ts: 4, dictated: true },
      { source: 'them', text: 'Legal needs a week.', ts: 5 },
    ],
  };
  // Markdown, as "Save as .md" writes it: no echo, no markers.
  const md = noteMarkdown(note);
  expect(md).toMatch(/^# Launch sync\n\nAttendees: Priya\n\n## Action plan/);
  expect(md).toContain('---\n\n## Raw notes\n\nAsk about legal');
  expect(md).toContain('**Them:** Legal needs a week.');
  expect(md.match(/\*\*(You|Them):\*\*/g)).toHaveLength(3);
  // Text: headings and emphasis dropped, tasks as ☐.
  const text = noteText(note);
  expect(text).toContain('☐ Book legal review');
  expect(text).not.toMatch(/^#|\*\*/m);
  // Email: the summary as text, in a mailto: link.
  const mail = new URL(mailtoUrl(note));
  expect(mail.protocol).toBe('mailto:');
  expect(mail.searchParams.get('subject')).toBe('Launch sync');
  expect(mail.searchParams.get('body')).toContain('☐ Book legal review');
  // The transcript: groups of one speaker, a pause where a recording
  // stopped (after line 2), the marker on its own; search drops dividers.
  const kinds = transcriptItems(note.transcript, [2]).map((i) => (i.kind === 'group' ? i.source : i.kind));
  expect(kinds).toEqual(['you', 'them', 'pause', 'dictated', 'them']);
  const found = transcriptItems(note.transcript, [2], { query: 'legal' });
  expect(found.map((i) => i.lines.map((l) => l.text))).toEqual([['Legal needs a week.']]);
  expect(transcriptItems(note.transcript, [], { showEcho: true })[1].lines).toHaveLength(1);
  // Grouped corners, and highlighting.
  expect([0, 1, 2].map((i) => bubbleShape(i, 3))).toEqual(['first', 'mid', 'last']);
  expect(bubbleShape(0, 1)).toBe('first');
  expect(highlightParts('Legal needs legal', 'legal').filter((p) => p.hit)).toHaveLength(2);
  // The consent message: the default until edited, saved empty when it is the default.
  expect(consentMessage('')).toBe(DEFAULT_CONSENT_MESSAGE);
  expect(consentMessage('  Mine  ')).toBe('Mine');
  expect(consentToSave(` ${DEFAULT_CONSENT_MESSAGE} `)).toBe('');
  expect(consentToSave('Mine ')).toBe('Mine');
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

  // Notes first: the serif title, the date, My thoughts; the live waveform on
  // Transcript and "+ Summary"; the consent line over Stop and Ask anything.
  const title = pad.getByRole('textbox', { name: 'Meeting title' });
  await expect(title).toHaveValue('Design review');
  expect(await title.evaluate((el) => getComputedStyle(el).fontFamily)).toMatch(/EB Garamond/);
  // The locale's own: "5 Oct, 22:32" (en-GB), "Oct 5, 10:32 PM" (en-US).
  await expect(pad.locator('.date')).toHaveText(/^\S+ \S+, \d{1,2}:\d{2}(\s?[AaPp]\.?\s?[Mm]\.?)?$/);
  await expect(tab(yap, 'My thoughts')).toHaveAttribute('aria-selected', 'true');
  await expect(tab(yap, 'Transcript').locator('.wave')).toBeVisible();
  await expect(tab(yap, 'Summary').locator('.plus')).toBeVisible();
  await expect(pad.locator('.consent')).toContainText("Let people know you're taking notes.");
  await expect(stopButton(pad)).toBeVisible();
  await expect(askInput(pad)).toBeVisible();
  await expect(pad.getByRole('button', { name: 'What did I miss?' })).toBeVisible();
  await shot(pad, '01-recording-my-thoughts');

  // Transcript: the timer box (clock, time, search, copy), the tip, and a
  // friendly empty state.
  await tab(yap, 'Transcript').click();
  await expect(pad.locator('.timerbox .elapsed')).toHaveText(/^\d+:\d\d$/);
  await expect(pad.getByText('Yap is listening')).toBeVisible();
  const hint = pad.locator('.hint');
  await expect(hint).toContainText('lines land about every 15 seconds');
  await shot(pad, '02-transcript-listening');

  // Live lines: a group per speaker turn with a coloured label, a bubble per
  // line, corners grouped like a chat; following the newest one.
  await say(
    yap,
    ['you', 'Morning! Shall we start with the onboarding flow?'],
    ['you', 'I sent the mocks round last night.'],
    ['them', 'Yes, I have the new mocks open.']
  );
  const log = transcript(pad);
  await expect(log.locator('.group.you .who')).toHaveText('You');
  await expect(log.locator('.group.them .who')).toHaveText('Them');
  await expect(log).toContainText('I have the new mocks open.');
  const you = log.locator('.group.you .bubble');
  await expect(you).toHaveCount(2);
  await expect(you.nth(0)).toHaveCSS('border-radius', '12px 12px 12px 4px');
  await expect(you.nth(1)).toHaveCSS('border-radius', '4px 12px 12px');
  const [youColour, themColour] = await Promise.all([
    log.locator('.group.you .who').evaluate((el) => getComputedStyle(el).color),
    log.locator('.group.them .who').evaluate((el) => getComputedStyle(el).color),
  ]);
  expect(youColour).not.toBe(themColour);
  // Echo (the call through the speakers) stays hidden, as in the Notes view.
  await say(
    yap,
    ['them', 'The first screen asks for a workspace name.'],
    ['you', 'ECHO the first screen asks for a workspace name', true]
  );
  const echo = pad.getByRole('button', { name: /Show 1 line your mic picked up from the speakers/ });
  await expect(echo).toBeVisible();
  await expect(log).not.toContainText('ECHO');
  await shot(pad, '03-recording-transcript');
  await echo.click();
  await expect(log.locator('.group.echo')).toContainText('from the speakers');
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
  // Shareable again once the meeting stops; the waveform goes.
  await pauseAll(yap);
  await expect.poll(() => affinity(yap)).toBe(0);
  await expect(tab(yap, 'Transcript').locator('.wave')).toHaveCount(0);
});

test('a new meeting note in the notepad: Start, nothing else yet', async ({ yap, shot }) => {
  const note = await yap.invoke('note_create', { title: '', folder: 'Meetings' });
  await yap.invoke('notepad_open', { noteId: note.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  await expect(pad.getByRole('textbox', { name: 'Meeting title' })).toHaveAttribute('placeholder', 'New note');
  await tab(yap, 'Transcript').click();
  await expect(pad.locator('.timerbox')).toHaveCount(0);
  await expect(pad.getByText('No transcript yet')).toBeVisible();
  await expect(pad.getByRole('button', { name: 'Start', exact: true })).toBeVisible();
  await expect(pad.locator('.consent')).toHaveCount(0);
  await expect(pad.getByRole('button', { name: 'Generate summary' })).toHaveCount(0);
  await shot(pad, '00-new-note-start');

  // Start records into it.
  await pad.getByRole('button', { name: 'Start', exact: true }).click();
  await expect.poll(() => recording(yap)).toBe(true);
  await expect(stopButton(pad)).toBeVisible();
  await expect(pad.locator('.timerbox')).toBeVisible();
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
  await shot(pad, '05-thoughts-typed-in-notepad');

  // Typed in the Notes view → the notepad.
  await raw.fill('Ask Priya about the launch date\nPress release needs legal review');
  await expect(thoughts).toHaveValue('Ask Priya about the launch date\nPress release needs legal review');
  await expectStore(yap, 'notes.json', (s) =>
    meetingNote(s, 'Sync check')?.content?.includes('legal review')
  );
  await shot(main, '06-thoughts-synced-in-notes');

  // The title too, from the notepad.
  await pad.getByRole('textbox', { name: 'Meeting title' }).fill('Launch sync with Priya');
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Launch sync with Priya');

  // Pause (Notes view): no summary, Resume in both, and a paused divider
  // after the last line.
  await say(yap, ['them', 'Legal wants a week with it.']);
  await main.getByRole('button', { name: 'Pause recording' }).click();
  await expect(pad.getByRole('button', { name: 'Resume' })).toBeVisible({ timeout: 20_000 });
  await expect(main.getByRole('button', { name: 'Resume' })).toBeVisible();
  await tab(yap, 'Transcript').click();
  await expect(transcript(pad).getByRole('separator', { name: 'Recording paused here' })).toHaveCount(1);
  await shot(pad, '07-paused-resume');
});

test('the transcript: paused and dictated dividers, hover copy, search, copy all', async ({ yap, shot }) => {
  await startMeeting(yap, 'Roadmap review');
  const pad = yap.notepad;
  const copied = await stubClipboard(pad);
  await tab(yap, 'Transcript').click();
  await say(yap, ['you', 'First, the roadmap for Q4.'], ['you', 'We have three themes.'], ['them', 'Sounds good, go ahead.']);
  const log = transcript(pad);
  await expect(log).toContainText('Sounds good');

  // A pause marks its place; Resume carries on under it.
  await yap.invoke('meeting_pause');
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expect(log.getByRole('separator', { name: 'Recording paused here' })).toHaveCount(1);
  await pad.getByRole('button', { name: 'Resume' }).click();
  await expect.poll(() => recording(yap)).toBe(true);
  // A dictation mid-meeting leaves a marker (meeting.rs keeps its words out).
  await say(yap, ['you', 'Back again, where were we?'], 'dictated', ['them', 'The second theme, the budget.']);
  const dictated = log.getByRole('separator', { name: 'You dictated here' });
  await expect(dictated).toHaveText('You dictated here · left out of the notes');
  const kinds = await log.locator(':scope > .group, :scope > .divider').evaluateAll((els) =>
    els.map((el) =>
      el.classList.contains('dictated')
        ? 'dictated'
        : el.classList.contains('divider')
          ? 'paused'
          : el.classList.contains('you')
            ? 'you'
            : 'them'
    )
  );
  expect(kinds).toEqual(['you', 'them', 'paused', 'you', 'dictated', 'them']);
  await shot(pad, '08-transcript-dividers');

  // Hovering a line shows its copy button.
  const row = log.locator('.row').filter({ hasText: 'Sounds good' });
  const lineCopy = row.getByRole('button', { name: 'Copy line' });
  await expect(lineCopy).toHaveCSS('opacity', '0');
  await row.hover();
  await expect(lineCopy).toHaveCSS('opacity', '1');
  await shot(pad, '09-line-hover-copy');
  await lineCopy.click();
  await expect.poll(copied).toBe('Sounds good, go ahead.');

  // Search: the matching lines, highlighted, without dividers.
  await pad.getByRole('button', { name: 'Search the transcript' }).click();
  const search = pad.getByRole('textbox', { name: 'Search the transcript' });
  await expect(search).toBeFocused();
  await search.fill('theme');
  await expect(log.locator('.bubble')).toHaveCount(2);
  await expect(log.locator('mark')).toHaveText(['theme', 'theme']);
  await expect(log.locator('.divider')).toHaveCount(0);
  await shot(pad, '10-transcript-search');
  await search.fill('nothing like this');
  await expect(pad.getByText('Nothing matches')).toBeVisible();
  await search.press('Escape');
  await expect(search).toHaveValue('');
  await expect(log.locator('.bubble')).toHaveCount(5);

  // Copy all: the spoken lines, no markers.
  await pad.getByRole('button', { name: 'Copy transcript' }).click();
  await expect
    .poll(copied)
    .toBe(
      'You: First, the roadmap for Q4.\nYou: We have three themes.\nThem: Sounds good, go ahead.\n' +
        'You: Back again, where were we?\nThem: The second theme, the budget.'
    );
  await expect(toast(pad, 'Transcript copied')).toBeVisible();
});

test('"What did I miss?" and Ask anything, in a sheet over the meeting', async ({ yap, fakeLlm, shot }) => {
  await startMeeting(yap, 'Weekly sync');
  const pad = yap.notepad;
  await tab(yap, 'Transcript').click();
  await say(yap, ['you', 'Let us start with the roadmap.'], ['them', 'Sure, the roadmap is on track.'], ['you', 'Great, next item.']);
  await expect(transcript(pad)).toContainText('next item');

  // Everything was on screen: nothing new, and no AI call for it. Stop
  // shrinks to a circle while the sheet is up.
  const before = fakeLlm.requests.length;
  await pad.getByRole('button', { name: 'What did I miss?' }).click();
  const sheet = askSheet(pad);
  await expect(sheet).toContainText('Nothing new since you last looked.');
  expect(fakeLlm.requests.length).toBe(before);
  await expect(stopButton(pad)).toHaveClass(/circle/);
  await shot(pad, '11-ask-nothing-new');

  // Minimised; looking away (My thoughts), the meeting goes on.
  await sheet.getByRole('button', { name: 'Minimise chat' }).click();
  await expect(sheet).toHaveCount(0);
  await expect(stopButton(pad)).not.toHaveClass(/circle/);
  await tab(yap, 'My thoughts').click();
  await say(
    yap,
    ['them', 'MISSED-1 Can you send the slides by Friday? The board meets Monday.'],
    ['them', 'MISSED-2 And we moved the demo to Thursday afternoon.']
  );
  await pad.getByRole('button', { name: 'What did I miss?' }).click();
  await expect(sheet).toContainText('2 new lines since you looked, opening with "MISSED-1 Can you send the"');
  await expect(sheet).toContainText('Them asked You to send the slides by Friday.');
  await expect(sheet.locator('.mq').last()).toHaveText('What did I miss?');
  const [catchUp] = fakeLlm.requests.filter((r) => r.kind === 'catchUp');
  const since = catchUp.user.split('Since they last looked')[1];
  expect(since).toContain('MISSED-1');
  expect(since).toContain('MISSED-2');
  expect(since).not.toContain('roadmap'); // seen already: context only
  await shot(pad, '12-ask-sheet-answer');

  // The answer counts as looking.
  await pad.getByRole('button', { name: 'What did I miss?' }).click();
  await expect(sheet.locator('.ma').last()).toContainText('Nothing new since you last looked.');

  // 👍 is kept on this PC only; ⎘ copies the answer.
  const copied = await stubClipboard(pad);
  const answer = sheet.locator('.ma').filter({ hasText: 'Them asked You' });
  await answer.getByRole('button', { name: 'Good answer' }).click();
  await expect(answer.getByRole('button', { name: 'Good answer' })).toHaveAttribute('aria-pressed', 'true');
  const ratings = await pad.evaluate(() => JSON.parse(localStorage.getItem('yapNotepadFeedback')));
  expect(Object.values(ratings).map((r) => r.rating)).toContain('up');
  await answer.getByRole('button', { name: 'Copy answer' }).click();
  await expect.poll(copied).toContain('Them asked You to send the slides by Friday.');

  // A question of your own, answered from the meeting.
  const input = askInput(pad);
  await input.fill('Who owns the budget?');
  await expect(pad.getByRole('button', { name: 'What did I miss?' })).toHaveCount(0); // the send arrow instead
  await input.press('Enter');
  await expect(sheet.locator('.mq').last()).toHaveText('Who owns the budget?');
  await expect(sheet).toContainText('Alice owns the budget');
  expect(fakeLlm.requests.filter((r) => r.kind === 'meetingAsk')).toHaveLength(1);
  await shot(pad, '13-ask-follow-up');

  // New chat clears the thread; Escape puts the sheet away.
  await sheet.getByRole('button', { name: 'New chat' }).click();
  await expect(sheet.locator('.mq')).toHaveCount(0);
  await input.press('Escape');
  await expect(sheet).toHaveCount(0);
  // Focusing Ask anything brings it back.
  await input.focus();
  await expect(sheet).toBeVisible();
});

test('the header: back to Notes, the ⋯ menu, Share, split screen, delete', async ({ yap, main, shot }) => {
  const pad = yap.notepad;
  const note = await startMeeting(yap, 'Header check');
  await say(yap, ['you', 'Quick check of the header.'], ['them', 'Looks fine from here.']);
  const copied = await stubClipboard(pad);

  // ⋯ while recording.
  const more = pad.getByRole('button', { name: 'More', exact: true });
  const menu = pad.getByRole('menu', { name: 'More' });
  await more.click();
  await expect(menu.getByRole('menuitem')).toHaveText([
    'Copy as Markdown',
    'Copy as text',
    'Copy consent message',
    'Audio settings',
    'Save as .md',
    'Delete',
  ]);
  await shot(pad, '14-more-menu-recording');
  await menu.getByRole('menuitem', { name: 'Copy as Markdown' }).click();
  await expect(menu).toHaveCount(0);
  await expect.poll(copied).toContain('# Header check');
  expect(await copied()).toContain('**Them:** Looks fine from here.');
  await expect(toast(pad, 'Notes copied as Markdown')).toBeVisible();
  await more.click();
  await menu.getByRole('menuitem', { name: 'Copy as text' }).click();
  await expect.poll(copied).toContain('Them: Looks fine from here.');
  expect(await copied()).not.toContain('**');

  // Audio settings: Settings → General in Yap's main window.
  await more.click();
  await menu.getByRole('menuitem', { name: 'Audio settings' }).click();
  await expect(settingsDialog(main).getByRole('group', { name: 'Meetings' })).toBeVisible();
  await closeSettings(main);

  // Share: a local popover; the link half copies the notes in one click.
  await pad.getByRole('button', { name: 'Share', exact: true }).click();
  const share = pad.getByRole('dialog', { name: 'Share notes' });
  await expect(share.getByRole('button')).toHaveText(['Copy as Markdown', 'Copy as text', 'Save as .md', 'Email']);
  await shot(pad, '15-share-popover');
  await share.getByRole('button', { name: 'Copy as Markdown' }).click();
  await expect(share).toHaveCount(0);
  await pad.evaluate(() => (window.__copied = null));
  await pad.getByRole('button', { name: 'Copy notes' }).click();
  await expect.poll(copied).toContain('# Header check');
  await expect(pad.locator('.linkbtn .swap')).toHaveClass(/on/);

  // Split screen: its tooltip, and a test run never moves another app's window.
  const split = pad.getByRole('button', { name: 'Split screen with meeting' });
  await split.hover();
  await expect(split).toHaveAttribute('data-tip', 'Split screen with meeting');
  await pad.waitForTimeout(500); // the tooltip's delay
  await shot(pad, '16-split-tooltip');
  await split.click();
  await expect(toast(pad, "Couldn't split the screen")).toContainText("Test runs never move other apps' windows.");

  // Back: the note in Yap's Notes view.
  await pad.getByRole('button', { name: 'Open in Yap' }).click();
  await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Header check');

  // After the meeting the ⋯ menu has no Audio settings; Delete asks first.
  await pauseAll(yap);
  await closeToasts(pad);
  await more.click();
  await expect(menu.getByRole('menuitem')).toHaveText([
    'Copy as Markdown',
    'Copy as text',
    'Copy consent message',
    'Save as .md',
    'Delete',
  ]);
  await shot(pad, '17-more-menu-stopped');
  await menu.getByRole('menuitem', { name: 'Delete' }).click();
  const confirm = pad.getByRole('alertdialog', { name: 'Delete this meeting?' });
  await expect(confirm).toBeVisible();
  await shot(pad, '18-delete-confirm');
  await confirm.getByRole('button', { name: 'Cancel' }).click();
  await expect(confirm).toHaveCount(0);
  expect(meetingNote(yap.readJson('notes.json'), 'Header check')?.id).toBe(note.id);
  await more.click();
  await menu.getByRole('menuitem', { name: 'Delete' }).click();
  await confirm.getByRole('button', { name: 'Delete' }).click();
  await expectStore(yap, 'notes.json', (s) => !s.notes.some((n) => n.id === note.id));
  await expect.poll(() => visible(yap, 'notepad')).toBe(false);

  // Deleting the meeting being recorded stops it first.
  const live = await startMeeting(yap, 'Delete while recording');
  await say(yap, ['you', 'This one goes.']);
  await more.click();
  await menu.getByRole('menuitem', { name: 'Delete' }).click();
  await expect(confirm).toContainText('The recording stops first.');
  await confirm.getByRole('button', { name: 'Delete' }).click();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expectStore(yap, 'notes.json', (s) => !s.notes.some((n) => n.id === live.id));
  await expect.poll(() => visible(yap, 'notepad')).toBe(false);
});

test('the consent line: Learn more offers a message for the chat, editable; ⋯ copies it', async ({
  yap,
  main,
  shot,
}) => {
  await startMeeting(yap, 'Consent check');
  const pad = yap.notepad;
  const copied = await stubClipboard(pad);
  const line = pad.locator('.consent');
  await expect(line).toContainText("Let people know you're taking notes.");
  await line.getByRole('button', { name: 'Learn more' }).click();
  const pop = pad.getByRole('dialog', { name: 'Consent message' });
  await expect(pop).toContainText('Yap transcribes on this PC; nothing is uploaded.');
  const message = pop.getByRole('textbox', { name: 'Message for the meeting chat' });
  await expect(message).toHaveValue(DEFAULT_CONSENT_MESSAGE);
  await shot(pad, '19-consent-message');

  // Copy: the message as shown.
  await pop.getByRole('button', { name: 'Copy' }).click();
  await expect.poll(copied).toBe(DEFAULT_CONSENT_MESSAGE);
  await expect(toast(pad, 'Copied: paste it into the meeting chat')).toBeVisible();
  await expect(pop).toBeVisible(); // it stays for an edit

  // An edit is saved when the box loses focus (here: a click elsewhere)…
  const mine = "Hi everyone, Yap is taking notes on my PC for this call. Shout if that's not OK.";
  await message.fill(mine);
  await pad.getByRole('textbox', { name: 'Meeting title' }).click();
  await expect(pop).toHaveCount(0);
  await expectStore(yap, 'config.json', (c) => c.meetingConsentMessage === mine);
  // …Settings' copy of the config takes it (its next save keeps it)…
  const dialog = await openSettings(main, 'General');
  const split = dialog.getByRole('group', { name: 'Meetings' }).getByRole('button', { name: 'Split the screen when joining' });
  await split.click();
  await expectStore(yap, 'config.json', (c) => c.meetingSplitScreen === true && c.meetingConsentMessage === mine);
  await split.click();
  await expectStore(yap, 'config.json', (c) => c.meetingSplitScreen === false && c.meetingConsentMessage === mine);
  await closeSettings(main);
  // …and the ⋯ menu copies the saved one.
  await pad.getByRole('button', { name: 'More', exact: true }).click();
  await pad.getByRole('menuitem', { name: 'Copy consent message' }).click();
  await expect.poll(copied).toBe(mine);

  // Emptied, it's Yap's default again.
  await line.getByRole('button', { name: 'Learn more' }).click();
  await expect(message).toHaveValue(mine);
  await message.fill('');
  await message.press('Escape');
  await expect(pop).toHaveCount(0);
  await expectStore(yap, 'config.json', (c) => c.meetingConsentMessage === '');
  await line.getByRole('button', { name: 'Learn more' }).click();
  await expect(message).toHaveValue(DEFAULT_CONSENT_MESSAGE);
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
  await shot(pad, '20-made-up-title');

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
  await shot(pad, '21-ai-title');
  await shot(main, '22-ai-title-in-notes');

  // Stopped (paused: no summary yet): the divider after the last line,
  // Generate summary over Resume and Ask anything.
  await pauseAll(yap);
  await tab(yap, 'Transcript').click();
  await expect(transcript(pad).getByRole('separator', { name: 'Recording paused here' })).toHaveCount(1);
  await expect(pad.getByRole('button', { name: 'Generate summary' })).toBeVisible();
  await expect(pad.getByRole('button', { name: 'Resume' })).toBeVisible();
  await expect(pad.locator('.consent')).toHaveCount(0);
  await transcript(pad).evaluate((el) => (el.scrollTop = 0));
  await shot(pad, '23-stopped-ai-title');
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
  await stopButton(pad).click();
  const ask = toast(pad, 'Started by mistake?');
  await expect(ask).toBeVisible({ timeout: 20_000 });
  await expect(ask).toContainText('Only a few words were captured. Keep this meeting or discard it.');
  await expect(ask.getByRole('button', { name: 'Discard' })).toBeVisible();
  await shot(pad, '24-started-by-mistake');

  // Keep: nothing written, nothing deleted; Resume and Generate summary.
  await ask.getByRole('button', { name: 'Keep' }).click();
  await expect(ask).toHaveCount(0);
  await expect(pad.getByRole('button', { name: 'Resume' })).toBeVisible();
  await expect(pad.getByRole('button', { name: 'Generate summary' })).toBeVisible();
  expect(meetingNote(yap.readJson('notes.json'), 'Kept by mistake')?.id).toBe(kept.id);
  expect((await yap.invoke('note_get', { id: kept.id })).enhancedContent ?? '').toBe('');
  await shot(pad, '25-kept-resume-generate');

  // Generate summary, when wanted after all: "+ Summary" loses its +.
  await pad.getByRole('button', { name: 'Generate summary' }).click();
  await expect(pad.getByRole('region', { name: 'Summary' }).locator('.rendered')).toBeVisible({ timeout: 15_000 });
  await expect(pad.getByRole('button', { name: 'Generate summary' })).toHaveCount(0);
  await expect(tab(yap, 'Summary').locator('.plus')).toHaveCount(0);

  // Discard: the note goes, and the notepad with it.
  const gone = await startMeeting(yap, 'Discarded by mistake');
  await say(yap, ['you', 'Oops.']);
  await stopButton(pad).click();
  await toast(pad, 'Started by mistake?').getByRole('button', { name: 'Discard' }).click();
  await expectStore(yap, 'notes.json', (s) => !s.notes.some((n) => n.id === gone.id));
  await expect.poll(() => visible(yap, 'notepad')).toBe(false);
});

test('"Started by mistake?" asks on the Yap bar when no Yap window is on screen', async ({ yap, shot }) => {
  const note = await startMeeting(yap, 'Bar by mistake');
  await say(yap, ['you', 'Wrong button.']);
  await closeNotepad(yap);
  // The main window off screen too, as after a start-hidden launch.
  await yap.invoke('plugin:window|close', { label: 'settings' });
  await expect.poll(() => visible(yap, 'settings')).toBe(false);

  // Ended from elsewhere (the bar's ■, an automatic stop): the bar asks.
  await yap.invoke('meeting_end', {});
  const bar = yap.overlay;
  const card = bar.getByRole('status').filter({ hasText: 'Started by mistake?' });
  await expect(card).toBeVisible({ timeout: 20_000 });
  await expect(card).toContainText('Only a few words were captured.');
  await bar.waitForTimeout(300); // the card's entrance
  await shot(bar, '26-started-by-mistake-on-bar');
  try {
    // The pretend cursor onto the card (bar.spec.js): it takes clicks.
    await expect(() => yap.invoke('bar_simulate', { pointer: 'card:meeting-mistake' })).toPass();
    await card.getByRole('button', { name: 'Discard' }).click();
    await expect(card).toHaveCount(0);
    await expectStore(yap, 'notes.json', (s) => !s.notes.some((n) => n.id === note.id));
  } finally {
    await yap.invoke('bar_simulate', { pointer: 'away' });
    await yap.invoke('open_settings'); // back as the suite started it
  }
});

test('Stop writes the summary in steps; a failure says so above the bar and Retry works', async ({
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
  await stopButton(pad).click();
  const summary = pad.getByRole('region', { name: 'Summary' });
  await expect(summary.locator('.steps')).toContainText('Step 2 of 3', { timeout: 20_000 });
  await expect(summary.locator('.stepline')).toContainText('into an action plan');
  await shot(pad, '27-summary-step-2-of-3');

  const failed = pad.getByRole('alert');
  await expect(failed).toContainText("The summary didn't come through", { timeout: 15_000 });
  await expect(failed).toContainText('HTTP 500');
  await expect(pad.getByRole('button', { name: 'Generate summary' })).toHaveCount(0);
  await shot(pad, '28-summary-error-retry');

  // Retry: the action plan.
  fakeLlm.delay('actionPlan', 0);
  await failed.getByRole('button', { name: 'Retry' }).click();
  await expect(summary.locator('.rendered').getByRole('heading', { name: 'Action plan' })).toBeVisible({
    timeout: 15_000,
  });
  await expect(failed).toHaveCount(0);
  await expect(pad.getByRole('button', { name: 'Generate summary' })).toHaveCount(0);
  await expect(pad.getByRole('button', { name: 'Resume' })).toBeVisible();
  await shot(pad, '29-summary-done');
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
    await shot(pad, '30-auto-stop-plan-in-notepad');
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
  await shot(pad, '31-shortcut-stopped-in-notepad');
});

test('Settings → General → Meetings: open the notepad, split the screen', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'General');
  const meetings = dialog.getByRole('group', { name: 'Meetings' });
  const open = meetings.getByRole('button', { name: 'Open the notepad when a meeting starts' });
  const split = meetings.getByRole('button', { name: 'Split the screen when joining' });
  await expect(open).toHaveAttribute('aria-pressed', 'true');
  await expect(split).toHaveAttribute('aria-pressed', 'false');
  await split.scrollIntoViewIfNeeded();
  await shot(main, '32-settings-notepad-rows');

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
  await expect(transcript(pad)).toHaveCount(0);
  await shot(pad, '33-live-transcript-off');
  await pauseAll(yap);
  await expect(transcript(pad)).toContainText('QUIET-LINE');

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
