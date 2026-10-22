// About you (Settings → General; src-tauri/src/about_you.rs,
// src/lib/AboutYouSettings.svelte, 2026-10-21; one Region picker since
// 2026-10-22):
// - "Region": "Automatic: United Kingdom (from Windows)" — in a test run the
//   debug-only YAP_E2E_LOCALE stands in for Windows (never the PC's own
//   settings): an en-GB format, home location United Kingdom, £ GBP,
//   metric, the Windows zone "GMT Standard Time" (→ Europe/London) — or any
//   country; one quiet line under it says what follows ("Prices in £ ·
//   miles and °C · British spelling · UK time"), and changes with the
//   country; the pick is saved (config.json), Automatic saves "";
// - no other region pickers and no spelling switch here: the spelling's
//   one switch is in Language Models → Dictation Cleanup ("Write in my
//   region's spelling"), its line naming the variety and the region;
// - "What should Yap call you?" and "Anything the AI should know about you"
//   (300 characters at most, counted), saved; "See what the AI is told"
//   shows the block;
// - Chat: the model's system prompt ends with that block (where, prices in
//   £, the name, the note) and its web line says to search for UK prices;
//   a web search asks Exa for results from the UK (userLocation "GB"), and
//   a price question's query names the UK.
//
// The model is the scripted fake AI (support/fake-llm.js), the web the fake
// web (support/fake-web.js): nothing goes on the internet.
import { test as base, expect, openView, openSettings, closeSettings, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { startFakeWeb } from './support/fake-web.js';

const UK = JSON.stringify({ locale: 'en-GB', country: 'GB', currency: 'GBP', symbol: '£', metric: true, timeZone: 'GMT Standard Time' });
const UK_FOLLOWS = 'Prices in £ · miles and °C · British spelling · UK time';
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

test('About you: one Region picker — Automatic from Windows, a country picked and put back — and the name and note saved', async ({
  yap,
  main,
  shot,
}) => {
  const dialog = await openSettings(main, 'General');
  const group = aboutGroup(dialog);
  await expect(group).toBeVisible();
  // One picker, Automatic: Windows' country; what follows from it, in a line.
  const region = group.getByRole('combobox', { name: 'Region' });
  await expect(region).toHaveValue('');
  await expect(region.locator('option').first()).toHaveText('Automatic: United Kingdom (from Windows)');
  await expect(region.locator('option[value="FR"]')).toHaveText('France');
  const follows = group.getByTestId('region-follows');
  await expect(follows).toHaveText(UK_FOLLOWS);
  // No other region pickers, and the spelling switch isn't here any more.
  await expect(group.getByRole('combobox')).toHaveCount(1);
  await expect(group.getByRole('button', { name: /spelling/i })).toHaveCount(0);
  await region.scrollIntoViewIfNeeded();
  await shot(main, '01-about-you');

  // A country picked: everything follows it (the clock stays the PC's).
  await region.selectOption('US');
  await expect(follows).toHaveText('Prices in $ · miles and °F · American spelling · UK time');
  await expectStore(yap, 'config.json', (c) => c.regionCountry === 'US');
  await region.selectOption('DE');
  await expect(follows).toHaveText('Prices in € · km and °C · British spelling · UK time');
  await expectStore(yap, 'config.json', (c) => c.regionCountry === 'DE');
  await shot(main, '02-about-you-region-picked');
  // Back to Automatic: saved as "", and none of the old fields.
  await region.selectOption('');
  await expect(follows).toHaveText(UK_FOLLOWS);
  await expectStore(
    yap,
    'config.json',
    (c) => c.regionCountry === '' && !('regionCurrency' in c) && !('regionTimeZone' in c) && !('regionLanguage' in c) && !('regionUnits' in c)
  );

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
  await expect(told).toContainText(
    'Give times in UK time and metric units, but miles for distances and speeds. Prices: in £ (GBP) when you have them in £'
  );
  await expect(told).toContainText('Their name is Nathan.');
  await expect(told).toContainText("They told Yap: \"I'm in Manchester, I play PC games, I work day shifts\"");
  await expect(group.getByText('never sent to Yap', { exact: false })).toBeVisible();
  await group.scrollIntoViewIfNeeded();
  await shot(main, '03-about-you-filled');
  await closeSettings(main);
});

test("Dictation Cleanup: the one spelling switch, named for the region's variety", async ({ yap, main, shot }) => {
  const dialog = await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: 'Dictation Cleanup' }).click();
  const spellingGroup = dialog.getByRole('group', { name: 'Spelling' });
  const spelling = spellingGroup.getByRole('button', { name: "Write in my region's spelling", exact: true });
  await expect(spelling).toBeVisible();
  await expect(spelling).toHaveAttribute('aria-pressed', 'true');
  // It works with cleanup: off with it.
  await expect(spelling).toBeDisabled();
  await dialog.getByRole('button', { name: 'Enable text cleanup', exact: true }).click();
  await expect(spelling).toBeEnabled();
  await expect(spellingGroup).toContainText(
    'British spelling, from your region (United Kingdom): honour, recognise, paralyse, metre, cancelled, catalogue, defence. Only the spelling changes, never your words.'
  );
  await spellingGroup.scrollIntoViewIfNeeded();
  await shot(main, '04-cleanup-spelling');
  // Saved off and on.
  await spelling.click();
  await expectStore(yap, 'config.json', (c) => c.regionSpelling === false);
  await spelling.click();
  await expectStore(yap, 'config.json', (c) => c.regionSpelling === true);

  // The region picked in About you: the line follows it.
  await openSettings(main, 'General');
  await aboutGroup(dialog).getByRole('combobox', { name: 'Region' }).selectOption('US');
  await openSettings(main, 'Language Models');
  await dialog.getByRole('tab', { name: 'Dictation Cleanup' }).click();
  await expect(spellingGroup).toContainText('American spelling, from your region (United States): honor, recognize');
  // Cleanup off and the region Automatic again, for the next test.
  await dialog.getByRole('button', { name: 'Enable text cleanup', exact: true }).click();
  await openSettings(main, 'General');
  await aboutGroup(dialog).getByRole('combobox', { name: 'Region' }).selectOption('');
  await expectStore(yap, 'config.json', (c) => c.regionCountry === '' && c.postProcessEnabled === false);
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
  // The search made to come first: a tool call required, over the same
  // tools as the next step.
  expect(steps[0].toolChoice).toBe('required');
  expect(steps[1].tools).toEqual(steps[0].tools);
  const system = steps[0].system;
  // The block, last in the prompt — without the clock, so the prompt stays
  // the same from turn to turn (a local model re-uses its cache)…
  const block = system.slice(system.lastIndexOf('About the user: '));
  expect(block).toMatch(/^About the user: they're in the United Kingdom \(British English\)\. Give times/);
  expect(block).not.toMatch(/\d\d:\d\d/);
  expect(block).toContain(
    `Give times in UK time and metric units, but miles for distances and speeds. Prices: in £ (GBP) when you have them in £; never change a price's currency sign — a price in another currency stays as it is, labelled, like "$15 a month (US price)".`
  );
  expect(block).toContain('Their name is Nathan.');
  expect(block).toContain('They told Yap: "I work day shifts"');
  // …and the clock with the question, in its context.
  expect(steps[0].user).toMatch(/^<context>\nIt's \w+day \d{1,2} \w+ \d{4}, \d\d:\d\d UK time \(UTC\+0[01]:00\)\./);
  expect(steps[0].user).toMatch(/<\/context>\n\nHow much is a Grok subscription\?$/);
  // The web tools' line on times names the user's; the one on prices, their
  // country.
  expect(system).toContain("convert it to the user's time (UK time)");
  expect(system).toContain('For a price, a subscription or something to buy, search for the UK price ("UK" in the query).');

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
  // The model's search didn't say where: Yap aimed it at the UK (its
  // "price", carried over from the search before, left out: fresh_query).
  const searches = fakeWeb.requests.slice(webFrom).filter((r) => r.service === 'exa');
  expect(searches.map((r) => r.body.query)).toEqual(['Grok subscription price UK', 'Grok Premium UK']);
  await expect(main.locator('.msgs .toolchips').last().locator('.toolchip').first()).toContainText('Searched the web: "Grok Premium UK"');
});
