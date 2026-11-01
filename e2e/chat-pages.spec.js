// Pages beside the chat (src-tauri/src/chat/pages.rs, chat/pages/view.rs;
// the look-and-show plan's Milestone 2):
// - `open_page` brings the page up in a Page tab: its own WebView2 (its own
//   profile), laid over the tab by Rust — no Yap IPC in it at all
//   (`window.__TAURI_INTERNALS__` absent, Yap's IPC unreachable); Back /
//   Reload / Save a shot / Open in your browser; the answer keeps it (its
//   pill, `pages` in chats.json); its tab closes it. A search opens on the
//   person's engine; a site they named, at its home page.
// - `read_open_page` reads the page open beside the chat; every question
//   says which tabs are open.
// - "Ask Yap about this" on words selected there: quoted over Chat's box,
//   asked as a selection.
// - `snap_page` (a model that reads images) takes a shot of the page (loaded
//   out of sight) and shows it as a picture: a picture tab and a pill, kept
//   like a pasted picture.
// - No model gets `offer_browser` any more: the tab's Open in … follows
//   Settings → General → Links.
//
// Nothing leaves the PC: the model is the scripted fake AI
// (support/fake-llm.js), the web a local fake (support/fake-web.js, through
// the debug-only YAP_WEB_STANDIN: the page loads from the stand-in). The
// page's own webview is a native window over Yap's: CDP screenshots of the
// main page don't show it, so the checks read it through the debug-only
// `e2e_page_view_eval` / `e2e_page_views`. Fictional pages only.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, expectStore, setConfig, openView, openSettings, closeSettings, settingsDialog } from './support/fixtures.js';
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

/** Sent in the conversation open now (with the panel open the chat list may step aside). */
async function say(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function ask(main, text) {
  await openView(main, 'Chat');
  // A new chat: the last test's tabs and pages are its own.
  await main.getByRole('button', { name: 'New chat' }).first().click();
  await expect(main.getByTestId('chat-doc')).toHaveCount(0);
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

test('open_page brings the live page up beside the chat: its own webview, no Yap IPC, Open in your browser', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'open_page', args: { url: FORT_JOY_URL, section: 'Map' } }] },
    { content: 'It’s open beside the chat: the map is under Map.' },
  ]);
  await ask(main, `Bring up the treasure map page ${FORT_JOY_URL}`);

  // Every tool-using model with the web gets the page tools, told how in one
  // line: no shot for one that can't see, no offer_browser for any.
  await expect.poll(() => chatCalls(fakeLlm, from).length).toBeGreaterThanOrEqual(2);
  const calls = chatCalls(fakeLlm, from);
  expect(calls[0].tools).toEqual(expect.arrayContaining(['open_page', 'read_open_page']));
  expect(calls[0].tools).not.toContain('snap_page');
  expect(calls[0].tools).not.toContain('offer_browser');
  expect(calls[0].system).toContain('open_page opens a page beside the chat');
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
  // (An address asked for: Yap opens it itself, no model.)
  await ask(main, `Show me ${FORT_JOY_URL}`);
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.loaded, { timeout: 20_000 }).toBe(true);
  await expect(main.getByText('wiki.example.com is open beside the chat.')).toBeVisible();

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

test('snap_page takes a shot of the page and shows it as a picture tab (a model that reads images)', async ({ yap, main, fakeLlm, shot }) => {
  // ("vision" in the fake's id: a model that reads images, the only kind offered snap_page.)
  await setConfig(yap, main, { ...chatConfig(fakeLlm), llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-vision-chat' } } });
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'snap_page', args: { url: FORT_JOY_URL } }] }, { content: 'Here’s a shot of the page, beside the chat.' }]);
  await ask(main, `Get me a screenshot of ${FORT_JOY_URL}`);

  // Its picture tab, as the turn goes.
  await expect(main.getByTestId('chat-picture')).toBeVisible({ timeout: 40_000 });
  await expect(main.getByTestId('chat-picture-caption')).toHaveText('A shot of wiki.example.com');
  await expect(main.getByText('Here’s a shot of the page')).toBeVisible();
  const result = toolResult(chatCalls(fakeLlm, from)[1]);
  expect(result).toMatchObject({ success: true, shown: true, picture: 'p1', w: 1280, h: 800, displayText: 'Took a shot of wiki.example.com/wiki/Fort_Joy' });
  // It can look at it.
  expect(result.note).toContain('look_at_picture p1');
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

