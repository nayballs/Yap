// Routines (desktop/src-tauri/src/chat/routines.rs): Skills that run by
// themselves, on this PC while Yap runs.
// - the wording (core/routines.js): when each runs, where it goes, its runs;
// - Settings → Skills → Routines: a routine made in the form (a Skill, on
//   weekdays at a time), Run now — the run waits its turn, runs, and the
//   runs list opens its chat: a chat of its own, named after the Skill and
//   marked in Chat's list; a second run goes to the same chat; the calls are
//   counted as Routines; light and dark;
// - after a meeting: a fake meeting recorded and ended, its summary written,
//   then its "after a meeting" routine runs on it — with the meeting's note
//   (a new Ask thread on it), one quiet card on the Yap bar with Yap in
//   another app — and one for another folder doesn't.
// The AI is the local fake (support/fake-llm.js); the meetings are fictional.
import { test as base, expect, openView, openSettings, closeSettings, closeToasts, setConfig, expectStore, inAnotherApp, sidebar } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { triggerLine, routineLine, runState, causeLine, runWhy, runResult, capLine, toSave, canGoWithNote, cleanTime, when, skillOptions, routineProblem } from '../src/lib/core/routines.js';

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;

const NOTES = {
  folders: ['Personal', 'Meetings', 'Clients'],
  notes: [
    {
      id: 1,
      title: 'Supplier call',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      createdTs: NOW - 2 * DAY,
      updatedTs: NOW - 2 * DAY,
      participants: ['Priya Shah'],
      enhancedContent: '## Recap\nThe packaging quote came in over budget.\n\n## Action plan\n### You\n- [ ] Draft the finance request (due: Wednesday)',
      transcript: [
        { source: 'you', text: 'Priya, is the quote final now?', ts: NOW - 2 * DAY },
        { source: 'them', text: 'Yes, it is final. I will send the revised quote on Friday.', ts: NOW - 2 * DAY + 20 },
      ],
    },
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
        name: 'routines',
        config: {
          llmScopes: {
            noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
            chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' },
          },
          meetingReminder: 'never',
          configVersion: 7,
        },
        data: { 'notes.json': NOTES },
      });
    },
    { scope: 'worker' },
  ],
});

const routinesGroup = (dialog) => dialog.getByRole('group', { name: 'Routines' });
const runsGroup = (dialog) => dialog.getByRole('group', { name: 'Recent runs' });
const list = (yap) => yap.invoke('routines_list');
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });

async function deleteAll(yap) {
  for (const r of (await list(yap)).routines) await yap.invoke('routine_delete', { id: r.id });
}

