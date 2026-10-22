// Who said what in a meeting (src-tauri/src/speakers.rs, src/lib/SpeakerMenu.svelte):
// the note's speaker map names the transcript's voices — "You", "Them", or,
// once told apart, "Speaker 1", "Speaker 2"… — and the menu on a name (or on
// a line's person button) renames a voice everywhere or just that line,
// merges two voices, and undoes a guess. A 1:1 meeting's calendar invite
// names "Them" after the other invitee, as a guess ("Alice Moreau?").
// The names reach the notepad and the Notes view (kept in step), copies and
// exports, every AI prompt (the fake AI records what it was sent), the action
// plan's owner checks, the local API and the MCP server.
//
// No audio device is opened: lines (with the voice the separation would give
// them, `speaker: "them-1"`) are handed to the recorder with
// `e2e_meeting_feed`. An old notes.json (before speakers existed) is seeded
// to check it still reads as You/Them, and can be named.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, closeToasts, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { newMeetingNote, meetingNote } from './support/meetings.js';
import { startCalendarFeed, calendarFile, vevent } from './support/calendar-feed.js';
import { startMcp } from './support/mcp-client.js';
import {
  speakerId,
  speakerInfo,
  speakerLabel,
  noteVoices,
  withLineSpeakers,
  transcriptItems,
  transcriptText,
  noteMarkdown,
} from '../src/lib/notepadText.js';

// A meeting note from before voices and line times existed.
const OLD_NOTE = {
  id: 501,
  title: 'Old standup',
  noteType: 'meeting',
  folder: 'Meetings',
  source: 'meeting',
  createdTs: 1_759_000_000,
  updatedTs: 1_759_000_000,
  transcript: [
    { source: 'you', text: 'Morning, anything blocking you?', ts: 1_759_000_010 },
    { source: 'them', text: 'Only the review, it lands today.', ts: 1_759_000_020 },
  ],
};

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
        name: 'speakers',
        config: {
          llmScopes: {
            noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
          },
          meetingReminder: 'never',
          bridgeEnabled: true,
        },
        data: { 'notes.json': { folders: ['Personal', 'Meetings'], notes: [OLD_NOTE] } },
      });
    },
    { scope: 'worker' },
  ],
});

const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const tab = (yap, name) => yap.notepad.getByRole('tab', { name });
const transcript = (pad) => pad.getByRole('log', { name: 'Transcript' });
const notesLog = (main) => main.getByRole('log', { name: 'Meeting transcript' });
/** The notepad's group labels, in order. */
const groupNames = (pad) => transcript(pad).locator(':scope > .group .who').allTextContents();
/** The Notes view's speaker labels, in order (one per run of a speaker). */
const notesNames = (main) => notesLog(main).locator('.bubble .who').allTextContents();

let clockTs = Math.floor(Date.now() / 1000);
/** Hand the recorder lines: `[source, text, speaker?]`. */
async function say(yap, ...lines) {
  const segments = lines.map(([source, text, speaker]) => ({
    source,
    text,
    ts: (clockTs += 6),
    ...(speaker ? { speaker } : {}),
  }));
  expect(await yap.invoke('e2e_meeting_feed', { segments })).toBe(segments.length);
}

async function pauseAll(yap) {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
}

async function stubClipboard(page) {
  await page.evaluate(() => {
    window.__copied = null;
    navigator.clipboard.writeText = async (t) => {
      window.__copied = t;
    };
  });
  return () => page.evaluate(() => window.__copied);
}

/** The local API (bridge.rs), as a coding agent would call it. */
async function api(yap, route) {
  const { port, token } = JSON.parse(fs.readFileSync(yap.bridgeFile, 'utf8'));
  const r = await fetch(`http://127.0.0.1:${port}${route}`, { headers: { authorization: `Bearer ${token}` } });
  expect(r.status).toBe(200);
  return (await r.json()).data;
}

test.afterEach(async ({ yap }) => {
  await pauseAll(yap);
  for (let i = 0; i < 3; i += 1) await yap.notepad.keyboard.press('Escape');
  await closeToasts(yap.notepad);
});

