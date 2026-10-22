// About you (Settings → General; src-tauri/src/about_you.rs,
// src/lib/AboutYouSettings.svelte, 2026-10-21):
// - "Region and language" as Windows has it — in a test run the debug-only
//   YAP_E2E_LOCALE stands in for Windows (never the PC's own settings): an
//   en-GB format, home location United Kingdom, £ GBP, metric, the Windows
//   zone "GMT Standard Time" (→ Europe/London) — shown as one line;
// - Change: pickers for the country, language, currency, units and time
//   zone; a country picked brings its own currency and units; "Use Windows'
//   settings" puts it back; the picks are saved (config.json);
// - "What should Yap call you?" and "Anything the AI should know about you"
//   (300 characters at most, counted), saved; "Use my region's spelling" on,
//   naming British English; "See what the AI is told" shows the block;
// - Chat: the model's system prompt ends with that block (where, the
//   clock, prices in £, the name, the note); a web search asks Exa for
//   results from the UK (userLocation "GB").
//
// The model is the scripted fake AI (support/fake-llm.js), the web the fake
// web (support/fake-web.js): nothing goes on the internet.
import { test as base, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeWeb } from './support/fake-web.js';

const UK = JSON.stringify({ locale: 'en-GB', country: 'GB', currency: 'GBP', symbol: '£', metric: true, timeZone: 'GMT Standard Time' });
const SUMMARY = 'United Kingdom · English (UK) · £ GBP · metric · UK time';
const EXA_KEY = 'fake-exa-key-for-tests';

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
        name: 'about-you',
        env: { ...fakeWeb.env, YAP_E2E_LOCALE: UK },
        config: {
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat' } },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const aboutGroup = (dialog) => dialog.getByRole('group', { name: 'About you' });

test('About you: the region from Windows, changed and put back, the name and note saved', async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'General');
  const group = aboutGroup(dialog);
  await expect(group).toBeVisible();
  await expect(group.getByText(SUMMARY)).toBeVisible();
  await expect(group.getByText("AI cleanup writes British English: honour, recognise, paralyse, metre, cancelled, catalogue, defence")).toBeVisible();
  await group.scrollIntoViewIfNeeded();
  await shot(main, '01-about-you');

  // Change: the pickers, each following Windows until picked.
  await group.getByRole('button', { name: 'Change' }).click();
  const pickers = group.getByRole('group', { name: 'Region and language' });
  const country = pickers.getByRole('combobox', { name: 'Country' });
  await expect(country).toHaveValue('');
  await expect(country.locator('option').first()).toHaveText('Windows: United Kingdom');
  await expect(pickers.getByRole('combobox', { name: 'Time zone' }).locator('option').first()).toHaveText('Windows: UK time (Europe/London)');
  await expect(pickers.getByRole('combobox', { name: 'Currency' }).locator('option').first()).toHaveText('Automatic: £ GBP');
  await shot(main, '02-about-you-change');

  // The US: its own currency and units; the language and the clock stay Windows'.
  await country.selectOption('US');
  await expect(group.getByText('United States · English (UK) · $ USD · US units · UK time')).toBeVisible();
  await expect(pickers.getByRole('combobox', { name: 'Currency' }).locator('option').first()).toHaveText('Automatic: $ USD');
  await expect(pickers.getByText(`Windows says: ${SUMMARY}.`)).toBeVisible();
  await expectStore(yap, 'config.json', (c) => c.regionCountry === 'US');
  // A currency and a time zone picked too.
  await pickers.getByRole('combobox', { name: 'Currency' }).selectOption('EUR');
  await pickers.getByRole('combobox', { name: 'Time zone' }).selectOption('America/New_York');
  await expect(group.getByText('United States · English (UK) · € EUR · US units · New York time')).toBeVisible();
  await shot(main, '03-about-you-us-picked');
  await expectStore(yap, 'config.json', (c) => c.regionCurrency === 'EUR' && c.regionTimeZone === 'America/New_York');
  // Back to Windows' settings.
  await pickers.getByRole('button', { name: "Use Windows' settings" }).click();
  await expect(group.getByText(SUMMARY)).toBeVisible();
  await expectStore(yap, 'config.json', (c) => c.regionCountry === '' && c.regionCurrency === '' && c.regionTimeZone === '');
  await group.getByRole('button', { name: 'Done' }).click();
  await expect(pickers).toBeHidden();

  // The name and the note, counted and capped.
  await group.getByRole('textbox', { name: 'What should Yap call you?' }).fill('Nathan');
  const about = group.getByRole('textbox', { name: 'Anything the AI should know about you' });
  await about.fill("I'm in Manchester, I play PC games, I work day shifts");
  await expect(group.getByText('53/300')).toBeVisible();
  await expect(about).toHaveAttribute('maxlength', '300');
  await expectStore(yap, 'config.json', (c) => c.aboutName === 'Nathan' && c.aboutMe.startsWith("I'm in Manchester"));

  // What the AI is told, as it reads now.
  await group.getByRole('button', { name: 'See what the AI is told' }).click();
  const told = group.getByLabel('What the AI is told');
  await expect(told).toContainText("About the user: they're in the United Kingdom (British English); it's ");
  await expect(told).toContainText('Give times in UK time and metric units. Prices: in £ (GBP) when you have them in £');
  await expect(told).toContainText('Their name is Nathan.');
  await expect(told).toContainText("They told Yap: \"I'm in Manchester, I play PC games, I work day shifts\"");
  await expect(group.getByText('never sent to Yap', { exact: false })).toBeVisible();
  await group.scrollIntoViewIfNeeded();
  await shot(main, '04-about-you-filled');

  // "Use my region's spelling": on by default, saved off and on.
  const spelling = group.getByRole('button', { name: "Use my region's spelling", exact: true });
  await expect(spelling).toHaveAttribute('aria-pressed', 'true');
  await spelling.click();
  await expectStore(yap, 'config.json', (c) => c.regionSpelling === false);
  await spelling.click();
  await expectStore(yap, 'config.json', (c) => c.regionSpelling === true);
  await closeSettings(main);
});

