// Plain Chat's start screen (16 Nov 2026, after the Claude app's new-chat screen;
// views/chat/ChatThread.svelte, ChatView.svelte, Composer.svelte, core/greeting.js):
//
// - The greeting by the time of day (core/greeting.js): "Afternoon, Sam" with About
//   you's name, "Good afternoon" without one; a small warm set late at night and very
//   early; a weekend touch now and then; one line per day and band.
// - A new chat: Yap's mark and the greeting in the display serif, centred, the box right
//   under it a little above the middle of the thread; none of the old meeting buttons
//   or lines; the box "How can I help you today?", two rows (the words, then + and the
//   mic), on the plain chat's column (the Claude app's proportions); the mic's tooltip
//   names the Voice Assistant's shortcut.
// - The first message sends the box down to its place at the foot: a gentle move, none
//   with Motion → Reduced.
// - A late night, shot with the test clock (`window.__yapTestClock` + `yap-test-clock`).
//
// The model is the scripted fake (support/fake-llm.js). Every name is fictional.
import { test as base, expect, openView, setConfig } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';
import { greetingFor, greetingBand, greetingLines, greetName, seedOf } from '../src/lib/core/greeting.js';

const NAME = 'Sam';

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
    async ({ fakeLlm }, use) =>
      use({
        name: 'chat-start',
        config: {
          memoryLearnChats: false,
          meetingReminder: 'never',
          webSearch: false,
          speakReplies: false,
          aboutName: NAME,
          assistantHotkey: 'kb:134',
          llmScopes: { chat: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-chat', apiKey: '', apiKeys: {}, prompt: '', disableThinking: false } },
        },
      }),
    { scope: 'worker' },
  ],
});

// Wednesday 18 and Saturday 21 November 2026, local time.
const at = (day, h, m = 0) => new Date(2026, 10, day, h, m);
const box = (main) => main.getByRole('textbox', { name: 'Message' });
const composer = (main) => main.locator('section.thread > .composer');
const start = (main) => main.getByTestId('chat-start');
const greeting = (main) => main.getByTestId('chat-greeting');
const rect = (loc) => loc.evaluate((el) => JSON.parse(JSON.stringify(el.getBoundingClientRect())));

async function theme(yap, main, which) {
  await setConfig(yap, main, { theme: which });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.yapTheme)).toBe(which);
}
/** The start screen's clock: a time of the test's (ms), or null for the real one. */
const setClock = (main, when) =>
  main.evaluate((ms) => {
    window.__yapTestClock = ms;
    window.dispatchEvent(new Event('yap-test-clock'));
  }, when == null ? null : when.getTime());
async function newChat(main) {
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
  await expect(start(main)).toBeVisible();
}

// ---------------------------------------------------------------- the greeting (pure)

