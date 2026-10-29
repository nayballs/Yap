// Smarter about the call itself (2026-10-29; calls/meeting_detect.rs,
// calls/call_audio.rs, meetings/sleep.rs): the call on another microphone
// ("Teams is using … but Yap is using …", Use it), muted in the call (no
// "Yap can't hear your mic"), the PC asleep mid-meeting (a line in the
// transcript, Resume), another call while one records ("Switch to …?"),
// an ending that isn't sure ("Stop taking notes?") and a call back within
// 2 minutes of an automatic stop (carried on in the same note), and the
// one offer to start a call app's notes automatically.
//
// Test mode reads nothing of Windows: `meeting_detect_simulate` stands in
// for a call app taking or letting go of the mic (`recorded`: how many of its
// meetings were recorded so far), `e2e_call_audio` for what Windows would say
// about the call (its microphone, its mute button, whether it still plays),
// `e2e_sleep` for the PC sleeping and waking, and the recorder plays you.wav /
// them.wav (4× real time) on made-up microphones (`YAP_E2E_MICS`).
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, closeToasts, setConfig, openSettings, closeSettings, settingsDialog } from './support/fixtures.js';
import { newMeetingNote } from './support/meetings.js';
import { writeWav } from './support/wav.js';
import { RUNS } from './support/yap.js';

const audioDir = path.join(RUNS, 'call-smarts-wavs');
const LAPTOP = { id: 'laptop', name: 'Microphone Array (Northwind Audio)' };
const HEADSET = { id: 'headset', name: 'Headset Microphone (Contoso H2)' };
const USB = { id: 'usb', name: 'Microphone (Fabrikam USB Mic)' };

test.use({
  yapOptions: {
    name: 'call-smarts',
    env: {
      YAP_E2E_MEETING_AUDIO: audioDir,
      YAP_E2E_MEETING_SPEED: '4',
      YAP_E2E_MICS: JSON.stringify([LAPTOP, HEADSET, USB]),
    },
    // No calendar cards in the way; "You" on the laptop's microphone.
    config: { meetingReminder: 'never', micOrder: [{ id: `e2e-${LAPTOP.id}`, name: LAPTOP.name }] },
  },
});

const toast = (page, text) => page.getByRole('status').filter({ hasText: text });
const meeting = (yap) => yap.invoke('meeting_state');
const recording = (yap) => meeting(yap).then((s) => s.recording);
const status = (yap) => yap.invoke('meeting_detect_status');
const question = (yap) => yap.invoke('meeting_checkin_status').then((s) => s.prompt);
const logCount = (yap, text) => fs.readFileSync(yap.logPath, 'utf8').split(text).length - 1;
const meetingNotes = (yap) => (yap.readJson('notes.json')?.notes ?? []).filter((n) => n.source === 'meeting');
const noteById = (yap, id) => (yap.readJson('notes.json')?.notes ?? []).find((n) => n.id === id);
/** A call app taking (or letting go of) the mic, no debounce. */
const simulate = (yap, appId, active, extra = {}) => yap.invoke('meeting_detect_simulate', { appId, active, ...extra });

/** you.wav / them.wav for the next recording: `you` / `them` = silent stretches. */
function meetingAudio({ seconds = 20, you = [], them = [] } = {}) {
  fs.mkdirSync(audioDir, { recursive: true });
  writeWav(path.join(audioDir, 'you.wav'), { seconds, gaps: you });
  writeWav(path.join(audioDir, 'them.wav'), { seconds, gaps: them });
}

/** The window a meeting question shows in (Rust picks: the notepad if on screen, else the main window). */
const pageOf = (yap, q) => (q.surface === 'notepad' ? yap.notepad : yap.main);

/** "Record notes" on `appId`'s call (started with `extra`); returns the new note's id. */
async function recordCall(yap, main, appId, label, extra = {}) {
  await simulate(yap, appId, true, extra);
  const prompt = toast(main, `${label} detected`);
  await prompt.getByRole('button', { name: 'Record notes' }).click();
  await expect.poll(() => recording(yap), { timeout: 15_000 }).toBe(true);
  return (await meeting(yap)).noteId;
}

test.beforeEach(async ({ yap }) => {
  meetingAudio({ seconds: 20, them: [[0, 20]] });
  await yap.invoke('e2e_call_audio', { call: null });
});

test.afterEach(async ({ yap }) => {
  if (await recording(yap)) {
    await yap.invoke('meeting_pause');
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  }
  for (const c of (await status(yap)).calls) await simulate(yap, c.app, false);
  await yap.invoke('e2e_call_audio', { call: null });
  await yap.invoke('e2e_meeting_quiet', {});
});

