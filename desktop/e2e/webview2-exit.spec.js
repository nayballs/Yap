// WebView2's browser process exiting (shell/webview_gone.rs; app-shell.md).
// Every Yap window dies with it while the Rust core goes on: the tray offers
// Restart Yap, and Yap restarts by itself once nothing would be lost, then
// says once, quietly, "Yap restarted its windows after WebView2 stopped".
// - Simulated (`e2e_webview2_gone`, debug builds in test mode): the windows
//   stay up to look at the tray's items and tooltip, the wait while a
//   meeting records, and the restart once it stops.
// - For real: CDP's `Browser.crash` ends WebView2's browser process (or,
//   should a runtime refuse it, the process is ended from outside).
// Yap restarts as a new process from the run folder's copy, on the same
// stdin and stdout: `reattach({ restarted: true })` finds it.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { test, expect } from '@playwright/test';
import { launchYap } from './support/yap.js';

const logText = (yap) => fs.readFileSync(yap.logPath, 'utf8');
/** The note a restart leaves for the next Yap (webview_gone.rs `MARKER`). */
const MARKER = 'webview2_restart.json';
const SAID = /Yap restarted its windows after WebView2 stopped/;

/** The notice, wherever it went: the main window's toast or the bar's card. */
async function noticeShown(yap) {
  await expect
    .poll(
      async () => {
        for (const page of [yap.main, yap.overlay]) {
          if ((await page.getByText(SAID).count().catch(() => 0)) > 0) return true;
        }
        return false;
      },
      { message: 'the notice on the main window or the bar', timeout: 30_000 },
    )
    .toBe(true);
}

/** Yap's own process (the one this harness started) has exited. */
const exited = (yap) => yap.child.exitCode !== null || yap.child.signalCode !== null;

test('simulated: Restart Yap in the tray; Yap waits out a meeting, then restarts by itself and says so once', async () => {
  test.setTimeout(180_000);
  const yap = await launchYap({ name: 'webview2-gone-wait', config: { meetingReminder: 'never' } });
  try {
    const note = await yap.invoke('note_create', { title: 'Weekly sync', folder: 'Meetings' });
    await yap.invoke('meeting_start', { noteId: note.id });
    await expect.poll(() => yap.invoke('meeting_state').then((s) => s.recording)).toBe(true);

    await yap.invoke('e2e_webview2_gone');
    // The tray: Restart Yap at the top of the menu (under the version line),
    // and its line in the tooltip.
    await expect
      .poll(async () => {
        const items = await yap.invoke('e2e_tray');
        return { second: items[1], tooltip: items.find(([id]) => id === 'tooltip')?.[1] ?? '' };
      })
      .toMatchObject({ second: ['restart_yap', 'Restart Yap: its windows stopped'], tooltip: expect.stringContaining("Yap's windows stopped") });
    expect(logText(yap)).toMatch(/tray: Yap's windows stopped: Restart Yap offered/);

    // A meeting records: Yap waits, looking again every 5 s.
    await expect.poll(() => logText(yap)).toMatch(/Not restarting Yap yet: a meeting recording would be lost/);
    await new Promise((r) => setTimeout(r, 7_000));
    expect(exited(yap)).toBe(false);
    expect(logText(yap)).not.toMatch(/Restarting Yap: its windows' WebView2 stopped/);

    // The meeting stops: Yap restarts within a look.
    await yap.invoke('meeting_pause');
    await expect.poll(() => exited(yap), { timeout: 60_000 }).toBe(true);
    expect(logText(yap)).toMatch(/Nothing would be lost: restarting Yap/);
    expect(logText(yap)).toMatch(/Restarting Yap: its windows' WebView2 stopped by=Itself/);

    await yap.reattach({ restarted: true });
    await yap.main.getByRole('navigation', { name: 'Main' }).waitFor();
    await noticeShown(yap);
    // Said once: the note for the new Yap is gone.
    expect(fs.existsSync(path.join(yap.dataDir, MARKER))).toBe(false);
    expect(logText(yap)).toMatch(/Yap restarted after WebView2 stopped by=Itself/);
    // The meeting it waited for is kept.
    await expect.poll(() => yap.invoke('note_get', { id: note.id }).then((n) => n?.title ?? null)).toBe('Weekly sync');
  } finally {
    await yap.stop();
  }
});

test("for real: WebView2's browser process crashes, and Yap restarts by itself and says so", async () => {
  test.setTimeout(180_000);
  const yap = await launchYap({ name: 'webview2-gone-crash' });
  try {
    const cdp = await yap.browser.newBrowserCDPSession();
    cdp.send('Browser.crash').catch(() => {});
    let how = 'Browser.crash';
    try {
      await expect.poll(() => logText(yap), { timeout: 10_000 }).toMatch(/WebView2's browser process exited/);
    } catch {
      // A runtime that refuses Browser.crash: its browser process ended from
      // outside (the one with this run's profile and no --type=).
      how = 'taskkill';
      const dir = path.join(yap.runDir, 'webview2');
      const ps = `Get-CimInstance Win32_Process -Filter "Name='msedgewebview2.exe'" | Where-Object { $_.CommandLine -like '*${dir.replace(/'/g, "''")}*' -and $_.CommandLine -notlike '*--type=*' } | ForEach-Object { $_.ProcessId }`;
      const pids = execFileSync('powershell.exe', ['-NoProfile', '-Command', ps], { encoding: 'utf8' }).split(/\s+/).filter(Boolean);
      for (const pid of pids) execFileSync('taskkill', ['/PID', pid, '/F'], { stdio: 'ignore' });
      await expect.poll(() => logText(yap), { timeout: 20_000 }).toMatch(/WebView2's browser process exited/);
    }
    test.info().annotations.push({ type: 'crashed by', description: how });
    expect(logText(yap)).toMatch(/tray: Yap's windows stopped: Restart Yap offered/);
    await expect.poll(() => exited(yap), { timeout: 60_000 }).toBe(true);
    await yap.reattach({ restarted: true });
    await yap.main.getByRole('navigation', { name: 'Main' }).waitFor();
    await noticeShown(yap);
  } finally {
    await yap.stop();
  }
});
