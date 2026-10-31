// The robot on rich snippets (pasting/rich.rs, 2026-10-31): a snippet whose
// text is formatted in Yap's markdown subset pastes as CF_HTML beside its
// plain text, in the one private, promised paste (read_check.rs: the app
// reads either), and the person's clipboard comes back as ever.
//
// - An Edge page's contenteditable box (an ordinary app, which takes HTML):
//   the bold, the link and the list land as formatting, the spoken words
//   around the snippet as text (HTML-looking words escaped, never markup).
// - The test pad's rich text box (a RichEdit, which takes no HTML): the
//   plain text lands — bullets as "• ", the link as "text (url)" — and the
//   pad saw the HTML offered beside it.
// - A PowerShell prompt in Windows Terminal: the plain text only, joined
//   on one line as a terminal's dictation is, and nothing runs.
import { test, expect, lf } from './support/robot.js';
import { edgeExe, hasWindowsTerminal } from './support/targets.js';

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Made up: a sign-off with a bold name, a link and a list.
const SNIPPET = {
  from: 'my sign off',
  to: 'Best wishes,\n**Robin**\n- [Book a call](https://example.com/b)\n- *Fridays*',
  fuzzy: true,
  snippet: true,
};
const PLAIN = 'Best wishes,\nRobin\n• Book a call (https://example.com/b)\n• Fridays';
const COPIED = 'What you had copied';

// The robot's own dictionary, put back after each test.
let kept = [];
test.beforeEach(async ({ robot }) => {
  kept = (await robot.yap.invoke('get_config')).dictionary ?? [];
  await robot.configure({ dictionary: [...kept, SNIPPET] });
});

test.afterEach(async ({ robot }) => {
  await robot.configure({ dictionary: kept });
});

test('an Edge contenteditable box gets the formatting, the spoken words as text, and the clipboard back', async ({ robot }) => {
  test.skip(!edgeExe(), 'Microsoft Edge is not installed');
  const page = await robot.page(
    '<div id="box" contenteditable="true" role="textbox" aria-multiline="true" aria-label="Message"><p><br></p></div>',
    'rich'
  );
  await page.set('');
  await robot.clipboard('setText', COPIED);
  const r = await robot.dictate(page, 'Thanks for <today> & all. My sign off.');
  expect(r.kind).toBe('other');
  expect(r.text).toBe(`Thanks for <today> & all. ${PLAIN}`);
  expect(r.html).toContain('<b>Robin</b>');
  expect(r.html).toContain('Thanks for &lt;today&gt; &amp; all.');
  const landed = () =>
    page.evaluate(() => {
      const box = document.getElementById('box');
      const texts = (sel) => [...box.querySelectorAll(sel)].map((e) => e.textContent.trim());
      return {
        bold: texts('b, strong'),
        italic: texts('i, em'),
        items: texts('li'),
        links: [...box.querySelectorAll('a')].map((a) => [a.textContent.trim(), a.getAttribute('href')]),
        markup: box.querySelectorAll('script, img, today').length,
        text: box.innerText.replace(/ /g, ' '),
      };
    });
  await expect.poll(async () => (await landed()).items).toEqual(['Book a call', 'Fridays']);
  const got = await landed();
  expect(got.bold).toContain('Robin');
  expect(got.italic).toContain('Fridays');
  expect(got.links).toEqual([['Book a call', 'https://example.com/b']]);
  // The spoken words are text, never markup.
  expect(got.markup).toBe(0);
  expect(got.text).toContain('Thanks for <today> & all. Best wishes,');
  // The read check counted the page's read, and the clipboard came back.
  expect((await robot.lastPaste()).verdict).toBe('pasted');
  await expect.poll(async () => (await robot.clipboard('check', COPIED)).matchesText).toBe(true);
  expect((await robot.clipboard('check')).private).toBe(true);
});

test("the test pad's rich text box takes no HTML: the plain text lands, the HTML offered beside it", async ({ robot }) => {
  const { pad } = robot;
  await pad.set('rich', '');
  await pad.pastes(); // (forget earlier ones)
  await robot.clipboard('setText', COPIED);
  const r = await robot.dictate(pad, 'Thanks. My sign off.');
  expect(r.text).toBe(`Thanks. ${PLAIN}`);
  expect(r.html).toBeTruthy();
  await expect.poll(async () => lf(await pad.text('rich'))).toBe(`Thanks. ${PLAIN}`);
  const pastes = await pad.pastes();
  expect(pastes).toHaveLength(1);
  expect(pastes[0]).toMatchObject({ field: 'rich', text: true, html: true });
  expect(Object.values(pastes[0].private).every(Boolean)).toBe(true);
  expect((await robot.lastPaste()).verdict).toBe('pasted');
  await expect.poll(async () => (await robot.clipboard('check', COPIED)).matchesText).toBe(true);
});

test('a PowerShell prompt gets the plain text on one line, no HTML, and nothing runs', async ({ robot }) => {
  test.skip(!hasWindowsTerminal(), 'Windows Terminal (wt.exe) is not installed');
  const sh = await robot.shell();
  const lines = () =>
    sh
      .text()
      .split(/\r?\n/)
      .map((l) => l.trimEnd())
      .filter(Boolean);
  await expect.poll(() => lines().at(-1), { message: 'the PowerShell prompt', timeout: 20_000 }).toBe('PS>');
  await sleep(500);
  const r = await robot.dictate(sh, 'Thanks. My sign off.');
  expect(r.kind).toBe('terminal (Windows Terminal)');
  expect(r.html).toBeNull();
  const line = 'Thanks. Best wishes, Robin: Book a call (https://example.com/b); Fridays';
  expect(r.text).toBe(line);
  await expect.poll(() => lines().at(-1)).toBe(`PS> ${line}`);
  // Still the last line a moment later: not run.
  await sleep(1_500);
  expect(lines().at(-1)).toBe(`PS> ${line}`);
  expect(sh.text()).not.toMatch(/is not recognized|CommandNotFoundException|<b>/);
});
