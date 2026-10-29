// The meeting notepad's newer chips, Skills and Chat's one-tap prompts
// (meetings/meeting_assist.rs, meetings/meeting_summary/chips.rs,
// chat/skills.rs; Wispr Flow's Notetaker chips and Skills):
// - Make me sound smart while a meeting records: something to say, citing
//   the latest talk, its pill opening the line in the transcript;
// - Make notes longer after it: the summary written out in more detail, a
//   task the model dropped put back, Undo;
// - Skills: "/" in the notepad's Ask box lists the ones that run on one
//   meeting (Enter runs one, Tab puts it in the box to add words), the
//   person's own made in Settings → Skills, a copy of a built-in;
// - Chat's one-tap prompts (Coach me, List my open to-dos, Write a weekly
//   recap) and "/" in Chat and the Meetings view's Ask bar, asked across
//   the meetings with their own passages;
// - a meeting's past chat renamed.
// The AI is the local fake (support/fake-llm.js), which records every
// request. No audio device is opened: transcript lines are handed to the
// recorder with `e2e_meeting_feed`.
import { test as base, expect, openView, openSettings, closeSettings, closeToasts } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { newMeetingNote } from './support/meetings.js';
import { meetingHelpers } from '../src/lib/core/notepadText.js';
import { slashQuery, splitCommand, matchSkills, skillFor, commandFromName, commandProblem, promptChips, asked, runsHere } from '../src/lib/core/skills.js';
import { pastChatItems, keptMessage, threadMessages } from '../src/lib/core/noteChat.js';

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;

/** A meeting with tasks, for List my open to-dos (Chat). */
const NOTES = {
  folders: ['Personal', 'Meetings'],
  notes: [
    {
      id: 1,
      title: 'Supplier call',
      noteType: 'meeting',
      folder: 'Meetings',
      source: 'meeting',
      createdTs: NOW - DAY,
      updatedTs: NOW - DAY,
      participants: ['Priya Shah'],
      enhancedContent:
        '## Recap\nThe supplier quote is over budget.\n\n## Action plan\n### You\n- [ ] Draft the finance request (due: Wednesday)\n### Priya Shah\n- [ ] Check the totals\n\n## Unassigned\n- [ ] Book the room',
      transcript: [
        { source: 'you', text: 'I will draft the finance request by Wednesday.', ts: NOW - DAY },
        { source: 'them', text: 'And someone should book the room for the review.', ts: NOW - DAY + 20 },
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
        name: 'meeting-chips',
        config: {
          llmScopes: {
            noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
            chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' },
          },
          meetingReminder: 'never',
        },
        data: { 'notes.json': NOTES },
      });
    },
    { scope: 'worker' },
  ],
});

const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const toast = (page, title) => page.getByRole('status').filter({ hasText: title });
const tab = (yap, name) => yap.notepad.getByRole('tab', { name });
const transcript = (pad) => pad.getByRole('log', { name: 'Transcript' });
const askSheet = (pad) => pad.getByRole('dialog', { name: 'Ask about this meeting' });
const askInput = (pad) => pad.getByRole('textbox', { name: 'Ask anything' });
const suggestions = (pad) => askSheet(pad).getByRole('group', { name: 'Suggestions' });
const skillMenu = (page) => page.getByRole('listbox', { name: 'Skills' });

let clockTs = Math.floor(Date.now() / 1000);
/** Hand the recorder transcript lines (`[source, text]`), as if just transcribed. */
async function say(yap, ...lines) {
  const segments = lines.map((line) => ({ source: line[0], text: line[1], ts: (clockTs += 6) }));
  expect(await yap.invoke('e2e_meeting_feed', { segments })).toBe(segments.length);
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
];

test.afterEach(async ({ yap, fakeLlm }) => {
  await pauseAll(yap);
  fakeLlm.reply('other', undefined);
  for (let i = 0; i < 3; i += 1) await yap.notepad.keyboard.press('Escape');
  await closeToasts(yap.notepad);
});