test('the wording: when each routine runs, where it goes, and how its runs read', async () => {
  expect(cleanTime('9:05')).toBe('09:05');
  expect(cleanTime('24:00')).toBeNull();
  expect(triggerLine({ kind: 'schedule', days: 'weekdays', time: '09:00' })).toBe('Weekdays at 09:00');
  expect(triggerLine({ kind: 'schedule', days: 'daily', time: '7:30' })).toBe('Every day at 07:30');
  expect(triggerLine({ kind: 'schedule', days: 'weekly', weekday: 4, time: '16:00' })).toBe('Fridays at 16:00');
  expect(triggerLine({ kind: 'meetingEnd', folder: '', outside: false })).toBe('After each meeting');
  expect(triggerLine({ kind: 'meetingEnd', folder: 'Clients', outside: false })).toBe('After meetings in Clients');
  expect(triggerLine({ kind: 'meetingEnd', folder: '', outside: true })).toBe('After meetings with people from outside');
  expect(triggerLine({ kind: 'meetingEnd', folder: 'Clients', outside: true })).toBe('After meetings in Clients with people from outside');
  expect(triggerLine({ kind: 'beforeEvent', minutes: 10 })).toBe('10 min before each meeting');

  const now = new Date(2026, 10, 16, 8, 0).getTime() / 1000;
  const at = (d, h, m) => new Date(2026, 10, d, h, m).getTime() / 1000;
  expect(when(at(16, 9, 0), now)).toBe('Today, 09:00');
  expect(when(at(17, 9, 0), now)).toBe('Tomorrow, 09:00');
  expect(when(at(15, 16, 0), now)).toBe('Yesterday, 16:00');
  expect(when(at(20, 16, 0), now)).toBe('Fri 20 Nov, 16:00');
  const r = { trigger: { kind: 'schedule', days: 'weekdays', time: '09:00' }, to: 'chat', on: true, next: at(16, 9, 0) };
  expect(routineLine(r, now)).toBe('Weekdays at 09:00 · In its own chat · Next: today, 09:00');
  expect(routineLine({ ...r, on: false }, now)).toBe('Weekdays at 09:00 · In its own chat · Off');
  expect(routineLine({ trigger: { kind: 'meetingEnd' }, to: 'note', on: true }, now)).toBe("After each meeting · With the meeting's note");
  expect(routineLine({ trigger: { kind: 'beforeEvent', minutes: 5 }, on: true }, now)).toBe('5 min before each meeting · In its own chat · No meeting coming up');
  expect(routineLine({ ...r, skillGone: true }, now)).toBe('Its Skill was deleted: edit it to pick another.');

  expect(runState({ state: 'done' })).toEqual({ label: 'Ran', tone: 'ok' });
  expect(runState({ state: 'done', cause: 'missed' }).label).toBe('Ran late');
  expect(runState({ state: 'failed' }).tone).toBe('bad');
  expect(runState({ state: 'skipped' }).label).toBe('Skipped');
  expect(runState({ state: 'waiting' }).label).toBe('Waiting');
  expect(causeLine({ cause: 'meeting', about: 'Supplier call' })).toBe('After “Supplier call”');
  expect(causeLine({ cause: 'event', about: 'Weekly sync' })).toBe('Before “Weekly sync”');
  expect(causeLine({ cause: 'missed', slot: at(16, 9, 0) })).toBe('Missed 09:00');
  expect(causeLine({ cause: 'now' })).toBe('Run now');
  expect(runWhy({ state: 'done', cause: 'missed', slot: at(16, 9, 0) })).toBe("Yap wasn't running at 09:00, so it ran when Yap found it.");
  expect(runWhy({ state: 'skipped', why: 'Today’s 20 runs are done.' })).toBe('Today’s 20 runs are done.');
  expect(runResult({ state: 'done', chat: 4 })).toEqual({ chat: 4 });
  expect(runResult({ state: 'done', note: 2 })).toEqual({ note: 2 });
  expect(runResult({ state: 'failed', chat: 4 })).toBeNull();
  expect(capLine(3)).toBe('3 of 20 runs today.');
  expect(capLine(20)).toBe("Today's 20 runs are done: routines run again tomorrow.");

  // With the meeting's note: only after a meeting, for a Skill on one meeting.
  const followup = { id: 'builtin:followup-email', command: 'followup-email', name: 'Follow-up email', runsOn: 'meeting', builtin: true };
  const todos = { id: 'builtin:todos', command: 'todos', name: 'To-dos', runsOn: 'meetings', builtin: true };
  const mine = { id: 's1', command: 'client-recap', name: 'Client recap', runsOn: 'both' };
  expect(canGoWithNote({ kind: 'meetingEnd' }, followup)).toBe(true);
  expect(canGoWithNote({ kind: 'meetingEnd' }, todos)).toBe(false);
  expect(canGoWithNote({ kind: 'schedule' }, followup)).toBe(false);
  const skills = [followup, todos, mine];
  expect(toSave({ skill: 'builtin:todos', trigger: { kind: 'meetingEnd', folder: 'Clients' }, to: 'note', words: ' x ' }, skills)).toEqual({
    id: '',
    skill: 'builtin:todos',
    words: 'x',
    trigger: { kind: 'meetingEnd', folder: 'Clients', outside: false },
    on: true,
    to: 'chat',
  });
  expect(routineProblem({ skill: '', trigger: { kind: 'schedule', time: '09:00' } }, skills)).toBe('Pick a Skill for it to run.');
  expect(routineProblem({ skill: 's1', trigger: { kind: 'schedule', time: '9' } }, skills)).toBe('Give it a time of day, like 09:00.');
  expect(routineProblem({ skill: 's1', trigger: { kind: 'beforeEvent', minutes: 10 } }, skills)).toBe('');
  expect(skillOptions(skills).map((o) => [o.label, o.group])).toEqual([
    ['/client-recap · Client recap', 'Your Skills'],
    ['/followup-email · Follow-up email', "Yap's Skills"],
    ['/todos · To-dos', "Yap's Skills"],
  ]);
});

