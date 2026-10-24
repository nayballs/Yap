// Meeting notes you can fix (src-tauri/src/note_edits.rs, tasks.rs;
// src/lib/noteEdits.js, TasksView.svelte), end to end:
// - a transcript line's words fixed in the notepad (double-click, Enter):
//   saved and marked edited with what Yap heard kept, shown in the Notes
//   view too, and the misheard word offered for the dictionary right there
//   ("Suggest words from your corrections", learn.rs);
// - with the meeting's audio kept (a recording played from WAVs, as in
//   meeting-audio-kept.spec.js): a line fixed in the Notes view (Edit line in
//   its menu) still plays what was said, and Transcribe again keeps it;
// - the summary's tasks ticked in the notepad, seen in the Tasks view (by
//   meeting, by person, "Said at" back to the moment) and ticked back there;
// - the summary edited in place: regenerating it asks first, the end of a
//   resumed meeting writes nothing over it ("kept") and says what's new
//   since it with Update, which replaces it once the person says so — the
//   ticked tasks staying ticked.
// The AI is a local fake (support/fake-llm.js); no audio device is opened.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { newMeetingNote, meetingNote } from './support/meetings.js';
import { writeWav } from './support/wav.js';
import { RUNS } from './support/yap.js';
import {
  lineRef,
  withLineEdit,
  taskText,
  withTaskMark,
  newSinceSummary,
  newSinceLine,
  transcriptItems,
} from '../src/lib/core/notepadText.js';

const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const toastIn = (page, title) => page.getByRole('status').filter({ hasText: title });
const noteOf = (yap, id) => yap.readJson('notes.json').notes.find((n) => n.id === id);

/** Pause whatever records (no summary) and wait for the last chunk. */
async function pauseAll(yap) {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
}

// ---------------------------------------------------------------- pure helpers

base('the pure helpers: a line edit from another window, ticks, what is new since the summary', () => {
  const t = [
    { source: 'you', text: 'hello', ts: 10, startMs: 0 },
    { source: 'them', text: 'we deploy on cube and eighties', ts: 12, startMs: 2000 },
  ];
  expect(lineRef(t, 1)).toEqual({ index: 1, source: 'them', ts: 12, startMs: 2000, text: 'we deploy on cube and eighties' });
  const line = { source: 'them', text: 'We deploy on Kubernetes.', ts: 12, startMs: 2000, edited: true, original: 'we deploy on cube and eighties' };
  const next = withLineEdit(t, { index: 1, line });
  expect(next[1]).toMatchObject({ text: 'We deploy on Kubernetes.', edited: true, original: 'we deploy on cube and eighties' });
  expect(t[1].text).toBe('we deploy on cube and eighties'); // a new array
  // Not this copy's line (lines moved meanwhile): fetch the note.
  expect(withLineEdit(t, { index: 0, line })).toBeNull();
  // The bubbles say which lines were edited.
  expect(transcriptItems(next)[1].lines[0]).toMatchObject({ edited: true, original: 'we deploy on cube and eighties' });

  const md = '## Action plan\n### You\n- [ ] Draft the agenda (due: tonight)\n* [x] Share the slides\n- not a task';
  expect(taskText(md, 2)).toBe('Draft the agenda (due: tonight)');
  expect(taskText(md, 4)).toBe('');
  expect(withTaskMark(md, 2, true)).toContain('- [x] Draft the agenda');
  expect(withTaskMark(md, 3, false)).toContain('* [ ] Share the slides');
  expect(withTaskMark(md, 4, true)).toBe(md);

  const note = {
    enhancedContent: 'Plan',
    summaryTs: 100,
    transcript: [
      { source: 'you', text: 'before', ts: 90 },
      { source: 'them', text: 'after', ts: 101 },
      { source: 'you', text: '', ts: 102, dictated: true },
      { source: 'you', text: 'echo', ts: 103, echo: true },
      { source: 'you', text: 'later', ts: 104 },
    ],
  };
  expect(newSinceSummary(note)).toBe(2);
  expect(newSinceSummary({ ...note, summaryTs: 0 })).toBe(0); // an older note: not known
  expect(newSinceSummary({ ...note, enhancedContent: '' })).toBe(0);
  expect(newSinceLine(1)).toBe('1 new line since this summary');
  expect(newSinceLine(3)).toBe('3 new lines since this summary');
});

