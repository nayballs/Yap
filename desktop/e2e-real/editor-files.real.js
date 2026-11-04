// Code editors and coding CLIs, for real (dictionary/screen_terms/code.rs,
// pasting/file_tags.rs, compose.rs): a stand-in for Cursor — an Edge page
// this spec's Yap counts as Cursor (YAP_REALTEST_EDITORS, robot mode only)
// with editor tabs and a file tree, Monaco's input standing in for the
// code editor, and an AI chat box (Cursor's class "aislash-editor-input")
// whose "@" opens a list of files picked with Tab — and the stand-in for
// Claude Code in Windows Terminal, its screen showing a file it read. Yap
// reads the names on screen at the dictation's start: "user service dot ts"
// comes out userService.ts, tagged in the chat box, plain in the editor,
// "@path" in Claude Code. A real Cursor isn't driven here.
import path from 'node:path';
import { test, expect } from './support/robot.js';
import { edgeExe, startPage } from './support/targets.js';
import { pickerPage } from './support/pickers.js';

test.skip(!edgeExe(), 'Microsoft Edge is not installed');
test.use({ robotOptions: { name: 'editor-files', env: { YAP_REALTEST_EDITORS: 'msedge' } } });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const FILES = [
  ['userService.ts', 'src/services'],
  ['index.ts', 'src'],
  ['README.md', ''],
];

/** The Cursor stand-in: tabs, a file tree, the code editor's input and the AI chat box (`#box`, focused). */
async function cursor(robot) {
  const tabs = `<div role="tablist" aria-label="Editors">${FILES.slice(0, 2)
    .map(([f], i) => `<div role="tab" aria-selected="${i === 0}">${f}</div>`)
    .join('')}</div>`;
  const tree = `<div role="tree" aria-label="Files Explorer"><div role="treeitem">src</div>${FILES.map(([f]) => `<div role="treeitem">${f}</div>`).join('')}</div>`;
  const editor = `<textarea id="editor" class="inputarea monaco-mouse-cursor-text" aria-label="Editor content;Press Alt+F1 for Accessibility Options." style="height:50px;width:100%"></textarea>`;
  const items = FILES.map(([f, dir]) => ({ label: `${f}  ${dir}`.trim(), insert: f }));
  const page = await robot.track(
    await startPage({
      nonce: robot.nonce,
      dir: path.join(robot.dir, `cursor-${robot.targets.length}`),
      title: `userService.ts - Yap - Cursor - ${robot.nonce}`,
      body: pickerPage({ items, boxAttrs: 'class="aislash-editor-input" aria-label="Chat"', aside: tabs + tree + editor }),
    }),
  );
  return {
    ...page,
    pills: () => page.evaluate(() => [...document.querySelectorAll('#box .pill')].map((p) => p.textContent)),
    box: () => page.evaluate(() => document.getElementById('box').innerText.replace(/ /g, ' ').trim()),
    editorText: () => page.evaluate(() => document.getElementById('editor').value),
    /** The same window, the code editor's input focused instead of the chat box. */
    editor: {
      ...page,
      async focus() {
        await page.focus();
        await page.evaluate(() => document.getElementById('editor').focus());
        await sleep(300); // Chromium's accessibility tree follows the focus
      },
    },
  };
}

test("in the AI chat box a file said aloud is tagged: '@' typed, picked from the list with Tab", async ({ robot }) => {
  const app = await cursor(robot);
  const r = await robot.dictate(app, 'Fix the bug in user service dot ts');
  expect(r.error).toBeNull();
  expect(r.text).toBe('Fix the bug in @userService.ts');
  expect(r.steps).toEqual([{ paste: 'Fix the bug in ' }, { typed: '@userService.ts', picked: true }]);
  await expect.poll(() => app.pills()).toEqual(['userService.ts']);
  await expect.poll(() => app.box()).toBe('Fix the bug in userService.ts');
});

test('in the code editor beside it: written as named, never tagged', async ({ robot }) => {
  const app = await cursor(robot);
  const r = await robot.dictate(app.editor, 'Fix the bug in user service dot ts');
  expect(r.error).toBeNull();
  expect(r.text).toBe('Fix the bug in userService.ts');
  expect(r.steps ?? []).toEqual([]);
  await expect.poll(() => app.editorText()).toBe('Fix the bug in userService.ts');
  expect(await app.pills()).toEqual([]);
});

test('Claude Code: the path its screen showed, as "@path" in the one paste', async ({ robot }) => {
  const cli = await robot.claudeCode('cli', { banner: '● Read(src/services/userService.ts)\n  ⎿  Read 120 lines' });
  const r = await robot.dictate(cli, 'Fix the bug in user service dot ts');
  expect(r.error).toBeNull();
  expect(r.kind).toBe('terminal (Claude Code in Windows Terminal)');
  expect(r.text).toBe('Fix the bug in @src/services/userService.ts');
  await expect.poll(() => cli.received()).toContain('Fix the bug in @src/services/userService.ts');
  expect(r.submit).toBeNull();
});