test('the chips each moment offers, and finding a Skill as it is typed', () => {
  const talk = { transcript: [{ source: 'you', text: 'Hello there everyone', ts: 1 }] };
  expect(meetingHelpers(talk, true)).toEqual(['catchUp', 'questions', 'smart']);
  expect(meetingHelpers(talk, false)).toEqual(['followUpEmail', 'blindSpots']);
  expect(meetingHelpers({ ...talk, enhancedContent: '## Recap\nx' }, false)).toEqual(['followUpEmail', 'blindSpots', 'longer']);

  const list = [
    { id: 'builtin:decisions', command: 'decisions', name: 'Decisions', runsOn: 'both', builtin: true },
    { id: 'builtin:followup-email', command: 'followup-email', name: 'Follow-up email', runsOn: 'meeting', builtin: true },
    { id: 'builtin:todos', command: 'todos', name: 'My open to-dos', runsOn: 'meetings', builtin: true, chip: 'List my open to-dos', question: 'List my open to-dos.' },
    { id: 's1', command: 'deal-risks', name: 'Deal risks', runsOn: 'both' },
  ];
  // "/" and a command so far opens the menu; words after it close it.
  expect(slashQuery('/')).toBe('');
  expect(slashQuery('/Dec')).toBe('dec');
  expect(slashQuery('/dec ')).toBeNull();
  expect(slashQuery('what about /dec')).toBeNull();
  expect(splitCommand('/decisions last week')).toEqual({ command: 'decisions', words: 'last week' });
  expect(splitCommand('/Risks')).toEqual({ command: 'risks', words: '' });
  expect(splitCommand('what /risks')).toBeNull();
  // A meeting's box lists what runs on one meeting; Chat what runs across them.
  expect(runsHere(list[1], 'meetings')).toBe(false);
  expect(matchSkills(list, '', 'meeting').map((s) => s.command)).toEqual(['decisions', 'followup-email', 'deal-risks']);
  expect(matchSkills(list, '', 'meetings').map((s) => s.command)).toEqual(['decisions', 'todos', 'deal-risks']);
  expect(matchSkills(list, 'risk', 'meeting').map((s) => s.command)).toEqual(['deal-risks']);
  expect(matchSkills(list, 'd', 'meeting').map((s) => s.command)).toEqual(['decisions', 'deal-risks']);
  expect(skillFor(list, 'todos', 'meeting')).toBeNull();
  expect(skillFor(list, 'todos', 'meetings').name).toBe('My open to-dos');
  expect(asked(list[0], 'last week')).toBe('/decisions last week');
  expect(promptChips(list).map((s) => s.chip)).toEqual(['List my open to-dos']);
  // Settings: a command from the name, and why one can't be.
  expect(commandFromName('Customer follow-up!')).toBe('customer-follow-up');
  expect(commandProblem('decisions', list)).toMatch(/one of Yap's own/);
  expect(commandProblem('deal-risks', list)).toMatch(/already have/);
  expect(commandProblem('deal-risks', list, 's1')).toBe('');
  expect(commandProblem('Bad Name', list)).toMatch(/lowercase/);
  // A kept answer keeps the lines it cites (a question never does);
  // Past chats renames the thread shown.
  expect(keptMessage({ role: 'assistant', text: 'x [1]', sources: [{ n: 1, note: 3 }] })).toEqual({ role: 'assistant', text: 'x [1]', sources: [{ n: 1, note: 3 }] });
  expect(keptMessage({ role: 'user', text: '/decisions', sources: [{ n: 1 }] })).toEqual({ role: 'user', text: '/decisions' });
  expect(threadMessages({ id: 2, messages: [{ role: 'assistant', text: 'a', sources: [{ n: 1 }] }] })[0].sources).toEqual([{ n: 1 }]);
  expect(pastChatItems([{ id: 2, title: 'A', updatedTs: 1 }], 2, () => {}, () => {}).at(-1).label).toBe('Rename this chat…');
  expect(pastChatItems([{ id: 2, title: 'A', updatedTs: 1 }], 2, () => {}).some((i) => i.label === 'Rename this chat…')).toBe(false);
});

test('Make me sound smart while recording, then Make notes longer after, with Undo', async ({ yap, main, fakeLlm, shot }) => {
  await newMeetingNote(main, 'Offsite planning', ['Alice']);
  await main.getByRole('button', { name: 'Record' }).click();
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  await say(yap, ...BUDGET_TALK);
  const sheet = askSheet(pad);

  // While recording: the sheet offers it beside What did I miss? and
  // Questions to ask; so does the Notes view's Ask bar.
  await askInput(pad).focus();
  await expect(suggestions(pad).getByRole('button')).toHaveText(['What did I miss?', 'Questions to ask', 'Make me sound smart']);
  await expect(main.getByRole('group', { name: 'Meeting helpers' }).getByRole('button')).toHaveText(['Questions to ask', 'Make me sound smart']);
  await suggestions(pad).getByRole('button', { name: 'Make me sound smart' }).click();
  await expect(sheet.locator('.mq').last()).toHaveText('Make me sound smart');
  const answer = sheet.locator('.ma').last();
  await expect(answer).toContainText('Ask whether finance needs the revised budget before Friday');
  // A number that's none of the passages is gone; the real one is a pill.
  await expect(answer).not.toContainText('[99]');
  const pill = answer.locator('a.md-passage').first();
  await expect(pill).toBeVisible();
  const smart = fakeLlm.requests.filter((r) => r.kind === 'smart').at(-1);
  expect(smart.user).toMatch(/^Meeting: Offsite planning\nAttendees: Alice\n/);
  expect(smart.user).toContain('The latest talk, in numbered passages:');
  expect(smart.user).toContain('<passage n="1"');
  expect(smart.user).toContain('send the revised budget to finance by Friday');
  expect(smart.tokens).toBeLessThan(2_500);
  await shot(pad, '10-sound-smart');
  // The pill opens the transcript at its line, lit.
  await pill.click();
  await expect(tab(yap, 'Transcript')).toHaveAttribute('aria-selected', 'true');
  await expect(transcript(pad).locator('.row.lit')).toHaveCount(1);
  await shot(pad, '11-the-cited-line');

  // After the meeting, with its summary: Make notes longer.
  await pauseAll(yap);
  await pad.getByRole('button', { name: 'Generate summary' }).click();
  const summary = pad.getByRole('region', { name: 'Summary' });
  await expect(summary.locator('.rendered')).toBeVisible({ timeout: 15_000 });
  await expect(summary).toContainText('Print the badges');
  const id = yap.readJson('notes.json').notes.find((n) => n.title === 'Offsite planning').id;
  const before = (await yap.invoke('note_get', { id })).enhancedContent;
  await askInput(pad).focus();
  await expect(suggestions(pad).getByRole('button')).toHaveText(['Follow-up email', 'Gaps and risks', 'Make notes longer']);
  await suggestions(pad).getByRole('button', { name: 'Make notes longer' }).click();
  await expect(sheet.locator('.ma').last()).toContainText('Your notes are longer now', { timeout: 15_000 });
  await expect(tab(yap, 'Summary')).toHaveAttribute('aria-selected', 'true');
  await expect(summary).toContainText('Travel is up twelve percent');
  // The task the model left out, put back by Yap's checks.
  await expect(summary).toContainText('Print the badges');
  const longer = fakeLlm.requests.filter((r) => r.kind === 'longer').at(-1);
  // The meeting first, the notes to rewrite last.
  expect(longer.user).toMatch(/^The meeting:\n<meeting>\nAttendees: Alice\n/);
  expect(longer.user).toContain('send the revised budget to finance by Friday');
  expect(longer.user).toMatch(/<notes>\n[\s\S]*Print the badges[\s\S]*\n<\/notes>\n\nNow write these notes out again, about twice as long/);
  const after = (await yap.invoke('note_get', { id })).enhancedContent;
  expect(after.length).toBeGreaterThan(before.length);
  await shot(pad, '12-notes-made-longer');
  // Undo: the summary as it was.
  await toast(pad, 'Notes made longer').getByRole('button', { name: 'Undo' }).click();
  await expect.poll(async () => (await yap.invoke('note_get', { id })).enhancedContent).toBe(before);
  await expect(summary).not.toContainText('Travel is up twelve percent');
});

test('Skills in the notepad: "/" lists them, Enter runs one, Tab adds words; your own and a copy from Settings', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  test.setTimeout(120_000);
  await newMeetingNote(main, 'Venue review', ['Alice']);
  await main.getByRole('button', { name: 'Record' }).click();
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  await say(yap, ...BUDGET_TALK);
  const sheet = askSheet(pad);

  // "/": the Skills that run on one meeting (not the across-meetings ones).
  await askInput(pad).fill('/');
  const menu = skillMenu(pad);
  await expect(menu.getByRole('option')).toHaveCount(4);
  await expect(menu.getByRole('option').first()).toContainText('/followup-email');
  await expect(menu).not.toContainText('/todos');
  await expect(suggestions(pad)).toHaveCount(0); // the chips step aside while typing
  await shot(pad, '20-skill-menu');
  // Narrowed as it's typed; Enter runs the one highlighted.
  await askInput(pad).fill('/dec');
  await expect(menu.getByRole('option')).toHaveCount(1);
  await pad.keyboard.press('Enter');
  await expect(sheet.locator('.mq').last()).toHaveText('/decisions');
  const answer = sheet.locator('.ma').last();
  await expect(answer.getByRole('heading', { name: 'Decisions' })).toBeVisible();
  await expect(answer.locator('a.md-passage').first()).toBeVisible();
  const run = fakeLlm.requests.filter((r) => r.kind === 'skill').at(-1);
  // The meeting's passages, then the Skill's instructions last.
  expect(run.user).toContain('<passage n="1"');
  expect(run.user).toContain('from="transcript"');
  expect(run.user).toMatch(/The Skill to carry out on this meeting: Decisions \(\/decisions\)\nInstructions:\nList every decision[^\n]*$/);
  await shot(pad, '21-skill-answer');
  // Tab puts it in the box, to add words.
  await askInput(pad).fill('/ri');
  await pad.keyboard.press('Tab');
  await expect(askInput(pad)).toHaveValue('/risks ');
  await askInput(pad).pressSequentially('about the venue');
  await pad.keyboard.press('Enter');
  await expect(sheet.locator('.mq').last()).toHaveText('/risks about the venue');
  await expect(sheet.locator('.ma').last()).toContainText('You asked about: about the venue');
  expect(fakeLlm.requests.filter((r) => r.kind === 'skill').at(-1).user).toMatch(/\nThe user's own words with it: about the venue$/);
  // Esc closes the menu, not the sheet.
  await askInput(pad).fill('/');
  await expect(menu).toBeVisible();
  await pad.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
  await expect(sheet).toBeVisible();
  await askInput(pad).fill('');

  // Settings → Skills: Yap's own (open one, save a copy), and a new one.
  const dialog = await openSettings(main, 'Skills');
  const decisions = dialog.locator('[data-skill="decisions"]');
  await decisions.getByRole('button', { name: 'Open' }).click();
  await expect(decisions).toContainText('List every decision that was made');
  await decisions.getByRole('button', { name: 'Save a copy' }).click();
  const copy = dialog.getByRole('group', { name: 'New Skill' });
  await expect(copy).toContainText("Based on Yap's /decisions.");
  await expect(copy.getByRole('textbox', { name: 'Skill command' })).toHaveValue('my-decisions');
  await copy.getByRole('textbox', { name: 'Skill instructions' }).fill('List every decision, with the date of the meeting.');
  await copy.getByRole('button', { name: 'Save Skill' }).click();
  await expect(dialog.locator('[data-skill="my-decisions"]')).toBeVisible();
  await dialog.getByRole('button', { name: 'New Skill' }).click();
  const form = dialog.getByRole('group', { name: 'New Skill' });
  await form.getByRole('textbox', { name: 'Skill name' }).fill('Budget numbers');
  await expect(form.getByRole('textbox', { name: 'Skill command' })).toHaveValue('budget-numbers');
  // A built-in's command can't be taken.
  await form.getByRole('textbox', { name: 'Skill command' }).fill('risks');
  await expect(form).toContainText("/risks is one of Yap's own Skills");
  await expect(form.getByRole('button', { name: 'Save Skill' })).toBeDisabled();
  await form.getByRole('textbox', { name: 'Skill command' }).fill('budget-numbers');
  await form.getByRole('textbox', { name: 'Skill instructions' }).fill('List every amount of money that was said, and who said it.');
  await form.getByRole('tab', { name: 'One meeting' }).click();
  await shot(main, '22-new-skill');
  await form.getByRole('button', { name: 'Save Skill' }).click();
  await expect(dialog.locator('[data-skill="budget-numbers"]')).toContainText('One meeting');
  const stored = yap.readJson('skills.json');
  expect(stored.skills.map((s) => [s.command, s.runsOn, s.basedOn ?? ''])).toEqual([
    ['my-decisions', 'both', 'decisions'],
    ['budget-numbers', 'meeting', ''],
  ]);
  await shot(main, '23-skills-settings');
  await closeSettings(main);

  // Yours, in the notepad's "/" menu, run on the meeting.
  await askInput(pad).fill('/bud');
  await expect(skillMenu(pad).getByRole('option')).toHaveText([/\/budget-numbers/]);
  await pad.keyboard.press('Enter');
  await expect(sheet.locator('.mq').last()).toHaveText('/budget-numbers');
  await expect(sheet.locator('.ma').last().getByRole('heading', { name: 'Budget numbers' })).toBeVisible();
  expect(fakeLlm.requests.filter((r) => r.kind === 'skill').at(-1).user).toContain('Instructions:\nList every amount of money');

  // Deleted in Settings: gone from the menu.
  const again = await openSettings(main, 'Skills');
  await again.locator('[data-skill="budget-numbers"]').getByRole('button', { name: 'Delete /budget-numbers' }).click();
  await main.getByRole('alertdialog').getByRole('button', { name: 'Delete' }).click();
  await expect(again.locator('[data-skill="budget-numbers"]')).toHaveCount(0);
  await closeSettings(main);
  await askInput(pad).fill('/bud');
  await expect(skillMenu(pad)).toHaveCount(0);
  await askInput(pad).fill('');
});

test("a meeting's past chat renamed from Past chats", async ({ yap, main }) => {
  await newMeetingNote(main, 'Rename me', []);
  await main.getByRole('button', { name: 'Record' }).click();
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  await say(yap, ...BUDGET_TALK.slice(0, 2));
  const sheet = askSheet(pad);
  // Two threads: the first, then New chat and another question.
  await askInput(pad).fill('Who owns the budget?');
  await pad.keyboard.press('Enter');
  await expect(sheet.locator('.ma').last()).toContainText('Alice owns the budget');
  await sheet.getByRole('button', { name: 'New chat' }).click();
  await askInput(pad).fill('What about the budget?');
  await pad.keyboard.press('Enter');
  await expect(sheet.locator('.ma').last()).toContainText('Alice owns the budget');
  await sheet.getByRole('button', { name: 'Past chats' }).click();
  await pad.getByRole('menuitem', { name: 'Rename this chat…' }).click();
  const name = sheet.getByRole('textbox', { name: 'Chat name' });
  await expect(name).toHaveValue('What about the budget?');
  await name.fill('Budget owner');
  await name.press('Enter');
  await expect(name).toHaveCount(0);
  await sheet.getByRole('button', { name: 'Past chats' }).click();
  await expect(pad.getByRole('menuitemcheckbox', { name: /^Budget owner · / })).toBeVisible();
  await pad.keyboard.press('Escape');
});

test('Chat: one-tap prompts and "/" ask across the meetings with their own passages; so does the Meetings view', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  fakeLlm.reply('other', () => '- Draft the finance request, by Wednesday [1].');
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
  const prompts = main.getByRole('group', { name: 'Try asking' });
  await expect(prompts.getByRole('button')).toHaveText(['Coach me', 'List my open to-dos', 'Write a weekly recap']);
  await shot(main, '30-chat-prompts');
  const asked = fakeLlm.requests.length;
  await prompts.getByRole('button', { name: 'List my open to-dos' }).click();
  await expect(main.locator('.cbubble.user').last()).toContainText('List my open to-dos.');
  await expect(main.getByTestId('chat-across-meetings')).toBeVisible();
  const answer = main.locator('.cbubble.assistant').last();
  await expect(answer).toContainText('Draft the finance request');
  await expect(answer.locator('a.md-passage')).toHaveCount(1);
  const todo = fakeLlm.requests.slice(asked).filter((r) => r.kind === 'other').at(-1);
  const sent = JSON.stringify(todo.messages);
  expect(sent).toContain('Skill: My open to-dos (/todos)');
  expect(sent).toContain('Open task — You: Draft the finance request (due: Wednesday)');
  expect(sent).toContain('Book the room');
  expect(sent).not.toContain('Check the totals'); // someone else's
  // (The earlier tests' meetings have open tasks too.)
  await expect(main.locator('.toolchip').filter({ hasText: /Ran \/todos · \d+ open tasks/ })).toBeVisible();
  await shot(main, '31-open-to-dos');

  // "/" in Chat lists what runs across the meetings.
  const box = main.getByRole('textbox', { name: 'Message' });
  await box.fill('/');
  const menu = skillMenu(main);
  await expect(menu).toContainText('/weekly-recap');
  await expect(menu).not.toContainText('/followup-email');
  await box.fill('/weekly');
  await box.press('Enter');
  await expect(main.locator('.cbubble.user').last()).toContainText('/weekly-recap');
  await expect.poll(() => JSON.stringify(fakeLlm.requests.filter((r) => r.kind === 'other').at(-1)?.messages ?? '')).toContain('Skill: Weekly recap (/weekly-recap)');

  // The Meetings view: the same prompts over its Ask bar (its examples still cycle in the box).
  await openView(main, 'Meetings');
  const over = main.getByRole('group', { name: 'Try asking' });
  await expect(over.getByRole('button')).toHaveText(['Coach me', 'List my open to-dos', 'Write a weekly recap']);
  await shot(main, '32-meetings-prompts');
  await over.getByRole('button', { name: 'Coach me' }).click();
  await expect(main.locator('.cbubble.user').last()).toContainText('Coach me on my last meeting');
  await expect
    .poll(() => JSON.stringify(fakeLlm.requests.filter((r) => r.kind === 'other').at(-1)?.messages ?? ''))
    .toContain('Skill: Coach me (/coach)');
});
