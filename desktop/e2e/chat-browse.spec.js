// Acting on the pages beside the chat (desktop/src-tauri/src/chat/pages/act.rs: the
// browser toolset's acting tools):
// - a strong model (the fake's "fake-gpt-5": documents::rich::strong) gets
//   browser_click / browser_type / browser_key / browser_hover /
//   browser_form_input beside the tools every model has, and browser_read's
//   mode "elements"; it fills a made-up rail site's journey form, sends it
//   and reads the next train — the tab says "Yap is using this page" (with
//   Stop) while the turn goes;
// - a click at a point of the latest screenshot (a strong model that reads
//   images): the element under it, judged as a ref click is — a point on a
//   password box refused;
// - a batch of steps (find, scroll, screenshot, zoom, click) runs in order and
//   stops at a card said no to;
// - typing into a page's search box and pressing Enter needs no card;
// - the tabs listed and closed;
// - a click that would buy asks first: a card in the thread, "Yap wants to
//   press ‘Buy tickets’ on rail.example.com" — Don't refuses it, Allow once
//   lets it through — and the payment page's card fields are never typed;
// - a sign-in form is the person's: its password and email refused;
// - with the page covering the thread (expanded), the page tab's strip asks
//   the card's question too, with Allow once / Don't, in step with the card;
// - Enter in a chat page's message box (an editable region outside any
//   form) asks first, and typing there goes in after the draft;
// - browser_tabs switches to a tab beside the chat (by its own number);
// - a page of web components (open shadow roots, nested; a closed one): its
//   fields listed, its password and code fields refused as anywhere, a click
//   at a point on its search button opening its dialog (said so), the box
//   found, typed in and sent by Enter through the submit check.
// A model that isn't strong ("fake-chat") gets none of it (chat-pages.spec.js).
//
// Nothing leaves the PC: the model is the scripted fake AI
// (support/fake-llm.js), the web a local fake (support/fake-web.js, through
// the debug-only YAP_WEB_STANDIN). The pages are their own WebView2s laid over
// Yap's window: the checks read them through the debug-only
// `e2e_page_view_eval` / `e2e_page_views`. Fictional sites only.
import { test as base, expect, expectStore, setConfig, openView } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeWeb, RAIL_URL, RAIL_RESULTS_URL, RAIL_SIGNIN_URL, FORT_JOY_URL, CHAT_URL, BOARD_URL, SMOOTH_URL, PREFS_URL, DOCS_URL, WIDE_URL, WIDE_BOX, TASKS_URL, FRAMES_URL } from './support/fake-web.js';

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      // The strong fakes' server says their context, as a strong model's
      // does (a strong model's whole toolset — the documents', the web's,
      // every browser tool — doesn't fit the 8K Yap assumes for a server
      // on this PC that doesn't say).
      fake.setServerModels([{ id: 'fake-chat' }, { id: 'fake-gpt-5' }, { id: 'fake-gpt-5-vision' }]);
      fake.serveOllama({ loaded: { 'fake-gpt-5': 32768, 'fake-gpt-5-vision': 32768 } });
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
      await use({ name: 'chat-browse', env: { ...fakeWeb.env }, config: chatConfig(fakeLlm) });
    },
    { scope: 'worker' },
  ],
});

/** Chat on a strong fake model (its id has "gpt-5") with the web on. */
function chatConfig(fakeLlm) {
  return { webSearch: true, memoryLearnChats: false, llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-gpt-5' } } };
}

const chatCalls = (fakeLlm, from) => fakeLlm.requests.slice(from).filter((r) => r.tools?.length);
const toolResults = (request) => request.messages.filter((m) => m.role === 'tool').map((m) => JSON.parse(m.content));
const lastResult = (request) => toolResults(request).at(-1);

/** A page tab's key, as the Chat view makes it. */
const keyOf = (url) => `page:${url.replace(/#.*$/, '').replace(/\/+$/, '')}`;
/** The pages alive now. */
const views = (yap) => yap.invoke('e2e_page_views');
/** A script run in page `url`'s own webview: its JSON result. */
const inPage = async (yap, url, script) => JSON.parse(await yap.invoke('e2e_page_view_eval', { key: keyOf(url), script }));

/** The chat page's message box, its words as one line (an editable region may hold non-breaking spaces). */
const BOX_TEXT = String.raw`document.getElementById('box').innerText.replace(/\s+/g, ' ').trim()`;

/** The ref of the element named `name` in the turn's latest read (its element lines). */
function refIn(request, name) {
  const read = toolResults(request).reverse().find((r) => typeof r?.data?.elements === 'string');
  const m = read && new RegExp(`\\[(\\d+)\\] \\w+ "${name}"`).exec(read.data.elements);
  return m ? Number(m[1]) : 0;
}

async function ask(main, text) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
  await expect(main.getByTestId('chat-doc')).toHaveCount(0);
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

async function say(main, text) {
  await main.getByRole('textbox', { name: 'Message' }).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

test('a strong model fills in a form and sends it; the tab says Yap is using it, with Stop', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  let release;
  const held = new Promise((r) => (release = r));
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: RAIL_URL } }] },
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    { tools: [{ name: 'browser_type', args: (r) => ({ ref: refIn(r, 'From'), text: 'Kellbrook' }) }] },
    { tools: [{ name: 'browser_type', args: (r) => ({ ref: refIn(r, 'To'), text: 'Port Ellis', submit: true }) }] },
    // (Held: the turn stays open while the tab's strip is checked.)
    { content: 'The next train leaves at 09:42 from platform 2.', gate: held },
  ]);
  await ask(main, `Find the next train from Kellbrook to Port Ellis on ${RAIL_URL}`);

  // The drive tier's tools and line, for a strong model.
  await expect.poll(() => chatCalls(fakeLlm, from).length, { timeout: 30_000 }).toBeGreaterThanOrEqual(5);
  const calls = chatCalls(fakeLlm, from);
  expect(calls[0].tools).toEqual(expect.arrayContaining(['browser_navigate', 'browser_read', 'browser_find', 'browser_click', 'browser_type', 'browser_key', 'browser_hover', 'browser_form_input', 'browser_tabs', 'browser_wait', 'browser_batch']));
  // The view's tools only for a model that sees it: this one reads the page.
  expect(calls[0].tools).not.toContain('browser_scroll');
  expect(calls[0].tools).not.toContain('browser_screenshot');
  expect(calls[0].system).toContain('To act on a page');
  expect(calls[0].system).toContain('Never passwords, card or bank details');

  // The read: the form's fields and button by ref.
  const read = lastResult(calls[2]);
  expect(read).toMatchObject({ success: true, displayText: 'Read rail.example.com beside the chat' });
  expect(read.data.elements).toMatch(/\[\d+\] textbox "From"/);
  expect(read.data.elements).toMatch(/\[\d+\] button "Find trains" \(submits the form\)/);
  // Typed, then sent: the page went to the results, its first words told.
  expect(lastResult(calls[3])).toMatchObject({ success: true, displayText: 'Typed in ‘From’ on rail.example.com', data: { field: 'Kellbrook' } });
  const sent = lastResult(calls[4]);
  expect(sent).toMatchObject({ success: true, displayText: 'Typed in ‘To’ on rail.example.com and sent it' });
  expect(sent.data.changed).toMatch(/^went to rail\.example\.com\/results\?from=Kellbrook&to=Port/);
  expect(sent.data.text).toContain('09:42 from platform 2');
  await expect.poll(async () => (await views(yap)).find((v) => v.key === keyOf(RAIL_URL))?.loaded).toBe(true);
  expect(await inPage(yap, RAIL_URL, 'location.pathname')).toMatch(/\/rail\.example\.com\/results$/);

  // While the turn goes: "Yap is using this page", with Stop.
  const strip = main.getByTestId('chat-page-acting');
  await expect(strip).toBeVisible();
  await expect(strip).toContainText('Yap is using this page');
  await expect(main.getByTestId('chat-page-acting-stop')).toBeVisible();
  await shot(main, '01-yap-is-using-this-page');
  release();
  await expect(main.getByText('The next train leaves at 09:42 from platform 2.')).toBeVisible();
  // The turn over: the strip goes; the steps folded under the answer.
  await expect(strip).toHaveCount(0);
  await expect(main.getByText(/Typed in 2 fields/i)).toBeVisible();
});

