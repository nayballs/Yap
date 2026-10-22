// Settings after the 2026-10-22 review (src/lib/settings/): the search at
// the top of the nav over a static index of every setting, the old links
// that must still land (nav.js ALIASES), the 860 × 600 layout (an icon rail,
// rows that stack), the Meetings page's call apps one level down, and the
// contrast of the greys and links, measured in the running app.
//
// The first tests need no app: the index and the link resolver are plain
// modules. "every index entry points at a real row" opens each entry's page
// in the app and finds its row, so the index can't drift from the pages.
import { test, expect, openSettings, closeSettings, settingsDialog, expectStore } from './support/fixtures.js';
import { SETTINGS_INDEX, searchSettings, groupResults, countByPage, highlight, stems } from '../src/lib/settings/settingsIndex.js';
import { NAV_GROUPS, PAGE_IDS, PAGE_LABELS, ALIASES, resolveTarget } from '../src/lib/settings/nav.js';

const SCOPES = ['cleanup', 'voiceAgent', 'noteFormatting', 'chat'];
const nav = (main) => settingsDialog(main).getByRole('navigation', { name: 'Settings sections' });
const navItem = (main, id) => nav(main).getByRole('button', { name: new RegExp(`^${PAGE_LABELS[id].replace(/[()&]/g, '\\$&')}`) });
const searchBox = (main) => settingsDialog(main).getByRole('searchbox', { name: 'Search settings' });
const results = (main) => settingsDialog(main).getByRole('listbox', { name: 'Settings found' });
/** Open a page (or an index entry) the way the app's own links do. */
const goto = (main, target) => main.evaluate((t) => window.dispatchEvent(new CustomEvent('yap-settings-goto', { detail: t })), target);

// ---------------------------------------------------------------- no app

test('the index: every entry has a page, a label and an anchor, and no anchor is used twice on a page', () => {
  expect(SETTINGS_INDEX.length).toBeGreaterThan(90);
  const seen = new Set();
  for (const e of SETTINGS_INDEX) {
    expect(PAGE_IDS, `${e.label}: page`).toContain(e.page);
    expect(e.label.trim().length, 'label').toBeGreaterThan(0);
    expect(e.anchor, `${e.label}: anchor`).toMatch(/^[a-z0-9-]+$/);
    if (e.scope) expect(SCOPES).toContain(e.scope);
    const key = `${e.page}/${e.scope ?? ''}/${e.label}`;
    expect(seen.has(key), `${key} twice`).toBe(false);
    seen.add(key);
  }
  // Every page the nav shows has something to find.
  for (const id of PAGE_IDS) expect(SETTINGS_INDEX.some((e) => e.page === id), id).toBe(true);
});

test('search: words, their forms, ranking, grouping and highlights', () => {
  expect(stems('paste')).toEqual(expect.arrayContaining(['paste', 'past']));
  expect(stems('pasting')).toEqual(expect.arrayContaining(['pasting', 'past']));
  expect(stems('notes')).toEqual(expect.arrayContaining(['notes', 'note']));

  // "paste": the Dictation rows first, best match (in the label) on top.
  const found = searchSettings('paste');
  expect(found[0]).toMatchObject({ page: 'dictation', label: 'Restore clipboard after paste' });
  expect(found.map((r) => r.label)).toEqual(expect.arrayContaining(['Safer pasting into terminals', 'Press Enter after pasting']));
  const groups = groupResults(found);
  expect(groups[0].page).toBe('dictation');
  // Within a page, a section's rows together (one heading each).
  const sections = groups[0].results.map((r) => r.section);
  expect(sections).toEqual([...sections].sort((a, b) => sections.indexOf(a) - sections.indexOf(b)));
  expect(countByPage(found).dictation).toBe(groups[0].results.length);

  // Every word must be found; keywords count (Teams is in "Ask about calls in").
  expect(searchSettings('teams calls').map((r) => r.label)).toContain('Ask about calls in');
  expect(searchSettings('win+v')[0].label).toBe('Restore clipboard after paste');
  expect(searchSettings('microphone')[0].label).toBe('Microphone');
  expect(searchSettings('dark mode')[0].label).toBe('Colours');
  expect(searchSettings('logs').map((r) => r.page)).toContain('privacy');
  expect(searchSettings('zzqx')).toEqual([]);
  expect(searchSettings('   ')).toEqual([]);

  // Highlights: every form of every word, case kept.
  expect(highlight('Safer pasting into terminals', 'paste')).toEqual([
    { text: 'Safer ', hit: false },
    { text: 'past', hit: true },
    { text: 'ing into terminals', hit: false },
  ]);
  expect(highlight('Restore clipboard', 'zzz')).toEqual([{ text: 'Restore clipboard', hit: false }]);
});