test('the greeting: by the time of day, with or without a name, the same all day and all night', () => {
  // The bands.
  expect([3, 5, 6, 7, 11, 12, 17, 18, 22, 23].map((h) => greetingBand(at(18, h, 30)))).toEqual([
    'late',
    'early',
    'early',
    'morning',
    'morning',
    'afternoon',
    'afternoon',
    'evening',
    'evening',
    'late',
  ]);
  // Mostly plain: a weekday's morning, afternoon and evening.
  expect(greetingFor(NAME, at(18, 9))).toEqual({ text: 'Morning, Sam', band: 'morning' });
  expect(greetingFor(NAME, at(18, 14))).toEqual({ text: 'Afternoon, Sam', band: 'afternoon' });
  expect(greetingFor(NAME, at(18, 20))).toEqual({ text: 'Evening, Sam', band: 'evening' });
  // No name (or only spaces): just the greeting.
  expect(greetingFor('', at(18, 9)).text).toBe('Good morning');
  expect(greetingFor('   ', at(18, 14)).text).toBe('Good afternoon');
  expect(greetingFor(null, at(18, 20)).text).toBe('Good evening');
  // The name as About you has it, on one line; a long one cut at a word.
  expect(greetingFor('  Sam   Rivers ', at(18, 14)).text).toBe('Afternoon, Sam Rivers');
  expect(greetName('Alexandra Montgomery-Fairweather the Third of Dunmore')).toBe('Alexandra Montgomery-Fairweather the');

  // The odd hours: one of a small warm set, the name in it (or not), never a placeholder left.
  for (const band of ['late', 'early']) {
    const lines = greetingLines(band);
    expect(lines.length).toBeGreaterThanOrEqual(3);
    for (const l of lines) {
      expect(l.named).toContain('{name}');
      expect(l.plain).not.toContain('{name}');
      expect(l.named.length).toBeLessThanOrEqual(40);
    }
  }
  const late = greetingFor(NAME, at(18, 23, 40));
  expect(late.band).toBe('late');
  expect(greetingLines('late').map((l) => l.named.replace('{name}', NAME))).toContain(late.text);
  expect(greetingLines('late').map((l) => l.plain)).toContain(greetingFor('', at(18, 23, 40)).text);
  expect(greetingLines('early').map((l) => l.named.replace('{name}', NAME))).toContain(greetingFor(NAME, at(18, 6, 10)).text);

  // Steady: the same line all through a band, and one night is one night (23:40 and 01:30 the next day).
  expect(greetingFor(NAME, at(18, 23, 5)).text).toBe(late.text);
  expect(greetingFor(NAME, at(19, 1, 30)).text).toBe(late.text);
  expect(greetingFor(NAME, at(19, 4, 59)).text).toBe(late.text);
  expect(seedOf(at(19, 1, 30))).toBe('2026-11-18:late');
  expect(seedOf(at(18, 14))).toBe('2026-11-18:afternoon');
  // …and the nights don't all say the same.
  const nights = new Set(Array.from({ length: 30 }, (_, i) => greetingFor(NAME, new Date(2026, 10, 1 + i, 23, 30)).text));
  expect(nights.size).toBeGreaterThan(1);

  // The weekend now and then ("Happy Saturday, Sam"), on its own day only, never late at night.
  const weekends = [];
  for (let d = 0; d < 26 * 7; d += 1) {
    const day = new Date(2026, 0, 1 + d, 10);
    const g = greetingFor(NAME, day).text;
    if (day.getDay() === 0 || day.getDay() === 6) weekends.push(g);
    else expect(g).toBe('Morning, Sam');
    if (g.startsWith('Happy ')) expect(g).toBe(`Happy ${day.getDay() === 6 ? 'Saturday' : 'Sunday'}, Sam`);
  }
  const happy = weekends.filter((g) => g.startsWith('Happy ')).length;
  expect(happy, 'some weekend mornings').toBeGreaterThan(0);
  expect(happy, 'mostly plain').toBeLessThan(weekends.length / 2);
  expect(greetingFor('', at(21, 23, 30)).text).not.toMatch(/^Happy/);
});

// ---------------------------------------------------------------- the start screen

