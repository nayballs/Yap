// Settings → General → Appearance (AppearanceSettings.svelte,
// lib/appearance.js, the reading tokens in app.css): the reading font
// (Default = Figtree, Sans = Geist, Serif = Literata, System = Segoe UI,
// OpenDyslexic — all bundled, each named in itself in the picker), the text
// size, and Motion.
//
// - The font and size reach only the reading text: Chat's messages and
//   answers (tables too; code stays monospace), a note and its Enhanced
//   view, the meeting notepad's My thoughts / Transcript / Summary and its
//   Ask answers — never the sidebar or the buttons.
// - The notepad (its own window) follows a change live, through the
//   auto-save's `yap-config-saved`.
// - Reduced motion stops every non-essential transition and animation in the
//   main window, the notepad and the Yap bar (spinners and countdowns stay);
//   System follows `prefers-reduced-motion` (Windows' "Animation effects").
import { test as base, expect, openView, openSettings, closeSettings, sidebar, expectStore } from './support/fixtures.js';
import { startFakeLlm } from './support/fake-llm.js';

const NOW = Math.floor(Date.now() / 1000);

const ANSWER = [
  "Here's the plan for **Thursday's offsite**, with what's still open:",
  '',
  '## Who does what',
  '',
  '| Task | Owner | Due | Cost |',
  '|------|-------|-----|-----:|',
  '| Book the venue | Priya | Mon 3 Nov | £1,240 |',
  '| Catering | *nobody yet* | Wed 5 Nov | £680 |',
  '| Travel | Sam | Fri 7 Nov | £415 |',
  '',
  '- Send the revised budget to finance by Friday.',
  '- Ask legal to review the press release: it needs a week.',
  '',
  'The script that adds it up is `budget.py`:',
  '',
  '```python',
  'total = sum(item.cost for item in plan)',
  'print(f"Total: £{total:,}")',
  '```',
  '',
  '> Catering is the one thing nobody owns yet.',
].join('\n');

const CHATS = {
  conversations: [
    {
      id: 1,
      title: 'Offsite plan',
      createdTs: NOW - 600,
      updatedTs: NOW - 600,
      messages: [
        { role: 'user', text: 'Can you turn the offsite notes into a plan with owners and costs?', ts: NOW - 600 },
        { role: 'assistant', text: ANSWER, ts: NOW - 590 },
        { role: 'user', text: 'What did Priya say about the budget?', ts: NOW - 500 },
        {
          role: 'assistant',
          text:
            'Priya said the budget is fine overall: travel is up 12%, mostly flights for the offsite, and the venue deposit ' +
            'landed this quarter instead of next, which makes the totals look worse than they are. She will send the ' +
            'revised budget to finance by Friday and flag the deposit so nobody panics.',
          ts: NOW - 490,
        },
      ],
    },
  ],
};

const NOTE = {
  id: 701,
  title: 'Offsite plan',
  noteType: 'meeting',
  folder: 'Meetings',
  source: 'meeting',
  participants: ['Priya'],
  createdTs: NOW - 3600,
  updatedTs: NOW - 3600,
  content: 'Ask Priya about the venue deposit\nCatering: who owns it?\nLegal review of the press release takes a week',
  enhancedContent: [
    '## Action plan',
    '',
    '### Priya',
    '- [ ] Send the revised budget to finance (due: Friday)',
    '- [ ] Book the venue',
    '',
    '### Unassigned',
    '- [ ] Catering for the offsite',
    '',
    '## Decisions',
    '',
    '| Item | Cost |',
    '|------|-----:|',
    '| Venue | £1,240 |',
    '| Travel | £415 |',
  ].join('\n'),
  transcript: [
    { source: 'you', text: 'Thanks for joining. Can we go through the budget, starting with travel?', ts: NOW - 3590 },
    { source: 'them', text: 'Travel is up twelve percent, mostly flights. I will send the revised budget by Friday.', ts: NOW - 3580 },
    { source: 'you', text: 'Perfect. Then catering is the only open item.', ts: NOW - 3570 },
  ],
};

const FONTS = {
  default: 'Figtree Variable',
  sans: 'Geist Variable',
  serif: 'Literata Variable',
  system: 'Segoe UI Variable Text',
  dyslexic: 'OpenDyslexic',
};
const SIZE_LABEL = { small: 'Small', default: 'Default', large: 'Large', larger: 'Larger' };

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
        name: 'appearance',
        config: {
          // The notepad's Ask anything answers from a local fake AI.
          llmScopes: {
            noteFormatting: { enabled: true, provider: 'custom', baseUrl: fakeLlm.base, model: 'fake-notes' },
          },
          meetingReminder: 'never',
        },
        data: {
          'chats.json': CHATS,
          'notes.json': { folders: ['Personal', 'Meetings'], notes: [NOTE] },
        },
      });
    },
    { scope: 'worker' },
  ],
});

