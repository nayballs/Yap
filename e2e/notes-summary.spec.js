// A meeting's summary and its Ask chat, end to end (src-tauri/src/meetings/
// meeting_summary/language.rs, notes/note_edits.rs, chat/note_chats.rs,
// meetings/meeting_end.rs; src/lib/core/summaryTools.js, noteChat.js):
// - the summary asked for in the meeting's language: a French meeting's
//   action plan carries the line that keeps it French and its headings in
//   English; an English one nothing; the layout opens with a recap and
//   gives each decision its reason;
// - the Ask thread kept with the meeting: asked in the notepad, there after
//   a restart in the notepad and in the note in Notes, a turn in Notes
//   followed by the notepad, and deleted with the meeting;
// - "Back to Yap's summary" after an edit, with Undo;
// - 👍 / 👎 and "What was wrong?", kept in the note and never sent;
// - Ctrl+F in a summary;
// - "Copy for email": rich HTML and plain text on the clipboard at once;
// - "Your summary is ready" when no Yap window is in front: a card on the
//   Yap bar whose Open note opens it in Notes; nothing with Yap in front;
// - exports without echo lines.
// The AI is a local fake (support/fake-llm.js); no audio device is opened.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView, expectStore, inAnotherApp } from './support/fixtures.js';
import { launchYap } from './support/yap.js';
import { startFakeLlm } from './support/fake-llm.js';
import { canRestore, applySummaryState } from '../src/lib/core/summaryState.js';
import { threadMessages, keptMessage, pastChatItems } from '../src/lib/core/noteChat.js';

const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const noteIn = (store, id) => store?.notes?.find((n) => n.id === id);
const tab = (pad, name) => pad.getByRole('tab', { name });
const askSheet = (pad) => pad.getByRole('dialog', { name: 'Ask about this meeting' });
const askInput = (pad) => pad.getByRole('textbox', { name: 'Ask anything' });
const stopButton = (pad) => pad.getByRole('button', { name: 'Stop', exact: true });

const config = (fakeLlm) => ({
  llmScopes: {
    noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
  },
  meetingReminder: 'never',
});

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
      await use({ name: 'notes-summary', config: config(fakeLlm) });
    },
    { scope: 'worker' },
  ],
});

/** Pause whatever records (no summary) and wait for the last chunk. */
async function pauseAll(yap) {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
}

