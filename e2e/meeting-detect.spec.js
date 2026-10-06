// Call detection (src-tauri/src/meeting_detect.rs): a call starting offers to
// take notes, "Record notes" creates a meeting note and records it, the call
// ending offers to stop and summarise, and Settings → General → Meetings
// decides whether, how and about which apps Yap asks.
// Test mode reads no registry: the debug-only `meeting_detect_simulate` hook
// stands in for an app taking or letting go of the mic (no debounce; its
// `fadeMs` shortens a prompt's 30 s fade), and a recording opens no audio
// device (it's silent here; e2e.rs), so it runs the same with or without a
// microphone. Each test uses its own call app — "Not now" quiets an app for
// 5 minutes, and the tests share one instance.
import {
  test,
  expect,
  openView,
  openSettings,
  closeSettings,
  closeToasts,
  expectStore,
  settingsDialog,
  mainOnScreen,
} from './support/fixtures.js';

test.use({ yapOptions: { name: 'meeting-detect' } });

const toast = (main, title) => main.getByRole('status').filter({ hasText: title });
/** A call starting (or ending). The prompt goes in-app only while the main
 *  window is on screen, so a starting call puts it back first (mainOnScreen). */
const simulate = async (yap, appId, active, fadeMs) => {
  if (active) await mainOnScreen(yap);
  return yap.invoke('meeting_detect_simulate', { appId, active, ...(fadeMs ? { fadeMs } : {}) });
};
const status = (yap) => yap.invoke('meeting_detect_status');
const meetingNotes = (yap) => (yap.readJson('notes.json')?.notes ?? []).filter((n) => n.source === 'meeting');
/** The Meetings card in Settings → General, and its "Ask about calls in" list. */
const meetingsCard = (main) => settingsDialog(main).getByRole('group', { name: 'Meetings' });
const callApps = (main) => meetingsCard(main).getByRole('group', { name: 'Ask about calls in' });

test('a call starting offers to take notes, and "Not now" leaves it alone', async ({ yap, main, shot }) => {
  await simulate(yap, 'webex', true);
  const prompt = toast(main, 'Webex call detected');
  await expect(prompt).toBeVisible();
  await expect(prompt).toContainText("Let people know you're taking notes.");
  await expect(prompt.getByRole('button', { name: 'Record notes' })).toBeVisible();
  await expect(prompt.getByRole('button', { name: 'Not now' })).toBeVisible();
  await expect(prompt.getByRole('button', { name: "Don't ask for Webex" })).toBeVisible();
  await shot(main, '01-prompt-call-detected');

  await prompt.getByRole('button', { name: 'Not now' }).click();
  await expect(prompt).toHaveCount(0);
  const s = await status(yap);
  expect(s.prompt).toBeNull();
  expect(s.calls.map((c) => c.app)).toEqual(['webex']);
  expect(await yap.invoke('meeting_state')).toMatchObject({ recording: false });
  expect(meetingNotes(yap)).toHaveLength(0);

  // Asked once per call: the call ending and the next Webex call within
  // 5 minutes stay quiet.
  await simulate(yap, 'webex', false);
  await simulate(yap, 'webex', true);
  await main.waitForTimeout(1_000);
  await expect(toast(main, 'Webex call')).toHaveCount(0);
  await simulate(yap, 'webex', false);
  expect((await status(yap)).calls).toEqual([]);
});

test('a call that ends before anyone answers takes its prompt with it', async ({ yap, main }) => {
  await simulate(yap, 'zoom', true);
  const prompt = toast(main, 'Zoom call detected');
  await expect(prompt).toBeVisible();
  await simulate(yap, 'zoom', false);
  await expect(prompt).toHaveCount(0);
  expect((await status(yap)).prompt).toBeNull();
});

test('a prompt left alone fades, and that counts as "Not now"', async ({ yap, main }) => {
  // 1.5 s instead of 30 s (debug-only, per simulated call).
  await simulate(yap, 'whereby', true, 1_500);
  const prompt = toast(main, 'Whereby call detected');
  await expect(prompt).toBeVisible();
  expect((await status(yap)).prompt).toMatchObject({ app: 'whereby', fadeMs: 1_500 });
  await expect(prompt).toHaveCount(0, { timeout: 6_000 });
  await expect.poll(() => status(yap).then((s) => s.prompt)).toBeNull();
  expect((await status(yap)).calls.map((c) => c.app)).toEqual(['whereby']);

  // As after "Not now", the app's next call within 5 minutes stays quiet.
  await simulate(yap, 'whereby', false);
  await simulate(yap, 'whereby', true);
  await main.waitForTimeout(1_000);
  await expect(toast(main, 'Whereby call')).toHaveCount(0);
  await simulate(yap, 'whereby', false);
});