test('a routine made in Settings → Skills runs on Run now, in a chat of its own marked in the list; again in the same chat; light and dark', async ({ yap, main, fakeLlm, shot }) => {
  await deleteAll(yap);
  await main.setViewportSize({ width: 1200, height: 860 });
  fakeLlm.reply('other', () => '- Draft the finance request, due Wednesday [1].');
  fakeLlm.usage('other', { usage: { prompt_tokens: 900, completion_tokens: 40, total_tokens: 940 } });
  const dialog = await openSettings(main, 'Skills');
  const group = routinesGroup(dialog);
  await expect(group).toBeVisible();
  await expect(group).toContainText('No routines yet.');

  // The form: a Skill, on a schedule (weekdays at 09:00 to start with).
  await group.getByRole('button', { name: 'New routine' }).click();
  const form = dialog.getByRole('group', { name: 'New routine' });
  await expect(form).toBeVisible();
  await expect(form.getByRole('button', { name: 'Save routine' })).toBeDisabled();
  await expect(form.getByRole('status')).toHaveText('Pick a Skill for it to run.');
  await form.getByRole('combobox', { name: 'Routine Skill' }).selectOption('builtin:todos');
  await form.getByRole('combobox', { name: 'Time of day' }).selectOption('08:30');
  // After a meeting, a Skill across meetings: no "with the note" to pick.
  await form.getByRole('tab', { name: 'After a meeting' }).click();
  await expect(form.getByRole('combobox', { name: 'Meetings in folder' })).toBeVisible();
  await expect(form.getByText('Where it goes')).toHaveCount(0);
  await form.getByRole('tab', { name: 'On a schedule' }).click();
  await expect(form.getByRole('combobox', { name: 'Time of day' })).toHaveValue('08:30');
  await shot(main, '01-routine-form-light');
  await form.getByRole('button', { name: 'Save routine' }).click();
  await expect(form).toBeHidden();
  const row = group.locator('[data-routine]');
  await expect(row).toHaveCount(1);
  await expect(row).toContainText('/todos');
  await expect(row).toContainText(/Weekdays at 08:30 · In its own chat · Next: /);
  await expectStore(yap, 'routines.json', (j) => j.routines?.length === 1 && j.routines[0].skill === 'builtin:todos' && j.routines[0].trigger.time === '08:30' && j.routines[0].trigger.days === 'weekdays', 'the routine to be saved');
  await closeToasts(main);

  // Run now: waits its turn, runs, and the runs list has it, with its chat.
  const before = fakeLlm.requests.length;
  await row.getByRole('button', { name: 'Run /todos now' }).click();
  const runs = runsGroup(dialog);
  const run = runs.locator('[data-run]').first();
  await expect(run).toHaveAttribute('data-state', 'done', { timeout: 30_000 });
  await expect(run).toContainText('/todos');
  await expect(run).toContainText('Ran');
  await expect(run).toContainText('Run now');
  await expect(runs).toContainText('1 of 20 runs today.');
  // Asked as the Skill, on the chat's own model.
  const asked = fakeLlm.requests.slice(before).filter((r) => r.kind === 'other' && r.model === 'fake-chat');
  expect(asked.length).toBeGreaterThan(0);
  expect(JSON.stringify(asked.at(-1).messages)).toContain('/todos');
  // …nothing that acts with nobody there among its tools.
  const offered = (asked.at(-1).tools || []).map((t) => t.function?.name ?? t.name);
  for (const name of ['open_app', 'look_at_screen', 'ask_user', 'start_agent']) expect(offered).not.toContain(name);
  const listed = await list(yap);
  const chatId = listed.runs[0].chat;
  expect(chatId).toBeGreaterThan(0);
  expect(listed.routines[0].chat).toBe(chatId);
  // Its calls counted as Routines (ai/usage.rs).
  await expect.poll(async () => JSON.stringify(await yap.invoke('ai_usage_summary')).includes('"routines"')).toBe(true);
  await shot(main, '02-routine-ran-light');

  // Open chat: Chat on its own chat, named after the Skill, marked in the list.
  await run.getByRole('button', { name: 'Open what /todos wrote' }).click();
  await expect(dialog).toBeHidden();
  await expect(main.locator('.cbubble.user').last()).toContainText('/todos');
  await expect(main.locator('.toolchip').filter({ hasText: /Ran \/todos/ })).toBeAttached();
  const conv = main.locator(`[data-conv="${chatId}"]`);
  await expect(conv).toHaveClass(/active/);
  await expect(conv.getByTestId('chat-routine-mark')).toBeVisible();
  // Named after its Skill.
  await expect(conv.locator('.ctitle')).toContainText(listed.routines[0].name);
  await shot(main, '03-routine-chat-light');

  // Run again: the same chat, two runs in it.
  const again = await openSettings(main, 'Skills');
  await routinesGroup(again).locator('[data-routine]').getByRole('button', { name: 'Run /todos now' }).click();
  await expect.poll(async () => (await list(yap)).runs.filter((r) => r.state === 'done').length, { timeout: 30_000 }).toBe(2);
  const second = await list(yap);
  expect(second.runs[0].chat).toBe(chatId);
  expect(second.routines[0].chat).toBe(chatId);
  const c = await yap.invoke('chat_get', { id: chatId });
  expect(c.messages.filter((m) => m.role === 'user').length).toBe(2);

  // Off: its line says so; the switch is saved.
  await routinesGroup(again).getByRole('button', { name: 'Run /todos by itself' }).click();
  await expect(routinesGroup(again).locator('[data-routine]')).toContainText('Off');
  await expectStore(yap, 'routines.json', (j) => j.routines[0].on === false, 'the switch to be saved');
  await routinesGroup(again).getByRole('button', { name: 'Run /todos by itself' }).click();
  await expectStore(yap, 'routines.json', (j) => j.routines[0].on === true, 'on again');

  // Dark.
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  try {
    const d = await openSettings(main, 'Skills');
    await routinesGroup(d).scrollIntoViewIfNeeded();
    await shot(main, '04-routines-dark');
    await routinesGroup(d).locator('[data-routine]').getByRole('button', { name: 'Edit the /todos routine' }).click();
    await expect(d.getByRole('group', { name: 'Edit routine' })).toBeVisible();
    await shot(main, '05-routine-form-dark');
    await d.getByRole('group', { name: 'Edit routine' }).getByRole('button', { name: 'Cancel' }).click();
    await closeSettings(main);
    await openView(main, 'Chat');
    await main.locator(`[data-conv="${chatId}"]`).click();
    await shot(main, '06-routine-chat-dark');
  } finally {
    await setConfig(yap, main, { theme: 'light' });
  }
  fakeLlm.usage('other', null);
});