test('open_page opens a search or a site the person named; read_open_page reads the page in front; Open in … follows Links', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { ...chatConfig(fakeLlm), webSearchBrowser: 'duckduckgo' });
  // Words: a search on the person's engine, beside the chat (Yap never reads it).
  let from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'open_page', args: { search: 'train times Kellbrook to Port Ellis' } }] }, { content: 'The search is open beside the chat.' }]);
  // (Worded so Yap leaves it to the model: "search for …" it would open itself.)
  await ask(main, 'I need the train times from Kellbrook to Port Ellis up beside the chat');
  await expect(main.getByText('The search is open beside the chat.')).toBeVisible({ timeout: 20_000 });
  expect(toolResult(chatCalls(fakeLlm, from)[1])).toMatchObject({ success: true, shown: true, displayText: 'Brought up a search for "train times Kellbrook to Port Ellis" beside the chat' });
  await expect(main.getByTestId('chat-page-address')).toHaveValue('https://duckduckgo.com/?q=train+times+Kellbrook+to+Port+Ellis', { timeout: 20_000 });
  // The next question says what's open; reading a search engine's results is refused.
  from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'read_open_page', args: {} }] }, { content: 'Pick a result and I can help.' }]);
  await main.getByRole('textbox', { name: 'Message' }).fill('What does it say?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.getByText('Pick a result and I can help.')).toBeVisible({ timeout: 20_000 });
  let calls = chatCalls(fakeLlm, from);
  expect(JSON.stringify(calls[0].messages)).toContain('Open beside the chat: tab 1');
  expect(JSON.stringify(calls[0].messages)).toContain('search results, in front');
  expect(toolResult(calls[1])).toMatchObject({ success: false, displayText: expect.stringMatching(/^Didn't read duckduckgo\.com\/\?q=train\+times.*: a search engine's results$/) });

  // A page: read as the person sees it, cited, its text the model's.
  from = fakeLlm.requests.length;
  // (An address asked for: Yap opens it itself.)
  await say(main, `Open ${FORT_JOY_URL}`);
  await expect(main.getByText('wiki.example.com is open beside the chat.')).toBeVisible({ timeout: 20_000 });
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.loaded, { timeout: 20_000 }).toBe(true);
  from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'read_open_page', args: {} }] }, { content: 'It marks every treasure chest [1].' }]);
  await main.getByRole('textbox', { name: 'Message' }).fill('What is this page about?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.getByText('It marks every treasure chest')).toBeVisible({ timeout: 20_000 });
  calls = chatCalls(fakeLlm, from);
  const read = toolResult(calls[1]);
  expect(read).toMatchObject({ success: true, displayText: 'Read wiki.example.com/wiki/Fort_Joy beside the chat' });
  // (The search is tab 1, this page tab 2, in front.)
  expect(read.data).toMatchObject({ tab: 2, title: 'Fort Joy - Example Wiki', url: FORT_JOY_URL, n: 1 });
  expect(read.data.text).toContain('Every treasure chest marked');
  await expect(main.getByText('Read the page beside the chat')).toBeVisible();

  // A site the person named: its home page, though no search found it.
  from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'open_page', args: { url: 'https://www.exampletube.com/' } }] }, { content: 'ExampleTube is open.' }]);
  await say(main, 'Open ExampleTube for me');
  await expect(main.getByText('ExampleTube is open.')).toBeVisible({ timeout: 20_000 });
  expect(toolResult(chatCalls(fakeLlm, from)[1])).toMatchObject({ success: true, displayText: 'Brought up exampletube.com beside the chat' });

  // No model gets offer_browser; Settings → General → Links: the browsers this
  // PC has registered, one picked, and the tab's Open in … names it.
  expect(calls[0].tools).not.toContain('offer_browser');
  await openSettings(main, 'General');
  const pick = main.getByRole('combobox', { name: 'Open links in' });
  await expect(pick.locator('option')).toHaveText(['Default browser', 'Brave', 'Firefox']);
  await pick.selectOption('Brave');
  await expect.poll(async () => (await yap.invoke('get_config')).openLinksIn).toBe('Brave');
  await closeSettings(main);
  expect(await yap.invoke('browser_label')).toBe('Brave');
  await expect(main.getByTestId('chat-page-open')).toHaveText('Open in Brave');
  const before = (await yap.invoke('e2e_opened_urls')).length;
  await main.getByTestId('chat-page-open').click();
  await expect.poll(async () => (await yap.invoke('e2e_opened_urls')).length).toBe(before + 1);
  // Back to Windows' default: "your browser" again.
  await setConfig(yap, main, { openLinksIn: '' });
  await expect(main.getByTestId('chat-page-open')).toHaveText('Open in your browser');
});