test('"Record notes" records the call into a meeting note; the call ending offers to stop and summarise', async ({
  yap,
  main,
  shot,
}) => {
  await simulate(yap, 'meet', true);
  const prompt = toast(main, 'Google Meet call detected');
  await expect(prompt).toBeVisible();
  // The longest name in the prompt's own line under the buttons.
  const never = prompt.getByRole('button', { name: "Don't ask for Google Meet" });
  await expect(never).toBeVisible();
  const [card, link] = [await prompt.boundingBox(), await never.boundingBox()];
  expect(link.x).toBeGreaterThanOrEqual(card.x);
  expect(link.x + link.width).toBeLessThanOrEqual(card.x + card.width);
  expect(link.height).toBeLessThan(24); // one line
  await shot(main, '02-prompt-long-app-name');
  await prompt.getByRole('button', { name: 'Record notes' }).click();

  // The new meeting note opens in Notes, recording. (When it can't record,
  // it says why and leaves no note: meeting-detect-no-mic.spec.js.)
  const title = main.getByPlaceholder('Untitled Note');
  await expect(title).toHaveValue(/^Google Meet call · \d{1,2} [A-Z][a-z]{2}, \d{2}:\d{2}$/, {
    timeout: 15_000,
  });
  await expect(main.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Notes', exact: true }))
    .toHaveAttribute('aria-current', 'page');
  await expect(main.getByRole('button', { name: 'End meeting & summarise' })).toBeVisible();
  await expect(toast(main, 'Recording notes')).toBeVisible();
  const notes = meetingNotes(yap);
  expect(notes).toHaveLength(1);
  expect(notes[0]).toMatchObject({ folder: 'Meetings', noteType: 'meeting', source: 'meeting' });
  expect(await yap.invoke('meeting_state')).toMatchObject({ recording: true, noteId: notes[0].id });
  await shot(main, '03-recording-started');

  // The call ends: stop and summarise? (No "Don't ask" here, and no fade:
  // it's about a recording that's still running.)
  await simulate(yap, 'meet', false);
  const ended = toast(main, 'Google Meet call ended');
  await expect(ended).toBeVisible();
  await expect(ended.getByRole('button', { name: 'Stop and summarise' })).toBeVisible();
  await expect(ended.getByRole('button', { name: 'Keep recording' })).toBeVisible();
  await expect(ended.getByRole('button', { name: /Don't ask/ })).toHaveCount(0);
  expect((await status(yap)).prompt).toMatchObject({ kind: 'end', fadeMs: null });
  await shot(main, '04-prompt-call-ended');

  // Stopping ends the meeting as "End meeting & summarise" does (Rust writes
  // the action plan; meeting_end.rs) and keeps the note open here. The test
  // recording being silent, it asks — in this window, where it was answered —
  // whether it was started by mistake instead of writing a summary.
  await ended.getByRole('button', { name: 'Stop and summarise' }).click();
  await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording), { timeout: 20_000 }).toBe(false);
  await expect(toast(main, 'Started by mistake?')).toBeVisible({ timeout: 20_000 });
  await expect(title).toHaveValue(notes[0].title);
  await shot(main, '05-stopped-and-summarised');
  expect((await status(yap)).prompt).toBeNull();
  await closeToasts(main);
});

test('"Keep recording" carries the notes into a rejoined huddle, which asks again when it ends', async ({
  yap,
  main,
}) => {
  await simulate(yap, 'slack', true);
  await toast(main, 'Slack huddle detected').getByRole('button', { name: 'Record notes' }).click();
  await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording), { timeout: 15_000 }).toBe(true);
  await simulate(yap, 'slack', false);
  const ended = toast(main, 'Slack huddle ended');
  await ended.getByRole('button', { name: 'Keep recording' }).click();
  await expect(ended).toHaveCount(0);

  // Rejoined: no "detected" prompt (Yap is already recording it)…
  await simulate(yap, 'slack', true);
  await main.waitForTimeout(1_000);
  await expect(toast(main, 'Slack huddle detected')).toHaveCount(0);
  expect(await yap.invoke('meeting_state')).toMatchObject({ recording: true });
  // …and its end asks again.
  await simulate(yap, 'slack', false);
  await toast(main, 'Slack huddle ended').getByRole('button', { name: 'Stop and summarise' }).click();
  await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording), { timeout: 20_000 }).toBe(false);
  await expect(toast(main, 'Started by mistake?')).toBeVisible({ timeout: 20_000 });
  await closeToasts(main);
});