// ---------------------------------------------------------------- with the fake AI

const PLAN = (round) =>
  [
    'The team set the offsite budget and the agenda.',
    '',
    '## Action plan',
    '',
    '### You',
    '- [ ] Draft the agenda (due: tonight)',
    '',
    '### Priya',
    // The second plan words it a little differently: the tick still carries over.
    round > 1 ? '- [ ] Send the revised budget to finance (due: Friday)' : '- [ ] Send the revised budget (due: Friday)',
    '',
    '## Decisions',
    '- Hold the offsite in Lisbon',
    '',
    '## Unassigned',
    '- [ ] Book the venue',
  ].join('\n');

const TALK = [
  ['you', "Thanks for joining. Let's settle the offsite budget and the agenda today."],
  ['them', 'Priya here: I will send the revised budget by Friday.'],
  ['you', 'Great. I will draft the agenda tonight and share it.'],
  ['them', 'Someone still needs to book the venue for the offsite.'],
  ['you', 'Agreed, and we hold it in Lisbon this year.'],
];

const aiTest = base.extend({
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
        name: 'notes-edit',
        config: {
          llmScopes: {
            noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
          },
          meetingReminder: 'never',
        },
      });
    },
    { scope: 'worker' },
  ],
});

aiTest.describe('with the fake AI', () => {
  const test = aiTest;

  test.afterEach(async ({ yap }) => {
    await pauseAll(yap);
  });

  /** A new meeting note, recording, the notepad on screen. */
  async function startMeeting(yap, title) {
    const note = await yap.invoke('note_create', { title, folder: 'Meetings' });
    await yap.invoke('meeting_start', { noteId: note.id });
    await expect.poll(() => visible(yap, 'notepad')).toBe(true);
    return note;
  }

  /** Lines as if just transcribed, dated from `t0` every 15 s. */
  async function say(yap, t0, lines) {
    const segments = lines.map(([source, text], i) => ({ source, text, ts: t0 + i * 15 }));
    expect(await yap.invoke('e2e_meeting_feed', { segments })).toBe(segments.length);
  }

  test('a line fixed in the notepad is saved, marked, and teaches the dictionary', async ({ yap, main, shot }) => {
    const note = await startMeeting(yap, 'Deploy sync');
    const t0 = Math.floor(Date.now() / 1000) - 600;
    await say(yap, t0, [
      ['you', 'Where are we with the rollout?'],
      ['them', 'We deploy on cube and eighties next week.'],
    ]);
    await pauseAll(yap);
    const pad = yap.notepad;
    await pad.getByRole('tab', { name: /Transcript/ }).click();
    const log = pad.getByRole('log', { name: 'Transcript' });
    const bubble = log.locator('.bubble').filter({ hasText: 'cube and eighties' });

    // Escape puts it back, untouched.
    await bubble.dblclick();
    const box = pad.getByRole('textbox', { name: /Edit line/ });
    await expect(box).toBeFocused();
    await box.fill('Something else entirely');
    await box.press('Escape');
    await expect(box).toHaveCount(0);
    await expect(bubble).toHaveText('We deploy on cube and eighties next week.');

    // Enter saves it.
    await bubble.dblclick();
    await box.fill('We deploy on Kubernetes next week.');
    await shot(pad, '01-editing-a-line');
    await box.press('Enter');
    await expectStore(yap, 'notes.json', (s) => {
      const l = s.notes.find((n) => n.id === note.id)?.transcript?.[1];
      return l?.edited === true && l.text === 'We deploy on Kubernetes next week.';
    });
    const line = noteOf(yap, note.id).transcript[1];
    expect(line.original).toBe('We deploy on cube and eighties next week.');
    expect(line.source).toBe('them');
    expect(line.ts).toBe(t0 + 15);
    const fixed = log.locator('.bubble').filter({ hasText: 'Kubernetes' });
    await expect(fixed.locator('.lineedited')).toHaveText('edited');
    await expect(fixed.locator('.lineedited')).toHaveAttribute('title', /Yap heard: “We deploy on cube and eighties next week\.”/);

    // The misheard word, offered for the dictionary where the fix was made.
    const offer = toastIn(pad, 'Add “Kubernetes” to your dictionary?');
    await expect(offer).toBeVisible();
    await expect(offer).toContainText('Yap heard “cube and eighties”.');
    await shot(pad, '02-line-edited-dictionary-offer');
    await offer.getByRole('button', { name: 'Add', exact: true }).click();
    await expectStore(yap, 'config.json', (c) => c.dictionary?.some((e) => e.from === 'cube and eighties' && e.to === 'Kubernetes'));
    await expect(toastIn(pad, '“Kubernetes” is in your dictionary')).toBeVisible();

    // The Notes view shows the fix too (and its menu edits as well).
    await openView(main, 'Notes');
    await main.locator('.folder').filter({ hasText: 'Meetings' }).click();
    await main.locator('.item').filter({ hasText: 'Deploy sync' }).click();
    const notesLog = main.getByRole('log', { name: 'Meeting transcript' });
    await expect(notesLog).toContainText('We deploy on Kubernetes next week.');
    await expect(notesLog.locator('.bubble').filter({ hasText: 'Kubernetes' }).locator('.lineedited')).toBeVisible();
    // Keyboard: Enter on a focused line edits it.
    const first = notesLog.locator('.bubble').filter({ hasText: 'rollout' });
    await first.focus();
    await first.press('Enter');
    const notesBox = main.getByRole('textbox', { name: /Edit line/ });
    await notesBox.fill('Where are we with the rollout, Priya?');
    await notesBox.press('Enter');
    await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === note.id)?.transcript?.[0]?.text === 'Where are we with the rollout, Priya?');
    // …and the notepad follows (yap-note-changed's lineEdit).
    await expect(log).toContainText('Where are we with the rollout, Priya?');
    await shot(main, '03-notes-view-edited-lines');
  });

  test('tasks tick in the notepad, show in the Tasks view, and tick back there', async ({ yap, main, fakeLlm, shot }) => {
    let round = 0;
    fakeLlm.reply('actionPlan', () => PLAN(++round));
    const note = await startMeeting(yap, 'Offsite budget');
    const t0 = Math.floor(Date.now() / 1000) - 1800;
    await say(yap, t0, TALK);
    const pad = yap.notepad;
    await pad.getByRole('button', { name: 'Stop', exact: true }).click();
    await pad.getByRole('tab', { name: 'Summary' }).click();
    const summary = pad.getByRole('region', { name: 'Summary' });
    const agenda = summary.getByRole('checkbox', { name: /Draft the agenda/ });
    await expect(agenda).toBeVisible({ timeout: 20_000 });
    await expect(summary.getByRole('checkbox')).toHaveCount(3);

    // Ticked in the notepad: the summary's own "- [x]", not an edit.
    await agenda.check();
    await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === note.id)?.enhancedContent?.includes('- [x] Draft the agenda (due: tonight)'));
    expect(noteOf(yap, note.id).summaryEditedMs).toBeUndefined();
    await shot(pad, '04-task-ticked-in-the-notepad');

    // The Tasks view: open ones by meeting, the person's own first.
    await openView(main, 'Tasks');
    const view = main.locator('.tasksview');
    const meeting = view.locator('section').filter({ has: main.getByRole('heading', { name: 'Offsite budget' }) });
    await expect(meeting.locator('.task')).toHaveCount(2);
    await expect(meeting.locator('.ttext')).toHaveText(['Send the revised budget', 'Book the venue']);
    await expect(meeting.locator('.task').first().locator('.owner')).toHaveText('Priya');
    await expect(meeting.locator('.task').first().locator('.due')).toHaveText('Due Friday');
    await expect(meeting.locator('.task').first().locator('.moment')).toContainText('Said at 0:15');
    await expect(view.locator('.count')).toHaveText('2 open');
    await shot(main, '05-tasks-by-meeting');
    // By person: You first (nothing open), Priya, then Unassigned.
    await view.getByRole('button', { name: 'By person' }).click();
    await expect(view.locator('h2.person')).toHaveText(['Priya', 'Unassigned']);
    // Show done: the one ticked in the notepad, ticked; unticked here, it's open there.
    await view.getByLabel(/Show done/).check();
    await expect(view.locator('h2.person')).toHaveText(['You', 'Priya', 'Unassigned']);
    const mine = view.getByRole('checkbox', { name: 'Draft the agenda' });
    await expect(mine).toBeChecked();
    await shot(main, '06-tasks-by-person-with-done');
    await mine.uncheck();
    await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === note.id)?.enhancedContent?.includes('- [ ] Draft the agenda'));
    await expect(agenda).not.toBeChecked();
    // Ticked here: ticked in the notepad, and still listed (struck through) for an undo.
    await view.getByLabel(/Show done/).uncheck();
    const budget = view.getByRole('checkbox', { name: 'Send the revised budget' });
    await budget.check();
    await expect(summary.getByRole('checkbox', { name: /Send the revised budget/ })).toBeChecked();
    await expect(view.locator('.task.done .ttext')).toHaveText('Send the revised budget');

    // "Said at" opens the meeting in Notes at that line.
    await view.getByRole('button', { name: /Said at 0:30/ }).click();
    await expect(sidebarCurrent(main)).toHaveText('Notes');
    const hilited = main.getByRole('log', { name: 'Meeting transcript' }).locator('.bubble.hilite');
    await expect(hilited).toContainText('I will draft the agenda tonight');
    await expect(hilited).toBeFocused();
    await shot(main, '07-opened-at-the-moment');
  });

  test('an edited summary is kept until the person says Replace; ticks survive', async ({ yap, main, fakeLlm, shot }) => {
    let round = 0;
    fakeLlm.reply('actionPlan', () => PLAN(++round));
    const note = await startMeeting(yap, 'Budget follow-up');
    await say(yap, Math.floor(Date.now() / 1000) - 1800, TALK);
    const pad = yap.notepad;
    await pad.getByRole('button', { name: 'Stop', exact: true }).click();
    await pad.getByRole('tab', { name: 'Summary' }).click();
    const summary = pad.getByRole('region', { name: 'Summary' });
    await expect(summary.getByRole('checkbox', { name: /Send the revised budget/ })).toBeVisible({ timeout: 20_000 });
    await summary.getByRole('checkbox', { name: /Send the revised budget/ }).check();
    await expectStore(yap, 'notes.json', (s) => s.notes.find((n) => n.id === note.id)?.enhancedContent?.includes('- [x] Send the revised budget'));

    // Edited in place: the person's now.
    await summary.getByRole('button', { name: 'Edit', exact: true }).click();
    const editor = pad.getByRole('textbox', { name: 'Edit the summary (Markdown)' });
    await expect(editor).toBeFocused();
    const mine = noteOf(yap, note.id).enhancedContent.replace('Hold the offsite in Lisbon', 'Hold the offsite in Porto');
    await editor.fill(mine);
    await shot(pad, '08-editing-the-summary');
    await summary.getByRole('button', { name: 'Done' }).click();
    await expectStore(yap, 'notes.json', (s) => {
      const n = s.notes.find((x) => x.id === note.id);
      return n?.summaryEditedMs > 0 && n.enhancedContent.includes('Porto');
    });
    await expect(summary.locator('.rendered')).toContainText('Hold the offsite in Porto');
    await expect(summary.locator('.sumedited')).toHaveText('Edited');

    // Regenerating it from the Notes view asks first; Cancel keeps it, no model call.
    const plans = () => fakeLlm.requests.filter((r) => r.kind === 'actionPlan').length;
    const before = plans();
    await openView(main, 'Notes');
    await main.locator('.folder').filter({ hasText: 'Meetings' }).click();
    await main.locator('.item').filter({ hasText: 'Budget follow-up' }).click();
    await main.locator('.planbtn').click(); // "Action plan"
    const confirm = main.getByRole('alertdialog');
    await expect(confirm).toContainText('Replace your edited summary?');
    await expect(confirm).toContainText('Tasks you ticked stay ticked.');
    await shot(main, '09-replace-asks-first');
    await confirm.getByRole('button', { name: 'Cancel' }).click();
    await expect(confirm).toBeHidden();
    await main.waitForTimeout(500);
    expect(plans()).toBe(before);
    expect(noteOf(yap, note.id).enhancedContent).toContain('Porto');

    // Resumed and ended again: the end writes nothing over it ("kept"), and
    // the summary says what's new since it.
    await pad.getByRole('button', { name: 'Resume' }).click();
    await expect.poll(() => recording(yap)).toBe(true);
    // Said after the summary read the meeting (dates are whole seconds, and
    // the fake model answers at once), before the next one will.
    await main.waitForTimeout(1100);
    const later = Math.floor(Date.now() / 1000);
    expect(
      await yap.invoke('e2e_meeting_feed', {
        segments: [
          { source: 'them', text: 'One more thing: the catering quote came in.', ts: later },
          { source: 'you', text: "Fine, let's sort that next week.", ts: later },
        ],
      })
    ).toBe(2);
    await pad.getByRole('button', { name: 'Stop', exact: true }).click();
    await expect.poll(() => yap.invoke('meeting_summary_status', { noteId: note.id }).then((s) => s?.state), { timeout: 20_000 }).toBe('kept');
    expect(plans()).toBe(before);
    expect(noteOf(yap, note.id).enhancedContent).toContain('Porto');
    await pad.getByRole('tab', { name: 'Summary' }).click();
    const fresh = summary.locator('.sumnew');
    await expect(fresh).toContainText('2 new lines since this summary');
    await shot(pad, '10-new-since-this-summary');

    // Update → asked → Replace: Yap's new summary, the tick carried over.
    await fresh.getByRole('button', { name: 'Update' }).click();
    const padConfirm = pad.getByRole('alertdialog');
    await expect(padConfirm).toContainText('Replace your edited summary?');
    await padConfirm.getByRole('button', { name: 'Replace summary' }).click();
    await expectStore(yap, 'notes.json', (s) => {
      const n = s.notes.find((x) => x.id === note.id);
      return n?.enhancedContent?.includes('Lisbon') && !n.summaryEditedMs;
    });
    expect(plans()).toBe(before + 1);
    const now = noteOf(yap, note.id);
    expect(now.enhancedContent).toContain('- [x] Send the revised budget to finance (due: Friday)');
    expect(now.enhancedContent).toContain('- [ ] Draft the agenda');
    await expect(summary.getByRole('checkbox', { name: /Send the revised budget to finance/ })).toBeChecked();
    await expect(summary.locator('.sumnew')).toHaveCount(0);
    await expect(summary.locator('.sumedited')).toHaveCount(0);
    await shot(pad, '11-replaced-ticks-kept');
  });
});

