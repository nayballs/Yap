// Visuals in the answer (`show_visual`: desktop/src-tauri/src/chat/made.rs, made/serve.rs;
// desktop/src/lib/views/chat/VisualFrame.svelte, visualBridge.js, made.svelte.js; after the Claude
// app's "Made a visual"):
// - A diagram: drawn above the answer's words in a sandboxed frame (`sandbox="allow-scripts"`, the
//   `yapmade` scheme), sized to what it draws, in Yap's colours — and the dark theme's the moment it
//   changes; folded and unfolded; seen large (⤢) and closed with Esc; kept in the chat's folder
//   (chat-made/<chat>/made.json and its code), shown again when the chat opens again.
// - The frame is apart from Yap: the probe inside it finds none of Yap's IPC, can't reach the page,
//   storage, the network (the policy's own violations, never a request), a popup or the window's
//   address — and what it posts to the page pretending to be Yap, or sends with no click, does nothing.
// - A widget's button sends a message into its own chat only when clicked (`sendPrompt`, text only); a
//   link in it opens in the person's browser only when clicked (a test run keeps it: e2e_opened_urls).
// - Made after outside content (seeded): its button puts the message in the box instead.
// The model is the scripted fake AI (support/fake-llm.js), named like a strong model (the tool is
// offered to those). Nothing leaves the PC. Fictional content only. Light and dark.
import fs from 'node:fs';
import path from 'node:path';
import { test as base, expect, setConfig, openView } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

const NOW = Math.floor(Date.now() / 1000);

const DIAGRAM = `<svg viewBox="0 0 640 120" xmlns="http://www.w3.org/2000/svg" font-family="var(--font)" font-size="14">
  <defs><marker id="a" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0 0L10 5L0 10z" fill="var(--muted)"/></marker></defs>
  <g stroke-width="1.5">
    <rect x="8" y="30" width="130" height="56" rx="10" fill="var(--c1-soft)" stroke="var(--c1)"/>
    <rect x="178" y="30" width="130" height="56" rx="10" fill="var(--c2-soft)" stroke="var(--c2)"/>
    <rect x="348" y="30" width="130" height="56" rx="10" fill="var(--c3-soft)" stroke="var(--c3)"/>
    <rect x="518" y="30" width="114" height="56" rx="10" fill="var(--surface)" stroke="var(--border)"/>
  </g>
  <g fill="var(--text)" text-anchor="middle"><text x="73" y="63">Hotkey</text><text x="243" y="63">Record</text><text x="413" y="63">Transcribe</text><text x="575" y="63">Paste</text></g>
  <g stroke="var(--muted)" stroke-width="1.5" marker-end="url(#a)"><path d="M140 58H174"/><path d="M310 58H344"/><path d="M480 58H514"/></g>
</svg>`;

