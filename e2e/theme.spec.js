// Settings → General → Appearance → Theme (System / Light / Dark;
// lib/core/appearance.js, app.css's dark block, index.html's head script,
// shell/theme.rs) and the Yap bar's "Bar colours" (Match app / Light / Dark).
//
// - Switching applies live to the main window, the meeting notepad and the
//   Yap bar (Match app), with Light / Dark as the bar's overrides.
// - System follows `prefers-color-scheme` (Windows' app mode), emulated here
//   over CDP, live.
// - No light flash: the hidden windows' webviews already have the theme's
//   background (what WebView2 shows before a page paints), and a page's
//   first frame is dark (the head script, before any of the app loads).
// - Contrast in dark: body text, descriptions, nav labels, links, switch
//   tracks, focus rings, keycaps and Insights' charts, measured in the app.
// - Screenshots of every view, every Settings page, the notepad,
//   onboarding, a toast, the dialogs and the bar's cards in dark.
import {
  test as base,
  expect,
  openView,
  openSettings,
  closeSettings,
  settingsDialog,
  sidebar,
  expectStore,
  closeToasts,
  openModelPicker,
} from './support/fixtures.js';

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86_400;

const HISTORY = [
  { ts: NOW - 120, raw: 'can we move the design review to thursday', text: 'Can we move the design review to Thursday?', model: 'parakeet-tdt-0.6b-v3', app: 'ms-teams.exe' },
  { ts: NOW - 900, raw: 'send the revised budget to finance by friday', text: 'Send the revised budget to finance by Friday.', model: 'parakeet-tdt-0.6b-v3', app: 'OUTLOOK.EXE' },
  { ts: NOW - 3600, raw: 'fix the login bug before the release', text: 'Fix the login bug before the release.', model: 'parakeet-tdt-0.6b-v3', app: 'Code.exe' },
  { ts: NOW - DAY - 200, raw: 'the offsite moves to the fourteenth', text: 'The offsite moves to the 14th.', model: 'parakeet-tdt-0.6b-v3', app: 'slack.exe' },
  { ts: NOW - 2 * DAY, raw: 'thanks everyone see you tomorrow', text: 'Thanks, everyone. See you tomorrow.', model: 'parakeet-tdt-0.6b-v3', app: 'ms-teams.exe' },
  { ts: NOW - 4 * DAY, raw: 'draft a reply to sam about the venue', text: 'Draft a reply to Sam about the venue.', model: 'parakeet-tdt-0.6b-v3', app: 'msedge.exe' },
];

const CHATS = {
  conversations: [
    {
      id: 1,
      title: 'Offsite plan',
      createdTs: NOW - 600,
      updatedTs: NOW - 600,
      messages: [
        { role: 'user', text: 'Can you turn the offsite notes into a plan with owners?', ts: NOW - 600 },
        {
          role: 'assistant',
          text: [
            "Here's the plan for **Thursday's offsite**:",
            '',
            '| Task | Owner | Due |',
            '|------|-------|-----|',
            '| Book the venue | Priya | Mon 3 Nov |',
            '| Catering | *nobody yet* | Wed 5 Nov |',
            '',
            '- Send the revised budget to finance by Friday.',
            '- Ask legal to review the press release.',
            '',
            'The total is in `budget.py`. More on [the venue](https://example.com/venue).',
            '',
            '> Catering is the one thing nobody owns yet.',
          ].join('\n'),
          ts: NOW - 590,
        },
      ],
    },
  ],
};