/** The main window's current sidebar view. */
function sidebarCurrent(main) {
  return main.getByRole('navigation', { name: 'Main' }).locator('[aria-current="page"] .navlabel');
}

// ---------------------------------------------------------------- with the meeting's audio

const audioDir = path.join(RUNS, 'notes-edit-wavs');

// A test type of its own: its worker's Yap plays the WAVs as the meeting.
const audioTest = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'notes-edit-audio',
        env: { YAP_E2E_MEETING_AUDIO: audioDir, YAP_E2E_MEETING_SPEED: '4' },
        // "STT stub" is in every stub line: what it turns into says which pass made it.
        config: { dictionary: [{ from: 'STT stub', to: 'Heard', fuzzy: false }] },
      });
    },
    { scope: 'worker' },
  ],
});

audioTest.describe('with the meeting audio kept', () => {
  const test = audioTest;

  test.afterEach(async ({ yap }) => {
    await pauseAll(yap);
  });

  test('a line fixed in the Notes view still plays what was said, and Transcribe again keeps it', async ({ yap, main, shot }) => {
    fs.mkdirSync(audioDir, { recursive: true });
    const pauses = [
      [4.5, 4.8],
      [9.5, 9.9],
      [14.2, 14.6],
      [19.5, 19.9],
    ];
    // You talk now and then; the call talks with short pauses.
    writeWav(path.join(audioDir, 'you.wav'), { seconds: 24, gaps: [[0, 1], [8, 13], [20, 24]] });
    writeWav(path.join(audioDir, 'them.wav'), { seconds: 24, gaps: pauses });
    await newMeetingNote(main, 'Kept fix');
    const played = () => fs.readFileSync(yap.logPath, 'utf8').split('e2e: the test audio has played to the end').length - 1;
    const before = played();
    await main.getByRole('button', { name: 'Record' }).click();
    await expect.poll(played, { message: 'the test audio to play out', timeout: 30_000 }).toBeGreaterThan(before);
    await main.getByRole('button', { name: 'Pause recording' }).click();
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
    await expectStore(yap, 'notes.json', (s) => {
      const t = meetingNote(s, 'Kept fix')?.transcript || [];
      return t.some((l) => l.source === 'them' && l.startMs != null) && t.some((l) => l.source === 'you' && l.text);
    });
    const noteId = meetingNote(yap.readJson('notes.json'), 'Kept fix').id;
    const first = meetingNote(yap.readJson('notes.json'), 'Kept fix').transcript;
    const at = first.findIndex((s) => s.source === 'them' && s.startMs != null);
    const old = first[at];
    const clip = (seg) =>
      main.evaluate(
        (args) => window.__TAURI_INTERNALS__.invoke('meeting_audio_clip', args).then((b) => b.byteLength),
        { noteId, ts: seg.ts, startMs: seg.startMs, endMs: seg.endMs }
      );
    const bytes = await clip(old);
    expect(bytes).toBeGreaterThan(44);

    // Edit line, from the line's own menu.
    const log = main.getByRole('log', { name: 'Meeting transcript' });
    const bubble = log.locator('.bubble.them').filter({ hasText: old.text }).first();
    await bubble.click({ button: 'right' });
    await main.getByRole('menuitem', { name: 'Edit line' }).click();
    const box = main.getByRole('textbox', { name: /Edit line/ });
    await box.fill('Budget call with Priya about the venue.');
    await box.press('Enter');
    await expectStore(yap, 'notes.json', (s) => meetingNote(s, 'Kept fix')?.transcript?.[at]?.edited === true);
    const fixed = meetingNote(yap.readJson('notes.json'), 'Kept fix').transcript[at];
    expect(fixed).toMatchObject({ text: 'Budget call with Priya about the venue.', original: old.text, startMs: old.startMs, endMs: old.endMs, ts: old.ts });
    // ▶ still plays what was said: the same stretch of the same audio.
    const edited = log.locator('.bubble').filter({ hasText: 'Budget call with Priya' });
    await edited.hover();
    await expect(edited.getByRole('button', { name: 'Play this line' })).toBeVisible();
    expect(await clip(fixed)).toBe(bytes);
    await shot(main, '12-edited-line-still-plays');

    // Transcribe again (a new spelling marks the new lines): the fix stays as written.
    const cfg = await yap.invoke('get_config');
    const patch = { dictionary: [{ from: 'STT stub', to: 'Again', fuzzy: false, snippet: false }] };
    await yap.invoke('save_config', { cfg: { ...cfg, ...patch } });
    await main.evaluate((p) => window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: p })), patch);
    await main.waitForTimeout(700);
    await main.locator('.item').filter({ hasText: 'Kept fix' }).click({ button: 'right' });
    await main.getByRole('menuitem', { name: /Transcribe again/ }).click();
    const confirm = main.getByRole('alertdialog');
    await expect(confirm).toContainText('lines you edited stay as you wrote them');
    await confirm.getByRole('button', { name: 'Transcribe again' }).click();
    await expectStore(
      yap,
      'notes.json',
      (s) => (meetingNote(s, 'Kept fix')?.transcript || []).some((l) => l.text.startsWith('[Again:')),
      'the meeting transcribed again'
    );
    const again = meetingNote(yap.readJson('notes.json'), 'Kept fix').transcript;
    const kept = again.filter((l) => l.edited);
    expect(kept).toHaveLength(1);
    expect(kept[0]).toMatchObject({ text: 'Budget call with Priya about the venue.', startMs: old.startMs, endMs: old.endMs });
    expect(again.some((l) => l.text.startsWith('[Heard:'))).toBe(false);
    // No new line of its side says it again (at most a sliver of overlap).
    for (const l of again.filter((x) => x.source === 'them' && !x.edited && x.startMs != null && x.ts - old.ts < 60)) {
      const shared = Math.min(l.endMs, old.endMs) - Math.max(l.startMs, old.startMs);
      expect(shared * 2, JSON.stringify(l)).toBeLessThan(Math.min(l.endMs - l.startMs, old.endMs - old.startMs));
    }
    await expect(log).toContainText('Budget call with Priya about the venue.');
    await expect(log).toContainText('[Again: received');
    await shot(main, '13-transcribed-again-fix-kept');
  });
});