test('the + opens a new tab: an address or words go in its address bar, Back and Forward, and its data clears', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, { ...chatConfig(fakeLlm), webSearchBrowser: 'duckduckgo' });
  // (An address asked for: Yap opens it itself, no model.)
  await ask(main, `Show me ${FORT_JOY_URL}`);
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.loaded, { timeout: 20_000 }).toBe(true);
  // A page showing: the row is there with one tab, the page's title its name, and the +.
  const tabs = main.getByTestId('chat-panel-tab');
  await expect(tabs).toHaveCount(1);
  await expect(tabs.first()).toHaveText('Fort Joy - Example Wiki');
  const address = main.getByTestId('chat-page-address');
  await expect(address).toHaveValue(FORT_JOY_URL);

  // The +: a new tab, its address bar waiting, the pages this chat read offered.
  await main.getByTestId('chat-panel-new-tab').click();
  await expect(tabs).toHaveCount(2);
  await expect(tabs.last()).toHaveText('New tab');
  await expect(main.getByTestId('chat-page-new')).toBeVisible();
  await expect(main.getByTestId('chat-page-new').getByRole('button', { name: /Fort Joy - Example Wiki/ })).toBeVisible();
  await expect(address).toBeFocused();
  await expect(address).toHaveValue('');
  // Nothing made until there's somewhere to go.
  expect((await views(yap)).filter((v) => v.key.startsWith('page:new:'))).toEqual([]);
  await shot(main, '04-a-new-tab');

  // An address without https://: opened as one.
  await address.fill('wiki.example.com/wiki/Fort_Joy');
  await address.press('Enter');
  const fresh = async () => (await views(yap)).find((v) => v.key.startsWith('page:new:'));
  await expect.poll(async () => (await fresh())?.loaded, { timeout: 20_000 }).toBe(true);
  const key = (await fresh()).key;
  await expect(main.getByTestId('chat-page-new')).toHaveCount(0);
  await expect(tabs.last()).toHaveText('Fort Joy - Example Wiki');
  await expect(address).not.toBeFocused();
  await expect(address).toHaveValue(FORT_JOY_URL);

  // Words: a search on the person's engine, in the tab (the stand-in answers for it).
  await address.click();
  await expect(address).toHaveValue(FORT_JOY_URL);
  await address.fill('fort joy chests');
  await address.press('Enter');
  await expect.poll(async () => (await fresh())?.url).toBe('https://duckduckgo.com/?q=fort+joy+chests');
  await expect(address).toHaveValue('https://duckduckgo.com/?q=fort+joy+chests', { timeout: 20_000 });
  // Back to the page, and Forward again.
  const backBtn = main.getByTestId('chat-page').getByRole('button', { name: 'Back', exact: true });
  const fwdBtn = main.getByTestId('chat-page').getByRole('button', { name: 'Forward', exact: true });
  await expect(backBtn).toBeEnabled({ timeout: 20_000 });
  await backBtn.click();
  await expect(address).toHaveValue(FORT_JOY_URL, { timeout: 20_000 });
  await expect(fwdBtn).toBeEnabled();
  await fwdBtn.click();
  await expect(address).toHaveValue('https://duckduckgo.com/?q=fort+joy+chests', { timeout: 20_000 });

  // Never another scheme, never this PC: said under the bar, nothing opened.
  await address.click();
  await address.fill('file:///C:/Windows/win.ini');
  await address.press('Enter');
  await expect(main.getByTestId('chat-page-address-error')).toContainText('Only web pages open here');
  await address.fill('192.168.1.5/admin');
  await address.press('Enter');
  await expect(main.getByTestId('chat-page-address-error')).toBeVisible();
  expect((await fresh()).url).toBe('https://duckduckgo.com/?q=fort+joy+chests');
  // Esc puts the address back.
  await address.press('Escape');
  await expect(main.getByTestId('chat-page-address-error')).toHaveCount(0);
  await expect(address).toHaveValue('https://duckduckgo.com/?q=fort+joy+chests');

  // Its tab shown again opens where the address bar took it.
  await tabs.first().click();
  await expect(address).toHaveValue(FORT_JOY_URL);
  await tabs.last().click();
  await expect(address).toHaveValue('https://duckduckgo.com/?q=fort+joy+chests');
  expect((await fresh()).key).toBe(key);

  // What the pages keep (their own profile) clears from Delete some of your data.
  await backBtn.click();
  await expect(address).toHaveValue(FORT_JOY_URL, { timeout: 20_000 });
  const probe = (script) => yap.invoke('e2e_page_view_eval', { key, script }).then((raw) => JSON.parse(raw));
  await probe("localStorage.setItem('yap-pages-probe','1'); 'ok'");
  expect(await probe("localStorage.getItem('yap-pages-probe')")).toBe('1');
  await openSettings(main, 'Privacy & data');
  const yours = settingsDialog(main).getByRole('group', { name: 'Your data' });
  await yours.scrollIntoViewIfNeeded();
  await yours.getByRole('button', { name: 'Delete…', exact: true }).click();
  const picker = main.getByRole('dialog', { name: 'Delete some of your data' });
  await expect(picker).toContainText('Cookies, sign-ins and site data from the web pages opened beside the chat');
  await picker.getByRole('checkbox', { name: /^Pages beside the chat/ }).check();
  await picker.getByRole('button', { name: 'Delete…', exact: true }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm.getByRole('list', { name: 'What goes' })).toContainText('Pages beside the chat');
  await confirm.getByRole('button', { name: 'Delete', exact: true }).click();
  await expect(main.getByRole('status').filter({ hasText: 'Deleted from this PC' })).toContainText('Pages beside the chat');
  await closeSettings(main);
  // The page stays open; what it kept is gone.
  await expect.poll(() => probe("localStorage.getItem('yap-pages-probe')"), { timeout: 10_000 }).toBe(null);
  expect(await probe('document.title')).toBe('Fort Joy - Example Wiki');
});

