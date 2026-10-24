// Real names on a meeting's voices (src-tauri/src/speakers/naming.rs): Yap
// suggests who each voice is from what the meeting says — someone hands
// over to Priya and this voice answers, someone thanks her right after it
// spoke, a voice introduces itself — plus the invite, and (when a meeting
// ends) the AI's answer, checked against the transcript. The person
// confirms on a card: "Speaker 2 is Priya?" — Yes / Someone else / Not
// now. A strong suggestion is also put on the voice as Yap's guess
// ("Priya?") so the action plan reads it; Yes makes it the person's own.
// A voice that repeats what you said asks "Speaker 3 is you?" ("It was
// me": your voice reached the call side).
//
// No model and no audio: placed lines go to the recorder with
// `e2e_meeting_feed`, the speaker engine's timelines are scripted with
// `e2e_diarize_timeline` (as speaker-labels.spec.js does), and the fake AI
// answers the "who is this voice?" question when a spec scripts it.
import { test as base, expect, closeToasts } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { pendingSuggestions, suggestionQuestion, suggestionWhy } from '../src/lib/core/notepadText.js';

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
        name: 'speaker-names',
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

const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const recording = (yap) => yap.invoke('meeting_state').then((s) => s.recording);
const tab = (yap, name) => yap.notepad.getByRole('tab', { name });
const transcript = (pad) => pad.getByRole('log', { name: 'Transcript' });
const groupNames = (pad) => transcript(pad).locator(':scope > .group .who').allTextContents();
const card = (page) => page.getByRole('region', { name: "Who's speaking" });

let clockTs = Math.floor(Date.now() / 1000) + 100_000;

/** A line as the recorder places one: a word every 500 ms (400 ms long). */
function placed(base, source, text, startMs) {
  const n = text.split(/\s+/).filter(Boolean).length;
  return {
    source,
    text,
    ts: base + Math.floor(startMs / 1000),
    startMs,
    endMs: startMs + (n - 1) * 500 + 400,
    words: Array.from({ length: n }, (_, k) => [k * 500, k * 500 + 400]),
  };
}

async function say(yap, base, ...lines) {
  const segments = lines.map(([source, text, startMs]) => placed(base, source, text, startMs));
  expect(await yap.invoke('e2e_meeting_feed', { segments })).toBe(segments.length);
}

/** The engine's labels: `[startMs, endMs, speaker]` turns, settled up to `labelledMs`. */
async function labels(yap, turns, labelledMs, extra = {}) {
  const segments = turns.map(([startMs, endMs, speaker]) => ({ startMs, endMs, speaker }));
  await yap.invoke('e2e_diarize_timeline', { segments, labelledMs, done: false, ...extra });
}