test("Chat: the model is told about the user, and Exa searches the user's country", async ({ yap, main, fakeLlm, fakeWeb }) => {
  // The name and note from the test before (one Yap per worker), set here
  // too so this test stands alone; web search on with an Exa key.
  const cfg = await yap.invoke('get_config');
  await yap.invoke('save_config', {
    cfg: { ...cfg, aboutName: 'Nathan', aboutMe: 'I work day shifts', webSearch: true, webSearchService: 'exa', webSearchKeys: { exa: EXA_KEY } },
  });
  await main.evaluate(
    (p) => window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: p })),
    { aboutName: 'Nathan', aboutMe: 'I work day shifts', webSearch: true, webSearchService: 'exa', webSearchKeys: { exa: EXA_KEY } }
  );
  await main.waitForTimeout(500);

  const from = fakeLlm.requests.length;
  const webFrom = fakeWeb.requests.length;
  // A price question: a model on this PC must search first (needs_web, price words).
  fakeLlm.scriptChat([
    { tools: [{ name: 'web_search', args: { query: 'Grok subscription price UK' } }] },
    { content: 'SuperGrok is £24 a month in the UK [1].' },
  ]);
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).click();
  await main.getByRole('textbox', { name: 'Message' }).fill('How much is a Grok subscription?');
  await main.getByRole('button', { name: 'Send' }).click();
  await expect(main.locator('.msgs .cbubble.assistant').last()).toContainText('SuperGrok is £24 a month');

  const steps = fakeLlm.requests.slice(from).filter((r) => r.tools.length);
  // The search made to come first: named, over the same tools as the next step.
  expect(steps[0].toolChoice).toEqual({ type: 'function', function: { name: 'web_search' } });
  expect(steps[1].tools).toEqual(steps[0].tools);
  const system = steps[0].system;
  // The block, last in the prompt — without the clock, so the prompt stays
  // the same from turn to turn (a local model re-uses its cache)…
  const block = system.slice(system.lastIndexOf('About the user: '));
  expect(block).toMatch(/^About the user: they're in the United Kingdom \(British English\)\. Give times/);
  expect(block).not.toMatch(/\d\d:\d\d/);
  expect(block).toContain(
    `Give times in UK time and metric units. Prices: in £ (GBP) when you have them in £; never change a price's currency sign — a price in another currency stays as it is, labelled, like "$15 a month (US price)".`
  );
  expect(block).toContain('Their name is Nathan.');
  expect(block).toContain('They told Yap: "I work day shifts"');
  // …and the clock with the question, in its context.
  expect(steps[0].user).toMatch(/^<context>\nIt's \w+day \d{1,2} \w+ \d{4}, \d\d:\d\d UK time \(UTC\+0[01]:00\)\./);
  expect(steps[0].user).toMatch(/<\/context>\n\nHow much is a Grok subscription\?$/);
  // The web tools' line on times names the user's.
  expect(system).toContain("convert it to the user's time (UK time)");

  // Exa was asked for results from the user's country.
  const exa = fakeWeb.requests.slice(webFrom).filter((r) => r.service === 'exa');
  expect(exa.length).toBeGreaterThan(0);
  expect(exa[0].body.userLocation).toBe('GB');
  expect(exa[0].body.query).toBe('Grok subscription price UK');

  // Prices in $ without saying whose: Yap says so under the answer.
  fakeLlm.scriptChat([
    { tools: [{ name: 'web_search', args: { query: 'Grok Premium price' } }] },
    { content: 'Grok Premium is $30/month [1].' },
  ]);
  await main.getByRole('textbox', { name: 'Message' }).fill('How much is Grok Premium?');
  await main.getByRole('button', { name: 'Send' }).click();
  const answer = main.locator('.msgs .cbubble.assistant').last();
  await expect(answer).toContainText('Grok Premium is $30/month');
  await expect(answer.locator('em')).toHaveText('Prices in $ are as the sources give them (most likely US dollars), not in £.');
});