test('Stop on the tab stops the turn acting on the page', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  let release;
  const held = new Promise((r) => (release = r));
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: RAIL_URL } }] },
    { tools: [{ name: 'browser_key', args: { keys: 'Tab' } }] },
    { content: 'Never shown.', gate: held },
  ]);
  await ask(main, `Have a look around ${RAIL_URL}`);
  const stop = main.getByTestId('chat-page-acting-stop');
  await expect(stop).toBeVisible({ timeout: 30_000 });
  // (Once the turn's last step is asked for: no scripted step left for the next test.)
  await expect.poll(() => fakeLlm.requests.slice(from).some((r) => r.gated)).toBe(true);
  await stop.click();
  await expect(main.getByTestId('chat-page-acting')).toHaveCount(0);
  release();
  await expect(main.getByText('Never shown.')).toHaveCount(0);
});

test('a click that buys asks first: Don’t refuses it, Allow once lets it through, card fields are never typed', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  let from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: RAIL_RESULTS_URL } }] },
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    { tools: [{ name: 'browser_click', args: (r) => ({ ref: refIn(r, 'Buy tickets') }) }] },
    { content: 'I didn’t buy it: you said no. You can buy it yourself in the panel.' },
  ]);
  await ask(main, `Buy me a ticket for the 09:42 here: ${RAIL_RESULTS_URL}`);

  // The read says the click asks first.
  await expect.poll(() => chatCalls(fakeLlm, from).length, { timeout: 30_000 }).toBeGreaterThanOrEqual(3);
  expect(lastResult(chatCalls(fakeLlm, from)[2])).toMatchObject({ success: true, displayText: expect.stringMatching(/^Read rail\.example\.com\/results/) });
  expect(lastResult(chatCalls(fakeLlm, from)[2]).data.elements).toMatch(/\[1\] link "Buy tickets" → rail\.example\.com\/buy\?train=0942 \(asks the user first\)/);
  // The card, in the thread: the turn waits for it.
  const card = main.getByTestId('chat-page-confirm');
  await expect(card).toBeVisible({ timeout: 20_000 });
  await expect(card).toContainText('Yap wants to press ‘Buy tickets’ on rail.example.com');
  await expect(card).toContainText('It may buy something.');
  await expect(card).toHaveAttribute('data-state', 'asking');
  await shot(main, '02-asks-before-buying');
  // Don't: refused, nothing clicked.
  await card.getByRole('button', { name: "Don't" }).click();
  await expect(main.getByText('I didn’t buy it: you said no.')).toBeVisible({ timeout: 20_000 });
  await expect(card).toHaveAttribute('data-state', 'refused');
  await expect(card).toContainText('Yap didn’t press it.');
  let calls = chatCalls(fakeLlm, from);
  expect(lastResult(calls[3])).toMatchObject({ success: false, displayText: "Didn't click ‘Buy tickets’: you said no", data: { asked: true, allowed: false } });
  expect(await inPage(yap, RAIL_RESULTS_URL, 'location.pathname')).toMatch(/\/results$/);

  // Asked again, Allow once: clicked (the ref still holds: the same page).
  from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_click', args: { ref: 1 } }] },
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    { tools: [{ name: 'browser_type', args: (r) => ({ ref: refIn(r, 'Card number'), text: 'my card' }) }] },
    { content: 'The payment page is open: enter your card details yourself.' },
  ]);
  await say(main, 'Go ahead and buy it.');
  await expect(main.getByTestId('chat-page-confirm')).toHaveCount(2, { timeout: 20_000 });
  const card2 = main.getByTestId('chat-page-confirm').nth(1);
  await expect(card2).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await card2.getByRole('button', { name: 'Allow once' }).click();
  await expect(main.getByText('enter your card details yourself')).toBeVisible({ timeout: 20_000 });
  await expect(card2).toHaveAttribute('data-state', 'allowed');
  calls = chatCalls(fakeLlm, from);
  const clicked = lastResult(calls[1]);
  expect(clicked).toMatchObject({ success: true, displayText: 'Clicked ‘Buy tickets’ on rail.example.com (you allowed it)', data: { asked: true, allowed: true } });
  expect(clicked.data.changed).toMatch(/^went to rail\.example\.com\/buy/);
  // The card fields: marked in the read, refused when tried, left empty.
  expect(lastResult(calls[2]).data.elements).toMatch(/textbox "Card number" \(Yap never types here: payment card details\)/);
  expect(lastResult(calls[3])).toMatchObject({ success: false, displayText: "Didn't type in ‘Card number’: Yap never types payment card details" });
  expect(await inPage(yap, RAIL_RESULTS_URL, "document.querySelector('[name=cardnumber]').value")).toBe('');
  expect(await inPage(yap, RAIL_RESULTS_URL, 'location.pathname')).toMatch(/\/buy$/);
});

test('a sign-in form is the person’s: its password and email are refused', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: RAIL_SIGNIN_URL } }] },
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    {
      tools: [
        { name: 'browser_type', args: (r) => ({ ref: refIn(r, 'Password'), text: 'Tulip-Kettle-42' }) },
        { name: 'browser_type', args: (r) => ({ ref: refIn(r, 'Email'), text: 'sam.rivers@example.com' }) },
      ],
    },
    { content: 'Please sign in yourself in the panel beside the chat.' },
  ]);
  await ask(main, `Sign me in at ${RAIL_SIGNIN_URL}`);
  await expect(main.getByText('Please sign in yourself in the panel beside the chat.')).toBeVisible({ timeout: 30_000 });
  const calls = chatCalls(fakeLlm, from);
  const lines = lastResult(calls[2]).data.elements;
  expect(lines).toMatch(/textbox "Password" \(Yap never types here: a password\)/);
  expect(lines).toMatch(/textbox "Email" \(a sign-in field: the user signs in themselves\)/);
  const [pw, email] = toolResults(calls[3]).slice(-2);
  expect(pw).toMatchObject({ success: false, displayText: "Didn't type in ‘Password’: Yap never types a password" });
  expect(email).toMatchObject({ success: false, displayText: "Didn't type in ‘Email’: sign in yourself in the panel", data: { refused: 'sign-in' } });
  expect(await inPage(yap, RAIL_SIGNIN_URL, "document.querySelector('[name=password]').value")).toBe('');
  expect(await inPage(yap, RAIL_SIGNIN_URL, "document.querySelector('[name=email]').value")).toBe('');
});