async function startMeeting(yap, title, participants) {
  const note = await yap.invoke('note_create', { title, folder: 'Meetings' });
  await yap.invoke('note_update', { id: note.id, participants });
  await yap.invoke('meeting_start', { noteId: note.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  await expect.poll(() => recording(yap)).toBe(true);
  await tab(yap, 'Transcript').click();
  clockTs += 600;
  return { id: note.id, base: clockTs };
}

async function stopMeeting(yap) {
  await yap.notepad.getByRole('button', { name: 'Stop', exact: true }).click();
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
}

test.afterEach(async ({ yap, fakeLlm }) => {
  fakeLlm.reply('speakerName', () => '{"quote": "", "name": "unknown"}');
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
  for (let i = 0; i < 3; i += 1) await yap.notepad.keyboard.press('Escape');
  await closeToasts(yap.notepad);
});

test('what a card says, and which cards are left', () => {
  const note = {
    transcript: [
      { source: 'them', speaker: 'them-1', text: 'Priya, can you start?' },
      { source: 'them', speaker: 'them-2', text: 'Sure.' },
      { source: 'them', speaker: 'them-3', text: 'Numbers are fine.' },
      { source: 'them', speaker: 'them-4', text: 'Can you see my screen?' },
    ],
    speakers: {
      'them-2': {
        name: 'Priya Shah',
        source: 'ai',
        suggestion: { name: 'Priya Shah', confidence: 0.8, why: { kind: 'asked', by: 'them-1', quote: 'Priya, can you start?' } },
      },
      'them-3': { suggestion: { name: 'Sam Patel', confidence: 0.4, why: { kind: 'invite' }, later: true } },
      'them-4': { suggestion: { me: true, confidence: 0.9, why: { kind: 'echo' } } },
    },
  };
  const cards = pendingSuggestions(note);
  // Put off ("Not now") isn't asked again; the surest first.
  expect(cards.map((c) => c.voice)).toEqual(['them-4', 'them-2']);
  expect(suggestionQuestion(note, cards[0])).toBe('Speaker 4 is you?');
  // The guess already on the voice isn't how the card calls it.
  expect(suggestionQuestion(note, cards[1])).toBe('Speaker 2 is Priya Shah?');
  expect(suggestionWhy(note, cards[1])).toBe('Speaker 1 asked “Priya, can you start?” and Speaker 2 answered');
  expect(suggestionWhy(note, { voice: 'them-3', name: 'Sam Patel', why: { kind: 'invite' } })).toBe(
    'Sam is the only other person invited'
  );
});

test('the meeting names its voices: a card asks, Yes makes the name theirs, Someone else renames', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  const { id, base } = await startMeeting(yap, 'Design review', ['Priya Shah', 'Sam Patel', 'Siobhan Kelly']);
  const pad = yap.notepad;
  await say(
    yap,
    base,
    ['them', 'Morning all, thanks for joining.', 0],
    ['them', 'Priya, can you walk us through the new screens?', 3_000],
    ['them', 'Sure. We finished the onboarding flow last week and tested it with five people.', 9_000],
    ['them', 'Thanks, Priya. Sam, what about the budget?', 17_500],
    ['them', 'We are on track, about two percent under for the quarter.', 22_500],
    ['you', 'Great, thanks both.', 29_000]
  );
  await expect(transcript(pad)).toContainText('two percent under');
  await labels(yap, [[0, 8_000, 1], [9_000, 16_500, 2], [17_500, 21_500, 1], [22_500, 28_000, 3]], 32_000);

  // Priya was asked and then thanked: strong, so already on her voice as a
  // guess. Sam was only asked: a question on a card, no name yet.
  await expect.poll(() => groupNames(pad)).toEqual(['Speaker 1', 'Priya Shah?', 'Speaker 1', 'Speaker 3', 'You']);
  const c = card(pad);
  await expect(c).toContainText('Speaker 2 is Priya Shah?');
  await expect(c).toContainText('Speaker 1 asked');
  await expect(c).toContainText('1 of 2');
  await shot(pad, '01-name-card');

  await c.getByRole('button', { name: 'Yes', exact: true }).click();
  await expect.poll(() => groupNames(pad)).toEqual(['Speaker 1', 'Priya Shah', 'Speaker 1', 'Speaker 3', 'You']);
  let stored = await yap.invoke('note_get', { id });
  expect(stored.speakers['them-2']).toMatchObject({ name: 'Priya Shah', source: 'user', locked: true });
  expect(stored.speakers['them-2'].suggestion).toBeUndefined();

  // Next: Sam. Someone else: the rename, and Sam is never suggested for it again.
  await expect(c).toContainText('Speaker 3 is Sam Patel?');
  await c.getByRole('button', { name: 'Someone else' }).click();
  await expect(c).toContainText('Who is Speaker 3?');
  await shot(pad, '02-someone-else');
  await c.getByRole('textbox', { name: 'Speaker name' }).fill('Siobhan Kelly');
  await c.getByRole('button', { name: 'Save' }).click();
  await expect.poll(() => groupNames(pad)).toEqual(['Speaker 1', 'Priya Shah', 'Speaker 1', 'Siobhan Kelly', 'You']);
  await expect(card(pad)).toHaveCount(0);
  stored = await yap.invoke('note_get', { id });
  expect(stored.speakers['them-3']).toMatchObject({ name: 'Siobhan Kelly', locked: true, not: ['Sam Patel'] });

  // The Notes view shows the same names.
  await pad.getByRole('button', { name: 'Open in Yap' }).click();
  await expect(main.getByRole('log', { name: 'Meeting transcript' })).toContainText('two percent under');
  await expect(main.getByRole('log', { name: 'Meeting transcript' }).getByRole('button', { name: 'Siobhan Kelly' })).toBeVisible();

  // The action plan reads the names.
  await labels(yap, [[0, 8_000, 1], [9_000, 16_500, 2], [17_500, 21_500, 1], [22_500, 28_000, 3]], 32_000, { done: true });
  const plans = fakeLlm.requests.filter((r) => r.kind === 'actionPlan').length;
  await stopMeeting(yap);
  await expect
    .poll(() => fakeLlm.requests.filter((r) => r.kind === 'actionPlan').length, { timeout: 20_000 })
    .toBeGreaterThan(plans);
  const plan = fakeLlm.requests.filter((r) => r.kind === 'actionPlan').at(-1);
  expect(plan.user).toContain('Priya Shah: Sure. We finished the onboarding flow');
  expect(plan.user).toContain('Siobhan Kelly: We are on track');
  expect(plan.user).toContain('Speakers besides You: Speaker 1 (not named yet); Priya Shah; Siobhan Kelly');
});

test("the AI's answer, checked against the transcript, is a card; Not now puts it off", async ({ yap, main, fakeLlm, shot }) => {
  const { id, base } = await startMeeting(yap, 'Finance sync', ['Sam Patel', 'Rochelle Grant']);
  const pad = yap.notepad;
  await say(
    yap,
    base,
    ['them', 'I spoke to Sam yesterday about the numbers for the quarter.', 0],
    ['them', 'Yes, as I told you yesterday, revenue is up four percent and costs are flat.', 6_000],
    ['you', 'That is good news for the hiring plan.', 13_000]
  );
  await expect(transcript(pad)).toContainText('costs are flat');
  await labels(yap, [[0, 5_500, 1], [6_000, 12_500, 2]], 16_000);
  // (The engine's final labels, when the recording stops.)
  await labels(yap, [[0, 5_500, 1], [6_000, 12_500, 2]], 16_000, { done: true });
  // Nothing in the transcript itself says who Speaker 2 is.
  await expect.poll(() => groupNames(pad)).toEqual(['Speaker 1', 'Speaker 2', 'You']);
  await expect(card(pad)).toHaveCount(0);

  // When the meeting ends the AI is asked, before the action plan; an answer
  // whose quote isn't in the transcript wouldn't count.
  fakeLlm.reply('speakerName', (user) =>
    /Which of the people is Speaker 2\?/.test(user)
      ? '{"quote": "I spoke to Sam yesterday about the numbers", "name": "Sam Patel"}'
      : '{"quote": "", "name": "unknown"}'
  );
  const asked = fakeLlm.requests.filter((r) => r.kind === 'speakerName').length;
  await stopMeeting(yap);
  await expect.poll(() => fakeLlm.requests.filter((r) => r.kind === 'speakerName').length, { timeout: 20_000 }).toBeGreaterThan(asked);
  const q = fakeLlm.requests.filter((r) => r.kind === 'speakerName').at(-1);
  expect(q.user).toContain('People in the meeting besides You: Sam Patel, Rochelle Grant');
  expect(q.user).toContain('Speaker 2 (the voice to identify): Yes, as I told you yesterday');

  await pad.getByRole('button', { name: 'Open in Yap' }).click();
  const c = card(main);
  await expect(c).toContainText('Speaker 2 is Sam Patel?', { timeout: 20_000 });
  await expect(c).toContainText('The AI matched “I spoke to Sam yesterday about the numbers”');
  await shot(main, '03-ai-card-in-notes');
  // Only the AI's word for it: a question, never a name on the voice.
  let stored = await yap.invoke('note_get', { id });
  expect(stored.speakers['them-2'].name).toBeUndefined();

  await c.getByRole('button', { name: 'Not now' }).click();
  await expect(card(main)).toHaveCount(0);
  stored = await yap.invoke('note_get', { id });
  expect(stored.speakers['them-2'].suggestion).toMatchObject({ name: 'Sam Patel', later: true });
  // The menu on the name still offers it.
  const log = main.getByRole('log', { name: 'Meeting transcript' });
  await log.getByRole('button', { name: 'Speaker 2' }).click();
  await expect(main.getByRole('menu', { name: 'Speaker: Speaker 2' }).getByRole('menuitem', { name: "It's Sam Patel" })).toBeVisible();
  await main.keyboard.press('Escape');
});

test('a voice that repeats you: "It was me" makes its lines yours, and Undo gives them back', async ({ yap, shot }) => {
  const { id, base } = await startMeeting(yap, 'Roadmap walkthrough', ['Priya Shah']);
  const pad = yap.notepad;
  await say(
    yap,
    base,
    ['you', 'Can everyone see my screen with the roadmap on it?', 0],
    ['them', 'Can everyone see my screen with the roadmap on it?', 300],
    ['them', 'Yes, we can all see it fine now, go ahead.', 6_000]
  );
  await expect(transcript(pad)).toContainText('go ahead');
  await labels(yap, [[300, 5_000, 1], [6_000, 11_000, 2]], 14_000);
  await expect.poll(() => groupNames(pad)).toEqual(['You', 'Speaker 1', 'Speaker 2']);
  const c = card(pad);
  await expect(c).toContainText('Speaker 1 is you?');
  await expect(c).toContainText('It repeats what you said');
  await shot(pad, '04-is-it-you');
  await c.getByRole('button', { name: 'It was me' }).click();
  // The repeat is hidden (left out of the summary too).
  await expect.poll(() => groupNames(pad)).toEqual(['You', 'Speaker 2']);
  let stored = await yap.invoke('note_get', { id });
  expect(stored.speakers['them-1']).toMatchObject({ mergedInto: 'you' });
  expect(stored.transcript[1].echo).toBe(true);
  await pad.getByRole('button', { name: 'Undo' }).click();
  await expect.poll(() => groupNames(pad)).toEqual(['You', 'Speaker 1', 'Speaker 2']);
  stored = await yap.invoke('note_get', { id });
  expect(stored.speakers['them-1'].not).toEqual(['You']);
  await expect(card(pad)).toHaveCount(0);

  // The menu on any voice told apart offers it too.
  await transcript(pad).getByRole('button', { name: 'Speaker 2' }).click();
  await expect(pad.getByRole('menu', { name: 'Speaker: Speaker 2' }).getByRole('menuitem', { name: 'It was me' })).toBeVisible();
  await pad.keyboard.press('Escape');
});

test("the call app's readings name the voices it showed talking (experimental)", async ({ yap, shot }) => {
  const { id, base } = await startMeeting(yap, 'Supplier review', ['Priya Shah', 'Sam Patel']);
  const pad = yap.notepad;
  await say(
    yap,
    base,
    ['them', 'The new supplier can deliver from March, at the same price as now.', 0],
    ['them', 'That works for us, as long as the contract has a break clause after a year.', 6_000]
  );
  await expect(transcript(pad)).toContainText('break clause');
  // What Teams or Meet showed talking, on the meeting's clock
  // (speakers/call_ui.rs reads it from the call window in a real call).
  const spans = [
    { fromMs: 300, toMs: 5_600, speaking: ['Priya Shah'] },
    { fromMs: 5_600, toMs: 6_300, speaking: [] },
    { fromMs: 6_300, toMs: 12_400, speaking: ['Sam Patel'] },
  ];
  expect(await yap.invoke('e2e_call_readings', { spans, selfName: 'Imogen Hart' })).toBe(3);
  await labels(yap, [[0, 5_500, 1], [6_000, 12_000, 2]], 15_000);
  // The call app's word is no guess: no "?", and no card to confirm.
  await expect.poll(() => groupNames(pad)).toEqual(['Priya Shah', 'Sam Patel']);
  await expect(card(pad)).toHaveCount(0);
  const stored = await yap.invoke('note_get', { id });
  expect(stored.speakers['them-1']).toMatchObject({ name: 'Priya Shah', source: 'platform' });
  expect(stored.speakers['them-2']).toMatchObject({ name: 'Sam Patel', source: 'platform' });
  await shot(pad, '05-names-from-the-call-app');
});
