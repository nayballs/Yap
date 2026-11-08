// Find and show pictures (desktop/src-tauri/src/chat/pictures.rs, web/page.rs'
// page_images, web/turn.rs' candidates, chat/stream.rs `Event::Picture`,
// chat/chats.rs `pictures`, privacy/net.rs "A picture from the web"):
// - A page read lists its pictures as ids (p1, p2…), the question's first,
//   never the site's logo; the model shows one: Yap downloads it from the
//   page's own host, stores it in chat-images/ like a pasted picture, sends
//   `{kind: "picture"}` to the page the moment it lands, keeps it with the
//   answer in chats.json (`pictures`) and on chat_send's reply, and the
//   network receipts list the download.
// - A model that reads images ("fake-vision-chat": "vision" in its id) may
//   look first: the next step carries the picture as an image part in a
//   user message after the tool results. One that doesn't is never offered
//   look_at_picture.
//
// Only the Rust side: the panel, its tabs and the pills are the frontend's
// (and its own specs'). Nothing leaves the PC: the model is the scripted
// fake AI (support/fake-llm.js), the web a local fake (support/fake-web.js,
// through the debug-only YAP_WEB_STANDIN), recorded as wiki.example.com in
// the receipts through YAP_E2E_NET_ALIASES. Fictional pages only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, expectStore, setConfig, openView } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeWeb, FORT_JOY_URL, FORT_JOY_MAP, EMBER_URL, EMBER_PHOTO } from './support/fake-web.js';
import { readPng } from './support/png.js';

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      // The fake that reads images says its context, as a real server does:
      // at the 8K Yap assumes for a server that doesn't, a model that sees
      // sits at the edge of its whole toolset (turn/fit.rs leaves its view
      // tools, look_at_picture among them, out). fake-chat stays 8K.
      fake.setServerModels([{ id: 'fake-chat' }, { id: 'llama3.1:8b' }, { id: 'qwen2.5-7b-instruct' }, { id: 'nomic-embed-text' }, { id: 'fake-vision-chat' }]);
      fake.serveOllama({ loaded: { 'fake-vision-chat': 32768 } });
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  fakeWeb: [
    async ({}, use) => {
      const fake = await startFakeWeb();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm, fakeWeb }, use) => {
      await use({
        name: 'chat-pictures',
        env: { ...fakeWeb.env, YAP_E2E_NET_ALIASES: `${new URL(fakeWeb.base).host}=wiki.example.com` },
        config: {
          webSearch: true,
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const QUESTION = `Get me the treasure map from ${FORT_JOY_URL}`;
const CAPTION = 'Fort Joy, every treasure chest marked';

/** chat_send from the main window, as the Chat view sends it, with the
 *  stream's events collected (a channel of the page's own). */
async function sendCollecting(main, text) {
  return main.evaluate(async (question) => {
    const events = [];
    const id = window.__TAURI_INTERNALS__.transformCallback((raw) =>
      events.push(raw && typeof raw === 'object' && 'message' in raw ? raw.message : raw)
    );
    const res = await window.__TAURI_INTERNALS__.invoke('chat_send', { conversationId: null, text: question, onStream: `__CHANNEL__:${id}` });
    // The channel's last messages may land just after the reply.
    await new Promise((r) => setTimeout(r, 300));
    return { res, events };
  }, text);
}

const chatCalls = (fakeLlm, from) => fakeLlm.requests.slice(from).filter((r) => r.tools?.length);
const toolResult = (request) => JSON.parse(request.messages.filter((m) => m.role === 'tool').at(-1).content);

test('a page’s lead picture is listed first: its infobox photo, lazy, no alt text, ahead of figures whose words match better', async ({ yap, main, fakeLlm, fakeWeb }) => {
  // (8 Nov 2026, Yap Lab: a wiki's infobox photo never made the five a page's pictures listed.)
  await setConfig(yap, main, { webSearch: true, llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } } });
  const from = fakeLlm.requests.length;
  const webFrom = fakeWeb.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'read_page', args: { url: EMBER_URL } }] },
    { tools: [{ name: 'show_picture', args: { picture: 'p1', caption: 'Mount Ember from the harbour' } }] },
    { content: 'That’s Mount Ember [1].' },
  ]);
  // (The figures' captions have the question's words; the lead photo's caption doesn't say "path" or "station".)
  const { res } = await sendCollecting(main, `Get me the best photo of Mount Ember from ${EMBER_URL}`);
  expect(res.reply).toContain('Mount Ember');
  const calls = chatCalls(fakeLlm, from);
  const listed = toolResult(calls[1]).data.pictures;
  // The lead photo first (its caption from the infobox's cell), then the rest by the question's words and size.
  expect(listed[0]).toEqual({ picture: 'p1', about: 'Mount Ember from the harbour', size: '300×200' });
  expect(listed.map((p) => p.picture)).toEqual(['p1', 'p3', 'p2']);
  // Shown: the infobox photo, its sharper copy (the srcset's 2x).
  expect(toolResult(calls[2])).toMatchObject({ success: true, shown: true, w: 600, h: 400 });
  const asked = fakeWeb.requests.slice(webFrom).filter((r) => r.service === 'standin').map((r) => `https://${r.host}${r.path}`);
  expect(asked).toContain(EMBER_PHOTO);
});