test('the title bar’s browser button (Ctrl+Shift+B) opens a new tab, the same tab again, or closes the panel', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
  await expect(main.getByTestId('chat-doc')).toHaveCount(0);
  const btn = main.getByTestId('chat-browser-button');
  await expect(btn).toHaveAttribute('aria-pressed', 'false');
  // Nothing open: a new tab, its address bar waiting.
  await btn.click();
  await expect(main.getByTestId('chat-page-new')).toBeVisible();
  await expect(main.getByTestId('chat-page-address')).toBeFocused();
  await expect(btn).toHaveAttribute('aria-pressed', 'true');
  // Again: the panel closes.
  await btn.click();
  await expect(main.getByTestId('chat-doc')).toHaveCount(0);
  await expect(btn).toHaveAttribute('aria-pressed', 'false');
  // Ctrl+Shift+B: the same tab back, not another.
  await main.keyboard.press('Control+Shift+B');
  await expect(main.getByTestId('chat-page-new')).toBeVisible();
  await expect(main.getByTestId('chat-panel-tab')).toHaveCount(1);
  await main.keyboard.press('Control+Shift+B');
  await expect(main.getByTestId('chat-doc')).toHaveCount(0);
  // Only while Chat shows.
  await openView(main, 'Home');
  await expect(btn).toHaveCount(0);
});