// The probe: everything a visual might try, its findings written into the page for the test to read.
const PROBE = `<style>.row{display:flex;gap:8px;align-items:center}</style>
<div class="row"><label>Bill <input id="bill" type="number" value="48"></label><button id="go" onclick="sendPrompt('Make the tip 20%')">Ask for 20%</button>
<a id="guide" href="https://example.com/tipping-guide">Tipping guide</a></div>
<pre id="probe">…</pre>
<script>
const out = { violations: [] };
document.addEventListener('securitypolicyviolation', (e) => out.violations.push(e.violatedDirective.split(' ')[0]));
(async () => {
  out.tauri = typeof (window.__TAURI_INTERNALS__ && window.__TAURI_INTERNALS__.invoke);
  out.ipc = typeof (window.ipc && window.ipc.postMessage);
  try { out.parentDoc = parent.document.body ? 'open' : 'none'; } catch (e) { out.parentDoc = 'blocked'; }
  try { out.parentTauri = typeof parent.__TAURI_INTERNALS__; } catch (e) { out.parentTauri = 'blocked'; }
  try { localStorage.setItem('k', '1'); out.storage = 'open'; } catch (e) { out.storage = 'blocked'; }
  try { await fetch('http://ipc.localhost/open_external', { method: 'POST', body: JSON.stringify({ url: 'https://evil.example/fetch' }) }); out.ipcFetch = 'reached'; } catch (e) { out.ipcFetch = 'blocked'; }
  try { await fetch('https://example.com/'); out.net = 'reached'; } catch (e) { out.net = 'blocked'; }
  out.img = await new Promise((res) => { const i = new Image(); i.onload = () => res('loaded'); i.onerror = () => res('blocked'); i.src = 'https://example.com/pixel.png'; setTimeout(() => res('timeout'), 4000); });
  try { out.popup = window.open('https://example.com/') ? 'opened' : 'blocked'; } catch (e) { out.popup = 'blocked'; }
  try { top.location.href = 'https://example.com/'; out.top = 'tried'; } catch (e) { out.top = 'blocked'; }
  try { if (window.chrome && chrome.webview) { chrome.webview.postMessage(JSON.stringify({ cmd: 'open_external', callback: 1, error: 2, payload: { url: 'https://evil.example/webview' } })); out.webview = 'tried'; } else out.webview = 'none'; } catch (e) { out.webview = 'blocked'; }
  // Tauri's own invoke, as WebView2 put it in every frame: refused, or never answered.
  const tryInvoke = (cmd, args) => new Promise((res) => {
    try { Promise.resolve(window.__TAURI_INTERNALS__.invoke(cmd, args)).then(() => res('ran'), (e) => res('refused: ' + String(e).slice(0, 80))); } catch (e) { res('threw: ' + String(e).slice(0, 80)); }
    setTimeout(() => res('no answer'), 3000);
  });
  out.invoke = await tryInvoke('open_external', { url: 'https://evil.example/invoke' });
  out.invokeRename = await tryInvoke('chat_rename', { id: 61, title: 'Renamed by a visual' });
  try { if (window.ipc && window.ipc.postMessage) { window.ipc.postMessage(JSON.stringify({ cmd: 'open_external', callback: 1, error: 2, payload: { url: 'https://evil.example/ipc' } })); out.ipcPost = 'tried'; } else out.ipcPost = 'none'; } catch (e) { out.ipcPost = 'blocked'; }
  // Pretending to be Yap's IPC, or a click, at the page: ignored.
  parent.postMessage({ cmd: 'open_external', payload: { url: 'https://evil.example/post' } }, '*');
  parent.postMessage({ yapVisual: 1, op: 'link', href: 'https://evil.example/auto' }, '*');
  sendPrompt('A message nobody clicked');
  out.origin = String(self.origin);
  setTimeout(() => { document.getElementById('probe').textContent = JSON.stringify(out); }, 300);
})();
</script>`;

const OUTSIDE_ID = 'v_00000000000a5e01';
const OUTSIDE = `<p>Pick one:</p><button id="pick" onclick="sendPrompt('Book the cheaper one')">The cheaper one</button>`;

const CHATS = {
  conversations: [
    {
      id: 61,
      title: 'Holiday options',
      createdTs: NOW - 900,
      updatedTs: NOW - 880,
      messages: [
        { role: 'user', text: 'Compare the two cabins from that page as something I can pick from', ts: NOW - 900 },
        { role: 'assistant', text: 'Here they are side by side.', ts: NOW - 899 },
      ],
    },
  ],
};

const test = base.extend({
  fakeLlm: [
    async ({}, use) => {
      const fake = await startFakeLlm();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ fakeLlm }, use) => {
      await use({
        name: 'chat-visuals',
        data: { 'chats.json': CHATS },
        config: {
          webSearch: false,
          meetingReminder: 'never',
          // Named like a strong model: the tool is offered to those (chat/made.rs `offered`).
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-gpt-5' } },
        },
        // A visual made after a page was read (seeded): its button fills the box.
        prepare: (dir) => {
          const folder = path.join(dir, 'chat-made', '61');
          fs.mkdirSync(folder, { recursive: true });
          const item = { id: OUTSIDE_ID, kind: 'visual', title: 'Two cabins', format: 'html', after: { index: 0, ts: NOW - 900 }, at: (NOW - 899) * 1000, outside: true, bytes: OUTSIDE.length };
          fs.writeFileSync(path.join(folder, 'made.json'), JSON.stringify({ version: 1, created: NOW - 900, items: [item] }));
          fs.writeFileSync(path.join(folder, `${OUTSIDE_ID}.html`), OUTSIDE);
        },
      });
    },
    { scope: 'worker' },
  ],
});

const box = (main) => main.getByRole('textbox', { name: 'Message' });
const visuals = (main) => main.getByTestId('chat-visual');