test('after a meeting: its routine runs on that meeting once the summary is written, with its note, said on the bar; one for another folder doesn’t', async ({ yap, main, fakeLlm, shot }) => {
  await deleteAll(yap);
  await closeSettings(main);
  // With the meeting's note (a Skill on one meeting), made in the form…
  const dialog = await openSettings(main, 'Skills');
  await routinesGroup(dialog).getByRole('button', { name: 'New routine' }).click();
  const form = dialog.getByRole('group', { name: 'New routine' });
  await form.getByRole('combobox', { name: 'Routine Skill' }).selectOption('builtin:followup-email');
  await form.getByRole('tab', { name: 'After a meeting' }).click();
  await expect(form.getByText('Where it goes')).toBeVisible();
  await form.getByRole('tab', { name: "With the meeting's note" }).click();
  await shot(main, '07-after-a-meeting-form');
  await form.getByRole('button', { name: 'Save routine' }).click();
  await expect(routinesGroup(dialog).locator('[data-routine]')).toContainText("After each meeting · With the meeting's note");
  // …and one only for meetings in Clients.
  await yap.invoke('routine_save', { routine: { skill: 'builtin:decisions', trigger: { kind: 'meetingEnd', folder: 'Clients', outside: false }, to: 'chat', on: true } });
  await closeSettings(main);
  await closeToasts(main);

  // A fake meeting, recorded and ended, with Yap in another app.
  await inAnotherApp(yap);
  const note = await yap.invoke('note_create', { title: 'Quick sync', folder: 'Meetings' });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(() => recording(yap)).toBe(true);
  const ts = Math.floor(Date.now() / 1000);
  await yap.invoke('e2e_meeting_feed', {
    segments: [
      { source: 'them', text: 'Thanks everyone, the deposit landed this quarter, so the budget looks worse than it is for now.', ts },
      { source: 'you', text: 'Understood. Alice, can you send the revised budget by Friday so we can close this out?', ts: ts + 8 },
      { source: 'them', text: 'Yes, I will send the revised budget by Friday afternoon.', ts: ts + 16 },
    ],
  });
  await yap.invoke('meeting_end', {});
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);

  // Its summary, then its routine on it: a new Ask thread kept with the note.
  await expect
    .poll(async () => (await list(yap)).runs.find((r) => r.cause === 'meeting')?.state, { timeout: 60_000 })
    .toBe('done');
  const runs = (await list(yap)).runs;
  const ran = runs.find((r) => r.cause === 'meeting');
  expect(ran.note).toBe(note.id);
  expect(ran.chat).toBe(0);
  expect(ran.command).toBe('followup-email');
  // The other folder's routine never ran.
  expect(runs.filter((r) => r.command === 'decisions')).toHaveLength(0);
  const threads = await yap.invoke('note_chat', { noteId: note.id });
  expect(threads.threads.length).toBe(1);
  const msgs = threads.current.messages;
  expect(msgs.map((m) => m.role)).toEqual(['user', 'assistant']);
  expect(msgs[0].text).toBe('/followup-email');
  expect(msgs[1].text).toContain('The deposit landed this quarter');
  // On the chat's own model, as a Skill on one meeting.
  expect(fakeLlm.requests.some((r) => r.kind === 'skill' && r.model === 'fake-chat')).toBe(true);

  // One quiet card on the Yap bar (Yap in another app).
  const card = yap.overlay.getByRole('status').filter({ hasText: 'Routine ran: /followup-email' });
  await expect(card).toBeVisible({ timeout: 15_000 });
  await shot(yap.overlay, '08-routine-card-on-the-bar');
  // Its Open: Notes on the meeting.
  await card.getByRole('button', { name: 'Open' }).click();
  await expect(sidebar(main).getByRole('button', { name: 'Notes', exact: true })).toHaveAttribute('aria-current', 'page');
  if (await visible(yap, 'notepad')) await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
  await deleteAll(yap);
});