test('with the page covering the thread, its strip asks the card’s question too: Allow once there lets it through', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: RAIL_RESULTS_URL } }] },
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    { tools: [{ name: 'browser_click', args: (r) => ({ ref: refIn(r, 'Buy tickets') }) }] },
    { content: 'The payment page is open: enter your card details yourself.' },
  ]);
  await ask(main, `Buy me a ticket for the 09:42 here: ${RAIL_RESULTS_URL}`);
  const card = main.getByTestId('chat-page-confirm');
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 30_000 });
  // The page expanded over the thread: the thread's card can't be seen, the strip asks.
  await main.getByRole('button', { name: 'Expand to full width' }).click();
  const strip = main.getByTestId('chat-page-ask');
  await expect(strip).toBeVisible();
  await expect(strip).toContainText('Yap wants to press ‘Buy tickets’ on rail.example.com');
  await expect(strip).toContainText('It may buy something.');
  await expect(main.getByTestId('chat-page-acting')).toHaveCount(0);
  await shot(main, '03-the-strip-asks');
  await main.getByTestId('chat-page-ask-allow').click();
  await expect(strip).toHaveCount(0);
  await expect.poll(async () => inPage(yap, RAIL_RESULTS_URL, 'location.pathname'), { timeout: 20_000 }).toMatch(/\/buy$/);
  // Back beside the chat: the thread's card says it was allowed (Rust's answer, in step).
  await main.getByRole('button', { name: 'Back beside the chat' }).click();
  await expect(main.getByText('enter your card details yourself')).toBeVisible({ timeout: 20_000 });
  await expect(card).toHaveAttribute('data-state', 'allowed');
  expect(lastResult(chatCalls(fakeLlm, from)[3])).toMatchObject({ success: true, displayText: 'Clicked ‘Buy tickets’ on rail.example.com (you allowed it)' });
});

test('asked out loud with Chat in view: a key press just runs, a click that buys asks out loud too, and a spoken yes lets it through', async ({ yap, main, fakeLlm, shot }) => {
  // (8 Nov 2026: every page action was refused on a spoken turn, "only for a question typed in Chat".)
  await setConfig(yap, main, { ...chatConfig(fakeLlm), speakReplies: true });
  await yap.invoke('tts_e2e_spoken', { clear: true });
  const spokenTexts = async () => (await yap.invoke('tts_e2e_spoken')).map((s) => s.text);
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: RAIL_RESULTS_URL } }] },
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    { tools: [{ name: 'browser_key', args: { keys: 'PageDown' } }] },
    { tools: [{ name: 'browser_click', args: (r) => ({ ref: refIn(r, 'Buy tickets') }) }] },
    { content: 'The payment page is open: enter your card details yourself.' },
  ]);
  try {
    // Asked out loud with Yap's main window in front (the test's focus): Chat in view.
    await yap.invoke('e2e_assistant_ask', { text: `Buy me a ticket for the 09:42 here: ${RAIL_RESULTS_URL}`, listenMs: 0 });
    // The key: no card, as a scroll needs none.
    await expect.poll(() => chatCalls(fakeLlm, from).length, { timeout: 30_000 }).toBeGreaterThanOrEqual(4);
    expect(chatCalls(fakeLlm, from)[0].tools).toEqual(expect.arrayContaining(['browser_click', 'browser_type', 'browser_key', 'browser_hover', 'browser_form_input']));
    expect(lastResult(chatCalls(fakeLlm, from)[3])).toMatchObject({ success: true, displayText: 'Pressed PageDown on rail.example.com' });
    // The click that buys: the thread's card, and the same question said out loud.
    const card = main.getByTestId('chat-page-confirm');
    await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
    await expect(card).toContainText('Yap wants to press ‘Buy tickets’ on rail.example.com');
    await expect.poll(spokenTexts, { timeout: 20_000 }).toContain('Press “Buy tickets” on rail dot example dot com? Say yes, or press Allow.'); // (the voice says a site's dots)
    await shot(main, '08-asked-out-loud-before-buying');
    // The spoken yes: the next question answers it, and isn't a question of its own.
    await yap.invoke('e2e_assistant_ask', { text: 'Yes, go ahead', listenMs: 0 });
    await expect(card).toHaveAttribute('data-state', 'allowed', { timeout: 20_000 });
    await expect(main.getByText('enter your card details yourself')).toBeVisible({ timeout: 20_000 });
    expect(lastResult(chatCalls(fakeLlm, from)[4])).toMatchObject({ success: true, displayText: 'Clicked ‘Buy tickets’ on rail.example.com (you allowed it)', data: { asked: true, allowed: true } });
    expect(await inPage(yap, RAIL_RESULTS_URL, 'location.pathname')).toMatch(/\/buy$/);
    const chats = yap.readJson('chats.json')?.conversations ?? [];
    expect(chats.some((c) => c.messages.some((m) => m.role === 'user' && m.text === 'Yes, go ahead'))).toBe(false);
  } finally {
    await setConfig(yap, main, { speakReplies: false });
  }
});

test('Enter in a chat page’s message box asks first; typing there keeps the draft', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  let from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: CHAT_URL } }] },
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    { tools: [{ name: 'browser_type', args: (r) => ({ ref: refIn(r, 'Message #general'), text: 'see you at 6', submit: true }) }] },
    { content: 'I didn’t send it: you said no.' },
  ]);
  await ask(main, `Tell #general see you at 6, on ${CHAT_URL}`);
  // The message box is no form: Enter there would send, so Yap asks (in the thread and the strip).
  await expect.poll(() => chatCalls(fakeLlm, from).length, { timeout: 30_000 }).toBeGreaterThanOrEqual(3);
  expect(lastResult(chatCalls(fakeLlm, from)[2]).data.elements).toMatch(/\[\d+\] textbox "Message #general" = "Hello"/);
  const card = main.getByTestId('chat-page-confirm');
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 20_000 });
  await expect(card).toContainText('Yap wants to press Enter in ‘Message #general’ on chat.example.com');
  await expect(card).toContainText('It may send something.');
  await expect(main.getByTestId('chat-page-ask')).toContainText('Yap wants to press Enter in ‘Message #general’');
  await card.getByRole('button', { name: "Don't" }).click();
  await expect(main.getByText('I didn’t send it: you said no.')).toBeVisible({ timeout: 20_000 });
  // The strip let it go with the card; nothing typed, nothing sent.
  await expect(main.getByTestId('chat-page-ask')).toHaveCount(0);
  expect(lastResult(chatCalls(fakeLlm, from)[3])).toMatchObject({ success: false, displayText: "Didn't press Enter in ‘Message #general’: you said no", data: { asked: true, allowed: false } });
  expect(await inPage(yap, CHAT_URL, "document.querySelectorAll('#sent li').length")).toBe(0);
  expect(await inPage(yap, CHAT_URL, BOX_TEXT)).toBe('Hello');

  // Typed without Enter: in after the draft, never over it; the result says what the box holds.
  from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    { tools: [{ name: 'browser_type', args: (r) => ({ ref: refIn(r, 'Message #general'), text: 'see you at 6' }) }] },
    { content: 'It’s in the box, not sent.' },
  ]);
  await say(main, 'Just type see you at 6 in the box then, don’t send it');
  await expect(main.getByText('It’s in the box, not sent.')).toBeVisible({ timeout: 30_000 });
  const typed = lastResult(chatCalls(fakeLlm, from)[2]);
  expect(typed).toMatchObject({ success: true, data: { field: 'Hello see you at 6' } });
  expect(typed.note).toMatch(/^The words went in at the cursor/);
  expect(await inPage(yap, CHAT_URL, BOX_TEXT)).toBe('Hello see you at 6');
  expect(await inPage(yap, CHAT_URL, "document.querySelectorAll('#sent li').length")).toBe(0);
});

