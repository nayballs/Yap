// Export a chat (chat/export.rs, views/chat/chatExport.js; 15 Nov 2026, the
// Claude app's Share made local): a conversation's ⋯ → Export… writes it as a
// Markdown file where the person picks, its pictures copied into a folder
// beside it — never uploaded. The Save as dialog is Windows' own, so the spec
// checks the menu and then writes through the same command it calls.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, openView } from './support/fixtures.js';
import { writePng } from './support/png.js';

const NOW = Math.floor(Date.now() / 1000);
const SHOT = 'img_00000000000000e1';
const GONE = 'img_00000000000000e2';
const SHOWN = 'img_00000000000000e3';

const CHATS = {
  conversations: [
    {
      id: 1,
      title: 'Garden: spring plan',
      createdTs: NOW - 600,
      updatedTs: NOW - 500,
      messages: [
        {
          role: 'user',
          text: 'What should go in the raised bed?\nIt gets the morning sun',
          ts: NOW - 600,
          images: [
            { id: SHOT, w: 40, h: 30, bytes: 100, source: 'snip' },
            { id: GONE, w: 40, h: 30, bytes: 100, source: 'paste' },
          ],
        },
        {
          role: 'assistant',
          text: '## For a sunny bed\n\n- **Lettuce** early on [1]\n- Beans after May',
          ts: NOW - 590,
          model: { provider: 'anthropic', id: 'claude-opus-5-5' },
          sources: [{ n: 1, title: 'Sowing guide', url: 'https://garden.example.com/sowing', site: 'garden.example.com' }],
          pictures: [{ id: SHOWN, w: 40, h: 30, caption: 'A raised bed in spring' }],
        },
      ],
    },
    { id: 2, title: 'Plain words', createdTs: NOW - 400, updatedTs: NOW - 400, messages: [{ role: 'user', text: 'Hello', ts: NOW - 400 }, { role: 'assistant', text: 'Hi there.', ts: NOW - 399 }] },
  ],
};

const test = base.extend({
  yapOptions: [
    async ({}, use) => {
      await use({
        name: 'chat-export',
        data: { 'chats.json': CHATS },
        prepare: (dir) => {
          const images = path.join(dir, 'chat-images');
          fs.mkdirSync(images, { recursive: true });
          fs.writeFileSync(path.join(images, `${SHOT}.png`), writePng(40, 30, [40, 140, 60]));
          fs.writeFileSync(path.join(images, `${SHOWN}.png`), writePng(40, 30, [200, 120, 40]));
          // (GONE has no file: kept past "Keep chat screenshots".)
        },
      });
    },
    { scope: 'worker' },
  ],
});

const row = (main, title) => main.locator('.conv').filter({ hasText: title });

test('Export… writes the chat as Markdown with its pictures beside it', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await row(main, 'Garden: spring plan').click({ button: 'right' });
  const menu = main.getByRole('menu').first();
  await expect(menu.getByRole('menuitem')).toHaveText(['Rename', 'Copy as Markdown', 'Export…', 'Outputs', 'Move to project', 'Archive', 'Delete…']);
  await shot(main, '01-export-in-the-menu');
  await main.keyboard.press('Escape');

  // What Export… does once Save as gives it a place.
  const out = path.join(yap.runDir, 'exports', 'Garden spring plan.md');
  fs.mkdirSync(path.dirname(out), { recursive: true });
  const result = await yap.invoke('chat_export', { id: 1, path: out });
  expect(result).toMatchObject({ pictures: 2, folder: 'Garden spring plan pictures' });

  const md = fs.readFileSync(out, 'utf8');
  expect(md).toMatch(/^# Garden: spring plan\n\n_Exported from Yap · .+ · 2 messages_\n/);
  // The person's line break kept; the answer's markdown and model as they were.
  expect(md).toContain('What should go in the raised bed?  \nIt gets the morning sun\n');
  expect(md).toMatch(/\*\*Yap\*\* · claude-opus-5-5 · .+\n\n## For a sunny bed\n\n- \*\*Lettuce\*\* early on \[1\]\n- Beans after May\n/);
  // Pictures numbered in the order they show, linked from the folder beside it.
  expect(md).toContain('![Screenshot 1](Garden%20spring%20plan%20pictures/1.png)');
  expect(md).toContain("_A picture that's no longer kept._");
  expect(md).toContain('![A raised bed in spring](Garden%20spring%20plan%20pictures/2.png)');
  expect(md).toContain('\nSources:\n\n1. [Sowing guide](https://garden.example.com/sowing)\n');
  const folder = path.join(path.dirname(out), 'Garden spring plan pictures');
  expect(fs.readdirSync(folder).sort()).toEqual(['1.png', '2.png']);
  expect(fs.readFileSync(path.join(folder, '1.png'))).toEqual(fs.readFileSync(path.join(yap.dataDir, 'chat-images', `${SHOT}.png`)));

  // A chat with no pictures: the file alone.
  const plain = path.join(yap.runDir, 'exports', 'Plain words.md');
  expect(await yap.invoke('chat_export', { id: 2, path: plain })).toMatchObject({ pictures: 0, folder: '' });
  expect(fs.readFileSync(plain, 'utf8')).toContain('\n**You** · ');
  expect(fs.existsSync(path.join(yap.runDir, 'exports', 'Plain words pictures'))).toBe(false);
});