test('who a line is and what it is called, read the way Rust reads the map', () => {
  const speakers = {
    them: { name: 'Priya Shah', email: 'priya@example.com', source: 'calendar' },
    'them-1': { name: 'Tom', source: 'user', locked: true },
    'them-2': { mergedInto: 'them-1' },
    'p-1': { name: 'Sam', source: 'user', locked: true },
  };
  // A line's voice: its speaker (else its side), through merges.
  expect(speakerId(speakers, { source: 'them' })).toBe('them');
  expect(speakerId(speakers, { source: 'them', speaker: 'them-2' })).toBe('them-1');
  expect(speakerId({}, { source: 'you' })).toBe('you');
  // Names: a calendar name nobody confirmed is a guess; unnamed voices.
  expect(speakerInfo(speakers, 'them')).toMatchObject({ name: 'Priya Shah', label: 'Priya Shah?', guess: true });
  expect(speakerInfo(speakers, 'them-1')).toMatchObject({ label: 'Tom', guess: false, locked: true });
  expect(speakerInfo({}, 'them-3').label).toBe('Speaker 3');
  expect(speakerInfo(speakers, 'you').label).toBe('You');
  const t = [
    { source: 'you', text: 'Hi all.', ts: 1 },
    { source: 'them', text: 'Hello.', ts: 2 },
    { source: 'them', text: 'I will book it.', ts: 3, speaker: 'them-1' },
    { source: 'them', text: 'And I the train.', ts: 4, speaker: 'them-2' },
    { source: 'you', text: 'Me, actually.', ts: 5, speaker: 'p-1', speakerLocked: true },
  ];
  expect(t.map((s) => speakerLabel(speakers, s))).toEqual(['You', 'Priya Shah?', 'Tom', 'Tom', 'Sam']);
  // Groups are runs of one voice (two ids merged into one are one run).
  const groups = transcriptItems(t, [], { speakers });
  expect(groups.map((g) => [g.label, g.source, g.lines.length])).toEqual([
    ['You', 'you', 1],
    ['Priya Shah?', 'them', 1],
    ['Tom', 'them', 2],
    ['Sam', 'them', 1],
  ]);
  // No map: You and Them, as before.
  expect(transcriptItems(t.slice(0, 2)).map((g) => g.label)).toEqual(['You', 'Them']);
  expect(noteVoices({ transcript: t, speakers })).toEqual(['you', 'them', 'them-1', 'p-1']);
  // Copies use the names.
  expect(transcriptText(t, speakers)).toContain('Priya Shah?: Hello.\nTom: I will book it.');
  expect(noteMarkdown({ title: 'x', transcript: t, speakers })).toContain('**Sam:** Me, actually.');
  // Another window's edit: every line's voice, or "fetch it" when lines differ.
  const relabelled = withLineSpeakers(t.slice(0, 2), [['you', false], ['p-1', true]]);
  expect(relabelled[1]).toMatchObject({ speaker: 'p-1', speakerLocked: true });
  expect(relabelled[0].speaker).toBe('');
  expect(withLineSpeakers(t, [['you', false]])).toBeNull();
});