test('the call on another microphone: asked once per pair, and "Use" moves "You" to it', async ({ yap, main, shot }) => {
  // Teams uses the headset; Yap's order puts "You" on the laptop's mic.
  await yap.invoke('e2e_call_audio', { call: { mic: { id: `e2e-${HEADSET.id}`, name: HEADSET.name } } });
  await recordCall(yap, main, 'teams', 'Teams call');
  await expect.poll(() => question(yap).then((q) => q?.kind), { timeout: 10_000 }).toBe('mic');
  let q = await question(yap);
  expect(q.title).toBe('Teams is using Contoso H2 but Yap is using Northwind Audio');
  expect([q.primary.label, q.secondary.label]).toEqual(['Use Contoso H2', 'Not now']);
  let card = toast(pageOf(yap, q), 'Teams is using Contoso H2');
  await expect(card).toBeVisible();
  await shot(pageOf(yap, q), '01-mic-mismatch');

  // Not now: never again for this call and pair.
  await card.getByRole('button', { name: 'Not now' }).click();
  await expect.poll(() => question(yap)).toBeNull();
  await main.waitForTimeout(1_500);
  expect(await question(yap)).toBeNull();

  // The call moves to another microphone: a new pair, asked again; "Use"
  // moves "You" to it for this meeting (the order stays as it was).
  await yap.invoke('e2e_call_audio', { call: { mic: { id: `e2e-${USB.id}`, name: USB.name } } });
  await expect.poll(() => question(yap).then((q) => q?.title), { timeout: 10_000 }).toBe(
    'Teams is using Fabrikam USB Mic but Yap is using Northwind Audio'
  );
  q = await question(yap);
  card = toast(pageOf(yap, q), 'Teams is using Fabrikam USB Mic');
  await card.getByRole('button', { name: 'Use Fabrikam USB Mic' }).click();
  await expect
    .poll(() => logCount(yap, 'e2e: "You" moves to the microphone asked for'), { timeout: 10_000 })
    .toBe(1);
  expect(await recording(yap)).toBe(true);
  // Now on the call's microphone: nothing more to say.
  await main.waitForTimeout(1_500);
  expect(await question(yap)).toBeNull();
  expect(yap.readJson('config.json').micOrder).toEqual([{ id: `e2e-${LAPTOP.id}`, name: LAPTOP.name }]);

  // A software microphone (Krisp and the like, fed by a real one) is never a mismatch.
  await yap.invoke('e2e_call_audio', { call: { mic: { id: 'e2e-krisp', name: 'Microphone (Krisp)', isVirtual: true } } });
  await main.waitForTimeout(1_500);
  expect(await question(yap)).toBeNull();
  await closeToasts(main);
});

test('muted in the call, "Yap can\'t hear your mic" stays quiet until you unmute', async ({ yap, main }) => {
  // 4 s of the meeting instead of 3 minutes: the call talks, "You" says nothing.
  await yap.invoke('e2e_meeting_quiet', { quietSecs: 4, talkSecs: 1 });
  meetingAudio({ seconds: 40, you: [[0, 40]] });
  await yap.invoke('e2e_call_audio', { call: { muted: true } });
  const warning = toast(main, "Yap can't hear your mic");
  await recordCall(yap, main, 'zoom', 'Zoom call');
  await expect.poll(() => logCount(yap, 'call audio: muted in the call'), { timeout: 10_000 }).toBeGreaterThan(0);
  // Well past 4 s of quiet (4× real time): not said.
  await main.waitForTimeout(3_000);
  await expect(warning).toHaveCount(0);

  // Unmuted, and the mic still hears nothing: now it's worth saying.
  await yap.invoke('e2e_call_audio', { call: { muted: false } });
  await expect(warning).toBeVisible({ timeout: 10_000 });
  await expect(warning).toContainText("Yap hasn't heard your mic");
  await closeToasts(main);
});