test('old links land: the aliases, and links that never existed stay put', () => {
  expect(resolveTarget('general#meetings', SCOPES)).toEqual({ page: 'meetings', anchor: '', scope: '' });
  expect(resolveTarget('general#screen-sharing', SCOPES)).toEqual({ page: 'meetings', anchor: 'screen-sharing', scope: '' });
  expect(resolveTarget('history', SCOPES)).toEqual({ page: 'privacy', anchor: 'history', scope: '' });
  expect(resolveTarget('advanced', SCOPES)).toEqual({ page: 'dictation', anchor: 'typing', scope: '' });
  expect(resolveTarget('advanced#nothing-here', SCOPES)).toEqual({ page: 'dictation', anchor: 'nothing-here', scope: '' });
  expect(resolveTarget('general#bar', SCOPES)).toEqual({ page: 'general', anchor: 'bar', scope: '' });
  expect(resolveTarget('cleanup#chat', SCOPES)).toEqual({ page: 'cleanup', anchor: '', scope: 'chat' });
  expect(resolveTarget('voice#assistant-shortcut', SCOPES)).toEqual({ page: 'voice', anchor: 'assistant-shortcut', scope: '' });
  expect(resolveTarget('account', SCOPES)).toEqual({ page: 'account', anchor: '', scope: '' });
  expect(resolveTarget({ page: 'cleanup', scope: 'voiceAgent' }, SCOPES)).toEqual({ page: 'cleanup', anchor: '', scope: 'voiceAgent' });
  expect(resolveTarget('memory', SCOPES)).toBeNull(); // a page this build doesn't have
  expect(resolveTarget('', SCOPES)).toBeNull();
  // Every alias goes to a page that exists.
  for (const [from, to] of Object.entries(ALIASES)) expect(PAGE_IDS, from).toContain(to.split('#')[0]);
  // The nav: four scopes under the unlabelled first group, by scope.
  expect(NAV_GROUPS.map((g) => g.label)).toEqual([null, 'Assistant', 'AI models', 'Connections', 'This PC']);
});

// ---------------------------------------------------------------- the app

test('every index entry points at a real row on its page', async ({ main }) => {
  test.setTimeout(180_000);
  await main.setViewportSize({ width: 1200, height: 800 });
  const dialog = await openSettings(main, 'General');
  const missing = [];
  for (const e of SETTINGS_INDEX) {
    await goto(main, { page: e.page, anchor: e.anchor, scope: e.scope });
    if (e.page === 'account') await expect(dialog.getByRole('heading', { level: 1, name: 'Account' })).toBeVisible();
    else await expect(navItem(main, e.page)).toHaveAttribute('aria-current', 'page');
    if (e.scope) await expect(dialog.locator('.scope-tabs').getByRole('tab', { selected: true })).toBeVisible();
    const anchors = [e.anchor, e.fallback].filter(Boolean);
    const row = dialog.locator(anchors.map((a) => `[data-setting="${a}"], #settings-${a}`).join(', ')).first();
    try {
      await expect(row).toBeAttached({ timeout: 5_000 });
    } catch {
      missing.push(`${e.page}${e.scope ? `/${e.scope}` : ''}: ${e.label} (${e.anchor})`);
    }
  }
  expect(missing, 'index entries with no row').toEqual([]);
  await closeSettings(main);
});