test('what an action changed is said as it was: a hover’s popup, Escape closing it, PageDown; acting on a tab behind brings it to the front; a ref that never was', async ({ yap, main, fakeLlm }) => {
  // (8 Nov 2026, Yap Lab: each of these said "nothing visible changed",
  // Escape left the preview up, and [99999] was told "it changed, or it went".)
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  const SAID = 'Edda Vane kept the light for thirty years.';
  const keepersTab = (r) => toolResults(r).flatMap((x) => x?.data?.tabs || []).find((t) => t.url.includes('Lighthouse_keepers'))?.tab;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_tabs', args: { action: 'new', url: SMOOTH_URL } }, { name: 'browser_tabs', args: { action: 'new', url: RAIL_URL } }] },
    { tools: [{ name: 'browser_tabs', args: { action: 'list' } }] },
    { tools: [{ name: 'browser_read', args: (r) => ({ mode: 'elements', tab: keepersTab(r) }) }] },
    { tools: [{ name: 'browser_hover', args: (r) => ({ ref: refIn(r, 'Edda Vane'), tab: keepersTab(r) }) }] },
    { tools: [{ name: 'browser_key', args: { keys: 'Escape' } }] },
    { tools: [{ name: 'browser_key', args: { keys: 'PageDown' } }] },
    { tools: [{ name: 'browser_click', args: { ref: 99999 } }] },
    { content: SAID },
  ]);
  await ask(main, `Open ${SMOOTH_URL} and ${RAIL_URL}, then show me the preview of the first keeper on the keepers page`);
  await expect(main.getByText(SAID)).toBeVisible({ timeout: 60_000 });
  const calls = chatCalls(fakeLlm, from);
  // The hover, on the keepers tab behind the rail site's: that tab in front first.
  const hovered = lastResult(calls[4]);
  expect(hovered).toMatchObject({ success: true, displayText: 'Hovered over ‘Edda Vane’ on wiki.example.com' });
  expect(hovered.data.changed).toMatch(/^a popup opened: ‘Edda Vane kept the Saltmarsh light for thirty years\.’/);
  await expect.poll(async () => (await views(yap)).find((v) => v.key === keyOf(SMOOTH_URL))?.onScreen, { timeout: 20_000 }).toBe(true);
  await expect.poll(async () => (await views(yap)).find((v) => v.key === keyOf(RAIL_URL))?.onScreen).toBe(false);
  // Escape: the hover let go, the preview closed — and said so.
  const escaped = lastResult(calls[5]);
  expect(escaped.data.changed).toMatch(/^a popup closed/);
  expect(await inPage(yap, SMOOTH_URL, "getComputedStyle(document.getElementById('pv')).display")).toBe('none');
  // PageDown: where it went.
  const paged = lastResult(calls[6]);
  expect(paged.data.changed).toMatch(/^scrolled to y \d+$/);
  expect(Number(/\d+$/.exec(paged.data.changed)[0])).toBeGreaterThan(0);
  // A ref the page never gave.
  const none = lastResult(calls[7]);
  expect(none).toMatchObject({ success: false, displayText: "Couldn't click: no element [99999] on this page" });
  expect(none.error).toMatch(/^There's no element \[99999\]: refs on this page go up to \d+\./);
});

test('a dropdown is chosen in from its options with no card; a search form sent by its own button after typing needs none', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  const SAID = 'Sorted by edit date, and searched for keepers.';
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: PREFS_URL } }] },
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    { tools: [{ name: 'browser_form_input', args: (r) => ({ ref: refIn(r, 'Sorting order'), value: 'Edit date' }) }] },
    { tools: [{ name: 'browser_type', args: (r) => ({ ref: refIn(r, 'Search the wiki'), text: 'keepers' }) }] },
    { tools: [{ name: 'browser_click', args: (r) => ({ ref: refIn(r, 'Go') }) }] },
    { content: SAID },
  ]);
  // (Its option's words aren't the person's, nor in the page's text: the list is closed.)
  await ask(main, `On ${PREFS_URL}, sort my searches by when pages were last changed, then search the wiki for keepers`);
  await expect(main.getByText(SAID)).toBeVisible({ timeout: 60_000 });
  const calls = chatCalls(fakeLlm, from);
  expect(lastResult(calls[2]).data.elements).toContain('combobox "Sorting order" = "Relevance" options: Relevance | Edit date | Creation date');
  const chose = lastResult(calls[3]);
  expect(chose).toMatchObject({ success: true, displayText: 'Chose ‘Edit date’ in ‘Sorting order’ on wiki.example.com', data: { chosen: 'Edit date' } });
  expect(chose.data.asked).toBeUndefined();
  // The page's own script took the choice (the page has gone on to the search since).
  expect(chose.data.changed).toMatch(/^new on the page: .*Sorted by edit date/);
  // Typed, then sent by its form's own button: a search, no card.
  expect(lastResult(calls[4])).toMatchObject({ success: true, data: { field: 'keepers' } });
  const went = lastResult(calls[5]);
  expect(went.data.changed).toMatch(/^went to wiki\.example\.com\/wiki\/search\?q=keepers/);
  expect(went.data.asked).toBeUndefined();
  await expect(main.getByTestId('chat-page-confirm')).toHaveCount(0);
});