/** A new meeting note, recording, the notepad on screen. */
async function startMeeting(yap, title) {
  const note = await yap.invoke('note_create', { title, folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  return note;
}

/** Lines as if just transcribed (`[source, text, echo?]`), 15 s apart from half an hour ago. */
async function say(yap, lines) {
  const t0 = Math.floor(Date.now() / 1000) - 1800;
  const segments = lines.map(([source, text, echo], i) => ({ source, text, ts: t0 + i * 15, ...(echo ? { echo: true } : {}) }));
  expect(await yap.invoke('e2e_meeting_feed', { segments })).toBe(segments.length);
}

const TALK = [
  ['you', "Thanks for joining. Let's settle the offsite budget and the agenda today."],
  ['them', 'Priya here: I will send the revised budget by Friday.'],
  ['you', 'Great. I will draft the agenda tonight and share it.'],
  ['them', 'Someone still needs to book the venue for the offsite.'],
  ['you', 'Agreed, and we hold it in Lisbon this year because the flights are cheaper.'],
];

const FRENCH = [
  ['you', 'Bonjour à tous. On fait le point sur le salon de Lyon : le budget, le traiteur et les chambres.'],
  ['them', 'Alice ici. Le devis du traiteur est arrivé hier et il est beaucoup trop cher pour nous.'],
  ['you', 'Alors on garde le lieu, mais on change de traiteur, parce que ce devis dépasse le budget.'],
  ['them', "Alice encore. D'accord, je vais demander deux autres devis avant vendredi."],
  ['you', "Merci Alice. De mon côté, j'envoie le plan de salle à toute l'équipe demain."],
];

const PLAN = [
  '## Recap',
  'The team set the offsite budget and the agenda.',
  '- Budget',
  '- Venue',
  '',
  '## Action plan',
  '',
  '### You',
  '- [ ] Draft the agenda (due: tonight)',
  '',
  '### Priya',
  '- [ ] Send the revised budget (due: Friday)',
  '',
  '## Decisions',
  '- Hold the offsite in Lisbon — the flights are cheaper',
  '',
  '## Unassigned',
  '- [ ] Book the venue',
].join('\n');

// ---------------------------------------------------------------- pure helpers

base('the pure helpers: Back to Yap’s summary, a kept thread, the email’s two formats', () => {
  const yaps = '## Action plan\n### You\n- [ ] Draft the agenda';
  // Offered once the person edited Yap's summary, not for a tick.
  expect(canRestore({ summaryEditedMs: 5, summaryAi: yaps, enhancedContent: '## Action plan\n### You\n- [ ] Draft it' })).toBe(true);
  expect(canRestore({ summaryEditedMs: 5, summaryAi: yaps, enhancedContent: yaps.replace('[ ]', '[x]') })).toBe(false);
  expect(canRestore({ summaryEditedMs: 0, summaryAi: yaps, enhancedContent: 'x' })).toBe(false);
  expect(canRestore({ summaryEditedMs: 5, summaryAi: '', enhancedContent: 'x' })).toBe(false);
  const note = { enhancedContent: 'a', summaryEditedMs: 1, summaryAi: 'b', summaryRating: { up: true, ms: 1 } };
  applySummaryState(note, { enhancedContent: 'c', summaryEditedMs: 0, summaryTs: 9, summaryAi: '', summaryRating: null });
  expect(note).toEqual({ enhancedContent: 'c', summaryEditedMs: 0, summaryTs: 9, summaryAi: '', summaryRating: null });

  // A kept thread as the windows show it, and back.
  const conv = { id: 7, messages: [{ role: 'user', text: 'Follow-up email' }, { role: 'assistant', text: 'Subject: x', plain: true, model: { provider: 'groq', id: 'm' } }] };
  const shown = threadMessages(conv, 'np');
  expect(shown.map((m) => m.id)).toEqual(['np7-0', 'np7-1']);
  expect(shown[1]).toMatchObject({ role: 'assistant', plain: true, stopped: false, model: { provider: 'groq', id: 'm' } });
  expect(keptMessage({ ...shown[1], revisedFrom: 120, streaming: false })).toEqual({ role: 'assistant', text: 'Subject: x', plain: true, model: { provider: 'groq', id: 'm' } });
  expect(keptMessage({ role: 'user', text: 'Hi' })).toEqual({ role: 'user', text: 'Hi' });
  const items = pastChatItems([{ id: 2, title: 'Who owns the budget?', updatedTs: 1 }, { id: 1, title: 'Follow-up email', updatedTs: 1 }], 2, () => {});
  expect(items.map((i) => [i.label.split(' · ')[0], i.checked])).toEqual([['Who owns the budget?', true], ['Follow-up email', false]]);
  expect(pastChatItems([], null, () => {})).toEqual([]);
});

// ---------------------------------------------------------------- with the fake AI

test.describe('with the fake AI', () => {
  test.afterEach(async ({ yap }) => {
    await pauseAll(yap);
    await yap.invoke('e2e_focus', { focused: true });
  });

  test('a French meeting is summarised in French, its headings in English; an English one as before', async ({ yap, fakeLlm }) => {
    const plans = () => fakeLlm.requests.filter((r) => r.kind === 'actionPlan');
    const before = plans().length;
    await startMeeting(yap, 'Salon de Lyon');
    await say(yap, FRENCH);
    await stopButton(yap.notepad).click();
    await expect.poll(() => plans().length, { timeout: 20_000 }).toBe(before + 1);
    const fr = plans().at(-1);
    // The meeting, then the line, last: French, the headings in English.
    expect(fr.user).toContain('traiteur');
    expect(fr.user.trimEnd()).toMatch(/Write the notes in French, the language of this meeting\. Keep these in English exactly as written: the headings \("## Recap", "## Action plan", "### You", "## Decisions", "## Open questions", "## Unassigned"\) and "\(due: …\)"\. Names stay as they are\.$/);
    // The layout: a recap first, each decision with its reason.
    expect(fr.system).toContain('Start with "## Recap"');
    expect(fr.system).toContain('why in a few words, only when a reason was said');

    const english = await startMeeting(yap, 'Budget follow-up');
    await say(yap, TALK);
    await stopButton(yap.notepad).click();
    await expect.poll(() => plans().length, { timeout: 20_000 }).toBe(before + 2);
    const en = plans().at(-1);
    expect(en.user).toContain('revised budget');
    expect(en.user).not.toContain('Write the notes in');
    await expectStore(yap, 'notes.json', (s) => !!noteIn(s, english.id)?.enhancedContent);
  });

  test('Back to Yap’s summary with Undo, 👍 👎 kept on this PC, Ctrl+F, and copy for email', async ({
    yap,
    main,
    fakeLlm,
    shot,
  }) => {
    fakeLlm.reply('actionPlan', () => PLAN);
    const note = await startMeeting(yap, 'Offsite planning');
    await say(yap, TALK);
    const pad = yap.notepad;
    await stopButton(pad).click();
    await tab(pad, 'Summary').click();
    const summary = pad.getByRole('region', { name: 'Summary' });
    await expect(summary.locator('.rendered')).toContainText('Hold the offsite in Lisbon', { timeout: 20_000 });
    await expectStore(yap, 'notes.json', (s) => noteIn(s, note.id)?.summaryAi?.includes('Lisbon'));
    const back = summary.getByRole('button', { name: "Back to Yap's summary" });
    await expect(back).toHaveCount(0);

    // Edited: Yap's version is kept beside it, and offered back.
    await summary.getByRole('button', { name: 'Edit', exact: true }).click();
    const editor = pad.getByRole('textbox', { name: 'Edit the summary (Markdown)' });
    await editor.fill(PLAN.replace('Hold the offsite in Lisbon', 'Hold the offsite in Porto'));
    await summary.getByRole('button', { name: 'Done' }).click();
    await expectStore(yap, 'notes.json', (s) => noteIn(s, note.id)?.summaryEditedMs > 0 && noteIn(s, note.id).enhancedContent.includes('Porto'));
    await expect(back).toBeVisible();
    await shot(pad, '01-back-to-yaps-summary');
    await back.click();
    await expect(summary.locator('.rendered')).toContainText('Hold the offsite in Lisbon');
    await expectStore(yap, 'notes.json', (s) => noteIn(s, note.id)?.enhancedContent.includes('Lisbon') && !noteIn(s, note.id).summaryEditedMs);
    await expect(back).toHaveCount(0);
    const restored = pad.getByRole('status').filter({ hasText: "Back to Yap's summary" });
    await expect(restored).toBeVisible();
    await shot(pad, '02-restored-with-undo');
    // Undo: the person's version back, theirs again.
    await restored.getByRole('button', { name: 'Undo' }).click();
    await expect(summary.locator('.rendered')).toContainText('Hold the offsite in Porto');
    await expectStore(yap, 'notes.json', (s) => noteIn(s, note.id)?.enhancedContent.includes('Porto') && noteIn(s, note.id).summaryEditedMs > 0);
    await expect(back).toBeVisible();
    await back.click();
    await expect(summary.locator('.rendered')).toContainText('Hold the offsite in Lisbon');

    // 👎 and what was wrong: kept in the note, nothing sent.
    const sent = fakeLlm.requests.length;
    await summary.getByRole('button', { name: 'Bad summary' }).click();
    const why = summary.getByRole('textbox', { name: /What was wrong/ });
    await expect(why).toBeFocused();
    await why.fill('Missed the catering task');
    await shot(pad, '03-what-was-wrong');
    await summary.getByRole('button', { name: 'Save', exact: true }).click();
    await expectStore(yap, 'notes.json', (s) => {
      const r = noteIn(s, note.id)?.summaryRating;
      return r?.up === false && r.why === 'Missed the catering task';
    });
    await expect(summary.getByRole('button', { name: 'Bad summary' })).toHaveAttribute('aria-pressed', 'true');
    await expect(why).toHaveCount(0);
    expect(fakeLlm.requests.length).toBe(sent);

    // Ctrl+F: every match lit, the one it's on counted, Esc puts it away.
    await pad.keyboard.press('Control+f');
    const find = pad.getByRole('search', { name: 'Find in the summary' });
    const box = find.getByRole('textbox', { name: 'Find in the summary' });
    await expect(box).toBeFocused();
    await box.fill('the');
    await expect(find).toContainText(/1 of [2-9]/);
    const lit = () => pad.evaluate(() => CSS.highlights.get('yap-find')?.size ?? 0);
    expect(await lit()).toBeGreaterThan(1);
    await box.press('Enter');
    await expect(find).toContainText(/2 of /);
    await shot(pad, '04-find-in-the-summary');
    await box.fill('nothing like this');
    await expect(find).toContainText('No matches');
    await box.press('Escape');
    await expect(find).toHaveCount(0);
    expect(await lit()).toBe(0);

    // Copy for email: HTML and plain text, together.
    await pad.evaluate(() => {
      window.__clip = [];
      navigator.clipboard.write = async (items) => {
        for (const item of items) for (const type of item.types) window.__clip.push([type, await (await item.getType(type)).text()]);
      };
    });
    await pad.getByRole('button', { name: 'More', exact: true }).click();
    await pad.getByRole('menuitem', { name: 'Copy summary for email' }).click();
    await expect.poll(() => pad.evaluate(() => window.__clip.length)).toBe(2);
    const clip = Object.fromEntries(await pad.evaluate(() => window.__clip));
    expect(clip['text/html']).toMatch(/^<div style="font-family/);
    expect(clip['text/html']).toContain('>Offsite planning</h2>');
    expect(clip['text/html']).toMatch(/<h3 style="[^"]+">Action plan<\/h3>/);
    expect(clip['text/html']).toContain('☐ Draft the agenda (due: tonight)');
    expect(clip['text/html']).not.toContain('<input');
    expect(clip['text/plain'].split('\n')[0]).toBe('Offsite planning');
    expect(clip['text/plain']).toContain('☐ Send the revised budget (due: Friday)');

    // The Notes view shows the rating, and a 👍 there is the note's (the notepad follows).
    await openView(main, 'Notes');
    await main.locator('.folder').filter({ hasText: 'Meetings' }).click();
    await main.locator('.item').filter({ hasText: 'Offsite planning' }).click();
    await main.locator('.tabs').getByRole('button', { name: 'Summary' }).click();
    await expect(main.getByRole('button', { name: 'Bad summary' })).toHaveAttribute('aria-pressed', 'true');
    await expect(main.locator('.why-said')).toHaveText('“Missed the catering task”');
    await main.getByRole('button', { name: 'Good summary' }).click();
    await expectStore(yap, 'notes.json', (s) => noteIn(s, note.id)?.summaryRating?.up === true && !noteIn(s, note.id).summaryRating.why);
    await expect(summary.getByRole('button', { name: 'Good summary' })).toHaveAttribute('aria-pressed', 'true');
    await expect(main.getByRole('button', { name: 'Copy for email' })).toBeVisible();
    await shot(main, '05-notes-rated-and-copy-for-email');
    // Pressing it again takes it back.
    await main.getByRole('button', { name: 'Good summary' }).click();
    await expectStore(yap, 'notes.json', (s) => !noteIn(s, note.id)?.summaryRating);
  });

  test('“Your summary is ready” on the Yap bar with no Yap window in front; Open note opens it', async ({
    yap,
    main,
    fakeLlm,
    shot,
  }) => {
    fakeLlm.reply('actionPlan', () => PLAN);
    const note = await startMeeting(yap, 'Supplier call');
    await say(yap, TALK);
    // In another app (the call): the meeting stops from the bar.
    await inAnotherApp(yap);
    await yap.invoke('meeting_end', { origin: 'overlay' });
    const card = yap.overlay.getByRole('status').filter({ hasText: 'Your summary is ready' });
    await expect(card).toBeVisible({ timeout: 20_000 });
    await expect(card).toContainText('“Supplier call” · 3 tasks');
    await yap.overlay.waitForTimeout(300);
    await shot(yap.overlay, '06-summary-ready-on-the-bar');
    await card.getByRole('button', { name: 'Open note' }).click();
    await expect(main.getByPlaceholder('Untitled Note')).toHaveValue('Supplier call', { timeout: 10_000 });
    await expect(card).toHaveCount(0);

    // With Yap in front: the window shows it itself, so no card.
    await yap.invoke('e2e_focus', { focused: true });
    const runs = fakeLlm.requests.filter((r) => r.kind === 'actionPlan').length;
    await yap.invoke('meeting_summarise', { noteId: note.id, replace: true });
    await expect.poll(() => fakeLlm.requests.filter((r) => r.kind === 'actionPlan').length, { timeout: 20_000 }).toBe(runs + 1);
    await expect.poll(() => yap.invoke('meeting_summary_status', { noteId: note.id }).then((s) => s?.state)).toBe('done');
    await yap.overlay.waitForTimeout(800);
    await expect(card).toHaveCount(0);
  });

  test('exports leave out echo lines', async ({ yap }) => {
    const note = await startMeeting(yap, 'Echo check');
    await say(yap, [
      ['you', 'Morning, everyone.'],
      ['them', 'Hi there, ECHO-CHECK.'],
      ['you', 'Hi there, ECHO-CHECK.', true],
      ['them', 'Legal needs a week.'],
    ]);
    await pauseAll(yap);
    const file = path.join(yap.runDir, 'echo-check.md');
    await yap.invoke('note_export', { id: note.id, path: file });
    const md = fs.readFileSync(file, 'utf8');
    expect(md).toContain('**Them:** Legal needs a week.');
    expect(md.match(/ECHO-CHECK/g)).toHaveLength(1);
    expect(md).not.toMatch(/\*\*You:\*\* Hi there/);
  });
});

// ---------------------------------------------------------------- the Ask thread, kept

test('the Ask thread is kept with the meeting: after a restart, in the notepad and in Notes; deleted with it', async ({ fakeLlm, shot }) => {
  test.setTimeout(180_000);
  const options = { name: 'notes-summary-thread', config: config(fakeLlm) };
  const first = await launchYap(options);
  let second = null;
  try {
    const note = await startMeeting(first, 'Thread keeper');
    await say(first, TALK);
    const pad = first.notepad;
    await askInput(pad).fill('Who owns the budget?');
    await askInput(pad).press('Enter');
    const sheet = askSheet(pad);
    await expect(sheet).toContainText('Alice owns the budget');
    await expectStore(first, 'chats.json', (s) => s.conversations.some((c) => c.note === note.id && c.messages.length === 2));
    // Never in Chat's list.
    expect(await first.invoke('chats_list')).toEqual([]);
    await pauseAll(first);
    await first.stop();

    // The next start: the thread is there when the meeting opens again.
    second = await launchYap({ ...options, restart: true });
    const yap = second;
    await yap.invoke('e2e_main_on_screen');
    await yap.invoke('notepad_open', { noteId: note.id });
    await expect.poll(() => visible(yap, 'notepad')).toBe(true);
    const pad2 = yap.notepad;
    await askInput(pad2).focus();
    const sheet2 = askSheet(pad2);
    await expect(sheet2.locator('.mq')).toHaveText(['Who owns the budget?']);
    await expect(sheet2).toContainText('Alice owns the budget');
    await shot(pad2, '07-thread-after-a-restart');

    // And in the note in Notes, where a helper's turn joins the same thread…
    const m = yap.main;
    await openView(m, 'Notes');
    await m.locator('.folder').filter({ hasText: 'Meetings' }).click();
    await m.locator('.item').filter({ hasText: 'Thread keeper' }).click();
    await expect(m.locator('.chatthread')).toContainText('Alice owns the budget');
    await m.getByRole('button', { name: 'Follow-up email' }).click();
    await expect(m.locator('.chatthread')).toContainText('Subject: Q3 budget follow-up');
    await expectStore(yap, 'chats.json', (s) => s.conversations.filter((c) => c.note === note.id).length === 1 && s.conversations[0].messages.length === 4);
    // …which the notepad follows.
    await expect(sheet2.locator('.mq')).toHaveText(['Who owns the budget?', 'Follow-up email']);
    await shot(m, '08-thread-in-notes');

    // New chat: another thread; the first under Past chats.
    await sheet2.getByRole('button', { name: 'New chat' }).click();
    await askInput(pad2).fill('Who books the venue?');
    await askInput(pad2).press('Enter');
    await expect(sheet2.locator('.mq')).toHaveText(['Who books the venue?']);
    await expectStore(yap, 'chats.json', (s) => s.conversations.filter((c) => c.note === note.id).length === 2);
    await sheet2.getByRole('button', { name: 'Past chats' }).click();
    await pad2.getByRole('menuitemcheckbox', { name: /^Who owns the budget\?/ }).click();
    await expect(sheet2.locator('.mq')).toHaveText(['Who owns the budget?', 'Follow-up email']);

    // Deleting the meeting deletes its threads.
    await yap.invoke('meeting_delete', { noteId: note.id });
    await expectStore(yap, 'chats.json', (s) => !s.conversations.some((c) => c.note === note.id));
  } finally {
    await first.stop();
    await second?.stop();
  }
});
