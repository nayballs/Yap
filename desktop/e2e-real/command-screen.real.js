// The robot on a Command that reads the window in front
// (dictation/command_screen.rs, 2026-10-31; Wispr Flow's Instruct Mode, on
// this PC): real windows, read over UI Automation, the draft typed where the
// cursor is.
// - An Edge page standing in for webmail (a made-up mail above a reply box,
//   something already typed in the box): "Hey Jarvis, reply saying Thursday
//   works" sends the mail's text to the Commands model — the box's own text
//   never — and the draft is typed into the box after what was there.
// - The test pad (a native window): the mail in its rich text box, the
//   reply typed into its plain box; a plain Command ("write an email to Sam
//   about the budget") sends no window at all; the rewrite shortcut's "make
//   it shorter" right after rewrites the draft in place, as after any
//   Command.
//
// `realtest_command_text` / `realtest_rewrite_text` stand in for the speech:
// the window in front becomes the target as at a recording's start. A
// stand-in model on this PC answers (e2e/support/fake-llm.js: Commands'
// draft, "Thursday afternoon works for me" when it was given a window).
// Written 2026-10-31, not yet run.
import { test, expect } from './support/robot.js';
import { edgeExe } from './support/targets.js';
import { startFakeLlm, rewriteWindow, rewriteInstruction } from '../e2e/support/fake-llm.js';

const MAIL = [
  'Re: Q3 review',
  'From: Sam Lee <sam.lee@example.com>',
  'Hi Alex,',
  "The finance pack won't be ready by Wednesday, so could we move the Q3 review to Thursday afternoon instead?",
  'Thanks, Sam',
];
const DRAFT = 'Thursday afternoon works for me, see you then.';
const SHORTER = 'Thursday works.';
const ALONE = 'Written from the words alone.';
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let llm;
test.beforeAll(async ({ robot }) => {
  llm = await startFakeLlm();
  llm.reply('rewrite', (user) => (rewriteWindow(user) ? DRAFT : /shorter/i.test(rewriteInstruction(user)) ? SHORTER : ALONE));
  const cfg = await robot.yap.invoke('get_config');
  await robot.configure({
    agentName: 'Jarvis',
    cleanupStyles: false,
    commandsScreen: true,
    rewriteLastDictation: true,
    // Commands on a model on this PC (a server at 127.0.0.1): the window may go.
    llmScopes: {
      ...cfg.llmScopes,
      voiceAgent: { enabled: true, provider: 'custom', baseUrl: llm.base, model: 'fake-commands', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false },
    },
  });
});

test.afterAll(async ({ robot }) => {
  const cfg = await robot.yap.invoke('get_config').catch(() => null);
  if (cfg) await robot.configure({ llmScopes: { ...cfg.llmScopes, voiceAgent: { ...cfg.llmScopes.voiceAgent, enabled: false } } }).catch(() => {});
  await llm?.close();
});

/** A Command by name in `target` (brought to the front), `text` as what was heard. */
const command = (robot, target, text) =>
  robot.withFocus(target, true, () => robot.yap.invoke('realtest_command_text', { text, hwnd: target.hwnd }));

/** The rewrite shortcut in `target`, `instruction` as what was said. */
const rewrite = (robot, target, instruction) =>
  robot.withFocus(target, true, () => robot.yap.invoke('realtest_rewrite_text', { text: instruction, hwnd: target.hwnd }));

const drafts = () => llm.requests.filter((r) => r.kind === 'rewrite');

test.describe('in Edge', () => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');

  test('webmail: the mail above the box goes to the model, the box\'s own text never, and the draft is typed after it', async ({ robot }) => {
    const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');
    const page = await robot.page(
      `<div id="mail">${MAIL.map((l) => `<p>${esc(l)}</p>`).join('')}</div><textarea id="box" aria-label="Reply" autofocus></textarea>`,
      'mail',
    );
    // Something typed already: the box is where the draft goes, never read.
    const typed = 'Draft so far TOPAZ17 ';
    await page.set(typed);
    const mark = robot.logMark();
    const d0 = drafts().length;

    const out = await command(robot, page, 'Hey Jarvis, reply saying Thursday works');
    expect(out).toMatchObject({ outcome: 'pasted', screen: 'rule:text', text: DRAFT });
    expect(drafts().length).toBe(d0 + 1);
    const window = rewriteWindow(drafts().at(-1).user);
    expect(window).toContain('could we move the Q3 review to Thursday afternoon');
    expect(window).toContain('Thanks, Sam');
    expect(window).not.toContain('TOPAZ17');
    // Typed where the cursor was, after what was there.
    await expect.poll(() => page.value()).toBe(`${typed}${DRAFT}`);
    // The log counts, never says.
    const said = robot.logSince(mark);
    expect(said).toContain('Command: read the window in front');
    for (const line of said.split('\n').filter((l) => / INFO | WARN | ERROR /.test(l))) {
      expect(line).not.toMatch(/Q3 review|Thursday|TOPAZ17/);
    }
  });
});

test('the test pad: the mail in another box of the window, the reply typed into its own; a plain Command sends no window; "make it shorter" rewrites the draft', async ({
  robot,
}) => {
  const { pad } = robot;
  await pad.set('rich', MAIL.join('\n'));
  await pad.set('box', 'PADMARK91 ');
  const d0 = drafts().length;

  const out = await command(robot, pad, 'Hey Jarvis, reply saying Thursday works');
  expect(out).toMatchObject({ outcome: 'pasted', screen: 'rule:text', text: DRAFT });
  const window = rewriteWindow(drafts().at(-1).user);
  expect(window).toContain('could we move the Q3 review to Thursday afternoon');
  expect(window).not.toContain('PADMARK91');
  await expect.poll(() => pad.text('box')).toBe(`PADMARK91 ${DRAFT}`);

  // Right after it, the rewrite shortcut with nothing selected rewrites the
  // draft where it is, as after any Command (an edit: no window).
  const shorter = await rewrite(robot, pad, 'make it shorter');
  expect(shorter).toMatchObject({ last: true, outcome: 'replaced', screen: '' });
  expect(rewriteWindow(drafts().at(-1).user)).toBe('');
  await expect.poll(() => pad.text('box')).toBe(`PADMARK91 ${SHORTER}`);

  // A plain Command: the words alone, nothing of the window.
  await pad.set('box', '');
  await sleep(300);
  const plain = await command(robot, pad, 'Hey Jarvis, write an email to Sam about the budget');
  expect(plain).toMatchObject({ outcome: 'pasted', screen: '', text: ALONE });
  expect(rewriteWindow(drafts().at(-1).user)).toBe('');
  expect(drafts().at(-1).user).not.toContain('Q3 review');
  await expect.poll(() => pad.text('box')).toBe(ALONE);
  expect(drafts().length).toBe(d0 + 3);
});