test('rename a voice everywhere or one line, merge two, in the notepad and the Notes view', async ({
  yap,
  main,
  shot,
}) => {
  await newMeetingNote(main, 'Offsite planning');
  await main.getByRole('button', { name: 'Record' }).click();
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  const copied = await stubClipboard(pad);
  await tab(yap, 'Transcript').click();
  // Two voices on the call, told apart (as the voice separation will).
  await say(
    yap,
    ['you', 'Morning both. Shall we start with the budget for the offsite?'],
    ['them', 'Yes, the budget first. I can send it to finance by Friday.', 'them-1'],
    ['them', 'And I will book the venue once the numbers are in.', 'them-2'],
    ['them', 'Great, then catering is the last open item.', 'them-1'],
    ['them', 'I can ask the caterer for a quote this week.', 'them-2'],
    ['you', 'Thanks, that covers it.']
  );
  const log = transcript(pad);
  await expect(log).toContainText('Thanks, that covers it.');
  expect(await groupNames(pad)).toEqual(['You', 'Speaker 1', 'Speaker 2', 'Speaker 1', 'Speaker 2', 'You']);

  // The menu on a name.
  await log.getByRole('button', { name: 'Speaker 1' }).first().click();
  const menu = pad.getByRole('menu', { name: 'Speaker: Speaker 1' });
  await expect(menu.getByRole('menuitem')).toHaveText([
    'Rename Speaker 1 everywhere…',
    'Rename just this line…',
    'Merge with…',
  ]);
  await shot(pad, '01-speaker-menu');
  await menu.getByRole('menuitem', { name: 'Rename Speaker 1 everywhere…' }).click();
  const form = pad.getByRole('dialog', { name: 'Rename speaker' });
  await form.getByRole('textbox', { name: 'Speaker name' }).fill('Priya');
  await shot(pad, '02-rename-everywhere');
  await form.getByRole('button', { name: 'Save' }).click();
  await expect(form).toHaveCount(0);
  expect(await groupNames(pad)).toEqual(['You', 'Priya', 'Speaker 2', 'Priya', 'Speaker 2', 'You']);
  await expectStore(yap, 'notes.json', (s) => {
    const n = meetingNote(s, 'Offsite planning');
    return n?.speakers?.['them-1']?.name === 'Priya' && n.speakers['them-1'].source === 'user' && n.speakers['them-1'].locked;
  });
  // The Notes view follows: one name per run of a speaker.
  await expect.poll(() => notesNames(main)).toEqual(['You', 'Priya', 'Speaker 2', 'Priya', 'Speaker 2', 'You']);

  // Merge: Speaker 2 is Priya too.
  await log.getByRole('button', { name: 'Speaker 2' }).first().click();
  await pad.getByRole('menu', { name: 'Speaker: Speaker 2' }).getByRole('menuitem', { name: 'Merge with…' }).click();
  const merge = pad.getByRole('menu', { name: 'Merge Speaker 2 with' });
  await expect(merge.getByRole('menuitem')).toHaveText(['Priya']);
  await shot(pad, '03-merge-with');
  await merge.getByRole('menuitem', { name: 'Priya' }).click();
  await expect.poll(() => groupNames(pad)).toEqual(['You', 'Priya', 'You']);
  await expect.poll(() => notesNames(main)).toEqual(['You', 'Priya', 'You']);
  // A later line from the merged voice reads as Priya.
  await say(yap, ['them', 'One more thing about the venue deposit.', 'them-2']);
  await expect.poll(() => groupNames(pad)).toEqual(['You', 'Priya', 'You', 'Priya']);

  // Just this line: someone else said it.
  const row = log.locator('.row').filter({ hasText: 'I can ask the caterer' });
  await row.hover();
  await row.getByRole('button', { name: 'Who said this?' }).click();
  const lineForm = pad.getByRole('dialog', { name: 'Rename speaker' });
  await expect(lineForm).toContainText('Who said this line?');
  await expect(lineForm.getByRole('textbox', { name: 'Speaker name' })).toBeFocused();
  await lineForm.getByRole('textbox', { name: 'Speaker name' }).fill('Tom');
  await shot(pad, '04-rename-just-this-line');
  await lineForm.getByRole('textbox', { name: 'Speaker name' }).press('Enter');
  await expect.poll(() => groupNames(pad)).toEqual(['You', 'Priya', 'Tom', 'You', 'Priya']);
  await expect.poll(() => notesNames(main)).toEqual(['You', 'Priya', 'Tom', 'You', 'Priya']);
  await shot(pad, '05-named-voices');
  await shot(main, '06-notes-view-named-voices');
  const stored = meetingNote(yap.readJson('notes.json'), 'Offsite planning');
  const tomLine = stored.transcript.find((s) => s.text.startsWith('I can ask the caterer'));
  expect(tomLine).toMatchObject({ speaker: 'p-1', speakerLocked: true });
  expect(stored.speakers['p-1']).toMatchObject({ name: 'Tom', source: 'user', locked: true });
  expect(stored.speakers['them-2']).toEqual({ mergedInto: 'them-1' });

  // Rename in the Notes view: the notepad follows.
  await notesLog(main).getByRole('button', { name: 'Tom' }).click();
  await main.getByRole('menu', { name: 'Speaker: Tom' }).getByRole('menuitem', { name: 'Rename Tom everywhere…' }).click();
  const mainForm = main.getByRole('dialog', { name: 'Rename speaker' });
  await mainForm.getByRole('textbox', { name: 'Speaker name' }).fill('Tom Baker');
  await mainForm.getByRole('button', { name: 'Save' }).click();
  await expect.poll(() => groupNames(pad)).toEqual(['You', 'Priya', 'Tom Baker', 'You', 'Priya']);

  // Copy: the names.
  await pad.getByRole('button', { name: 'Copy transcript' }).click();
  await expect.poll(copied).toContain('Priya: Yes, the budget first.');
  expect(await copied()).toContain('Tom Baker: I can ask the caterer for a quote this week.');

  // "You" is the mic: no rename everywhere, no merge — only who said a line.
  await log.getByRole('button', { name: 'You', exact: true }).first().click();
  await expect(pad.getByRole('menu', { name: 'Speaker: You' }).getByRole('menuitem')).toHaveText([
    'Rename just this line…',
  ]);
  await pad.keyboard.press('Escape');
  await expect(pad.getByRole('menu', { name: 'Speaker: You' })).toHaveCount(0);
  // Escape closed only the menu: the transcript tab is still up.
  await expect(log).toBeVisible();
});