/** A new chat, its question sent. */
async function ask(main, text) {
  await openView(main, 'Chat');
  const show = main.getByRole('button', { name: 'Show your chats' });
  if (await show.isVisible()) await show.click();
  await main.getByRole('button', { name: 'New chat' }).click();
  await box(main).fill(text);
  await main.getByRole('button', { name: 'Send' }).click();
}

/** The conversation `title` open. */
async function openConversation(main, title) {
  await openView(main, 'Chat');
  const show = main.getByRole('button', { name: 'Show your chats' });
  if (await show.isVisible()) await show.click();
  await main.locator('.conv').filter({ hasText: title }).click();
}

/** The frame of visual `n` (a Playwright Frame), once its document loaded. */
async function frameOf(main, n = 0) {
  const el = visuals(main).nth(n).getByTestId('chat-visual-frame');
  await expect(el).toBeVisible({ timeout: 20_000 });
  let frame = null;
  await expect
    .poll(async () => {
      frame = await (await el.elementHandle()).contentFrame();
      return frame ? await frame.evaluate(() => document.readyState).catch(() => '') : '';
    }, { message: 'the visual’s document to load' })
    .toBe('complete');
  return frame;
}

const theVar = (frame, name) => frame.evaluate((n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim(), name);

test('a diagram: drawn above the answer, sized to it, in Yap’s colours, folded, seen large, kept', async ({ yap, main, fakeLlm, shot }) => {
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([
    { tools: [{ name: 'show_visual', args: { title: 'How dictation works', code: DIAGRAM } }] },
    { content: 'The diagram shows the four steps, from the hotkey to the paste.' },
  ]);
  await ask(main, 'Draw me a diagram of how dictation works');
  await expect(visuals(main)).toHaveCount(1, { timeout: 20_000 });
  await expect(main.getByText('The diagram shows the four steps')).toBeVisible();
  await expect(visuals(main).getByText('Made a visual')).toBeVisible();
  // Its step, said as what it was.
  await expect(main.getByTestId('chat-steps').first()).toContainText('Made a visual');

  // Offered to this model, with the line saying when.
  const offered = fakeLlm.requests.slice(from).find((r) => r.tools?.length);
  expect(offered.tools).toContain('show_visual');
  const result = JSON.parse(fakeLlm.requests.slice(from).filter((r) => r.tools?.length)[1].messages.filter((m) => m.role === 'tool').at(-1).content);
  expect(result).toMatchObject({ success: true, displayText: 'Made a visual', data: { shown: 'Made a visual: How dictation works' } });

  // Above the answer's words.
  const above = await main.evaluate(() => {
    const v = document.querySelector('[data-testid="chat-visual"]');
    const words = [...document.querySelectorAll('.cbubble.assistant')].find((b) => b.textContent.includes('four steps'));
    return !!(v && words && v.compareDocumentPosition(words) & Node.DOCUMENT_POSITION_FOLLOWING);
  });
  expect(above).toBe(true);

  // Sandboxed, from the yapmade scheme.
  const el = visuals(main).getByTestId('chat-visual-frame');
  await expect(el).toHaveAttribute('sandbox', 'allow-scripts');
  expect(await el.getAttribute('src')).toMatch(/^http:\/\/yapmade\.localhost\/v%2F\d+%2Fv_[0-9a-f]{16}$/);
  const frame = await frameOf(main);
  expect(await frame.evaluate(() => String(self.origin))).toBe('null');
  await expect(frame.locator('svg text').first()).toHaveText('Hotkey');

  // Sized to what it draws: its height is the drawing's (no scroll bar, no empty space).
  await expect
    .poll(async () => {
      const [fh, svgH] = [await el.evaluate((f) => f.getBoundingClientRect().height), await frame.evaluate(() => document.querySelector('svg').getBoundingClientRect().height)];
      return Math.abs(fh - svgH) <= 8 && svgH > 40;
    }, { message: 'the frame to take the drawing’s height' })
    .toBe(true);

  // Yap's colours, and the dark theme's the moment it changes.
  const fg = await main.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--yap-fg').trim());
  await expect.poll(() => theVar(frame, '--text')).toBe(fg);
  expect(await frame.evaluate(() => getComputedStyle(document.querySelector('text')).fill)).not.toBe('');
  await shot(main, '01-diagram');
  await setConfig(yap, main, { theme: 'dark' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe('dark');
  const darkFg = await main.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--yap-fg').trim());
  expect(darkFg).not.toBe(fg);
  await expect.poll(() => theVar(frame, '--text')).toBe(darkFg);
  await expect.poll(() => frame.evaluate(() => document.documentElement.dataset.theme)).toBe('dark');
  // The frame element's scheme is its document's (else Chromium paints an opaque backdrop behind it).
  expect(await el.evaluate((f) => getComputedStyle(f).colorScheme)).toBe('dark');
  expect(await frame.evaluate(() => document.documentElement.style.colorScheme)).toBe('dark');
  await shot(main, '02-diagram-dark');
  await setConfig(yap, main, { theme: 'light' });

  // Folded, and back.
  await visuals(main).getByTestId('chat-visual-fold').click();
  await expect(visuals(main).getByTestId('chat-visual-frame')).toHaveCount(0);
  await expect(visuals(main).getByTestId('chat-visual-fold')).toHaveAttribute('aria-expanded', 'false');
  await visuals(main).getByTestId('chat-visual-fold').click();
  await expect(visuals(main).getByTestId('chat-visual-frame')).toBeVisible();

  // Seen large; Esc closes it.
  await visuals(main).getByTestId('chat-visual-expand').click();
  const large = main.getByRole('dialog', { name: 'How dictation works' });
  await expect(large).toBeVisible();
  await expect(large.locator('iframe')).toHaveAttribute('sandbox', 'allow-scripts');
  await shot(main, '03-diagram-large');
  await main.keyboard.press('Escape');
  await expect(large).toHaveCount(0);

  // Kept in the chat's own folder, never chats.json…
  const chats = yap.readJson('chats.json');
  const conv = chats.conversations.find((c) => c.messages.some((m) => m.text?.includes('Draw me a diagram')));
  const index = JSON.parse(fs.readFileSync(path.join(yap.dataDir, 'chat-made', String(conv.id), 'made.json'), 'utf8'));
  expect(index.created).toBe(conv.createdTs);
  expect(index.items).toEqual([expect.objectContaining({ kind: 'visual', title: 'How dictation works', format: 'svg', after: { index: 0, ts: conv.messages[0].ts } })]);
  expect(fs.readFileSync(path.join(yap.dataDir, 'chat-made', String(conv.id), `${index.items[0].id}.svg`), 'utf8')).toBe(DIAGRAM);
  expect(JSON.stringify(chats)).not.toContain('Hotkey');
  // …and shown again when the chat opens again.
  await openConversation(main, 'Holiday options');
  await expect(visuals(main)).toHaveCount(1);
  await openConversation(main, conv.title);
  await expect(visuals(main)).toHaveCount(1);
  await expect((await frameOf(main)).locator('svg text').first()).toHaveText('Hotkey');
});

test('the frame is apart from Yap: no IPC, no page, no network; its button sends only when clicked', async ({ yap, main, fakeLlm, shot }) => {
  fakeLlm.scriptChat([
    { tools: [{ name: 'show_visual', args: { title: 'Tip calculator', code: PROBE } }] },
    { content: 'Here is a small tip calculator.' },
  ]);
  const opened = (await yap.invoke('e2e_opened_urls')).length;
  await ask(main, 'Make me a tip calculator I can keep');
  await expect(main.getByText('Here is a small tip calculator.')).toBeVisible({ timeout: 20_000 });
  const frame = await frameOf(main);
  let found = null;
  await expect
    .poll(async () => {
      const t = await frame.locator('#probe').textContent();
      found = t && t.startsWith('{') ? JSON.parse(t) : null;
      return !!found;
    }, { message: 'the probe to finish', timeout: 15_000 })
    .toBe(true);
  expect(found).toMatchObject({
    // WebView2 puts Tauri's (frozen) internals in every frame; the bridge takes away `chrome.webview`, which
    // their messages need, before the visual's code runs…
    webview: 'none',
    parentDoc: 'blocked',
    parentTauri: 'blocked',
    storage: 'blocked',
    ipcFetch: 'blocked',
    net: 'blocked',
    img: 'blocked',
    popup: 'blocked',
    origin: 'null',
  });
  // …so Tauri's own invoke, or a raw message, never runs a command: its request is blocked by the policy,
  // its message has nowhere to go (thrown inside Tauri's code, in the frame: the only error expected here).
  expect(found.invoke).not.toBe('ran');
  expect(found.invokeRename).not.toBe('ran');
  const errors = yap.pageErrors.slice(yap.seen.pageErrors);
  expect(errors.every((e) => /reading 'postMessage'/.test(e.message))).toBe(true);
  yap.seen.pageErrors = yap.pageErrors.length;
  // Blocked by the frame's own policy (connect-src, img-src), not by luck.
  expect(found.violations).toEqual(expect.arrayContaining(['connect-src', 'img-src']));
  // Yap's window stayed where it was.
  expect(main.url()).not.toContain('example.com');
  // Nothing it posted or sent by itself reached Yap: no link opened, no message sent.
  await main.waitForTimeout(800);
  expect((await yap.invoke('e2e_opened_urls')).slice(opened).filter((u) => u.includes('evil.example'))).toEqual([]);
  await expect(main.getByText('A message nobody clicked')).toHaveCount(0);
  expect(yap.readJson('chats.json').conversations.find((c) => c.id === 61).title).toBe('Holiday options');
  await shot(main, '04-widget');

  // The person's click on its button: a message in this chat, as if typed.
  const from = fakeLlm.requests.length;
  fakeLlm.scriptChat([{ content: 'With a 20% tip on £48 that’s £9.60.' }]);
  await frame.locator('#go').click();
  await expect(main.locator('.cbubble.user').filter({ hasText: 'Make the tip 20%' })).toBeVisible({ timeout: 10_000 });
  await expect(main.getByText('With a 20% tip on £48')).toBeVisible({ timeout: 20_000 });
  expect(fakeLlm.requests.slice(from).some((r) => JSON.stringify(r.messages ?? r.user ?? '').includes('Make the tip 20%'))).toBe(true);

  // Its link, clicked: the person's browser (kept by a test run), http(s) only.
  await frame.locator('#guide').click();
  await expect.poll(async () => (await yap.invoke('e2e_opened_urls')).slice(opened)).toContain('https://example.com/tipping-guide');
  expect((await yap.invoke('e2e_opened_urls')).filter((u) => u.includes('evil.example'))).toEqual([]);
});

test('made after outside content: its button puts the message in the box to read first', async ({ main, shot }) => {
  await openConversation(main, 'Holiday options');
  await expect(visuals(main)).toHaveCount(1);
  const frame = await frameOf(main);
  await frame.locator('#pick').click();
  await expect(box(main)).toHaveValue('Book the cheaper one');
  await expect(main.locator('.cbubble.user').filter({ hasText: 'Book the cheaper one' })).toHaveCount(0);
  await shot(main, '05-outside-fills-the-box');
  await box(main).fill('');
});

// Retry (15 Nov 2026, chat/chats/branches.rs): a visual hangs off its question, which a retry keeps — so the
// answer shown shows only what its own turn made (the fork's `since`, made.svelte.js `placeVisuals`).
test('a retried answer shows only the visuals its own turn made; ‹ back, the first answer’s are there again', async ({ main, fakeLlm }) => {
  fakeLlm.scriptChat([
    { tools: [{ name: 'show_visual', args: { title: 'Steps', code: DIAGRAM } }] },
    { content: 'The steps, drawn.' },
  ]);
  await ask(main, 'Draw the steps of a dictation');
  await expect(visuals(main)).toHaveCount(1, { timeout: 20_000 });
  await expect(main.getByText('The steps, drawn.')).toBeVisible();
  // (A second on: the retried answer's turn starts after the first answer was saved, as a person's click does.)
  await main.waitForTimeout(1_100);
  fakeLlm.scriptChat([{ content: 'In words: press, speak, press again.' }]);
  await main.locator('.cbubble.assistant').last().hover();
  await main.getByTestId('chat-retry').last().click();
  await expect(main.getByText('In words: press, speak, press again.')).toBeVisible({ timeout: 20_000 });
  await expect(main.getByTestId('chat-branch-count')).toHaveText('2/2');
  await expect(visuals(main)).toHaveCount(0);
  await main.getByTestId('chat-branch-prev').click();
  await expect(main.getByText('The steps, drawn.')).toBeVisible();
  await expect(visuals(main)).toHaveCount(1);
});