test('Settings search: Ctrl+F, results by page, ↑/↓, Enter lands on the row, Esc clears then closes', async ({ yap, main, shot }) => {
  await main.setViewportSize({ width: 1200, height: 800 });
  const dialog = await openSettings(main, 'General');
  await main.keyboard.press('Control+f');
  await expect(searchBox(main)).toBeFocused();
  await main.keyboard.type('paste');

  await expect(dialog.getByRole('heading', { level: 1, name: 'Results for “paste”' })).toBeVisible();
  const expected = searchSettings('paste');
  await expect(results(main).getByRole('option')).toHaveCount(expected.length);
  await expect(dialog).toContainText(`${expected.length} settings on ${groupResults(expected).length} pages`);
  // The nav: a count on pages with results, the rest dimmed.
  await expect(navItem(main, 'dictation')).toContainText(String(countByPage(expected).dictation));
  await expect(navItem(main, 'about')).toHaveClass(/dim/);
  // The matched words highlighted.
  await expect(results(main).locator('mark').first()).toHaveText(/past/i);
  const first = results(main).getByRole('option').first();
  await expect(first).toHaveAttribute('aria-selected', 'true');
  await expect(first).toContainText('Restore clipboard after paste');
  await shot(main, '01-search-paste');

  // ↓ moves the highlight; the box keeps the focus.
  await main.keyboard.press('ArrowDown');
  const second = results(main).getByRole('option').nth(1);
  await expect(second).toHaveAttribute('aria-selected', 'true');
  await expect(searchBox(main)).toBeFocused();
  await main.keyboard.press('ArrowUp');
  await expect(first).toHaveAttribute('aria-selected', 'true');

  // Enter: its page, scrolled to the row, the row washed, the switch focused.
  await main.keyboard.press('Enter');
  await expect(navItem(main, 'dictation')).toHaveAttribute('aria-current', 'page');
  const row = dialog.locator('[data-setting="restore-clipboard"]');
  await expect(row).toBeInViewport();
  await expect(row).toHaveClass(/set-flash/);
  await expect(row.getByRole('button', { name: 'Restore clipboard after paste' })).toBeFocused();
  await expect(searchBox(main)).toHaveValue('');
  await shot(main, '02-search-landed');

  // A plain switch works right in the results.
  await searchBox(main).click();
  await main.keyboard.type('clipboard');
  const live = results(main).getByRole('button', { name: 'Restore clipboard after paste' });
  await expect(live).toHaveAttribute('aria-pressed', 'true');
  await live.click();
  await expectStore(yap, 'config.json', (c) => c.restoreClipboard === false);
  await live.click();
  await expectStore(yap, 'config.json', (c) => c.restoreClipboard === true);

  // Nothing found says so.
  await searchBox(main).fill('zzqx');
  await expect(dialog).toContainText('Nothing matches “zzqx”');

  // Esc: first the search, then Settings.
  await searchBox(main).focus();
  await main.keyboard.press('Escape');
  await expect(searchBox(main)).toHaveValue('');
  await expect(dialog).toBeVisible();
  await main.keyboard.press('Escape');
  await expect(dialog).toBeHidden();

  // "/" opens it too, from anywhere but a text field.
  await openSettings(main, 'About');
  await dialog.getByRole('heading', { level: 1, name: 'About' }).click();
  await main.keyboard.press('/');
  await expect(searchBox(main)).toBeFocused();
  await main.keyboard.type('speaker');
  await expect(results(main).getByRole('option').first()).toBeVisible();
  await closeSettings(main);
  // Closed and opened again: a clean page, no old results.
  await openSettings(main);
  await expect(searchBox(main)).toHaveValue('');
  await closeSettings(main);
});

test('old links land where their settings went', async ({ yap, main, shot }) => {
  await main.setViewportSize({ width: 1200, height: 800 });
  const dialog = await openSettings(main, 'About');
  const cases = [
    ['general#meetings', 'meetings', 'Call detection'],
    ['general#screen-sharing', 'meetings', null, '#settings-screen-sharing'],
    ['history', 'privacy', 'History'],
    ['advanced', 'dictation', 'Typing into apps'],
    ['general#bar', 'general', 'Yap bar'],
    ['voice#assistant-shortcut', 'voice', 'Assistant shortcut'],
    ['meetings#call-apps-list', 'meetings', 'Call apps'],
  ];
  for (const [link, page, group, sel] of cases) {
    await goto(main, link);
    await expect(navItem(main, page), link).toHaveAttribute('aria-current', 'page');
    const target = sel ? dialog.locator(sel) : dialog.getByRole('group', { name: group, exact: true });
    await expect(target, link).toBeInViewport();
  }
  await goto(main, 'cleanup#chat');
  await expect(dialog.locator('.scope-tabs').getByRole('tab', { name: 'Chat & Voice Assistant' })).toHaveAttribute('aria-selected', 'true');
  // A page this build doesn't have: Settings stays where it was.
  await goto(main, 'memory');
  await expect(navItem(main, 'cleanup')).toHaveAttribute('aria-current', 'page');

  // The way Rust sends one (meeting_guard.rs's Windows notification, still in
  // the notification centre with the old string): through the app's event.
  await closeSettings(main);
  await yap.invoke('plugin:event|emit', { event: 'yap-open-settings', payload: 'general#screen-sharing' });
  await expect(dialog).toBeVisible();
  await expect(navItem(main, 'meetings')).toHaveAttribute('aria-current', 'page');
  await expect(dialog.getByRole('button', { name: 'Hide Yap from screen sharing' })).toBeInViewport();
  await shot(main, '03-old-screen-sharing-link');
  await closeSettings(main);
});