const MEETING = {
  id: 801,
  title: 'Design review',
  noteType: 'meeting',
  folder: 'Meetings',
  source: 'meeting',
  participants: ['Priya', 'Sam'],
  createdTs: NOW - 3 * 3600,
  updatedTs: NOW - 3 * 3600,
  content: 'Ask Priya about the venue deposit\nCatering: who owns it?',
  enhancedContent: [
    '## Action plan',
    '',
    '### Priya',
    '- [ ] Send the revised budget to finance (due: Friday)',
    '',
    '### Unassigned',
    '- [ ] Catering for the offsite',
    '',
    '## Decisions',
    '- The offsite moves to the 14th.',
  ].join('\n'),
  transcript: [
    { source: 'you', text: 'Thanks for joining. Can we go through the budget first?', ts: NOW - 3 * 3600 + 10 },
    { source: 'them', text: 'Travel is up twelve percent, mostly flights. I will send the revised budget by Friday.', ts: NOW - 3 * 3600 + 20 },
    { source: 'you', text: 'Perfect. Then catering is the only open item.', ts: NOW - 3 * 3600 + 30 },
  ],
};
const PERSONAL = {
  id: 802,
  title: 'Launch checklist',
  noteType: 'personal',
  folder: 'Personal',
  source: 'manual',
  createdTs: NOW - DAY,
  updatedTs: NOW - DAY,
  content: '- Fix the login bug\n- Write the release notes\n- Tell support about the new pricing',
};

const test = base.extend({
  yapOptions: [
    {
      name: 'theme',
      config: {
        theme: 'light',
        meetingReminder: 'never',
        dictionary: [
          { from: 'jaison', to: 'JSON', fuzzy: true, snippet: false },
          { from: 'cube and eighties', to: 'Kubernetes', fuzzy: true, snippet: false },
          { from: 'my address', to: '1 Example Street\nLondon', fuzzy: false, snippet: true },
        ],
      },
      data: {
        'history.json': HISTORY,
        'chats.json': CHATS,
        'notes.json': { folders: ['Personal', 'Meetings'], notes: [MEETING, PERSONAL] },
      },
    },
    { scope: 'worker' },
  ],
});

const visible = (yap, label) => yap.invoke('plugin:window|is_visible', { label });
const themeOf = (page) => page.evaluate(() => document.documentElement.dataset.yapTheme);
const barThemeOf = (page) => page.evaluate(() => document.documentElement.dataset.barTheme);
const bgOf = (page, sel = 'html') => page.evaluate((s) => getComputedStyle(document.querySelector(s)).backgroundColor, sel);
const token = (page, name) =>
  page.evaluate((n) => getComputedStyle(document.documentElement).getPropertyValue(n).trim(), name);
/** Open a Settings page by id, as the app's own links do. */
const goto = (main, target) =>
  main.evaluate((t) => window.dispatchEvent(new CustomEvent('yap-settings-goto', { detail: t })), target);

/** Settings → General → Appearance → Theme; waits for the save. */
async function pickTheme(main, yap, label) {
  const dialog = await openSettings(main, 'General');
  const group = dialog.getByRole('group', { name: 'Appearance' });
  await group.getByRole('tablist', { name: 'Theme' }).getByRole('tab', { name: label, exact: true }).click();
  await expectStore(yap, 'config.json', (c) => c.theme === label.toLowerCase(), `the theme ${label} to be saved`);
  return group;
}

/** Settings → General → Yap bar → Colours; waits for the save. */
async function pickBarColours(main, yap, label, value) {
  const dialog = await openSettings(main, 'General');
  const group = dialog.getByRole('group', { name: 'Yap bar' });
  await group.getByRole('tablist', { name: 'Bar colours' }).getByRole('tab', { name: label, exact: true }).click();
  await expectStore(yap, 'config.json', (c) => c.barTheme === value, `the bar colours ${value} to be saved`);
}

/** A taller viewport, so a whole Settings page or view fits one screenshot. */
async function tall(page, height, fn) {
  const cdp = await page.context().newCDPSession(page);
  try {
    await cdp.send('Emulation.setDeviceMetricsOverride', { width: 1200, height, deviceScaleFactor: 1, mobile: false });
    await fn();
  } finally {
    await cdp.send('Emulation.clearDeviceMetricsOverride').catch(() => {});
    await cdp.detach().catch(() => {});
  }
}

async function closeNotepad(yap) {
  if (await visible(yap, 'notepad')) {
    await yap.notepad.getByRole('button', { name: 'Close notepad' }).click();
    await expect.poll(() => visible(yap, 'notepad')).toBe(false);
  }
}