test('personal chat apps like Discord aren\'t asked about unless switched on', async ({ yap, main }) => {
  await simulate(yap, 'discord', true);
  await main.waitForTimeout(1_000);
  await expect(toast(main, 'Discord call')).toHaveCount(0);
  const s = await status(yap);
  expect(s.calls.map((c) => c.app)).toEqual(['discord']); // noticed, not asked about
  expect(s.prompt).toBeNull();
  expect(s.apps.find((a) => a.id === 'discord')).toMatchObject({ asks: false, asksByDefault: false });
  expect(s.apps.find((a) => a.id === 'teams')).toMatchObject({ asksByDefault: true });
  await simulate(yap, 'discord', false);
});

test('"Don\'t ask for Teams" stops the prompts for Teams, and Settings shows it', async ({ yap, main, shot }) => {
  await simulate(yap, 'teams', true);
  const prompt = toast(main, 'Teams call detected');
  await prompt.getByRole('button', { name: "Don't ask for Teams" }).click();
  await expect(prompt).toHaveCount(0);

  // Saved as the person's choice, and confirmed in the window.
  const confirmed = toast(main, "Won't ask about Teams calls");
  await expect(confirmed).toBeVisible();
  await confirmed.hover(); // its timer waits while the pointer is on it
  await expect(confirmed.getByRole('button', { name: 'Open Settings' })).toBeVisible();
  await shot(main, '06-wont-ask-confirmation');
  await expectStore(yap, 'config.json', (c) => c.meetingDetectApps?.teams === false);
  const s = await status(yap);
  expect(s.prompt).toBeNull();
  expect(s.apps.find((a) => a.id === 'teams').asks).toBe(false);

  // "Open Settings" lands on General → Meetings, where Teams is off.
  await confirmed.getByRole('button', { name: 'Open Settings' }).click();
  await expect(settingsDialog(main)).toBeVisible();
  await expect(
    settingsDialog(main).getByRole('navigation', { name: 'Settings sections' }).getByRole('button', { name: /^General/ })
  ).toHaveAttribute('aria-current', 'page');
  const teams = callApps(main).getByRole('button', { name: 'Teams', exact: true });
  await expect(teams).toHaveAttribute('aria-pressed', 'false');
  await expect(teams).toBeInViewport();
  await shot(main, '07-settings-teams-off');
  await closeSettings(main);

  // Its next call isn't asked about (it's off; nothing was snoozed).
  await simulate(yap, 'teams', false);
  await simulate(yap, 'teams', true);
  await main.waitForTimeout(1_000);
  await expect(toast(main, 'Teams call detected')).toHaveCount(0);
  await simulate(yap, 'teams', false);

  // Switched back on in Settings: picked up at once, the next call asks.
  await openSettings(main, 'General');
  await teams.click();
  await expect(teams).toHaveAttribute('aria-pressed', 'true');
  await expectStore(yap, 'config.json', (c) => c.meetingDetectApps?.teams === true);
  await expect.poll(() => status(yap).then((st) => st.apps.find((a) => a.id === 'teams').asks)).toBe(true);
  await closeSettings(main);
  await simulate(yap, 'teams', true);
  await expect(toast(main, 'Teams call detected')).toBeVisible();
  await simulate(yap, 'teams', false);
  await expect(toast(main, 'Teams call detected')).toHaveCount(0);
  expect((await status(yap)).calls).toEqual([]);
});

test('"Quietly" keeps the prompt out of the window', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'General');
  const style = meetingsCard(main).getByRole('tablist', { name: 'How Yap asks' });
  await style.getByRole('tab', { name: 'Quietly' }).click();
  await expect(style.getByRole('tab', { name: 'Quietly' })).toHaveAttribute('aria-selected', 'true');
  await expect(meetingsCard(main)).toContainText('No pop-up: it waits in the notification centre and the tray menu');
  await expectStore(yap, 'config.json', (c) => c.meetingDetectStyle === 'quiet');
  await expect.poll(() => status(yap).then((s) => s.style)).toBe('quiet');
  await closeSettings(main);

  // No card in the window: the prompt waits for the notification centre
  // (test runs post no Windows notifications) and the tray menu.
  await simulate(yap, 'jitsi', true);
  await expect.poll(() => status(yap).then((s) => s.prompt?.app)).toBe('jitsi');
  expect((await status(yap)).prompt).toMatchObject({ kind: 'start', quiet: true, inApp: false });
  await main.waitForTimeout(1_000);
  await expect(toast(main, 'Jitsi Meet call')).toHaveCount(0);
  await shot(main, '08-quiet-no-card');
  await simulate(yap, 'jitsi', false);
  expect((await status(yap)).prompt).toBeNull();

  // Back to pop-ups.
  await openSettings(main, 'General');
  await style.getByRole('tab', { name: 'Pop-up' }).click();
  await expectStore(yap, 'config.json', (c) => c.meetingDetectStyle === 'popup');
  await expect.poll(() => status(yap).then((s) => s.style)).toBe('popup');
  await expect(dialog).toBeVisible();
});