test('Meetings: the call apps are one level down, behind a link row with the count', async ({ yap, main, shot }) => {
  await main.setViewportSize({ width: 1200, height: 800 });
  const dialog = await openSettings(main, 'Meetings');
  const { apps } = await yap.invoke('meeting_detect_status');
  const asking = apps.filter((a) => a.asks);
  const link = dialog.getByRole('group', { name: 'Call detection' }).getByRole('button', { name: /^Ask about calls in/ });
  await expect(link).toContainText(`${asking.length} of ${apps.length} apps`);
  // The first few it asks about, as their icons.
  await expect(link.locator('[data-app-icon]')).toHaveCount(Math.min(5, asking.length));
  // Not on the page itself: no app switches here.
  await expect(dialog.getByRole('button', { name: 'Teams', exact: true })).toHaveCount(0);
  await link.click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Ask about calls in' })).toBeVisible();
  const list = dialog.getByRole('group', { name: 'Ask about calls in' });
  await expect(list.getByRole('button')).toHaveCount(apps.length);
  await shot(main, '04-meetings-call-apps');
  // Back to Meetings.
  await dialog.locator('.page-back').click();
  await expect(dialog.getByRole('heading', { level: 1, name: 'Meetings' })).toBeVisible();
  await expect(link).toBeVisible();
  await closeSettings(main);
});

test('860 × 600: the nav is an icon rail with tooltips, and rows stack instead of squeezing', async ({ main, shot }) => {
  await main.setViewportSize({ width: 860, height: 600 });
  try {
    const dialog = await openSettings(main, 'Dictation');
    // The rail: no labels or group names, every page still named for
    // assistive tech, all of them on screen without scrolling.
    const box = await nav(main).boundingBox();
    expect(box.width).toBeLessThan(70);
    await expect(nav(main).locator('.navlabel')).toHaveCount(0);
    await expect(nav(main).locator('.navcap')).toHaveCount(0);
    const items = nav(main).locator('.navitem');
    await expect(items).toHaveCount(PAGE_IDS.length - 1);
    for (let i = 0; i < (await items.count()); i++) await expect(items.nth(i)).toBeInViewport();
    // A tooltip names the page under the pointer.
    await navItem(main, 'shortcuts').hover();
    await expect(dialog.getByRole('tooltip')).toHaveText('Shortcuts');
    await shot(main, '05-narrow-rail');

    // No page scrolls sideways, and no row is squeezed to a sliver.
    for (const page of ['general', 'dictation', 'meetings', 'voice', 'cleanup', 'privacy']) {
      await goto(main, page);
      await expect(navItem(main, page)).toHaveAttribute('aria-current', 'page');
      const content = dialog.locator('.content');
      const { sw, cw } = await content.evaluate((el) => ({ sw: el.scrollWidth, cw: el.clientWidth }));
      expect(sw, `${page} scrolls sideways`).toBeLessThanOrEqual(cw);
      const narrowest = await content.evaluate((el) =>
        Math.min(...[...el.querySelectorAll('.row .lhs')].map((l) => l.getBoundingClientRect().width).filter((w) => w > 0))
      );
      expect(narrowest, `${page}: a row's label column`).toBeGreaterThan(200);
    }
    // "Cancel and finish buttons" (one word a line at this size before).
    await goto(main, 'dictation#dictation-buttons');
    const label = dialog.locator('[data-setting="dictation-buttons"] .label');
    expect((await label.boundingBox()).height).toBeLessThan(24);
    await shot(main, '06-narrow-dictation');

    // The rail's search: a box over the page.
    await dialog.getByRole('button', { name: 'Search settings' }).click();
    await expect(searchBox(main)).toBeFocused();
    await main.keyboard.type('micro');
    await expect(results(main).getByRole('option').first()).toContainText('Microphone');
    await shot(main, '07-narrow-search');
    await main.keyboard.press('Escape');
    await main.keyboard.press('Escape');
    await expect(searchBox(main)).toHaveCount(0);
  } finally {
    await main.setViewportSize({ width: 1200, height: 800 });
  }
  // Wide again: labels and group names back.
  await expect(nav(main).locator('.navcap')).toHaveCount(4);
  await closeSettings(main);
});

