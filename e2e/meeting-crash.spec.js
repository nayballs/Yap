// A crash mid-meeting (src-tauri/src/meeting_audio.rs): the meeting's audio
// is on disk as it's recorded, so when the test instance is killed outright
// mid-meeting — no exit hooks, the last chunks never transcribed — the next
// start finds it:
// - the note says so ("Yap closed while this meeting was recording…") with
//   Transcribe it, and Yap offers it in one place (notice.rs: a toast in the
//   main window while it's looked at);
// - Transcribe it puts the part that never became lines into the note, where
//   it belongs, then the end of the meeting runs as a stop would have
//   (meeting_end.rs: its stop mark, the action plan — no AI set up here,
//   so "needsAi");
// - "Transcribe again" (the Notes view's menu) afterwards keeps the summary
//   and the name on a line.
// Two instances on one portable data folder, one after the other; neither
// is the fixture's. The recorder plays you.wav / them.wav at 4× (e2e.rs).
import fs from 'node:fs';
import path from 'node:path';
import { test, expect, openView } from './support/fixtures.js';
import { newMeetingNote, meetingNote } from './support/meetings.js';
import { writeWav } from './support/wav.js';
import { launchYap, RUNS } from './support/yap.js';

const NAME = 'meeting-crash';
const audioDir = path.join(RUNS, 'meeting-crash-wavs');
const options = {
  name: NAME,
  env: { YAP_E2E_MEETING_AUDIO: audioDir, YAP_E2E_MEETING_SPEED: '4' },
  config: { dictionary: [{ from: 'STT stub', to: 'Heard', fuzzy: false }] },
};

const PLAYED = 'e2e: the test audio has played to the end';
const logCount = (yap, text) => fs.readFileSync(yap.logPath, 'utf8').split(text).length - 1;
const secsOf = (seg) => Number(/received ([\d.]+)s/.exec(seg.text)?.[1] ?? NaN);
const sum = (xs) => xs.reduce((a, b) => a + b, 0);
const secs = (t, source) => sum(t.filter((s) => s.source === source && s.text).map(secsOf));
const TITLE = 'Supplier call';

/** Poll a JSON store of `yap` until `check` holds. */
async function store(yap, file, check, message) {
  await expect.poll(() => {
    try {
      return check(yap.readJson(file));
    } catch {
      return false;
    }
  }, { message, timeout: 20_000 }).toBe(true);
}

test.setTimeout(180_000);

