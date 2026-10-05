// A two-hour meeting, time-compressed: its rolling digests keep every AI
// call small while it records, and "End meeting & summarise" turns it into
// an action plan with a section per person in seconds. The transcript is
// handed to the recorder directly (`e2e_meeting_feed`, test mode only); the
// AI is a local fake (support/fake-llm.js) that records every request and
// tests Yap's checks by inventing an owner and dropping a task.
import { test as base, expect, expectStore } from './support/fixtures.js';
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
        name: 'meeting-ai',
        config: {
          llmScopes: {
            noteFormatting: {
              enabled: true,
              provider: 'custom',
              baseUrl: fakeLlm.base,
              model: 'fake-notes',
            },
          },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const MEETING_SECS = 2 * 60 * 60;
const EVERY = 15; // one chunk per 15 s, alternating You / Them

const FILLER = [
  'we walked through the plan for the offsite and how the two days should flow, covering travel, rooms, the sessions we want, and the follow up with the wider team afterwards',
  'the main concern is keeping the sessions practical so people leave with decisions rather than slides, so each block needs an owner and a clear question to answer',
  'there was some back and forth about timing, since half the team is remote and the travel budget is tighter than last year, which limits how many people can fly in',
  'we also compared the two venues on cost, distance from the airport and room sizes, and noted that the cheaper one has no breakout rooms on the second day',
];
// The last ten minutes are quick wrap-up remarks: too little talk for a
// digest of their own, so they reach the action plan as raw transcript.
const WRAP_UP = 440;
const WRAP_UP_FILLER = 'quick wrap-up remarks on next steps';

/** Two hours of synthetic talk, with tasks said out loud at known points. */
function twoHourMeeting(t0) {
  const special = {
    80: ['them', 'Alice here: I will send the revised budget by Friday so we can plan around it.'],
    200: ['you', 'Bob, can you book the venue by Monday? We need the rooms confirmed.'],
    201: ['them', 'Bob: sure, I will book the venue by Monday and send the confirmation.'],
    260: ['them', 'One open point: who handles catering? Nobody has picked that up yet.'],
    320: ['you', 'Someone should update the wiki with the plan once we agree on it.'],
    380: ['them', 'Okay, we decided to hold the offsite in Lisbon, the numbers work best there.'],
    470: ['you', 'I will draft the agenda tonight and share it with everyone tomorrow.'],
  };
  return Array.from({ length: MEETING_SECS / EVERY }, (_, i) => {
    // ~58 tokens a chunk: ten minutes ≈ 2,300 tokens, one digest's worth.
    const filler =
      i < WRAP_UP ? `${FILLER[i % FILLER.length]}, which we will revisit` : WRAP_UP_FILLER;
    const [source, said] = special[i] ?? [i % 2 ? 'them' : 'you', filler];
    const id = `SEG-${String(i + 1).padStart(4, '0')}`;
    return { source, text: `${id} ${said}`, ts: t0 + i * EVERY };
  });
}

/** The note's stored action plan, once written. */
async function storedPlan(yap, title) {
  await expectStore(yap, 'notes.json', (s) => !!meetingNote(s, title)?.enhancedContent);
  return meetingNote(yap.readJson('notes.json'), title).enhancedContent;
}

test('a two-hour meeting: rolling digests, then the action plan in seconds', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await newMeetingNote(main, 'Q3 planning', ['Alice', 'Bob']);
  await main.getByRole('button', { name: 'Record' }).click();
  await expect(main.getByRole('button', { name: 'End meeting & summarise' })).toBeVisible();

  // Two hours of transcript, ten minutes (40 chunks) at a time, as if just
  // transcribed. Each ten minutes of steady talk becomes one digest, written
  // in the background while the meeting goes on.
  const t0 = Math.floor(Date.now() / 1000) - MEETING_SECS;
  const segments = twoHourMeeting(t0);
  for (let i = 0; i < segments.length; i += 40) {
    const batch = segments.slice(i, i + 40);
    expect(await yap.invoke('e2e_meeting_feed', { segments: batch })).toBe(batch.length);
    if (i < WRAP_UP) {
      await expectStore(
        yap,
        'notes.json',
        (s) => meetingNote(s, 'Q3 planning')?.digests?.length === i / 40 + 1
      );
    }
  }
  await expectStore(
    yap,
    'notes.json',
    (s) => meetingNote(s, 'Q3 planning')?.transcript?.length === segments.length
  );
  await expect(main.locator('.tdigest')).toContainText('AI notes up to 1:49:45');
  await shot(main, '01-two-hour-meeting-digested-live');

  // 2 hours of talk → 11 digest calls, not one per chunk (the wrap-up is too
  // short to need one), each fitting an 8k-context model with room to spare.
  const digests = fakeLlm.requests.filter((r) => r.kind === 'digest');
  const stored = meetingNote(yap.readJson('notes.json'), 'Q3 planning').digests;
  expect(digests).toHaveLength(11);
  expect(stored).toHaveLength(11);
  for (const r of digests) {
    expect(r.tokens).toBeLessThanOrEqual(4_500);
    expect(r.maxTokens).toBe(700);
  }
  // Each part picks up exactly where the last one stopped.
  stored.forEach((d, i) => expect(d.fromSeg).toBe(i ? stored[i - 1].toSeg : 0));
  // The digests' checks: "Zed" was never mentioned and nobody said
  // "Tuesday", so the task the model gave Zed is Unassigned, no deadline.
  const tasks = stored.flatMap((d) => d.actions);
  expect(tasks).toContainEqual({ owner: 'Unassigned', task: 'order pizza', due: '' });
  expect(tasks).toContainEqual({ owner: 'Alice', task: 'send the revised budget', due: 'Friday' });
  expect(tasks).toContainEqual({ owner: 'Bob', task: 'book the venue', due: 'Monday' });

  // The AI notes so far, in the transcript box.
  await main.getByRole('button', { name: 'AI notes so far' }).click();
  await expect(main.getByRole('region', { name: 'AI notes so far' })).toContainText(
    'Alice: send the revised budget (due Friday)'
  );
  await shot(main, '02-ai-notes-so-far');
  await main.getByRole('button', { name: 'Transcript', exact: true }).click();

  // The end of the meeting: one call, over the digests + the last stretch.
  const ended = Date.now();
  await main.getByRole('button', { name: 'End meeting & summarise' }).click();
  const plan = main.locator('.rendered');
  await expect(plan.getByRole('heading', { name: 'Action plan' })).toBeVisible({ timeout: 15_000 });
  expect(Date.now() - ended).toBeLessThan(15_000);

  const finals = fakeLlm.requests.filter((r) => r.kind === 'actionPlan');
  expect(finals).toHaveLength(1);
  const [final] = finals;
  expect(final.tokens).toBeLessThanOrEqual(6_000); // of ~30k tokens of talk
  expect(final.maxTokens).toBe(1_500);
  expect(final.user).toContain('Attendees: Alice, Bob');
  expect(final.user).toContain('## Digest of the meeting so far');
  expect(final.user).toContain('## Transcript of the last part');
  expect(final.user).not.toContain('SEG-0001'); // the raw opening isn't sent again
  expect(final.user).toContain('SEG-0480'); // the latest stretch is
  expect(fakeLlm.requests.filter((r) => r.kind === 'digest')).toHaveLength(11);

  // One section per person, then Decisions, Open questions and Unassigned.
  for (const name of ['Alice', 'Bob', 'You', 'Decisions', 'Open questions', 'Unassigned']) {
    await expect(plan.getByRole('heading', { name, exact: true })).toBeVisible();
  }
  const enhanced = await storedPlan(yap, 'Q3 planning');
  // The task the final merge dropped is back under its owner…
  expect(enhanced).toContain('### Bob\n- [ ] book the venue (due: Monday)');
  expect(enhanced).toContain('### You\n- [ ] draft the agenda');
  // …and the owner it made up is gone: the task is unassigned, without its
  // made-up deadline.
  expect(enhanced).not.toContain('Mallory');
  expect(enhanced).not.toContain('Thursday');
  const unassigned = enhanced.split('## Unassigned')[1];
  expect(unassigned).toMatch(/^- \[ \] Print the badges$/m);
  expect(unassigned).toContain('- [ ] update the wiki');
  expect(unassigned).toContain('- [ ] order pizza');
  await shot(main, '03-action-plan');

  // Copy as plain text for a chat or an email (the page's clipboard is
  // stubbed: a test never touches the real one).
  await main.evaluate(() => {
    window.__copied = null;
    navigator.clipboard.writeText = async (t) => {
      window.__copied = t;
    };
  });
  await main.getByRole('button', { name: 'Copy text' }).click();
  await expect.poll(() => main.evaluate(() => window.__copied)).toContain('☐ send the revised budget');
  const text = await main.evaluate(() => window.__copied);
  expect(text).not.toMatch(/^#/m);
  expect(text).toMatch(/^Alice$/m);
});