/**
 * WCAG contrast ratios of every visible element matching `selector`: its
 * `prop` colour (text by default; a border or background for non-text)
 * against the first opaque background behind it (its own, for `prop`
 * 'backgroundColor', counts from the parent).
 */
function contrasts(page, selector, prop = 'color') {
  return page.evaluate(
    ([sel, p]) => {
      const rgb = (c) => {
        const m = c.match(/[\d.]+/g)?.map(Number) ?? [0, 0, 0, 1];
        // color-mix() computes to `color(srgb r g b / a)`, channels 0–1.
        const k = c.startsWith('color(srgb') ? 255 : 1;
        return { r: m[0] * k, g: m[1] * k, b: m[2] * k, a: m.length > 3 ? m[3] : 1 };
      };
      const lum = ({ r, g, b }) =>
        [r, g, b]
          .map((v) => v / 255)
          .map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
          .reduce((acc, v, i) => acc + v * [0.2126, 0.7152, 0.0722][i], 0);
      const over = (fg, bg) => ({
        r: fg.r * fg.a + bg.r * (1 - fg.a),
        g: fg.g * fg.a + bg.g * (1 - fg.a),
        b: fg.b * fg.a + bg.b * (1 - fg.a),
        a: 1,
      });
      const bgOf = (el) => {
        for (let e = el; e; e = e.parentElement) {
          const c = rgb(getComputedStyle(e).backgroundColor);
          if (c.a > 0.99) return c;
        }
        return { r: 255, g: 255, b: 255, a: 1 };
      };
      const ratio = (a, b) => {
        const [x, y] = [lum(a), lum(b)].sort((m, n) => n - m);
        return (x + 0.05) / (y + 0.05);
      };
      const els = [...document.querySelectorAll(sel)].filter((e) => e.getClientRects().length);
      return els.map((el) => {
        const bg = bgOf(p === 'color' ? el : el.parentElement ?? el);
        const fg = over(rgb(getComputedStyle(el)[p]), bg);
        return Math.round(ratio(fg, bg) * 100) / 100;
      });
    },
    [selector, prop]
  );
}
/** The lowest contrast among `selector`'s elements (there must be some). */
async function minContrast(page, selector, prop = 'color') {
  const values = await contrasts(page, selector, prop);
  expect(values.length, `${selector}: none on the page`).toBeGreaterThan(0);
  return Math.min(...values);
}
/** A selector (or a list of them) inside the Settings modal. */
const inSettings = (sel) =>
  sel
    .split(',')
    .map((s) => `[role="dialog"][aria-label="Settings"] ${s.trim()}`)
    .join(', ');

test.afterEach(async ({ yap, main }) => {
  await closeNotepad(yap);
  await main.emulateMedia({ colorScheme: null }).catch(() => {});
  await yap.notepad.emulateMedia({ colorScheme: null }).catch(() => {});
  await yap.overlay.emulateMedia({ colorScheme: null }).catch(() => {});
});

// ---------------------------------------------------------------- the theme