test('a message typed into a chat’s box, then its send button (an arrow, no words) pressed right after: a send, asked as Enter there is', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: CHAT_URL } }] },
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    { tools: [{ name: 'browser_type', args: (r) => ({ ref: refIn(r, 'Message #general'), text: 'see you at 6' }) }] },
    { tools: [{ name: 'browser_click', args: (r) => ({ ref: refIn(r, '➤') }) }] },
    { content: 'I didn’t send it: you said no.' },
  ]);
  await ask(main, `Tell #general see you at 6, on ${CHAT_URL}`);
  const card = main.getByTestId('chat-page-confirm');
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 30_000 });
  await expect(card).toContainText('Yap wants to press ‘➤’ on chat.example.com');
  await expect(card).toContainText('It may send something.');
  await card.getByRole('button', { name: "Don't" }).click();
  await expect(main.getByText('I didn’t send it: you said no.')).toBeVisible({ timeout: 20_000 });
  const calls = chatCalls(fakeLlm, from);
  expect(lastResult(calls[3])).toMatchObject({ success: true, data: { field: 'Hello see you at 6' } });
  expect(lastResult(calls[4])).toMatchObject({ success: false, displayText: "Didn't click ‘➤’: you said no", data: { asked: true, allowed: false } });
  expect(await inPage(yap, CHAT_URL, "document.querySelectorAll('#sent li').length")).toBe(0);
});

test('browser_tabs switches to a tab beside the chat', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  // (Tab numbers are each tab's own for the session: the read lists them.)
  const wikiTab = (r) => lastResult(r).data.tabs.find((t) => t.url.includes('Fort_Joy')).tab;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_tabs', args: { action: 'new', url: FORT_JOY_URL } }, { name: 'browser_tabs', args: { action: 'new', url: RAIL_URL } }] },
    { tools: [{ name: 'browser_read', args: {} }] },
    { tools: [{ name: 'browser_tabs', args: (r) => ({ action: 'switch', tab: wikiTab(r) }) }] },
    { content: 'The wiki is in front.' },
  ]);
  await ask(main, `Open ${FORT_JOY_URL} and ${RAIL_URL}, then show me the wiki`);
  await expect(main.getByText('The wiki is in front.')).toBeVisible({ timeout: 30_000 });
  const calls = chatCalls(fakeLlm, from);
  const n = wikiTab(calls[2]);
  expect(n).toBe((await views(yap)).find((v) => v.key === keyOf(FORT_JOY_URL)).tab);
  expect(lastResult(calls[3])).toMatchObject({ success: true, displayText: `Showed tab ${n} beside the chat`, data: { tab: n } });
  await expect(main.getByTestId('chat-page-title')).toHaveText('Fort Joy - Example Wiki', { timeout: 20_000 });
  await expect.poll(async () => (await views(yap)).find((v) => v.key === keyOf(FORT_JOY_URL))?.onScreen, { timeout: 20_000 }).toBe(true);
  await expect.poll(async () => (await views(yap)).find((v) => v.key === keyOf(RAIL_URL))?.onScreen).toBe(false);
});

/** Chat on a strong fake model that reads images ("gpt-5" and "vision" in its id). */
function seeingConfig(fakeLlm) {
  return { ...chatConfig(fakeLlm), llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-gpt-5-vision' } } };
}

/** The latest screenshot's size in a request's results ("900×700" → [900, 700]). */
function shotSize(request) {
  const shotResult = toolResults(request).reverse().find((r) => typeof r?.data?.size === 'string' && r.frame);
  return shotResult ? shotResult.data.size.split('×').map(Number) : [0, 0];
}

test('a click at a point of the screenshot: the element under it, judged as a ref click is; a password box refused', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, seeingConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: BOARD_URL } }] },
    { tools: [{ name: 'browser_screenshot', args: {} }] },
    // The east field: three quarters across, a third down.
    { tools: [{ name: 'browser_click', args: (r) => ({ x: Math.round(shotSize(r)[0] * 0.75), y: Math.round(shotSize(r)[1] * 0.3) }) }] },
    // The password box, a quarter from the bottom.
    { tools: [{ name: 'browser_click', args: (r) => ({ x: Math.round(shotSize(r)[0] * 0.2), y: Math.round(shotSize(r)[1] * 0.74) }) }] },
    { content: 'I picked the east field; the password is yours to type.' },
  ]);
  await ask(main, `On the field map, pick the east field: ${BOARD_URL}`);
  await expect(main.getByText('I picked the east field')).toBeVisible({ timeout: 60_000 });
  const calls = chatCalls(fakeLlm, from);
  expect(calls[0].tools).toContain('browser_screenshot');
  expect(lastResult(calls[2])).toMatchObject({ success: true, displayText: 'Looked at the page · maps.example.com' });
  expect(lastResult(calls[2]).frame).toContain("browser_click's x, y are in this picture's pixels");
  // The picture the model got is the page's view at its CSS pixels: a point in it is that point of the page.
  expect(shotSize(calls[3])).toEqual(await inPage(yap, BOARD_URL, '[window.innerWidth, window.innerHeight]'));
  const clicked = lastResult(calls[3]);
  // (A block with no words: named by its id.)
  expect(clicked).toMatchObject({ success: true, displayText: 'Clicked ‘e’ on maps.example.com', data: { at: { role: 'div' } } });
  expect(await inPage(yap, BOARD_URL, "document.getElementById('picked').textContent")).toBe('Picked: east field');
  const refused = lastResult(calls[4]);
  expect(refused).toMatchObject({ success: false, displayText: "Didn't click ‘Password’: Yap never types a password", data: { refused: 'sensitive' } });
  expect(await inPage(yap, BOARD_URL, "document.activeElement ? document.activeElement.id : ''")).not.toBe('pw');
});

/** The ref of the match named `name` in the turn's latest find (its match lines). */
function foundRef(request, name) {
  const found = toolResults(request).reverse().find((r) => typeof r?.data?.matches === 'string');
  const m = found && new RegExp(`\\[(\\d+)\\] \\w+ "${name}"`).exec(found.data.matches);
  return m ? Number(m[1]) : 0;
}