test('"Open YouTube" and "search for train tickets…" open beside the chat by Yap itself, no model', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { ...chatConfig(fakeLlm), webSearchBrowser: 'duckduckgo' });
  const from = fakeLlm.requests.length;
  // A site asked for by name: its home page, at once.
  await ask(main, 'Can you open up YouTube for me?');
  await expect(main.getByText('YouTube is open beside the chat.')).toBeVisible({ timeout: 20_000 });
  await expect(main.getByTestId('chat-page-address')).toHaveValue('https://www.youtube.com/', { timeout: 20_000 });
  await expect(main.getByText('Brought up a page')).toBeVisible();
  // A search asked for that the free sources don't cover (travel): the search itself, on the person's engine.
  await say(main, 'Search for train tickets from Kellbrook to Port Ellis.');
  await expect(main.getByText('The search for “train tickets from Kellbrook to Port Ellis” is open beside the chat, on DuckDuckGo.')).toBeVisible({ timeout: 20_000 });
  await expect(main.getByTestId('chat-page-address')).toHaveValue('https://duckduckgo.com/?q=train+tickets+from+Kellbrook+to+Port+Ellis', { timeout: 20_000 });
  // No model was asked for either; both are kept with their answers.
  expect(chatCalls(fakeLlm, from)).toEqual([]);
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.pages?.[0]?.url === 'https://www.youtube.com/') && c.messages.some((m) => m.pages?.[0]?.title === 'Search: train tickets from Kellbrook to Port Ellis'))
  );
});

test('look_at_screen: a model that reads images sees every monitor, Yap left out; it is never told it cannot see', async ({ yap, main, fakeLlm }) => {
  // ("vision" in the fake's id: a model that reads images. The web off: the screen isn't the web's.)
  await setConfig(yap, main, { webSearch: false, assistantScreen: true, memoryLearnChats: false, llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-vision-chat' } } });
  // Two fake monitors (a test run never captures the real screen).
  await yap.invoke('e2e_snip', {
    monitors: [
      { x: 0, y: 0, width: 1280, height: 720, scale: 1, hdr: false, whiteNits: 80 },
      { x: 1280, y: 0, width: 1280, height: 720, scale: 1, hdr: false, whiteNits: 80 },
    ],
    windows: [],
    start: false,
  });
  // (A page left open by the last test: the panel closed, so the chat list shows.)
  await openView(main, 'Chat');
  const closePanel = main.getByTestId('chat-page').getByRole('button', { name: 'Close the panel' });
  if (await closePanel.count()) await closePanel.click();
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'look_at_screen', args: {} }] }, { content: 'Those are the speakers on your other monitor: a 3.5mm input.' }]);
  await ask(main, 'Here are the speakers, can you have a look?');
  await expect(main.getByText('Those are the speakers on your other monitor')).toBeVisible({ timeout: 20_000 });
  const calls = chatCalls(fakeLlm, from);
  expect(calls[0].tools).toContain('look_at_screen');
  expect(calls[0].system).toContain("Never say you can't see: look.");
  // Its result, then the two monitors as the user's message.
  expect(toolResult(calls[1])).toMatchObject({ success: true, data: { monitors: 2 }, displayText: 'Looked at your screen (2 monitors)' });
  const parts = calls[1].messages.at(-1).content;
  expect(parts.filter((p) => p.type === 'image_url')).toHaveLength(2);
  expect(parts.find((p) => p.type === 'text').text).toContain('Monitor 1 of 2 as it is now');
  await expect(main.getByText('Looked at your screen', { exact: true })).toBeVisible();
  // What was looked at shows with the question, in the screenshot frame, and is kept with it.
  await expect(main.getByRole('group', { name: 'Screenshots' }).last().getByRole('button', { name: /^Screenshot,/ })).toHaveCount(2);
  await expectStore(yap, 'chats.json', (s) =>
    s.conversations.some((c) => c.messages.some((m) => m.role === 'user' && m.text.startsWith('Here are the speakers') && m.images?.filter((i) => i.source === 'screen').length === 2))
  );
});