test('a page’s picture is shown: downloaded, stored, sent to the page, kept with the answer, in the receipts', async ({
  yap,
  main,
  fakeLlm,
  fakeWeb,
}) => {
  await setConfig(yap, main, { webSearch: true, llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } } });
  const from = fakeLlm.requests.length;
  const webFrom = fakeWeb.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'read_page', args: { url: FORT_JOY_URL } }] },
    { tools: [{ name: 'show_picture', args: { picture: 'p1', caption: CAPTION } }] },
    { content: 'It’s open beside the chat [1]. I went by its caption.' },
  ]);
  const { res, events } = await sendCollecting(main, QUESTION);
  expect(res.reply).toContain('open beside the chat');

  // A model that doesn't read images: show_picture, never look_at_picture.
  const calls = chatCalls(fakeLlm, from);
  expect(calls[0].tools).toContain('show_picture');
  expect(calls[0].tools).not.toContain('look_at_picture');
  expect(calls[0].system).toContain('the source that owns the subject');
  expect(calls[0].system).toContain("You can't see pictures");
  // The page read listed its pictures, the map first, never the logo.
  const read = toolResult(calls[1]);
  expect(read.data.pictures).toEqual([
    { picture: 'p1', about: 'Fort Joy map · Every treasure chest marked', size: '1600×1200' },
    { picture: 'p2', about: 'Concept art of the keep', size: '1200×675' },
  ]);
  expect(toolResult(calls[2])).toMatchObject({ success: true, shown: true, w: 320, h: 240 });

  // Downloaded once, from the stand-in for the picture's own host.
  const asked = fakeWeb.requests.slice(webFrom).filter((r) => r.service === 'standin').map((r) => `${r.host}${r.path}`);
  expect(asked).toEqual(['wiki.example.com/wiki/Fort_Joy', 'wiki.example.com/images/fort_joy_map.png']);

  // The page heard it the moment it landed.
  const shown = events.filter((e) => e?.kind === 'picture').map((e) => e.picture);
  expect(shown).toHaveLength(1);
  const picture = shown[0];
  expect(picture).toMatchObject({
    w: 320,
    h: 240,
    caption: CAPTION,
    alt: 'Fort Joy map',
    imageUrl: FORT_JOY_MAP,
    page: { title: 'Fort Joy - Example Wiki', site: 'wiki.example.com', url: FORT_JOY_URL },
    n: 1,
  });
  expect(picture.id).toMatch(/^img_[0-9a-f]{16}$/);
  // The activity line said what it was showing.
  expect(events.some((e) => e?.kind === 'step' && e.step.mark === 'picture' && e.step.line === `Showing ‘${CAPTION}’`)).toBe(true);
  // The reply carries it, for the answer's pills.
  expect(res.pictures).toEqual([picture]);
  expect(res.toolsUsed).toContain(`Showed ‘${CAPTION}’ from wiki.example.com`);

  // Stored in chat-images as a PNG, like a pasted picture.
  const file = path.join(yap.dataDir, 'chat-images', `${picture.id}.png`);
  expect(fs.existsSync(file)).toBe(true);
  const png = readPng(fs.readFileSync(file));
  expect([png.width, png.height]).toEqual([320, 240]);

  // Kept with the answer, its page among the answer's sources.
  await expectStore(yap, 'chats.json', (s) => {
    const conv = s.conversations.find((c) => c.id === res.conversationId);
    const answer = conv?.messages.at(-1);
    return (
      answer?.pictures?.length === 1 &&
      answer.pictures[0].id === picture.id &&
      answer.pictures[0].page.url === FORT_JOY_URL &&
      answer.sources?.some((src) => src.n === 1 && src.url === FORT_JOY_URL)
    );
  });

  // The network receipts list the download, by its purpose and host.
  await expectStore(yap, 'connections.json', (s) =>
    s.entries.some((e) => e.label === 'A picture from the web' && e.host === 'wiki.example.com')
  );
});