test('a 1:1 meeting names "Them" after the other invitee, as a guess; "Not Alice" undoes it', async ({
  yap,
  fakeLlm,
  shot,
}) => {
  const feed = await startCalendarFeed();
  const owner = { name: 'Imogen', email: 'imogen@example.com' };
  feed.set(
    calendarFile(owner.email, [
      vevent({
        uid: 'one-to-one@e2e',
        summary: 'Alice and Imogen 1:1',
        start: new Date(Date.now() - 60_000),
        end: new Date(Date.now() + 30 * 60_000),
        attendees: [{ name: 'Alice Moreau', email: 'alice@example.com' }, owner],
      }),
    ])
  );
  try {
    await yap.invoke('calendar_add_link', { link: feed.url, kind: 'ics' });
    const synced = await yap.invoke('calendar_sync');
    const event = synced.events.find((e) => e.title === 'Alice and Imogen 1:1');
    const noteId = await yap.invoke('calendar_event', { key: event.key, action: 'start' });
    await expect.poll(() => visible(yap, 'notepad')).toBe(true);
    const pad = yap.notepad;
    // Lines said while the person looks at their own notes.
    await tab(yap, 'My thoughts').click();
    await say(
      yap,
      ['you', 'Hi Alice, thanks for making time. How is the launch going?'],
      ['them', 'Pretty well. I need to send the slides to legal by Thursday.'],
      ['you', 'Great, let me know if legal pushes back.']
    );
    const n = await yap.invoke('note_get', { id: noteId });
    expect(n.speakers.them).toEqual({ name: 'Alice Moreau', email: 'alice@example.com', source: 'calendar' });

    // "What did I miss?": the prompt knows her as the other speaker, and
    // that it's a guess.
    const before = fakeLlm.requests.filter((r) => r.kind === 'catchUp').length;
    await pad.getByRole('button', { name: 'What did I miss?' }).click();
    await expect.poll(() => fakeLlm.requests.filter((r) => r.kind === 'catchUp').length).toBeGreaterThan(before);
    const asked = fakeLlm.requests.filter((r) => r.kind === 'catchUp').at(-1);
    expect(asked.user).toContain('Alice Moreau: Pretty well. I need to send the slides');
    expect(asked.user).toContain('Speakers besides You: Alice Moreau (a guess: the only other person invited)');
    expect(asked.system).toContain('says which are only guesses');
    await pad.getByRole('dialog', { name: 'Ask about this meeting' }).getByRole('button', { name: 'Minimise chat' }).click();

    // The transcript shows her name as a guess.
    await tab(yap, 'Transcript').click();
    const log = transcript(pad);
    await expect(log).toContainText('let me know');
    expect(await groupNames(pad)).toEqual(['You', 'Alice Moreau?', 'You']);
    await expect(log.getByRole('button', { name: 'Alice Moreau?' })).toHaveClass(/guess/);
    await shot(pad, '07-calendar-guess');

    // The menu offers to confirm the guess or undo it.
    await log.getByRole('button', { name: 'Alice Moreau?' }).click();
    const menu = pad.getByRole('menu', { name: 'Speaker: Alice Moreau?' });
    await expect(menu).toContainText('A guess from the calendar invite');
    await expect(menu.getByRole('menuitem')).toHaveText([
      "It's Alice Moreau",
      'Not Alice Moreau',
      'Rename Alice Moreau everywhere…',
      'Rename just this line…',
    ]);
    await shot(pad, '08-guess-menu');
    await menu.getByRole('menuitem', { name: 'Not Alice Moreau' }).click();
    await expect.poll(() => groupNames(pad)).toEqual(['You', 'Them', 'You']);
    const after = await yap.invoke('note_get', { id: noteId });
    expect(after.speakers.them).toEqual({ not: ['Alice Moreau'] });

    // Starting the meeting again (the calendar ties it once more): no guess.
    await pauseAll(yap);
    await yap.invoke('meeting_start', { noteId });
    await expect.poll(() => recording(yap)).toBe(true);
    await say(yap, ['them', 'Back again, sorry, my connection dropped.']);
    await expect.poll(() => groupNames(pad)).toEqual(['You', 'Them', 'You', 'Them']);
    expect((await yap.invoke('note_get', { id: noteId })).speakers.them.name).toBeUndefined();
    await shot(pad, '09-not-alice');
  } finally {
    await pauseAll(yap);
    const s = await yap.invoke('calendar_status');
    for (const c of s.connections) await yap.invoke('calendar_disconnect', { id: c.id });
    await feed.close();
  }
});