test('a meeting Yap was killed during is transcribed at the next start, then summarised', async ({ shot }) => {
  // 48 s a side, with pauses where the recorder cuts.
  fs.mkdirSync(audioDir, { recursive: true });
  const them = [[4.5, 4.8], [9.5, 9.9], [14.2, 14.6], [19.5, 19.9], [24.5, 24.9], [29.4, 29.8], [34.5, 34.9], [39.4, 39.8], [44.5, 44.9]];
  writeWav(path.join(audioDir, 'you.wav'), { seconds: 48, gaps: [[0, 2], [10, 20], [30, 48]] });
  writeWav(path.join(audioDir, 'them.wav'), { seconds: 48, gaps: them });

  let first = await launchYap(options);
  let second = null;
  try {
    const main = first.main;
    await newMeetingNote(main, TITLE);
    await main.getByPlaceholder('Start writing…').fill('Check the delivery dates');
    await main.getByRole('button', { name: 'Record' }).click();
    // The first lines are in; the audio plays out, and its last stretch
    // waits in the recorder for its chunk.
    await store(first, 'notes.json', (s) => (meetingNote(s, TITLE)?.transcript || []).some((t) => t.source === 'them'), 'the first lines');
    await expect.poll(() => logCount(first, PLAYED), { timeout: 30_000 }).toBe(1);
    // A moment for the audio's writer (every half second) to have it all on disk.
    await main.waitForTimeout(1_500);
    const before = meetingNote(first.readJson('notes.json'), TITLE);
    const noteId = before.id;
    const heard = secs(before.transcript, 'them');
    expect(heard).toBeLessThan(47);
    await shot(main, '01-before-the-crash');

    // The crash: killed outright, mid-meeting.
    await first.kill();
    const folder = path.join(first.dataDir, 'meeting-audio', String(noteId));
    const metaFile = fs.readdirSync(folder).find((f) => f.endsWith('.json'));
    const meta = JSON.parse(fs.readFileSync(path.join(folder, metaFile), 'utf8'));
    expect(meta.endedMs ?? null).toBeNull();
    // A summary written earlier stays through all that follows (set while
    // Yap is down).
    const notes = first.readJson('notes.json');
    notes.notes.find((n) => n.id === noteId).enhancedContent = '## Summary\n- Delivery moves to Friday';
    fs.writeFileSync(path.join(first.dataDir, 'notes.json'), JSON.stringify(notes, null, 2));

    // The next start finds what never became lines.
    second = await launchYap({ ...options, restart: true });
    const yap = second;
    await expect
      .poll(() => yap.invoke('meeting_audio_note', { noteId }).then((a) => a.recoverableSecs), { timeout: 20_000 })
      .toBeGreaterThan(0);
    expect(fs.readFileSync(yap.logPath, 'utf8')).toContain('Meeting audio: Yap stopped during a meeting');
    // On the note itself.
    const m = yap.main;
    await openView(m, 'Notes');
    await m.locator('.folder').filter({ hasText: 'Meetings' }).click();
    const row = m.locator('.item').filter({ hasText: TITLE });
    await row.click();
    const banner = m.getByRole('status').filter({ hasText: 'Yap closed while this meeting was recording' });
    await expect(banner).toBeVisible();
    await expect(banner).toContainText('transcribed; the audio is kept');
    await shot(m, '02-recovery-banner');

    // And offered in one place: a toast while the main window is looked at.
    await yap.invoke('e2e_main_on_screen');
    await yap.invoke('e2e_notice_reset');
    await yap.invoke('e2e_meeting_recovery_offer');
    // (The start's own offer may be up too, if the window was looked at then.)
    const offer = m.getByRole('status').filter({ hasText: 'Yap closed during a meeting' }).last();
    await expect(offer).toContainText(new RegExp(`of “${TITLE}” (wasn't|weren't) transcribed`));
    await shot(m, '03-recovery-offer');
    await offer.getByRole('button', { name: 'Transcribe it' }).click();

    // The rest of the meeting is in the note: all 48 s of the call.
    await store(yap, 'notes.json', (s) => Math.abs(secs(meetingNote(s, TITLE).transcript, 'them') - 48) < 0.6, 'the rest transcribed');
    const after = meetingNote(yap.readJson('notes.json'), TITLE);
    expect(after.transcript.slice(0, before.transcript.length).map((t) => t.text)).toEqual(before.transcript.map((t) => t.text));
    expect(after.transcript.map((t) => t.ts)).toEqual([...after.transcript.map((t) => t.ts)].sort((a, b) => a - b));
    // Then the end of the meeting, as a stop: its mark, and the action plan
    // job (no AI model in a test run).
    await store(yap, 'notes.json', (s) => (meetingNote(s, TITLE).breaks || []).includes(meetingNote(s, TITLE).transcript.length), 'the stop mark');
    await expect
      .poll(() => yap.invoke('meeting_summary_status', { noteId }).then((j) => j?.state), { timeout: 20_000 })
      .toBe('needsAi');
    await expect(banner).toHaveCount(0);
    const closed = JSON.parse(fs.readFileSync(path.join(yap.dataDir, 'meeting-audio', String(noteId), metaFile), 'utf8'));
    expect(closed.endedMs).toBeTruthy();
    expect(await yap.invoke('meeting_audio_note', { noteId }).then((a) => a.recoverableSecs)).toBe(0);
    await shot(m, '04-recovered');

    // Transcribe again from the Notes view's menu: the summary and a name
    // put on a line stay.
    const line = after.transcript.findIndex((t) => t.source === 'them' && t.startMs != null);
    await yap.invoke('speaker_rename_lines', { noteId, lines: [line], name: 'Robin Ashe' });
    const cfg = await yap.invoke('get_config');
    await yap.invoke('save_config', { cfg: { ...cfg, dictionary: [{ from: 'STT stub', to: 'Again', fuzzy: false }] } });
    await row.click({ button: 'right' });
    await m.getByRole('menuitem', { name: 'Transcribe again…' }).click();
    const confirm = m.getByRole('alertdialog');
    await expect(confirm).toContainText(`Transcribe “${TITLE}” again?`);
    await confirm.getByRole('button', { name: 'Transcribe again' }).click();
    await store(
      yap,
      'notes.json',
      (s) => meetingNote(s, TITLE).transcript.every((t) => !t.text || t.text.startsWith('[Again:')),
      'transcribed again'
    );
    const again = meetingNote(yap.readJson('notes.json'), TITLE);
    expect(again.enhancedContent).toBe('## Summary\n- Delivery moves to Friday');
    expect(again.content).toBe('Check the delivery dates');
    expect(secs(again.transcript, 'them')).toBeCloseTo(48, 0);
    const named = again.transcript.find((t) => t.speaker && t.source === 'them');
    expect(again.speakers[named.speaker].name).toBe('Robin Ashe');
    await expect(m.getByRole('log', { name: 'Meeting transcript' })).toContainText('[Again: received');
    await shot(m, '05-transcribed-again');
  } finally {
    // The second runs the first's copy of the exe: it goes first.
    await second?.stop().catch(() => {});
    await first.stop().catch(() => {});
  }
});