test('web components: inside open shadow roots as in the light DOM — fields listed, secrets refused, a click opens their dialog, the box typed in and sent', async ({ yap, main, fakeLlm }) => {
  // (8 Nov 2026, Yap Lab on MDN: its search is a web component — the click
  // hit the host and "nothing visible changed", typing was "not a field", a
  // find found nothing.)
  await setConfig(yap, main, seeingConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  const SAID = 'I searched the docs for table.';
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: DOCS_URL } }] },
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    { tools: [{ name: 'browser_type', args: (r) => ({ ref: refIn(r, 'Password'), text: 'hunter2' }) }] },
    { tools: [{ name: 'browser_type', args: (r) => ({ ref: refIn(r, 'Verification code'), text: '123456' }) }] },
    { tools: [{ name: 'browser_screenshot', args: {} }] },
    // The search button: the top right, a quarter of the view wide.
    { tools: [{ name: 'browser_click', args: (r) => ({ x: Math.round(shotSize(r)[0] * 0.875), y: Math.round(shotSize(r)[1] * 0.06) }) }] },
    { tools: [{ name: 'browser_find', args: { query: 'Search the docs' } }] },
    { tools: [{ name: 'browser_type', args: (r) => ({ ref: foundRef(r, 'Search the docs'), text: 'table' }) }] },
    { tools: [{ name: 'browser_key', args: { keys: 'Enter' } }] },
    { content: SAID },
  ]);
  await ask(main, `Search the docs for table on ${DOCS_URL}, and sign me in: my password is hunter2`);
  await expect(main.getByText(SAID)).toBeVisible({ timeout: 60_000 });
  const calls = chatCalls(fakeLlm, from);
  // Listed from inside the roots: the button, the sign-in form's fields.
  const read = lastResult(calls[2]);
  expect(read.data.elements).toMatch(/\[\d+\] button "Search"/);
  expect(read.data.elements).toMatch(/\[\d+\] textbox "Password" \(Yap never types here: a password\)/);
  // Labelled in its own root (the page's other "code-l" says "Nickname").
  expect(read.data.elements).toMatch(/\[\d+\] textbox "Verification code" \(Yap never types here: a security code\)/);
  expect(read.data.elements).not.toContain('Closed field');
  expect(read.data.elements).not.toContain('Nickname');
  // The secrets: refused as in the light DOM, nothing typed.
  expect(lastResult(calls[3])).toMatchObject({ success: false, displayText: "Didn't type in ‘Password’: Yap never types a password", data: { refused: 'sensitive' } });
  expect(lastResult(calls[4])).toMatchObject({ success: false, displayText: "Didn't type in ‘Verification code’: Yap never types a security code", data: { refused: 'sensitive' } });
  // A point on the search button: the button inside its root, not the host; its dialog opened, the focus in its box.
  const clicked = lastResult(calls[6]);
  expect(clicked).toMatchObject({ success: true, displayText: 'Clicked ‘Search’ on docs.example.com', data: { at: { role: 'button' } } });
  expect(clicked.data.changed).toMatch(/^a dialog opened: ‘Search the docs’; the focus moved into ‘Search the docs’/);
  // Found, typed in, sent by Enter: a search, no card.
  expect(lastResult(calls[7]).data.matches).toMatch(/\[\d+\] searchbox "Search the docs"/);
  expect(lastResult(calls[8])).toMatchObject({ success: true, displayText: 'Typed in ‘Search the docs’ on docs.example.com', data: { field: 'table' } });
  const sent = lastResult(calls[9]);
  expect(sent).toMatchObject({ success: true, displayText: 'Pressed Enter on docs.example.com' });
  expect(sent.data.changed).toMatch(/^went to docs\.example\.com\/search\?q=table/);
  expect(sent.data.asked).toBeUndefined();
  await expect(main.getByTestId('chat-page-confirm')).toHaveCount(0);
  expect(await inPage(yap, DOCS_URL, 'location.pathname')).toMatch(/\/docs\.example\.com\/search$/);
});

/** The ref of the match named `name` in any of the turn's finds so far (the latest first). */
function anyFound(request, name) {
  for (const r of toolResults(request).reverse()) {
    const m = typeof r?.data?.matches === 'string' && new RegExp(`\\[(\\d+)\\] \\w+ "${name}"`).exec(r.data.matches);
    if (m) return Number(m[1]);
  }
  return 0;
}

test('clicks that aren’t one and drags: a double click selects or edits, a right-click opens the page’s own menu or says it has none, drags reorder a list and move a puck, a drop on the trash asks first', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  const SAID = 'Renamed it, Card C is first and the puck is at the right; I left Card B.';
  const find = (query) => ({ tools: [{ name: 'browser_find', args: { query } }] });
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: TASKS_URL } }] },
    find('lantern'),
    { tools: [{ name: 'browser_click', args: (r) => ({ ref: foundRef(r, 'lantern'), count: 2 }) }] },
    find('Spring plan'),
    { tools: [{ name: 'browser_click', args: (r) => ({ ref: foundRef(r, 'Spring plan'), count: 2 }) }] },
    { tools: [{ name: 'browser_type', args: { text: 'Summer plan' } }] },
    find('Paint the shed'),
    { tools: [{ name: 'browser_click', args: (r) => ({ ref: foundRef(r, 'Paint the shed'), button: 'right' }) }] },
    { tools: [{ name: 'browser_key', args: { keys: 'Escape' } }] },
    find('Plain notes'),
    { tools: [{ name: 'browser_click', args: (r) => ({ ref: foundRef(r, 'Plain notes with no menu of their own.'), button: 'right' }) }] },
    find('Card'),
    { tools: [{ name: 'browser_drag', args: (r) => ({ from: { ref: foundRef(r, 'Card C') }, to: { ref: foundRef(r, 'Card A') } }) }] },
    find('Puck'),
    find('Right end'),
    { tools: [{ name: 'browser_drag', args: (r) => ({ from: { ref: anyFound(r, 'Puck') }, to: { ref: anyFound(r, 'Right end') } }) }] },
    find('Trash'),
    { tools: [{ name: 'browser_drag', args: (r) => ({ from: { ref: anyFound(r, 'Card B') }, to: { ref: anyFound(r, 'Trash') } }) }] },
    { content: SAID },
  ]);
  await ask(main, `On the task board, rename Spring plan to Summer plan, put Card C first and the puck at the right end, then bin Card B: ${TASKS_URL}`);
  // The drop on the trash: Yap's card asks, in the thread.
  const card = main.getByTestId('chat-page-confirm');
  await expect(card).toBeVisible({ timeout: 90_000 });
  await expect(card).toContainText('Yap wants to drag ‘Card B’ onto ‘Trash’');
  await expect(card).toContainText('It may delete something.');
  await shot(main, 'drag-onto-the-trash-asks');
  await card.getByRole('button', { name: "Don't" }).click();
  await expect(main.getByText(SAID)).toBeVisible({ timeout: 30_000 });
  const calls = chatCalls(fakeLlm, from);
  // A double click on a word: it's selected, as a person's selects it.
  const word = lastResult(calls[3]);
  expect(word).toMatchObject({ success: true, displayText: 'Double-clicked ‘lantern’ on board.example.com', data: { selected: 'lantern' } });
  expect(word.data.changed).toMatch(/^selected ‘lantern’/);
  // A double click the page edits on: its editor, the focus in it, the words typed there.
  expect(lastResult(calls[5]).data.changed).toContain('the focus moved into ‘Title’');
  expect(lastResult(calls[6])).toMatchObject({ success: true, displayText: 'Typed in ‘Title’ on board.example.com', data: { field: 'Summer plan' } });
  expect(await inPage(yap, TASKS_URL, "document.getElementById('title-edit').value")).toBe('Summer plan');
  // A right-click: the page's own menu, never a card; none of its own: said so.
  const menu = lastResult(calls[8]);
  expect(menu).toMatchObject({ success: true, displayText: 'Right-clicked ‘Paint the shed’ on board.example.com' });
  expect(menu.data.changed).toMatch(/^a menu opened: ‘Rename Archive’/);
  expect(menu.data.asked).toBeUndefined();
  expect(lastResult(calls[11]).data.changed).toMatch(/^nothing opened: the page has no menu of its own there/);
  // A drag in a list (HTML5): where it went, said as it is.
  const moved = lastResult(calls[13]);
  expect(moved).toMatchObject({ success: true, displayText: 'Dragged ‘Card C’ to ‘Card A’ on board.example.com', data: { dropped: true, moved: { to: 1, of: 3 } } });
  expect(moved.data.changed).toMatch(/^moved ‘Card C’ from place 3 to place 1 of 3 \(before ‘Card A’\)/);
  // The puck, by the pointer's own events: at the right.
  const puck = lastResult(calls[16]);
  expect(puck).toMatchObject({ success: true, displayText: 'Dragged ‘Puck’ to ‘Right end’ on board.example.com' });
  expect(puck.data.changed).toContain('Puck at the right');
  // The drop on the trash: asked and refused, nothing deleted.
  expect(lastResult(calls[18])).toMatchObject({ success: false, displayText: "Didn't drag ‘Card B’: you said no", data: { asked: true, allowed: false } });
  expect(await inPage(yap, TASKS_URL, "[].map.call(document.querySelectorAll('#order li'), function (l) { return l.textContent; })")).toEqual(['Card C', 'Card A', 'Card B']);
  expect(await inPage(yap, TASKS_URL, "[document.getElementById('where').textContent, document.getElementById('bin').textContent]")).toEqual(['Puck at the right.', '']);
  // Folded under the answer: the clicks and the drags counted.
  await expect(main.getByTestId('chat-steps').last().locator('summary')).toContainText('dragged 2 times on the page');
});