test('a new chat: the mark and the greeting, the box right under it in the middle, nothing about meetings', async ({ yap, main, shot }) => {
  await main.setViewportSize({ width: 1440, height: 900 });
  await setClock(main, at(18, 14, 5));
  await newChat(main);
  await expect(greeting(main)).toHaveText('Afternoon, Sam');
  await expect(start(main).locator('img.mark')).toBeVisible();
  // The display serif.
  expect(await greeting(main).evaluate((el) => getComputedStyle(el).fontFamily)).toMatch(/Garamond/);
  // None of the old empty chat's buttons and lines.
  for (const gone of ['Ask across your meetings', 'Prepare for my next meeting', 'Write my stand-up', 'List my open to-dos']) {
    await expect(main.getByRole('button', { name: gone })).toHaveCount(0);
  }
  await expect(main.getByRole('group', { name: 'Try asking' })).toHaveCount(0);
  await expect(main.getByText('for Skills, like')).toHaveCount(0);
  await expect(main.getByText('in any app to ask out loud')).toHaveCount(0);
  // The box: "How can I help you today?", the mic naming the assistant's shortcut.
  await expect(box(main)).toHaveAttribute('placeholder', 'How can I help you today?');
  await expect(main.getByRole('button', { name: 'Dictate your message' })).toHaveAttribute('title', 'Dictate. Or press F23 in any app to ask out loud');

  // Laid out as the Claude app's: centred together, the box a little above the thread's middle.
  const thread = await rect(main.locator('section.thread'));
  const g = await rect(greeting(main));
  const c = await rect(composer(main));
  const bar = await rect(composer(main).locator('.inputbar'));
  const mid = (r) => r.left + r.width / 2;
  expect(Math.abs(mid(g) - mid(c)), 'the greeting over the box').toBeLessThanOrEqual(24);
  expect(Math.abs(mid(c) - mid(thread)), 'the box in the thread’s centre').toBeLessThanOrEqual(2);
  expect(bar.top - g.bottom, 'the box right under the greeting').toBeGreaterThan(12);
  expect(bar.top - g.bottom).toBeLessThan(60);
  const middle = (bar.top + bar.height / 2 - thread.top) / thread.height;
  expect(middle, 'the box about the middle').toBeGreaterThan(0.38);
  expect(middle).toBeLessThan(0.52);

  // Two rows: the words, then + on the left and the mic on the right; the Claude app's proportions — its box
  // 3.7 times as wide as "How can I help you today?" in the box's own type, and 0.16 as tall as wide.
  const words = await rect(box(main));
  const plus = await rect(main.getByRole('button', { name: 'Chat options' }));
  const mic = await rect(main.getByRole('button', { name: 'Dictate your message' }));
  expect(plus.top).toBeGreaterThanOrEqual(words.bottom - 1);
  expect(mic.top).toBeGreaterThanOrEqual(words.bottom - 1);
  expect(plus.left - bar.left).toBeLessThan(20);
  expect(bar.right - mic.right).toBeLessThan(20);
  const said = await box(main).evaluate((el) => {
    const ctx = document.createElement('canvas').getContext('2d');
    const s = getComputedStyle(el);
    ctx.font = `${s.fontWeight} ${s.fontSize} ${s.fontFamily}`;
    return ctx.measureText(el.placeholder).width;
  });
  const ratio = bar.width / said;
  expect(ratio, `the box ${bar.width} px for ${said.toFixed(1)} px of words`).toBeGreaterThan(3.55);
  expect(ratio).toBeLessThan(3.9);
  expect(bar.height / bar.width).toBeGreaterThan(0.13);
  expect(bar.height / bar.width).toBeLessThan(0.19);
  await shot(main, '01-start-afternoon-light');
  await theme(yap, main, 'dark');
  await shot(main, '02-start-afternoon-dark');
  await theme(yap, main, 'light');

  // Late at night: one of the warm set.
  await setClock(main, at(18, 23, 40));
  const night = greetingFor(NAME, at(18, 23, 40)).text;
  await expect(greeting(main)).toHaveText(night);
  await expect(start(main)).toHaveAttribute('data-band', 'late');
  await shot(main, '03-start-late-light');
  await theme(yap, main, 'dark');
  await shot(main, '04-start-late-dark');
  await theme(yap, main, 'light');

  // No name in About you: just the greeting. (A narrow window: the box its own width, the greeting wraps.)
  await setConfig(yap, main, { aboutName: '' });
  await setClock(main, at(18, 9, 15));
  await expect(greeting(main)).toHaveText('Good morning');
  await main.setViewportSize({ width: 900, height: 700 });
  const narrow = await rect(composer(main));
  const t2 = await rect(main.locator('section.thread'));
  expect(narrow.right).toBeLessThanOrEqual(t2.right);
  await shot(main, '05-start-no-name-narrow');
  await setConfig(yap, main, { aboutName: NAME });
  await main.setViewportSize({ width: 1440, height: 900 });
  await setClock(main, null);
});