const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const css = (loc, prop) => loc.evaluate((el, p) => getComputedStyle(el)[p], prop);
/** The first family of an element's computed font-family. */
const family = async (loc) => (await css(loc, 'fontFamily')).split(',')[0].replace(/["']/g, '').trim();

/** Elements (outside the kept spinners and countdowns) that still move. */
function moving(page) {
  return page.evaluate(() => {
    const keep = '.spin, .spinner, .progress, .drain, .run-down, .undoline > span, [data-keep-motion]';
    const secs = (v) => v.split(',').some((d) => parseFloat(d) > 0);
    return [...document.querySelectorAll('body *')].filter((el) => {
      if (el.matches(keep)) return false;
      const s = getComputedStyle(el);
      return secs(s.transitionDuration) || s.animationName !== 'none';
    }).length;
  });
}

/** Settings → General → Appearance, picking what's given; waits for the save. */
async function pick(main, yap, { font, size, motion }) {
  const dialog = await openSettings(main, 'General');
  const group = dialog.getByRole('group', { name: 'Appearance' });
  if (font) await group.getByRole('combobox', { name: 'Reading font' }).selectOption(font);
  if (size) await group.getByRole('tablist', { name: 'Text size' }).getByRole('tab', { name: SIZE_LABEL[size], exact: true }).click();
  if (motion) await group.getByRole('tablist', { name: 'Motion' }).getByRole('tab', { name: motion === 'reduced' ? 'Reduced' : 'System', exact: true }).click();
  await expectStore(
    yap,
    'config.json',
    (c) =>
      (!font || c.readingFont === font) && (!size || c.readingSize === size) && (!motion || c.motion === motion),
    'the appearance to be saved'
  );
  return group;
}

async function openChat(main) {
  await openView(main, 'Chat');
  await main.locator('.conv').filter({ hasText: 'Offsite plan' }).click();
  await expect(main.locator('.cbubble.assistant')).toHaveCount(2);
}

async function openNote(main) {
  await openView(main, 'Notes');
  await main.locator('.folder').filter({ hasText: 'Meetings' }).click();
  await main.locator('.item').filter({ hasText: 'Offsite plan' }).click();
  await expect(main.locator('textarea.raw')).toHaveValue(/venue deposit/);
}

async function closeNotepad(yap) {
  if (await visible(yap, 'notepad')) {
    await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
    await expect.poll(() => visible(yap, 'notepad')).toBe(false);
  }
}

test.afterEach(async ({ yap, main }) => {
  await closeNotepad(yap);
  await main.emulateMedia({ reducedMotion: null }).catch(() => {});
});

test('the reading font: every choice reaches chats and notes, never the sidebar; each is named in itself', async ({
  yap,
  main,
  shot,
}) => {
  const group = await pick(main, yap, { font: 'default', size: 'default', motion: 'system' });
  await expect(group).toBeVisible();
  // Each option drawn in its own font, and the box in the one picked.
  const picker = group.getByRole('combobox', { name: 'Reading font' });
  await expect(picker.locator('option')).toHaveText(['Default', 'Sans', 'Serif', 'System', 'OpenDyslexic']);
  const optionFonts = await picker
    .locator('option')
    .evaluateAll((os) => os.map((o) => o.style.fontFamily.split(',')[0].replace(/["']/g, '').trim()));
  expect(optionFonts).toEqual(Object.values(FONTS));
  await expect(group.getByText('Figtree, the same as the rest of Yap')).toBeVisible();
  await group.scrollIntoViewIfNeeded();
  await shot(main, '01-settings-appearance');

  const uiFont = await family(sidebar(main));
  expect(uiFont).toBe('Figtree Variable');

  let n = 2;
  for (const [id, name] of Object.entries(FONTS)) {
    const g = await pick(main, yap, { font: id });
    expect(await family(g.getByRole('combobox', { name: 'Reading font' }))).toBe(name);
    // The preview under the picker reads in it straight away.
    expect(await family(g.locator('.sample p'))).toBe(name);
    await closeSettings(main);

    await openChat(main);
    const answer = main.locator('.cbubble.assistant').first();
    const question = main.locator('.cbubble.user').first();
    expect(await family(answer)).toBe(name);
    expect(await family(question)).toBe(name);
    // A table in an answer reads in it too; code stays monospace.
    expect(await family(answer.locator('table'))).toBe(name);
    expect(await family(answer.locator('pre code'))).toBe('ui-monospace');
    expect(await family(answer.locator('code').first())).toBe('ui-monospace');
    // …and never the app's own chrome.
    expect(await family(sidebar(main))).toBe(uiFont);
    expect(await family(main.getByRole('button', { name: 'New chat' }).first())).toBe(uiFont);
    // The bundled font itself loaded (no stand-in), from inside the app.
    if (id !== 'system') {
      await expect
        .poll(() => main.evaluate((f) => [...document.fonts].some((ff) => ff.family.replace(/["']/g, '') === f && ff.status === 'loaded'), name))
        .toBe(true);
    }
    await main.evaluate(() => document.fonts.ready);
    await shot(main, `0${n}-chat-${id}`);

    await openNote(main);
    expect(await family(main.locator('textarea.raw'))).toBe(name);
    expect(await family(sidebar(main))).toBe(uiFont);
    await main.locator('.tabs .tab').filter({ hasText: 'Enhanced' }).click();
    const rendered = main.locator('.editor .rendered');
    await expect(rendered).toContainText('Send the revised budget');
    expect(await family(rendered)).toBe(name);
    // The transcript's lines too, but not their speaker labels.
    expect(await family(main.locator('.transcript .bubble p').first())).toBe(name);
    expect(await family(main.locator('.transcript .bubble .who').first())).toBe(uiFont);
    if (id === 'serif' || id === 'dyslexic') {
      await main.evaluate(() => document.fonts.ready);
      await shot(main, `0${n}b-note-${id}`);
    }
    n += 1;
  }
  await pick(main, yap, { font: 'default' });
});

test('text size: a few steps around each view’s own size, for the reading text only', async ({ yap, main, shot }) => {
  await pick(main, yap, { font: 'default', size: 'default', motion: 'system' });
  await closeSettings(main);
  await openChat(main);
  const answer = main.locator('.cbubble.assistant').first();
  const navSize = await css(sidebar(main).getByRole('button', { name: 'Chat', exact: true }), 'fontSize');
  expect(await css(answer, 'fontSize')).toBe('12.5px');

  for (const [size, px] of [
    ['small', '11.5px'],
    ['large', '14px'],
    ['larger', '15.625px'],
  ]) {
    await pick(main, yap, { size });
    await closeSettings(main);
    expect(await css(answer, 'fontSize')).toBe(px);
    // The line spacing grows with it (unitless × the size).
    expect(parseFloat(await css(answer, 'lineHeight'))).toBeCloseTo(parseFloat(px) * 1.6, 1);
    expect(await css(sidebar(main).getByRole('button', { name: 'Chat', exact: true }), 'fontSize')).toBe(navSize);
  }
  await shot(main, '10-chat-larger');

  // A note: 13 px by default, ×1.25.
  await openNote(main);
  expect(await css(main.locator('textarea.raw'), 'fontSize')).toBe('16.25px');

  // OpenDyslexic's wide letters come a little smaller (×0.9), with more room between lines.
  await pick(main, yap, { font: 'dyslexic', size: 'default' });
  await closeSettings(main);
  await openChat(main);
  expect(await css(answer, 'fontSize')).toBe('11.25px');
  expect(parseFloat(await css(answer, 'lineHeight'))).toBeCloseTo(11.25 * 1.6 * 1.08, 1);

  await pick(main, yap, { font: 'default', size: 'default' });
});

test('the meeting notepad follows the font and size live: My thoughts, Transcript, Summary, Ask', async ({
  yap,
  main,
  fakeLlm,
  shot,
}) => {
  await pick(main, yap, { font: 'default', size: 'default', motion: 'system' });
  await closeSettings(main);
  await yap.invoke('notepad_open', { noteId: NOTE.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  const thoughts = pad.getByRole('textbox', { name: 'My thoughts' });
  await expect(thoughts).toHaveValue(/venue deposit/);
  expect(await family(thoughts)).toBe('Figtree Variable');
  expect(await css(thoughts, 'fontSize')).toBe('15px');

  // Picked in the main window's Settings → the notepad changes by itself.
  await pick(main, yap, { font: 'serif', size: 'large' });
  await expect.poll(() => family(thoughts), { message: 'the notepad to follow the font' }).toBe('Literata Variable');
  await expect.poll(() => css(thoughts, 'fontSize')).toBe('16.8px');
  // Its chrome keeps Yap's font.
  expect(await family(pad.getByRole('tab', { name: 'Transcript' }))).toBe('Figtree Variable');

  await pad.getByRole('tab', { name: 'Transcript' }).click();
  const line = pad.locator('.bubble').first();
  await expect(line).toContainText('budget');
  expect(await family(line)).toBe('Literata Variable');

  await pad.getByRole('tab', { name: 'Summary' }).click();
  const summary = pad.locator('.rendered');
  await expect(summary).toContainText('Send the revised budget');
  expect(await family(summary)).toBe('Literata Variable');
  expect(await css(summary, 'fontSize')).toBe('16.8px');
  await pad.evaluate(() => document.fonts.ready);
  await shot(pad, '20-notepad-summary-serif-large');

  // Ask anything: the question and the answer read in it too.
  const input = pad.getByRole('textbox', { name: 'Ask anything' });
  await input.fill('Who owns the budget?');
  await input.press('Enter');
  const sheet = pad.getByRole('dialog', { name: 'Ask about this meeting' });
  await expect(sheet).toContainText('Alice owns the budget');
  expect(fakeLlm.requests.some((r) => r.kind === 'meetingAsk')).toBe(true);
  expect(await family(sheet.locator('.mq').last())).toBe('Literata Variable');
  expect(await family(sheet.locator('.mabody').last())).toBe('Literata Variable');
  expect(await family(sheet.getByRole('button', { name: 'Copy answer' }))).toBe('Figtree Variable');
  await shot(pad, '21-notepad-ask-serif');

  // OpenDyslexic, live too.
  await pick(main, yap, { font: 'dyslexic', size: 'default' });
  await expect.poll(() => family(sheet.locator('.mabody').last())).toBe('OpenDyslexic');
  await pad.keyboard.press('Escape');
  await pad.getByRole('tab', { name: 'My thoughts' }).click();
  await expect.poll(() => family(thoughts)).toBe('OpenDyslexic');
  await pad.mouse.move(380, 700); // off the tab, so its tooltip isn't over the text
  await pad.evaluate(() => document.fonts.ready);
  await shot(pad, '22-notepad-thoughts-dyslexic');

  await pick(main, yap, { font: 'default', size: 'default' });
  await expect.poll(() => family(thoughts)).toBe('Figtree Variable');
});

test('Motion: Reduced stops transitions and animations in every window; System follows Windows', async ({
  yap,
  main,
  shot,
}) => {
  const group = await pick(main, yap, { font: 'default', size: 'default', motion: 'system' });
  await yap.invoke('notepad_open', { noteId: NOTE.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  const thumb = group.locator('.seg .thumb').first();
  const pill = yap.overlay.locator('.pill');

  // Full motion: things glide.
  expect(await css(thumb, 'transitionProperty')).toBe('transform');
  expect(await moving(main)).toBeGreaterThan(0);
  expect(await moving(pad)).toBeGreaterThan(0);
  if (await pill.count()) expect(await css(pill, 'transitionDuration')).not.toBe('0s');

  await pick(main, yap, { motion: 'reduced' });
  await expect(group.getByText('No sliding, fading or pulsing in any Yap window')).toBeVisible();
  expect(await main.evaluate(() => document.documentElement.dataset.motion)).toBe('reduced');
  expect(await css(thumb, 'transitionProperty')).toBe('none');
  expect(await css(thumb, 'transitionDuration')).toBe('0s');
  expect(await moving(main)).toBe(0);
  // A spinner and a countdown still turn and drain; anything else stops.
  const kept = await main.evaluate(() => {
    const spin = document.createElement('span');
    spin.className = 'spin';
    spin.style.animation = 'yap-test-spin 1s linear infinite';
    const other = document.createElement('span');
    other.style.animation = 'yap-test-spin 1s linear infinite';
    document.body.append(spin, other);
    const r = [getComputedStyle(spin).animationName, getComputedStyle(other).animationName];
    spin.remove();
    other.remove();
    return r;
  });
  expect(kept).toEqual(['yap-test-spin', 'none']);
  await shot(main, '30-settings-motion-reduced');

  // The notepad and the Yap bar follow (their own windows).
  await expect.poll(() => pad.evaluate(() => document.documentElement.dataset.motion)).toBe('reduced');
  expect(await moving(pad)).toBe(0);
  await expect.poll(() => yap.overlay.evaluate(() => document.documentElement.dataset.motion)).toBe('reduced');
  expect(await moving(yap.overlay)).toBe(0);
  if (await pill.count()) expect(await css(pill, 'transitionProperty')).toBe('none');

  // System: motion again, unless Windows' "Animation effects" is off.
  await pick(main, yap, { motion: 'system' });
  expect(await main.evaluate(() => document.documentElement.dataset.motion)).toBeUndefined();
  expect(await css(thumb, 'transitionProperty')).toBe('transform');
  await expect.poll(() => pad.evaluate(() => document.documentElement.dataset.motion ?? null)).toBe(null);
  await expect(group.getByText('Follows Windows’ “Animation effects”, which is on')).toBeVisible();

  await main.emulateMedia({ reducedMotion: 'reduce' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.motion ?? null)).toBe('reduced');
  expect(await css(thumb, 'transitionProperty')).toBe('none');
  await expect(group.getByText('Follows Windows’ “Animation effects”, which is off: motion is reduced')).toBeVisible();
  await main.emulateMedia({ reducedMotion: 'no-preference' });
  await expect.poll(() => main.evaluate(() => document.documentElement.dataset.motion ?? null)).toBe(null);
});