test('a same-origin frame is the page’s: read, its fields listed, clicked at a point and typed in, its form sent; a sandboxed one is said as it is', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, seeingConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  const SAID = 'You are signed up as Edda Vane; the payment frame is yours to fill in.';
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: FRAMES_URL } }] },
    { tools: [{ name: 'browser_read', args: {} }] },
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    { tools: [{ name: 'browser_type', args: (r) => ({ ref: refIn(r, 'Frame password'), text: 'hunter2' }) }] },
    { tools: [{ name: 'browser_screenshot', args: {} }] },
    // The frame's name box (30–48 % of the view), then its Save (50–70 %).
    { tools: [{ name: 'browser_click', args: (r) => ({ x: Math.round(shotSize(r)[0] * 0.5), y: Math.round(shotSize(r)[1] * 0.39) }) }] },
    { tools: [{ name: 'browser_type', args: { text: 'Edda Vane' } }] },
    { tools: [{ name: 'browser_click', args: (r) => ({ x: Math.round(shotSize(r)[0] * 0.5), y: Math.round(shotSize(r)[1] * 0.6) }) }] },
    { tools: [{ name: 'browser_click', args: (r) => ({ ref: refIn(r, 'Payment') }) }] },
    { content: SAID },
  ]);
  await ask(main, `Sign me up for the newsletter as Edda Vane, my password is hunter2: ${FRAMES_URL}`);
  await expect(main.getByText(SAID)).toBeVisible({ timeout: 90_000 });
  const calls = chatCalls(fakeLlm, from);
  // Read: the frame's words with the page's; the sandboxed frame said as it is.
  const read = lastResult(calls[2]);
  expect(read.data.text).toContain('The form is in a frame below.');
  expect(read.data.text).toContain('Tell us your name.');
  expect(read.data.frames).toEqual([{ site: 'forms.example.com', sandboxed: true, title: 'Payment' }]);
  expect(read.note).toContain("It has a frame Yap can't read inside (forms.example.com)");
  const elements = lastResult(calls[3]).data.elements;
  expect(elements).toMatch(/\[\d+\] textbox "Name"/);
  expect(elements).toMatch(/\[\d+\] textbox "Frame password" \(Yap never types here: a password\)/);
  expect(elements).toMatch(/\[\d+\] frame "Payment" \(a frame the page keeps apart \(sandboxed, forms\.example\.com\): Yap can't read or act inside it; a screenshot shows it\)/);
  // Its password field: refused, as anywhere.
  expect(lastResult(calls[4])).toMatchObject({ success: false, displayText: "Didn't type in ‘Frame password’: Yap never types a password", data: { refused: 'sensitive' } });
  // A point of the shot inside the frame: its box (the frame's place taken off), the focus there; typed where it is.
  const box = lastResult(calls[6]);
  expect(box).toMatchObject({ success: true, displayText: 'Clicked ‘Name’ on forms.example.com', data: { at: { role: 'textbox' } } });
  expect(box.data.changed).toContain('the focus moved into ‘Name’');
  expect(lastResult(calls[7])).toMatchObject({ success: true, displayText: 'Typed in ‘Name’ on forms.example.com', data: { field: 'Edda Vane' } });
  // Its Save: the frame's form sent, the frame gone somewhere new (no card: it saves).
  const saved = lastResult(calls[8]);
  expect(saved).toMatchObject({ success: true, displayText: 'Clicked ‘Save’ on forms.example.com', data: { at: { role: 'button' } } });
  expect(saved.data.changed).toContain('a frame on the page went to forms.example.com/saved?name=Edda+Vane');
  expect(saved.data.asked).toBeUndefined();
  // The sandboxed frame: never reached inside.
  expect(lastResult(calls[9])).toMatchObject({ success: false, displayText: "Couldn't click ‘Payment’: a frame Yap can't reach inside" });
  expect(await inPage(yap, FRAMES_URL, "document.getElementById('f').contentDocument.body.innerText")).toContain('on the list');
});

test('a desktop width: a point of a shot from before the width changed is refused, one of a shot after lands on the wide layout’s search box', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, seeingConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  const SAID = 'I searched the guide for lantern.';
  const boxY = (r) => {
    const shotResult = toolResults(r).reverse().find((x) => typeof x?.data?.size === 'string' && x.frame);
    return Math.round(((WIDE_BOX.top + WIDE_BOX.height / 2) / shotResult.data.scroll.h) * shotSize(r)[1]);
  };
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: WIDE_URL } }] },
    { tools: [{ name: 'browser_screenshot', args: {} }] },
    { tools: [{ name: 'browser_tabs', args: { action: 'width', width: 1280 } }] },
    { tools: [{ name: 'browser_click', args: (r) => ({ x: Math.round(shotSize(r)[0] * 0.8), y: boxY(r) }) }] },
    { tools: [{ name: 'browser_screenshot', args: {} }] },
    { tools: [{ name: 'browser_click', args: (r) => ({ x: Math.round(shotSize(r)[0] * 0.8), y: boxY(r) }) }] },
    { tools: [{ name: 'browser_type', args: { text: 'lantern', submit: true } }] },
    { content: SAID },
  ]);
  await ask(main, `Search the field guide for lantern: ${WIDE_URL}`);
  await expect(main.getByText(SAID)).toBeVisible({ timeout: 90_000 });
  const calls = chatCalls(fakeLlm, from);
  expect(lastResult(calls[2]).data.width).toBeLessThan(900);
  // The shot from before is another layout now: its points refused, said why.
  const stale = lastResult(calls[4]);
  expect(stale).toMatchObject({ success: false, displayText: "Couldn't click: the page changed size since the screenshot" });
  expect(stale.error).toMatch(/laid out 1280×\d+ px now, \d+×\d+ then/);
  // A new shot at 1280: its point is the desktop layout's search box.
  expect(lastResult(calls[5]).data).toMatchObject({ width: 1280, desktop: 1280 });
  const clicked = lastResult(calls[6]);
  expect(clicked).toMatchObject({ success: true, displayText: 'Clicked ‘Search the guide’ on docs.example.com', data: { at: { role: 'searchbox' } } });
  const sent = lastResult(calls[7]);
  expect(sent).toMatchObject({ success: true, displayText: 'Typed in ‘Search the guide’ on docs.example.com and sent it' });
  expect(sent.data.changed).toMatch(/^went to docs\.example\.com\/search\?q=lantern/);
  await expect(main.getByTestId('chat-page-width')).toBeVisible();
});