test('the + menu, the Claude app’s shape: icon rows, Skills ›, Research, Style ›; the keyboard; Ctrl+U; only the model under the box', async ({ yap, main, shot }) => {
  await main.setViewportSize({ width: 1440, height: 900 });
  await setClock(main, at(18, 14, 5));
  await newChat(main);
  // Windows' file picker never opens in a test run: the test's own picks nothing and counts (chatState `pickFiles`).
  await main.evaluate(() => {
    window.__pickerOpens = 0;
    window.__yapTestPickFiles = async () => {
      window.__pickerOpens += 1;
      return [];
    };
  });
  const pickerOpens = () => main.evaluate(() => window.__pickerOpens);
  const plus = main.getByRole('button', { name: 'Chat options' });
  const menu = main.getByTestId('chat-plus-menu');
  // Under the box: the model only (Style and Research went into the menu).
  const foot = main.locator('section.thread > .composer .composerfoot');
  await expect(foot.getByRole('button', { name: /^Style/ })).toHaveCount(0);
  await expect(foot.getByRole('button', { name: 'Research' })).toHaveCount(0);
  await expect(main.getByTestId('chat-box-modes')).toHaveCount(0);

  // Enter on + opens it at its first row; the rows top to bottom, an icon each.
  await plus.focus();
  await main.keyboard.press('Enter');
  await expect(menu).toBeVisible();
  const rows = menu.locator('[data-mi="top"]');
  await expect(rows.first()).toBeFocused();
  expect((await rows.allTextContents()).map((t) => t.replace(/\s+/g, ' ').trim())).toEqual([
    'Upload or add files Ctrl+U',
    expect.stringMatching(/^Take a screenshot/),
    'Skills',
    'Research',
    'Style Normal',
    'Voice replies',
    'Voice settings…',
  ]);
  expect(await rows.evaluateAll((els) => els.every((e) => e.querySelector(':scope > svg')))).toBe(true);
  // On the start screen it opens downwards, as the app's does.
  const boxRect = await rect(composer(main).locator('.inputbar'));
  expect((await rect(menu)).top).toBeGreaterThan(boxRect.top + boxRect.height / 2);
  await shot(main, '08-plus-menu-light');
  await theme(yap, main, 'dark');
  await shot(main, '09-plus-menu-dark');
  await theme(yap, main, 'light');

  // ↓ ↓ ↓ to Research, ↓ to Style, → opens it on its first style, ← back, Esc closes, + has the focus again.
  for (let i = 0; i < 4; i += 1) await main.keyboard.press('ArrowDown');
  await expect(main.getByTestId('chat-plus-style')).toBeFocused();
  await main.keyboard.press('ArrowRight');
  const styles = main.getByTestId('chat-plus-style-menu');
  await expect(styles.getByRole('menuitemradio', { name: 'Normal' })).toBeFocused();
  await expect(styles.getByRole('menuitemradio', { name: 'Normal' })).toHaveAttribute('aria-checked', 'true');
  await shot(main, '10-plus-style-submenu-light');
  await theme(yap, main, 'dark');
  await shot(main, '11-plus-style-submenu-dark');
  await theme(yap, main, 'light');
  await main.keyboard.press('ArrowDown');
  await expect(styles.getByRole('menuitemradio', { name: 'Concise' })).toBeFocused();
  await main.keyboard.press('ArrowLeft');
  await expect(styles).toHaveCount(0);
  await expect(main.getByTestId('chat-plus-style')).toBeFocused();
  await main.keyboard.press('Escape');
  await expect(menu).toHaveCount(0);
  await expect(plus).toBeFocused();
  // ↑ on + opens it at its last row.
  await main.keyboard.press('ArrowUp');
  await expect(rows.last()).toBeFocused();
  await main.keyboard.press('Escape');

  // A Style picked with the keyboard: Space on Concise; its chip in the box, × back to Normal.
  await plus.focus();
  await main.keyboard.press(' ');
  for (let i = 0; i < 4; i += 1) await main.keyboard.press('ArrowDown');
  await main.keyboard.press('ArrowRight');
  await main.keyboard.press('ArrowDown');
  await main.keyboard.press('Enter');
  await expect(menu).toHaveCount(0);
  const styleChip = main.getByTestId('chat-style-on');
  await expect(styleChip).toContainText('Concise');
  await shot(main, '12-style-chip-in-the-box-light');
  await theme(yap, main, 'dark');
  await shot(main, '13-style-chip-in-the-box-dark');
  await theme(yap, main, 'light');
  await styleChip.getByRole('button', { name: /back to Normal/ }).click();
  await expect(styleChip).toHaveCount(0);

  // Skills ›: the "/" menu's list; a pick goes in the box, its words to follow.
  await plus.click();
  await main.getByTestId('chat-plus-skills').click();
  const skillsMenu = main.getByTestId('chat-plus-skills-menu');
  await expect(skillsMenu.getByRole('menuitem', { name: /\/standup/ })).toBeVisible();
  await expect(skillsMenu.getByRole('menuitem', { name: 'More Skills…' })).toBeVisible();
  await skillsMenu.getByRole('menuitem', { name: /\/standup/ }).click();
  await expect(box(main)).toHaveValue('/standup ');
  await expect(box(main)).toBeFocused();
  await box(main).fill('');

  // Upload or add files: from the menu, and Ctrl+U in the chat.
  const before = await pickerOpens();
  await plus.click();
  await menu.getByRole('button', { name: 'Upload or add files' }).click();
  await expect.poll(pickerOpens).toBe(before + 1);
  await box(main).focus();
  await main.keyboard.press('Control+u');
  await expect.poll(pickerOpens).toBe(before + 2);
  await main.evaluate(() => delete window.__yapTestPickFiles);
  await setClock(main, null);
});