test('the PC asleep mid-meeting: paused, a line in its place, and Resume carries on', async ({ yap, main, shot }) => {
  await newMeetingNote(main, 'Lid closed');
  await main.getByRole('button', { name: 'Record' }).click();
  await expect.poll(() => recording(yap), { timeout: 10_000 }).toBe(true);
  const noteId = (await meeting(yap)).noteId;
  await main.waitForTimeout(1_500);

  // 12 minutes asleep: the recording pauses cleanly (no summary), the line
  // goes in after its last chunk, and Yap offers to carry on.
  await yap.invoke('e2e_sleep', { asleepMs: 12 * 60_000 + 20_000 });
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expect
    .poll(() => noteById(yap, noteId)?.transcript?.find((s) => s.asleepMs != null) ?? null, { timeout: 20_000 })
    .toMatchObject({ source: 'you', text: '', asleepMs: 740_000 });
  await expect.poll(() => question(yap).then((q) => q?.kind), { timeout: 10_000 }).toBe('asleep');
  const q = await question(yap);
  expect(q.title).toBe('Your PC was asleep');
  expect(q.body).toContain('for the 12 min it slept');
  const card = toast(pageOf(yap, q), 'Your PC was asleep');
  await expect(card).toBeVisible();
  await shot(pageOf(yap, q), '02-asleep-resume');

  // In the transcript, where it happened: the Notes view's, and the
  // notepad's, where it stands for the paused divider.
  const log = main.getByRole('log', { name: 'Meeting transcript' });
  await expect(log.getByRole('separator', { name: 'Your PC was asleep' })).toHaveText('Your PC was asleep for 12 min');
  await yap.invoke('notepad_open', { noteId });
  await yap.notepad.getByRole('tab', { name: 'Transcript' }).click();
  const pad = yap.notepad.getByRole('log', { name: 'Transcript' });
  await expect(pad.getByRole('separator', { name: 'Your PC was asleep' })).toHaveText('Your PC was asleep for 12 min');
  await expect(pad.getByRole('separator', { name: 'Recording paused here' })).toHaveCount(0);
  await shot(yap.notepad, '02b-asleep-line');

  await card.getByRole('button', { name: 'Resume' }).click();
  await expect.poll(() => meeting(yap), { timeout: 15_000 }).toMatchObject({ recording: true, noteId });
  await expect.poll(() => question(yap)).toBeNull();
  await closeToasts(main);
});

test('after a sleep, the call back on the mic within 2 minutes carries on in the same note', async ({ yap, main }) => {
  const noteId = await recordCall(yap, main, 'webex', 'Webex call');
  await yap.invoke('e2e_sleep', { asleepMs: 3 * 60_000 });
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  // Waking forgets the calls; Webex is back on the mic: no "detected" prompt,
  // the same note records again.
  expect((await status(yap)).calls).toEqual([]);
  await simulate(yap, 'webex', true);
  await expect.poll(() => meeting(yap), { timeout: 15_000 }).toMatchObject({ recording: true, noteId });
  await expect(toast(main, 'Webex call detected')).toHaveCount(0);
  await expect(toast(main, 'Back on your Webex call')).toBeVisible({ timeout: 10_000 });
  expect(meetingNotes(yap).filter((n) => n.id === noteId)).toHaveLength(1);
  await closeToasts(main);
});

test('another call while one records offers to switch: Keep recording, or Switch to a new note', async ({ yap, main, shot }) => {
  const first = await recordCall(yap, main, 'goto', 'GoTo Meeting call');
  // Whereby starts while GoTo's notes record.
  await simulate(yap, 'whereby', true);
  let prompt = toast(main, 'Switch to Whereby?');
  await expect(prompt).toBeVisible({ timeout: 10_000 });
  await expect(prompt).toContainText('Switching ends those notes and starts new ones.');
  expect((await status(yap)).prompt).toMatchObject({ kind: 'switch', answers: ['switch', 'keep'] });
  await shot(main, '03-switch-prompt');
  await prompt.getByRole('button', { name: 'Keep recording' }).click();
  await expect(prompt).toHaveCount(0);
  expect(await meeting(yap)).toMatchObject({ recording: true, noteId: first });
  await simulate(yap, 'whereby', false);

  // A Jitsi call next: Switch ends GoTo's notes and starts Jitsi's.
  await simulate(yap, 'jitsi', true);
  prompt = toast(main, 'Switch to Jitsi Meet?');
  await prompt.getByRole('button', { name: 'Switch' }).click();
  await expect
    .poll(async () => {
      const s = await meeting(yap);
      return s.recording && s.noteId !== first;
    }, { timeout: 30_000 })
    .toBe(true);
  const second = (await meeting(yap)).noteId;
  expect(noteById(yap, second).title).toMatch(/^Jitsi Meet call · /);
  expect((await status(yap)).calls.find((c) => c.app === 'jitsi')).toMatchObject({ noteId: second });
  await closeToasts(main);
});