test('the names reach the AI title, Ask, the action plan, copies, the local API and MCP', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  // A meeting with no title: the AI names it once there's enough talk.
  const note = await yap.invoke('note_create', { title: '', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  const copied = await stubClipboard(pad);
  await say(yap, ['them', 'Hi, this is the budget review. I have the numbers ready for the quarter.', 'them-1']);
  await yap.invoke('speaker_rename', { noteId: note.id, speaker: 'them-1', name: 'Priya' });
  const titled = fakeLlm.requests.filter((r) => r.kind === 'title').length;
  await say(
    yap,
    ['you', 'Thanks Priya. Can you walk me through travel and the offsite, since those two moved the most since last quarter?'],
    ['them', 'Travel is up twelve percent, mostly flights for the offsite, and the venue deposit landed early this quarter.', 'them-1'],
    ['you', 'Okay, so the budget is fine overall, it is the timing. Can you send the revised budget to finance by Friday?'],
    ['them', 'Yes, I will send the revised budget by Friday and copy you on it. I will also flag the deposit for finance.', 'them-1'],
    ['you', 'Perfect, then catering is the only open item and nobody owns it yet, so we pick it up next week.'],
    ['them', 'Agreed. I will also check whether the venue can hold the date if the headcount for both days goes up a little.', 'them-1'],
    ['you', 'Good idea. Let us meet again next Tuesday to close catering and confirm the final headcount with everyone.']
  );
  // The AI title read who said what.
  await expect.poll(() => fakeLlm.requests.filter((r) => r.kind === 'title').length).toBeGreaterThan(titled);
  const title = fakeLlm.requests.filter((r) => r.kind === 'title').at(-1);
  expect(title.user).toContain('Priya: Hi, this is the budget review.');
  expect(title.user).toContain('Speakers besides You: Priya');

  // Ask anything.
  const input = pad.getByRole('textbox', { name: 'Ask anything' });
  await input.click();
  await input.fill('Who owns the budget?');
  await input.press('Enter');
  await expect.poll(() => fakeLlm.requests.filter((r) => r.kind === 'meetingAsk').length).toBeGreaterThan(0);
  expect(fakeLlm.requests.filter((r) => r.kind === 'meetingAsk').at(-1).user).toContain(
    'Priya: Yes, I will send the revised budget by Friday'
  );
  await pad.keyboard.press('Escape');

  // The action plan: Priya is a known person; an invented owner isn't.
  fakeLlm.reply('actionPlan', (user) =>
    [
      'The budget is fine; the deposit moved quarters.',
      '',
      '## Action plan',
      '',
      ...(user.includes('Priya: Yes, I will send') ? ['### Priya', '- [ ] Send the revised budget to finance (due: Friday)', ''] : []),
      '### Mallory',
      '- [ ] Print the badges',
      '',
      '## Open questions',
      '- Who owns catering?',
    ].join('\n')
  );
  await pad.getByRole('button', { name: 'Stop', exact: true }).click();
  const summary = pad.getByRole('region', { name: 'Summary' });
  await expect(summary).toContainText('Send the revised budget to finance', { timeout: 20_000 });
  const plan = fakeLlm.requests.filter((r) => r.kind === 'actionPlan').at(-1);
  expect(plan.user).toContain('Speakers besides You: Priya');
  expect(plan.user).toContain('Priya: Yes, I will send the revised budget by Friday');
  const stored = await yap.invoke('note_get', { id: note.id });
  expect(stored.enhancedContent).toContain('### Priya\n- [ ] Send the revised budget to finance (due: Friday)');
  expect(stored.enhancedContent).not.toContain('### Mallory');
  expect(stored.enhancedContent).toMatch(/## Unassigned[\s\S]*Print the badges/);
  await shot(pad, '10-action-plan-with-names');

  // Copy as Markdown, and Save as .md (note_export).
  await pad.getByRole('button', { name: 'More', exact: true }).click();
  await pad.getByRole('menu', { name: 'More' }).getByRole('menuitem', { name: 'Copy as Markdown' }).click();
  await expect.poll(copied).toContain('**Priya:** Yes, I will send the revised budget by Friday');
  const file = path.join(yap.dataDir, 'speakers-export.md');
  await yap.invoke('note_export', { id: note.id, path: file });
  const md = fs.readFileSync(file, 'utf8');
  expect(md).toContain('**Priya:** Travel is up twelve percent');
  expect(md).toContain('**You:** Okay, so the budget is fine overall');

  // The local API hands out the map and each line's voice.
  const viaApi = await api(yap, `/v1/notes/${note.id}`);
  expect(viaApi.speakers['them-1']).toMatchObject({ name: 'Priya', source: 'user', locked: true });
  expect(viaApi.transcript.filter((s) => s.speaker === 'them-1')).toHaveLength(4);
  expect(viaApi.transcript[0].startMs).toBeUndefined(); // fed lines have no place; recorded ones do
  const found = await api(yap, '/v1/meetings/search?q=deposit');
  expect(found.find((m) => m.id === note.id).matches[0].speaker).toBe('Priya');

  // An AI app reads it through yap.exe mcp, names and all.
  const mcp = startMcp(path.join(yap.appDir, 'yap.exe'), { YAP_BRIDGE_FILE: yap.bridgeFile });
  try {
    await mcp.initialize();
    const r = await mcp.tool('get_meeting', { id: note.id });
    expect(r.isError, r.text).toBe(false);
    expect(r.text).toMatch(/\[\d+:\d{2}\] Priya: Yes, I will send the revised budget by Friday/);
    expect(r.text).toContain("Other lines are labelled with who spoke: Priya");
  } finally {
    await mcp.close?.();
  }
  await openView(main, 'Notes');
});

test('a note from before speakers reads as You and Them, and can be named', async ({ yap, main, shot }) => {
  await yap.invoke('notepad_open', { noteId: OLD_NOTE.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  await tab(yap, 'Transcript').click();
  await expect.poll(() => groupNames(pad)).toEqual(['You', 'Them']);
  await shot(pad, '11-old-note');
  // The same note in the Notes view.
  await pad.getByRole('button', { name: 'Open in Yap' }).click();
  await expect(notesLog(main)).toContainText('Only the review, it lands today.');
  expect(await notesNames(main)).toEqual(['You', 'Them']);

  await transcript(pad).getByRole('button', { name: 'Them', exact: true }).click();
  await pad.getByRole('menu', { name: 'Speaker: Them' }).getByRole('menuitem', { name: 'Rename Them everywhere…' }).click();
  await pad.getByRole('textbox', { name: 'Speaker name' }).fill('Sam');
  await pad.getByRole('button', { name: 'Save' }).click();
  await expect.poll(() => groupNames(pad)).toEqual(['You', 'Sam']);
  await expect.poll(() => notesNames(main)).toEqual(['You', 'Sam']);
  await expectStore(yap, 'notes.json', (s) => {
    const n = s.notes.find((x) => x.id === OLD_NOTE.id);
    // Named; its old lines untouched (no voice or place of their own).
    return n?.speakers?.them?.name === 'Sam' && n.transcript.every((seg) => !('speaker' in seg) && !('startMs' in seg));
  });
  await pad.getByRole('button', { name: 'Close notepad' }).click();
});