test('Settings lists how Yap asks and the apps it asks about', async ({ yap, main, shot }) => {
  await openSettings(main, 'General');
  const meetings = meetingsCard(main);
  await meetings.scrollIntoViewIfNeeded();
  await expect(
    meetings.getByRole('tablist', { name: 'How Yap asks' }).getByRole('tab', { name: 'Pop-up' })
  ).toHaveAttribute('aria-selected', 'true');
  await expect(meetings).toContainText('A card in Yap, or on the Yap bar while you work in another app');

  // One switch per call app, showing what Yap does now (the person's
  // choice, else the app's default); two columns.
  const { apps } = await status(yap);
  expect(apps.map((a) => a.label)).toEqual([
    'Teams', 'Zoom', 'Google Meet', 'Webex', 'Slack', 'GoTo Meeting', 'Whereby', 'Jitsi Meet',
    'Discord', 'WhatsApp', 'Signal', 'Telegram',
  ]);
  for (const app of apps) {
    await expect(callApps(main).getByRole('button', { name: app.label, exact: true }))
      .toHaveAttribute('aria-pressed', String(app.asks));
  }
  const zoom = await callApps(main).getByRole('button', { name: 'Zoom', exact: true }).boundingBox();
  const teams = await callApps(main).getByRole('button', { name: 'Teams', exact: true }).boundingBox();
  expect(Math.abs(zoom.y - teams.y)).toBeLessThan(4); // side by side
  await shot(main, '09-settings-meetings');

  // Switching one on saves it and Yap asks about its calls at once.
  const whatsapp = callApps(main).getByRole('button', { name: 'WhatsApp', exact: true });
  await whatsapp.click();
  await expectStore(yap, 'config.json', (c) => c.meetingDetectApps?.whatsapp === true);
  await closeSettings(main);
  await simulate(yap, 'whatsapp', true);
  await expect(toast(main, 'WhatsApp call detected')).toBeVisible();
  await simulate(yap, 'whatsapp', false);
  await openSettings(main, 'General');
  await whatsapp.click();
  await expectStore(yap, 'config.json', (c) => c.meetingDetectApps?.whatsapp === false);
});

test('the Settings toggle turns call detection off', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'General');
  const meetings = dialog.getByRole('group', { name: 'Meetings' });
  const toggle = meetings.getByRole('button', { name: 'Detect calls and offer to take notes' });
  await expect(toggle).toHaveAttribute('aria-pressed', 'true');
  await expect(meetings.getByText("Recording a call? Let people know you're taking notes.")).toBeVisible();

  await toggle.click();
  await expect(toggle).toHaveAttribute('aria-pressed', 'false');
  await expectStore(yap, 'config.json', (c) => c.meetingDetection === false);
  await expect.poll(() => status(yap).then((s) => s.enabled)).toBe(false);
  // How and about which apps it asks wait, greyed out, until it's back on.
  await expect(meetings.getByRole('tab', { name: 'Quietly' })).toBeDisabled();
  await expect(callApps(main).getByRole('button', { name: 'Zoom', exact: true })).toBeDisabled();
  await meetings.scrollIntoViewIfNeeded();
  await shot(main, '10-settings-detection-off');
  await closeSettings(main);

  await simulate(yap, 'goto', true);
  await main.waitForTimeout(1_000);
  await expect(toast(main, 'GoTo Meeting call')).toHaveCount(0);
  expect((await status(yap)).calls).toEqual([]);

  // Back on: the next call is offered again.
  await openSettings(main, 'General');
  await toggle.click();
  await expectStore(yap, 'config.json', (c) => c.meetingDetection === true);
  await expect.poll(() => status(yap).then((s) => s.enabled)).toBe(true);
  await expect(callApps(main).getByRole('button', { name: 'Zoom', exact: true })).toBeEnabled();
  await closeSettings(main);
  await simulate(yap, 'goto', true);
  await expect(toast(main, 'GoTo Meeting call detected')).toBeVisible();
  await simulate(yap, 'goto', false);
  await expect(toast(main, 'GoTo Meeting call detected')).toHaveCount(0);
  await openView(main, 'Home');
});