// The whole way, in the Chat view as the person uses it (the panel's own
// spec, chat-picture-panel.spec.js, seeds its pictures): the question typed
// and sent, the page read, the picture shown, and the panel opening on it
// mid-turn, with the thumbnail pill under the answer.
test('asked in Chat: the picture opens in the panel beside the answer, its pill under it', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { webSearch: true, llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } } });
  fakeLlm.scriptChat([
    { tools: [{ name: 'read_page', args: { url: FORT_JOY_URL } }] },
    { tools: [{ name: 'show_picture', args: { picture: 'p1', caption: CAPTION } }] },
    { content: 'It’s open beside the chat [1].' },
  ]);
  await openView(main, 'Chat');
  await main.getByRole('textbox', { name: 'Message' }).fill(QUESTION);
  await main.getByRole('button', { name: 'Send' }).click();

  const picture = main.getByTestId('chat-picture');
  await expect(picture).toBeVisible({ timeout: 20_000 });
  await expect(main.getByTestId('chat-picture-caption')).toHaveText(CAPTION);
  await expect(main.getByTestId('chat-picture-source')).toHaveText(/From Fort Joy - Example Wiki\s*·\s*Open in your browser/);
  // The stored picture itself, drawn from Yap's own store.
  await expect
    .poll(() => main.getByTestId('chat-picture-img').evaluate((img) => img.complete && img.naturalWidth > 0), { timeout: 10_000 })
    .toBe(true);
  // The answer, with its pill.
  await expect(main.getByText('It’s open beside the chat')).toBeVisible();
  await expect(main.getByTestId('chat-picture-pill')).toHaveCount(1);
  await expect(main.getByTestId('chat-picture-pill')).toContainText(CAPTION);
  // Its steps, folded under it, say what it did (chatSteps.js).
  await expect(main.getByText('Read a page, showed a picture')).toBeVisible();
  await shot(main, 'asked-in-chat');
});

test('a model that reads images looks before it shows: the next step carries the picture', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, {
    webSearch: true,
    llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-vision-chat' } },
  });
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'read_page', args: { url: FORT_JOY_URL } }] },
    { tools: [{ name: 'look_at_picture', args: { picture: 'p1' } }] },
    { tools: [{ name: 'show_picture', args: { picture: 'p1', caption: CAPTION } }] },
    { content: 'That’s the treasure map; it’s open beside the chat [1].' },
  ]);
  const { res, events } = await sendCollecting(main, QUESTION);
  expect(res.reply).toContain('treasure map');
  const calls = chatCalls(fakeLlm, from);
  expect(calls[0].tools).toEqual(expect.arrayContaining(['look_at_picture', 'show_picture']));
  expect(calls[0].system).toContain('look_at_picture before show_picture');
  // After the look: its tool result, then the picture as the user's.
  const after = calls[2].messages;
  expect(after.at(-2).role).toBe('tool');
  expect(after.at(-1).role).toBe('user');
  const parts = after.at(-1).content;
  expect(parts.find((p) => p.type === 'image_url').image_url.url).toMatch(/^data:image\/png;base64,/);
  expect(parts.find((p) => p.type === 'text').text).toContain('Picture p1');
  expect(calls[2].images).toHaveLength(1);
  expect(res.toolsUsed).toEqual(['Read wiki.example.com/wiki/Fort_Joy', 'Looked at a picture from wiki.example.com', `Showed ‘${CAPTION}’ from wiki.example.com`]);
  expect(events.filter((e) => e?.kind === 'picture')).toHaveLength(1);
  expect(events.some((e) => e?.kind === 'step' && e.step.line === 'Looking at a picture')).toBe(true);
});