test('a call that ends but still looks on asks "Stop taking notes?"; back within 2 minutes, it carries on', async ({
  yap,
  main,
  shot,
}) => {
  await setConfig(yap, main, { meetingCallEnd: 'stop' });
  try {
    const noteId = await recordCall(yap, main, 'meet', 'Google Meet call');
    // Meet let go of the mic, but its tab still shows the meeting: asked,
    // not stopped, even with "Stop and summarise automatically".
    await yap.invoke('e2e_call_audio', { call: { stillOn: true } });
    await simulate(yap, 'meet', false);
    const unsure = toast(main, 'Stop taking notes?');
    await expect(unsure).toBeVisible({ timeout: 10_000 });
    await expect(unsure).toContainText('Google Meet let go of your microphone, but the call still looks open.');
    expect((await status(yap)).prompt).toMatchObject({ kind: 'unsure', answers: ['stop', 'keep'] });
    expect(await recording(yap)).toBe(true);
    await shot(main, '04-stop-taking-notes');

    // The tab closes: sure now, and it stops as the setting says.
    await yap.invoke('e2e_call_audio', { call: { stillOn: false } });
    await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
    await expect(unsure).toHaveCount(0);

    // Meet back on the mic within 2 minutes (a dropped connection): the
    // same note records again, no prompt.
    const carried = 'meeting detect: the call came back; carrying on in the same note';
    const before = logCount(yap, carried);
    await simulate(yap, 'meet', true);
    await expect.poll(() => meeting(yap), { timeout: 20_000 }).toMatchObject({ recording: true, noteId });
    await expect.poll(() => logCount(yap, carried)).toBe(before + 1);
    // Said once the notices before it ("Google Meet call ended", its
    // summary's) have had their turn: one at a time (ui/noticePolicy.js).
    await expect(toast(main, 'Back on your Google Meet call')).toBeVisible({ timeout: 30_000 });
    await expect(toast(main, 'Google Meet call detected')).toHaveCount(0);
  } finally {
    await setConfig(yap, main, { meetingCallEnd: 'ask' });
    await closeToasts(main);
  }
});

test('the third meeting of an app\'s calls, stopped by hand: "Start notes automatically?" once', async ({ yap, main, shot }) => {
  // Two Slack huddles recorded before; this is the third.
  const noteId = await recordCall(yap, main, 'slack', 'Slack huddle', { recorded: 2 });
  const words = 'we agreed to ship the new onboarding on Thursday and Priya will send the release notes to the team before then';
  await yap.invoke('e2e_meeting_feed', { segments: [{ source: 'them', text: words, ts: Math.floor(Date.now() / 1000) }] });
  await yap.invoke('meeting_end', { origin: 'settings' });
  await expect.poll(() => recording(yap), { timeout: 20_000 }).toBe(false);
  await expect.poll(() => question(yap).then((q) => q?.kind), { timeout: 15_000 }).toBe('autostart');
  const q = await question(yap);
  expect(q).toMatchObject({ noteId, title: 'Start notes automatically for Slack huddles?' });
  const card = toast(pageOf(yap, q), 'Start notes automatically for Slack huddles?');
  await expect(card).toBeVisible();
  await shot(pageOf(yap, q), '05-auto-start-offer');
  await card.getByRole('button', { name: 'Start automatically' }).click();
  await expect.poll(() => yap.readJson('config.json').meetingAutoStartApps).toEqual(['slack']);
  // Asked once ever: written down.
  expect(yap.readJson('call_apps.json')).toMatchObject({ recorded: { slack: 3 }, offered: ['slack'] });
  await simulate(yap, 'slack', false);

  // The next huddle counts down by itself (Not now cancels it).
  await simulate(yap, 'slack', true);
  await expect.poll(() => status(yap).then((s) => s.prompt?.autoStartAt ?? null), { timeout: 10_000 }).not.toBeNull();
  const counting = toast(main, 'Slack huddle detected');
  await expect(counting).toContainText(/Starting notes in \d/);
  await counting.getByRole('button', { name: 'Not now' }).click();
  await expect(counting).toHaveCount(0);
  await simulate(yap, 'slack', false);

  // Settings → Meetings → Ask about calls in lists it, with a way back.
  await openSettings(main, 'Meetings');
  await settingsDialog(main).getByRole('button', { name: /^Ask about calls in/ }).click();
  const auto = settingsDialog(main).getByRole('group', { name: 'Start notes automatically in' });
  const slack = auto.getByRole('button', { name: 'Slack', exact: true });
  await expect(slack).toHaveAttribute('aria-pressed', 'true');
  await shot(main, '06-settings-auto-start-apps');
  await slack.click();
  await expect(slack).toHaveAttribute('aria-pressed', 'false');
  await expect.poll(() => yap.readJson('config.json').meetingAutoStartApps).toEqual([]);
  await closeSettings(main);
  await closeToasts(main);
});
