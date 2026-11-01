// Acting on the pages beside the chat (src-tauri/src/chat/pages/act.rs: the
// drive tier, step 3 of the page panel):
// - a strong model (the fake's "fake-gpt-5": documents::rich::strong) gets
//   click / type_text / scroll / go_to / switch_tab and read_open_page's
//   `elements`; it fills a made-up rail site's journey form, sends it and
//   reads the next train — the tab says "Yap is using this page" (with Stop)
//   while the turn goes;
// - a click that would buy asks first: a card in the thread, "Yap wants to
//   press ‘Buy tickets’ on rail.example.com" — Don't refuses it, Allow once
//   lets it through — and the payment page's card fields are never typed;
// - a sign-in form is the person's: its password and email refused;
// - switch_tab shows the tab beside the chat.
// A model that isn't strong ("fake-chat") gets none of it (chat-pages.spec.js).
//
// Nothing leaves the PC: the model is the scripted fake AI
// (support/fake-llm.js), the web a local fake (support/fake-web.js, through
// the debug-only YAP_WEB_STANDIN). The pages are their own WebView2s laid over
// Yap's window: the checks read them through the debug-only
// `e2e_page_view_eval` / `e2e_page_views`. Fictional sites only.
import { test as base, expect, setConfig, openView } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeWeb, RAIL_URL, RAIL_RESULTS_URL, RAIL_SIGNIN_URL, FORT_JOY_URL } from './support/fake-web.js';

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
    { tools: [{ name: 'open_page', args: { url: RAIL_URL } }] },
    { tools: [{ name: 'read_open_page', args: { elements: true } }] },
    { tools: [{ name: 'type_text', args: (r) => ({ ref: refIn(r, 'From'), text: 'Kellbrook' }) }] },
    { tools: [{ name: 'type_text', args: (r) => ({ ref: refIn(r, 'To'), text: 'Port Ellis', submit: true }) }] },
    // (Held: the turn stays open while the tab's strip is checked.)
    { content: 'The next train leaves at 09:42 from platform 2.', gate: held },
  ]);
  await ask(main, `Find the next train from Kellbrook to Port Ellis on ${RAIL_URL}`);

  // The drive tier's tools and line, for a strong model.
  await expect.poll(() => chatCalls(fakeLlm, from).length, { timeout: 30_000 }).toBeGreaterThanOrEqual(5);
  const calls = chatCalls(fakeLlm, from);
  expect(calls[0].tools).toEqual(expect.arrayContaining(['open_page', 'read_open_page', 'click', 'type_text', 'scroll', 'go_to', 'switch_tab']));
  expect(calls[0].system).toContain('To act on the page beside the chat');
  expect(calls[0].system).toContain('Yap never types passwords, card or bank details');

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
    { tools: [{ name: 'open_page', args: { url: RAIL_URL } }] },
    { tools: [{ name: 'scroll', args: { to: 'down' } }] },
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
    { tools: [{ name: 'open_page', args: { url: RAIL_RESULTS_URL } }] },
    { tools: [{ name: 'read_open_page', args: { elements: true } }] },
    { tools: [{ name: 'click', args: (r) => ({ ref: refIn(r, 'Buy tickets') }) }] },
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
    { tools: [{ name: 'click', args: { ref: 1 } }] },
    { tools: [{ name: 'read_open_page', args: { elements: true } }] },
    { tools: [{ name: 'type_text', args: (r) => ({ ref: refIn(r, 'Card number'), text: 'my card' }) }] },
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
    { tools: [{ name: 'open_page', args: { url: RAIL_SIGNIN_URL } }] },
    { tools: [{ name: 'read_open_page', args: { elements: true } }] },
    {
      tools: [
        { name: 'type_text', args: (r) => ({ ref: refIn(r, 'Password'), text: 'Tulip-Kettle-42' }) },
        { name: 'type_text', args: (r) => ({ ref: refIn(r, 'Email'), text: 'sam.rivers@example.com' }) },
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

test('switch_tab shows the tab beside the chat', async ({ yap, main, fakeLlm }) => {
  await setConfig(yap, main, chatConfig(fakeLlm));
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'open_page', args: { url: FORT_JOY_URL } }, { name: 'open_page', args: { url: RAIL_URL } }] },
    { tools: [{ name: 'switch_tab', args: { tab: 1 } }] },
    { content: 'The wiki is in front.' },
  ]);
  await ask(main, `Open ${FORT_JOY_URL} and ${RAIL_URL}, then show me the wiki`);
  await expect(main.getByText('The wiki is in front.')).toBeVisible({ timeout: 30_000 });
  expect(lastResult(chatCalls(fakeLlm, from)[2])).toMatchObject({ success: true, displayText: 'Showed tab 1 beside the chat', data: { tab: 1 } });
  await expect(main.getByTestId('chat-page-title')).toHaveText('Fort Joy - Example Wiki', { timeout: 20_000 });
  await expect.poll(async () => (await views(yap)).find((v) => v.key === keyOf(FORT_JOY_URL))?.onScreen, { timeout: 20_000 }).toBe(true);
  await expect.poll(async () => (await views(yap)).find((v) => v.key === keyOf(RAIL_URL))?.onScreen).toBe(false);
});