test('Theme: switching applies live to the main window, the notepad and the bar; the bar can keep its own colours', async ({
  yap,
  main,
  shot,
}) => {
  const pad = yap.notepad;
  const bar = yap.overlay;
  // Light to start with (the suite's seed).
  expect(await themeOf(main)).toBe('light');
  expect(await bgOf(main)).toBe('rgb(240, 237, 231)');
  expect(await themeOf(pad)).toBe('light');
  expect(await barThemeOf(bar)).toBe('light');
  expect(await yap.invoke('e2e_webview_background', { label: 'settings' })).toBe('#f0ede7');

  // The notepad on screen beside the main window, to watch it change.
  await yap.invoke('notepad_open', { noteId: MEETING.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);

  const group = await pickTheme(main, yap, 'Dark');
  await expect(group.getByRole('tablist', { name: 'Theme' }).getByRole('tab', { name: 'Dark' })).toHaveAttribute('aria-selected', 'true');
  // The main window: at once, frame and sheet, native controls dark too.
  expect(await themeOf(main)).toBe('dark');
  expect(await bgOf(main)).toBe('rgb(22, 20, 16)');
  expect(await main.evaluate(() => getComputedStyle(document.documentElement).colorScheme)).toBe('dark');
  expect(await token(main, '--yap-s1')).toBe('#1c1a16');
  // The custom title bar's buttons follow.
  expect(await main.locator('.winbtn').first().evaluate((el) => getComputedStyle(el).color)).toBe('rgb(242, 239, 232)');
  // The notepad (another window) after the save, by itself.
  await expect.poll(() => themeOf(pad), { message: 'the notepad to follow' }).toBe('dark');
  expect(await bgOf(pad)).toBe('rgb(28, 26, 22)');
  expect(await bgOf(pad, '.pad')).toBe('rgb(28, 26, 22)');
  // The bar: Match app, so dark too — and still a transparent window with
  // no colour scheme of its own (the grey-box bug).
  await expect.poll(() => barThemeOf(bar), { message: 'the bar to match the app' }).toBe('dark');
  expect(await token(bar, '--bar-surface')).toBe('#1c1a16');
  expect(await bgOf(bar)).toBe('rgba(0, 0, 0, 0)');
  expect(await bar.evaluate(() => getComputedStyle(document.documentElement).colorScheme)).toBe('normal');
  // Rust gave the webviews the same background.
  await expect.poll(() => yap.invoke('e2e_webview_background', { label: 'settings' })).toBe('#161410');
  expect(await yap.invoke('e2e_webview_background', { label: 'notepad' })).toBe('#1c1a16');
  expect(await yap.invoke('e2e_webview_background', { label: 'onboarding' })).toBe('#161410');
  await group.scrollIntoViewIfNeeded();
  await shot(main, 'theme-dark-settings');
  await shot(pad, 'theme-dark-notepad');

  // The bar's own colours: Light while Yap is dark, then back to Match app.
  await pickBarColours(main, yap, 'Light', 'light');
  await expect.poll(() => barThemeOf(bar)).toBe('light');
  expect(await themeOf(main)).toBe('dark');
  await pickBarColours(main, yap, 'Match app', 'app');
  await expect.poll(() => barThemeOf(bar)).toBe('dark');

  // Light again: everything follows back.
  await pickTheme(main, yap, 'Light');
  expect(await themeOf(main)).toBe('light');
  await expect.poll(() => themeOf(pad)).toBe('light');
  await expect.poll(() => barThemeOf(bar)).toBe('light');
  await expect.poll(() => yap.invoke('e2e_webview_background', { label: 'notepad' })).toBe('#faf9f6');
  await closeSettings(main);
});

test('System follows Windows’ app mode (prefers-color-scheme), live, in every window', async ({ yap, main }) => {
  const pad = yap.notepad;
  const bar = yap.overlay;
  await yap.invoke('notepad_open', { noteId: MEETING.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  await main.emulateMedia({ colorScheme: 'light' });
  await pad.emulateMedia({ colorScheme: 'light' });
  await bar.emulateMedia({ colorScheme: 'light' });

  const group = await pickTheme(main, yap, 'System');
  const row = group.locator('[data-setting="theme"]');
  await expect(row).toContainText('which is light right now');
  expect(await themeOf(main)).toBe('light');

  // Windows switches to dark: every window follows without a save.
  await main.emulateMedia({ colorScheme: 'dark' });
  await pad.emulateMedia({ colorScheme: 'dark' });
  await bar.emulateMedia({ colorScheme: 'dark' });
  await expect.poll(() => themeOf(main)).toBe('dark');
  await expect(row).toContainText('which is dark right now');
  await expect.poll(() => themeOf(pad)).toBe('dark');
  await expect.poll(() => barThemeOf(bar)).toBe('dark');
  expect(await bgOf(pad)).toBe('rgb(28, 26, 22)');

  // …and back.
  await main.emulateMedia({ colorScheme: 'light' });
  await pad.emulateMedia({ colorScheme: 'light' });
  await bar.emulateMedia({ colorScheme: 'light' });
  await expect.poll(() => themeOf(main)).toBe('light');
  await expect.poll(() => themeOf(pad)).toBe('light');
  await expect.poll(() => barThemeOf(bar)).toBe('light');

  // An explicit choice wins over Windows.
  await main.emulateMedia({ colorScheme: 'dark' });
  await pickTheme(main, yap, 'Light');
  expect(await themeOf(main)).toBe('light');
  await closeSettings(main);
});

test('no light flash: a hidden window is dark before its page paints, and before any of the app loads', async ({ yap, main }) => {
  await pickTheme(main, yap, 'Dark');
  await closeSettings(main);
  // The hidden windows' webviews: what WebView2 shows in the moment before
  // a page paints (a window shown after a while hidden, a resize).
  expect(await visible(yap, 'notepad')).toBe(false);
  expect(await visible(yap, 'onboarding')).toBe(false);
  await expect.poll(() => yap.invoke('e2e_webview_background', { label: 'notepad' })).toBe('#1c1a16');
  expect(await yap.invoke('e2e_webview_background', { label: 'onboarding' })).toBe('#161410');

  // A page's first frame: reload the hidden windows with the app's scripts
  // blocked, so only index.html's head script and the stylesheet apply —
  // what the first paint shows before any of the app runs.
  for (const [page, label, bg] of [
    [yap.notepad, 'notepad', 'rgb(28, 26, 22)'],
    [yap.onboarding, 'onboarding', 'rgb(22, 20, 16)'],
  ]) {
    const cdp = await page.context().newCDPSession(page);
    try {
      await cdp.send('Network.enable');
      await cdp.send('Network.setBlockedURLs', { urls: ['*/assets/*.js'] });
      await page.reload({ waitUntil: 'load' });
      expect(await page.evaluate(() => !!document.querySelector('#app')?.childElementCount), `${label}: the app didn't run`).toBe(false);
      expect(await page.evaluate(() => document.documentElement.dataset.window)).toBe(label);
      expect(await themeOf(page)).toBe('dark');
      expect(await bgOf(page), `${label}: first paint`).toBe(bg);
      expect(await bgOf(page, 'body'), `${label}: first paint`).toBe(bg);
      expect(await page.evaluate(() => getComputedStyle(document.documentElement).colorScheme)).toBe('dark');
    } finally {
      await cdp.send('Network.setBlockedURLs', { urls: [] }).catch(() => {});
      await cdp.detach().catch(() => {});
    }
    await page.reload({ waitUntil: 'load' });
    await expect.poll(() => page.evaluate(() => document.querySelector('#app')?.childElementCount ?? 0)).toBeGreaterThan(0);
  }

  // Opening the notepad: on screen, dark from its first frame.
  await yap.invoke('notepad_open', { noteId: MEETING.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  expect(await bgOf(yap.notepad)).toBe('rgb(28, 26, 22)');

  await pickTheme(main, yap, 'Light');
  await closeSettings(main);
});

test('contrast in dark: text, descriptions, nav labels, links, switches, focus rings, keycaps and charts pass AA', async ({
  yap,
  main,
}) => {
  await pickTheme(main, yap, 'Dark');
  // Every ratio measured, for the report (and the docs' numbers).
  const measured = {};
  const check = async (name, page, selector, atLeast, prop = 'color') => {
    measured[name] = await minContrast(page, selector, prop);
    expect(measured[name], `${name} (${selector})`).toBeGreaterThanOrEqual(atLeast);
  };
  // Settings → General, as settings-search.spec.js measures it in light.
  await check('settings descriptions', main, inSettings('.row .desc, .toggle-row .desc'), 4.5);
  await check('settings labels', main, inSettings('.row .label, .toggle-row .label'), 7);
  await check('page lines', main, inSettings('.page-h p'), 4.5);
  await check('group headings', main, inSettings('.group h2'), 4.5);
  await check('nav group labels', main, inSettings('.navcap'), 4.5);
  await check('nav labels', main, inSettings('.navlabel'), 4.5);
  await check('links', main, inSettings('.set-link'), 4.5);
  await check('version line', main, inSettings('.navfoot .ver'), 4.5);
  await check('close button', main, inSettings('.modal-x'), 4.5);
  // Switches: an off track's edge against the page, an on track on it and
  // its knob on the track (non-text, 3:1).
  await check('switch off edge', main, inSettings('.switch:not(.on)'), 3, 'borderTopColor');
  await check('switch on track', main, inSettings('.switch.on'), 3, 'backgroundColor');
  await check('switch on knob', main, inSettings('.switch.on .knob'), 3, 'backgroundColor');
  // The focus ring: Yap's brighter amber on dark, 3:1 and up against the page.
  const cdp = await main.context().newCDPSession(main);
  try {
    await cdp.send('DOM.enable');
    await cdp.send('CSS.enable');
    const { root } = await cdp.send('DOM.getDocument', { depth: -1 });
    for (const selector of ['[data-setting="autostart"] .switch-btn', '[data-setting="theme"] .seg-btn', 'nav[aria-label="Settings sections"] .navitem:not(.active)']) {
      const { nodeId } = await cdp.send('DOM.querySelector', { nodeId: root.nodeId, selector });
      expect(nodeId, selector).toBeGreaterThan(0);
      await cdp.send('CSS.forcePseudoState', { nodeId, forcedPseudoClasses: ['focus', 'focus-visible'] });
      await main.waitForTimeout(300);
      const shadow = await main.locator(selector).first().evaluate((el) => getComputedStyle(el).boxShadow);
      await cdp.send('CSS.forcePseudoState', { nodeId, forcedPseudoClasses: [] });
      expect(shadow, selector).toContain('rgb(240, 176, 74)');
    }
  } finally {
    await cdp.detach().catch(() => {});
  }
  const ring = await main.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--yap-focus-ring').trim());
  expect(ring).toBe('#f0b04a');
  // The ring against the page it sits on (non-text, 3:1).
  measured['focus ring'] = await main.evaluate(() => {
    const el = document.createElement('span');
    el.style.cssText = 'position:fixed;width:4px;height:4px;background:var(--yap-focus-ring)';
    document.querySelector('[role="dialog"][aria-label="Settings"] .content')?.appendChild(el);
    const c = getComputedStyle(el).backgroundColor;
    el.remove();
    return c;
  }).then(async (c) => {
    const [r, g, b] = c.match(/\d+/g).map(Number);
    const lum = (v) => [v[0], v[1], v[2]].map((x) => x / 255).map((x) => (x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4)).reduce((a, x, i) => a + x * [0.2126, 0.7152, 0.0722][i], 0);
    return Math.round(((lum([r, g, b]) + 0.05) / (lum([0x1c, 0x1a, 0x16]) + 0.05)) * 100) / 100;
  });
  expect(measured['focus ring']).toBeGreaterThanOrEqual(3);
  await closeSettings(main);

  // The main window: its nav labels, the keycaps on Home, the tip card.
  await openView(main, 'Home');
  await check('main nav labels', main, 'nav[aria-label="Main"] .navlabel', 4.5);
  await check('keycaps', main, '.key', 4.5);
  await check('tip card', main, '.hero h2, .hero p', 4.5);
  await check('feed text', main, '.item .text, .item .time', 4.5);
  // Insights: the heatmap's busiest cells and the bars against what they sit
  // on (3:1, graphics), their labels as text.
  await openView(main, 'Insights');
  await check('heatmap cells', main, '.acell[data-level="4"]', 3, 'backgroundColor');
  await check('chart bars', main, '.appbar span', 3, 'backgroundColor');
  await check('chart labels', main, '.appname, .appwords, .legend', 4.5);
  // Chat: an answer, its link.
  await openView(main, 'Chat');
  await main.locator('.conv').filter({ hasText: 'Offsite plan' }).click();
  await check('chat text', main, '.cbubble', 4.5);
  await check('chat links', main, 'a.md-link', 4.5);
  console.log(`dark-theme contrast (lowest of each): ${JSON.stringify(measured)}`);
  await test.info().attach('contrast.json', { body: JSON.stringify(measured, null, 2), contentType: 'application/json' });

  await pickTheme(main, yap, 'Light');
  await closeSettings(main);
});

// ---------------------------------------------------------------- screens

test('every view, every Settings page, the notepad, onboarding, a toast, the dialogs and the bar in dark', async ({
  yap,
  main,
  shot,
}) => {
  test.setTimeout(240_000);
  await pickTheme(main, yap, 'Dark');
  await closeSettings(main);
  await expect.poll(() => themeOf(main)).toBe('dark');

  // The main window's views.
  for (const view of ['Home', 'Insights', 'Chat', 'Notes', 'Meetings', 'Tasks', 'Upload', 'Dictionary', 'Integrations']) {
    await openView(main, view);
    if (view === 'Chat') {
      await main.locator('.conv').filter({ hasText: 'Offsite plan' }).click();
      await expect(main.locator('.cbubble.assistant')).toHaveCount(1);
    }
    if (view === 'Notes') {
      await main.locator('.folder').filter({ hasText: 'Meetings' }).click();
      await main.locator('.item').filter({ hasText: 'Design review' }).click();
      await expect(main.locator('textarea.raw')).toHaveValue(/venue deposit/);
    }
    await main.waitForTimeout(250);
    await shot(main, `view-${view.toLowerCase()}`);
    if (view === 'Notes') {
      await main.locator('.tabs .tab').filter({ hasText: 'Summary' }).click();
      await expect(main.locator('.editor .rendered')).toContainText('Send the revised budget');
      await shot(main, 'view-notes-enhanced');
    }
  }

  // Every Settings page, whole.
  await openSettings(main, 'General');
  const pages = ['general', 'dictation', 'shortcuts', 'meetings', 'commands', 'voice', 'memory', 'models', 'cleanup', 'connectors', 'mcp', 'privacy', 'about', 'account'];
  for (const id of pages) {
    await goto(main, id);
    await main.waitForTimeout(250);
    const content = settingsDialog(main).locator('.content');
    const height = await content.evaluate((el) => el.scrollHeight).catch(() => 800);
    await tall(main, Math.min(Math.max(800, height + 140), 4000), async () => {
      await main.waitForTimeout(200);
      await shot(main, `settings-${id}`);
    });
  }
  // Language Models' other tabs, cleanup on.
  await closeSettings(main);

  // A toast.
  await yap.invoke('e2e_notice', { surface: 'window', kind: 'e2e-theme', level: 'info', title: 'Calendar connected', body: '6 meetings in the next 7 days.', timeoutMs: 60_000 });
  await expect(main.getByRole('status').filter({ hasText: 'Calendar connected' })).toBeVisible();
  await shot(main, 'toast');
  await closeToasts(main);

  // The Connect calendar dialog (with the bar's card picture).
  await openView(main, 'Meetings');
  await main.getByRole('button', { name: 'Connect calendar' }).first().click();
  const connect = main.getByRole('dialog', { name: /calendar/i });
  await expect(connect).toBeVisible();
  await shot(main, 'dialog-connect-calendar');
  await connect.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(connect).toBeHidden();

  // A confirm dialog (deleting a dictation asks first).
  await openView(main, 'Home');
  const row = main.locator('.item').first();
  await row.hover();
  await row.getByRole('button', { name: 'Delete', exact: true }).click();
  const confirm = main.getByRole('alertdialog');
  await expect(confirm).toBeVisible();
  await shot(main, 'dialog-confirm');
  await confirm.getByRole('button', { name: 'Cancel' }).click();
  await expect(confirm).toBeHidden();

  // The meeting notepad.
  await yap.invoke('notepad_open', { noteId: MEETING.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  await expect.poll(() => themeOf(pad)).toBe('dark');
  await expect(pad.getByRole('textbox', { name: 'My thoughts' })).toHaveValue(/venue deposit/);
  await shot(pad, 'notepad-thoughts');
  await pad.getByRole('tab', { name: 'Transcript' }).click();
  await shot(pad, 'notepad-transcript');
  await pad.getByRole('tab', { name: 'Summary' }).click();
  await shot(pad, 'notepad-summary');
  await closeNotepad(yap);

  // Onboarding.
  await yap.invoke('open_onboarding');
  await expect.poll(() => visible(yap, 'onboarding')).toBe(true);
  await expect.poll(() => themeOf(yap.onboarding)).toBe('dark');
  await yap.onboarding.waitForTimeout(400);
  await shot(yap.onboarding, 'onboarding');
  await yap.invoke('close_onboarding');

  // The bar (Match app → dark): the pill, a notice card and a call card.
  const bar = yap.overlay;
  await expect.poll(() => barThemeOf(bar)).toBe('dark');
  await yap.invoke('bar_simulate', { card: 'update' });
  await expect(bar.getByRole('status').filter({ hasText: 'is ready' })).toBeVisible();
  await bar.waitForTimeout(300);
  await shot(bar, 'bar-update-card');
  await yap.invoke('bar_simulate', { card: 'call' });
  await bar.waitForTimeout(400);
  await shot(bar, 'bar-call-card');

  await pickTheme(main, yap, 'Light');
  await closeSettings(main);
});

test('more of Yap in dark: call apps, Language Models with cleanup on, the Chat model picker, Actions, menus, the Ask sheet', async ({
  yap,
  main,
  shot,
}) => {
  test.setTimeout(180_000);
  await pickTheme(main, yap, 'Dark');
  const dialog = settingsDialog(main);

  // Settings → Meetings → the call apps, each with its mark.
  await goto(main, 'meetings#call-apps-list');
  await expect(dialog.locator('[data-app-icon]').first()).toBeVisible();
  await shot(main, 'settings-call-apps');

  // Language Models with cleanup on: providers, the key, the models, Prompt Studio.
  await goto(main, 'cleanup');
  await dialog.getByRole('button', { name: 'Clean up dictations with AI' }).click();
  await expectStore(yap, 'config.json', (c) => c.postProcessEnabled === true);
  const height = await dialog.locator('.content').evaluate((el) => el.scrollHeight);
  await tall(main, Math.min(height + 140, 4000), async () => {
    await main.waitForTimeout(250);
    await shot(main, 'settings-cleanup-on');
  });
  await dialog.getByRole('button', { name: 'Clean up dictations with AI' }).click();
  await expectStore(yap, 'config.json', (c) => c.postProcessEnabled === false);
  await closeSettings(main);

  // The Chat model picker and a right-click menu.
  const picker = await openModelPicker(main);
  await shot(main, 'chat-model-picker');
  await main.keyboard.press('Escape');
  await expect(picker).toBeHidden();
  await main.locator('.conv').filter({ hasText: 'Offsite plan' }).click({ button: 'right' });
  await expect(main.getByRole('menu')).toBeVisible();
  await shot(main, 'context-menu');
  await main.keyboard.press('Escape');

  // Notes → Actions.
  await openView(main, 'Notes');
  await main.getByRole('button', { name: 'Actions', exact: true }).click();
  const actions = main.getByRole('dialog').filter({ has: main.getByRole('button', { name: 'Add action' }) });
  await expect(actions).toBeVisible();
  await shot(main, 'notes-actions');
  await actions.getByRole('button', { name: 'Close', exact: true }).click();
  await expect(actions).toBeHidden();

  // The notepad's ⋯ menu and its Ask sheet.
  await yap.invoke('notepad_open', { noteId: MEETING.id });
  await expect.poll(() => visible(yap, 'notepad')).toBe(true);
  const pad = yap.notepad;
  await pad.getByRole('button', { name: 'More', exact: true }).click();
  await expect(pad.getByRole('menu', { name: 'More' })).toBeVisible();
  await shot(pad, 'notepad-menu');
  await pad.keyboard.press('Escape');
  await pad.getByRole('textbox', { name: 'Ask anything' }).click();
  await pad.waitForTimeout(500);
  await shot(pad, 'notepad-ask-sheet');
  await pad.keyboard.press('Escape');
  await closeNotepad(yap);

  await pickTheme(main, yap, 'Light');
  await closeSettings(main);
});