test('a batch runs its steps in order, its shots in the steps, and stops at a card said no to', async ({ yap, main, fakeLlm, shot }) => {
  await setConfig(yap, main, seeingConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: RAIL_RESULTS_URL } }] },
    {
      tools: [
        {
          name: 'browser_batch',
          args: {
            actions: [
              { tool: 'browser_find', args: { query: 'Buy tickets' } },
              { tool: 'browser_scroll', args: { direction: 'down' } },
              { tool: 'browser_screenshot' },
              { tool: 'browser_zoom', args: { region: 'top' } },
              { tool: 'browser_click', args: { ref: 1 } },
              { tool: 'browser_screenshot' },
            ],
          },
        },
      ],
    },
    { content: 'I stopped before buying: you said no.' },
  ]);
  await ask(main, `Have a look at the 09:42 and buy it: ${RAIL_RESULTS_URL}`);
  // The click asks first, in the middle of the batch: Don't.
  const card = main.getByTestId('chat-page-confirm');
  await expect(card).toHaveAttribute('data-state', 'asking', { timeout: 40_000 });
  await expect(card).toContainText('Yap wants to press ‘Buy tickets’ on rail.example.com');
  await card.getByRole('button', { name: "Don't" }).click();
  await expect(main.getByText('I stopped before buying: you said no.')).toBeVisible({ timeout: 30_000 });
  const calls = chatCalls(fakeLlm, from);
  const batch = lastResult(calls[2]);
  expect(batch).toMatchObject({ success: false, data: { ran: 5, of: 6 } });
  expect(batch.displayTexts).toEqual([
    expect.stringMatching(/^Found \d+ matches for ‘Buy tickets’ on rail\.example\.com$/),
    'Scrolled down rail.example.com',
    'Looked at the page · rail.example.com',
    'Zoomed in on the page · rail.example.com',
    "Didn't click ‘Buy tickets’: you said no",
  ]);
  expect(batch.note).toMatch(/^Stopped at step 5 of 6/);
  // Its two pictures went to the model, in order; nothing was bought.
  const sent = calls[2].messages.filter((m) => m.role === 'user' && Array.isArray(m.content)).flatMap((m) => m.content.filter((p) => p.type === 'image_url'));
  expect(sent).toHaveLength(2);
  expect(await inPage(yap, RAIL_RESULTS_URL, 'location.pathname')).toMatch(/\/results$/);
  // Each step a chip; the two shots on theirs.
  const steps = main.getByTestId('chat-steps').last();
  await expect(steps.getByTestId('chat-steps-shots').getByTestId('chat-step-shot')).toHaveCount(2);
  await steps.locator('summary').click();
  await expect(steps.locator('.toolchip')).toHaveCount(6);
  await expect(steps.locator('.toolchip.withshot')).toHaveCount(2);
  await expectStore(yap, 'chats.json', (s) => s.conversations.some((c) => c.messages.some((m) => m.text === 'I stopped before buying: you said no.' && m.pictures?.filter((p) => p.chip).length === 2)));
  await shot(main, '04-a-batch-stopped-at-a-card');
});

test('typing into a page’s search box and pressing Enter needs no card', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: CHAT_URL } }] },
    { tools: [{ name: 'browser_read', args: { mode: 'elements' } }] },
    { tools: [{ name: 'browser_type', args: (r) => ({ ref: refIn(r, 'Find in chat'), text: 'Sam' }) }] },
    { tools: [{ name: 'browser_key', args: { keys: 'Enter' } }] },
    { tools: [{ name: 'browser_key', args: { keys: 'ctrl+w' } }] },
    { content: 'I searched the chat for Sam.' },
  ]);
  await ask(main, `Find Sam in the chat on ${CHAT_URL}`);
  await expect(main.getByText('I searched the chat for Sam.')).toBeVisible({ timeout: 40_000 });
  const calls = chatCalls(fakeLlm, from);
  expect(lastResult(calls[3])).toMatchObject({ success: true, displayText: 'Typed in ‘Find in chat’ on chat.example.com', data: { field: 'Sam' } });
  const pressed = lastResult(calls[4]);
  expect(pressed).toMatchObject({ success: true, displayText: 'Pressed Enter on chat.example.com' });
  expect(pressed.data.asked).toBeUndefined();
  expect(pressed.data.focused).toMatch(/textbox "Find in chat"/);
  await expect(main.getByTestId('chat-page-confirm')).toHaveCount(0);
  // A key Yap doesn't press: said so, nothing pressed.
  expect(lastResult(calls[5])).toMatchObject({ success: false, displayText: "Couldn't press keys: none Yap presses" });
  // The message box sent nothing.
  expect(await inPage(yap, CHAT_URL, "document.querySelectorAll('#sent li').length")).toBe(0);
});

test('the tabs beside the chat: listed, and one closed', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  const railTab = (r) => lastResult(r).data.tabs.find((t) => t.url.startsWith(RAIL_URL)).tab;
  fakeLlm.scriptChat([
    { tools: [{ name: 'browser_navigate', args: { url: FORT_JOY_URL } }] },
    { tools: [{ name: 'browser_tabs', args: { action: 'new', url: RAIL_URL } }] },
    { tools: [{ name: 'browser_tabs', args: { action: 'list' } }] },
    { tools: [{ name: 'browser_tabs', args: (r) => ({ action: 'close', tab: railTab(r) }) }] },
    { content: 'The rail tab is closed; the wiki stays.' },
  ]);
  await ask(main, `Open ${FORT_JOY_URL} and ${RAIL_URL}, then close the rail one`);
  await expect(main.getByText('The rail tab is closed; the wiki stays.')).toBeVisible({ timeout: 40_000 });
  const calls = chatCalls(fakeLlm, from);
  expect(lastResult(calls[2])).toMatchObject({ success: true, displayText: 'Brought up rail.example.com beside the chat' });
  expect(lastResult(calls[3])).toMatchObject({ success: true, displayText: 'Listed the tabs beside the chat (2)' });
  const n = railTab(calls[3]);
  expect(lastResult(calls[4])).toMatchObject({ success: true, displayText: `Closed tab ${n} beside the chat` });
  await expect.poll(async () => (await views(yap)).some((v) => v.key === keyOf(RAIL_URL))).toBe(false);
  expect((await views(yap)).some((v) => v.key === keyOf(FORT_JOY_URL))).toBe(true);
});