test('contrast: descriptions, headings, nav labels and links pass AA; switches and focus 3:1', async ({ main }) => {
  await main.setViewportSize({ width: 1200, height: 800 });
  await openSettings(main, 'General');
  const measure = (sel) =>
    settingsDialog(main).evaluate((root, s) => {
      const rgb = (c) => {
        const m = c.match(/[\d.]+/g)?.map(Number) ?? [0, 0, 0, 1];
        return { r: m[0], g: m[1], b: m[2], a: m.length > 3 ? m[3] : 1 };
      };
      const lum = ({ r, g, b }) =>
        [r, g, b]
          .map((v) => v / 255)
          .map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4))
          .reduce((acc, v, i) => acc + v * [0.2126, 0.7152, 0.0722][i], 0);
      const over = (fg, bg) => ({ r: fg.r * fg.a + bg.r * (1 - fg.a), g: fg.g * fg.a + bg.g * (1 - fg.a), b: fg.b * fg.a + bg.b * (1 - fg.a), a: 1 });
      const bgOf = (el) => {
        for (let e = el; e; e = e.parentElement) {
          const c = rgb(getComputedStyle(e).backgroundColor);
          if (c.a > 0.99) return c;
        }
        return { r: 255, g: 255, b: 255, a: 1 };
      };
      const ratio = (a, b) => {
        const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
        return (x + 0.05) / (y + 0.05);
      };
      const [selector, prop] = s;
      const els = [...root.querySelectorAll(selector)].filter((e) => e.getClientRects().length);
      return els.map((el) => {
        const bg = bgOf(el.parentElement ?? el);
        const fg = over(rgb(getComputedStyle(el)[prop]), bg);
        return Math.round(ratio(fg, bg) * 100) / 100;
      });
    }, sel);
  const min = async (selector, prop = 'color') => {
    const values = await measure([selector, prop]);
    expect(values.length, `${selector}: none on the page`).toBeGreaterThan(0);
    return Math.min(...values);
  };
  expect(await min('.row .desc, .toggle-row .desc')).toBeGreaterThanOrEqual(4.5);
  expect(await min('.page-h p')).toBeGreaterThanOrEqual(4.5);
  expect(await min('.group h2')).toBeGreaterThanOrEqual(4.5);
  expect(await min('.navcap')).toBeGreaterThanOrEqual(4.5);
  expect(await min('.set-link')).toBeGreaterThanOrEqual(4.5);
  expect(await min('.navfoot .ver')).toBeGreaterThanOrEqual(4.5);
  // An off switch's edge against the page (non-text: 3:1).
  expect(await min('.switch:not(.on)', 'borderTopColor')).toBeGreaterThanOrEqual(3);
  // The close button.
  expect(await min('.modal-x')).toBeGreaterThanOrEqual(4.5);
  // One focus ring: the same amber on a switch, a select, a segmented
  // control, a link row and a nav item (:focus-visible forced over CDP, as
  // the keyboard would).
  const cdp = await main.context().newCDPSession(main);
  try {
    await cdp.send('DOM.enable');
    await cdp.send('CSS.enable');
    const { root } = await cdp.send('DOM.getDocument', { depth: -1 });
    const ring = async (selector) => {
      const { nodeId } = await cdp.send('DOM.querySelector', { nodeId: root.nodeId, selector });
      expect(nodeId, selector).toBeGreaterThan(0);
      await cdp.send('CSS.forcePseudoState', { nodeId, forcedPseudoClasses: ['focus', 'focus-visible'] });
      await main.waitForTimeout(300); // past the controls' 150 ms transitions
      const shadow = await main.locator(selector).first().evaluate((el) => getComputedStyle(el).boxShadow);
      await cdp.send('CSS.forcePseudoState', { nodeId, forcedPseudoClasses: [] });
      return shadow;
    };
    for (const selector of [
      '[data-setting="autostart"] .switch-btn',
      '[data-setting="region"] select',
      '[data-setting="bar-colours"] .seg-btn',
      'nav[aria-label="Settings sections"] .navitem:not(.active)',
    ]) {
      expect(await ring(selector), selector).toContain('rgb(194, 105, 10)');
    }
  } finally {
    await cdp.detach().catch(() => {});
  }
  await closeSettings(main);
});