test('the first message sends the box down to its place: a gentle move, none with reduced motion', async ({ yap, main, fakeLlm, shot }) => {
  await main.setViewportSize({ width: 1440, height: 900 });
  await setClock(main, at(18, 14, 5));
  for (const motion of ['system', 'reduced']) {
    await setConfig(yap, main, { motion });
    await newChat(main);
    // (System: Windows' own Animation effects, which this PC may have off.)
    const still = await main.evaluate(() => document.documentElement.dataset.motion === 'reduced');
    if (motion === 'reduced') expect(still).toBe(true);
    const from = await rect(composer(main));
    // Every frame's top of the box, from just before the message goes.
    await main.evaluate(() => {
      const rec = { tops: [], on: true };
      window.__boxMove = rec;
      const step = () => {
        const el = document.querySelector('section.thread > .composer');
        if (el) rec.tops.push(Math.round(el.getBoundingClientRect().top));
        if (rec.on) requestAnimationFrame(step);
      };
      requestAnimationFrame(step);
    });
    fakeLlm.scriptChat([{ content: 'Leeks like a long, cool autumn.' }]);
    await box(main).fill('What grows well in November?');
    await box(main).press('Enter');
    await expect(main.locator('.cbubble.assistant').last()).toContainText('Leeks like a long, cool autumn.');
    await expect(start(main)).toHaveCount(0);
    await main.waitForTimeout(500);
    const tops = await main.evaluate(() => {
      window.__boxMove.on = false;
      return window.__boxMove.tops;
    });
    // At the foot now, on the same column.
    const to = await rect(composer(main));
    const thread = await rect(main.locator('section.thread'));
    expect(to.top).toBeGreaterThan(from.top + 100);
    expect(thread.bottom - to.bottom).toBeLessThan(40);
    expect(Math.abs(to.left - from.left)).toBeLessThanOrEqual(1);
    expect(Math.abs(to.width - from.width)).toBeLessThanOrEqual(1);
    // The box's words wait for the next message: "Reply…".
    await expect(box(main)).toHaveAttribute('placeholder', 'Reply…');
    // The way down: through the frames between (a move), or none at all.
    const between = tops.filter((t) => t > from.top + 4 && t < to.top - 4);
    if (!still) expect(between.length, `frames on the way down: ${tops.join(',')}`).toBeGreaterThanOrEqual(3);
    else expect(between.length, `no move with reduced motion: ${tops.join(',')}`).toBe(0);
    if (motion === 'system') {
      await shot(main, '06-after-first-message-light');
      await theme(yap, main, 'dark');
      await shot(main, '07-after-first-message-dark');
      await theme(yap, main, 'light');
    }
  }
  await setConfig(yap, main, { motion: 'system' });
  await setClock(main, null);
});
