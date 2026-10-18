// WebView2's own context menu in Yap's windows (src-tauri/src/context_menu.rs),
// with a real right-click (SendInput, the helper's `rightclick`) on the robot's
// own Yap, and a look at what windows Yap's process tree — WebView2's
// browser process included — puts up. Plain UI: none at all, Inspect
// included (a debug build, which this is, offers it only for a
// Shift+right-click). A text field: its menu (so the check does see a menu
// when there is one), holding the editing commands only; Esc closes it. A
// developer's Shift+right-click: Inspect, alone. F12 still opens DevTools in
// a debug build (closed again). Robot mode shows native menus (plain test
// mode never does). The pointer goes back where it was.
import { test, expect } from './support/robot.js';
import { helper } from './support/targets.js';

test.use({ robotOptions: { name: 'robot-menus' } });

const NAVIGATES = ['back', 'forward', 'reload', 'saveAs', 'print', 'moreTools'];
const EDITING = ['undo', 'redo', 'cut', 'copy', 'paste', 'pasteAndMatchStyle', 'selectAll', 'emoji', 'spellcheck'];

test('a right-click: nothing on plain UI, only the edit menu in a text field; Shift+right-click is Inspect in a debug build', async ({
  robot,
}) => {
  const { yap } = robot;
  const main = yap.main;
  const menus = () => yap.invoke('e2e_context_menus').then((m) => m.settings ?? {});
  const kept = (m) => m.items.filter((i) => i.kept && i.kind !== 'separator').map((i) => i.name);

  await yap.invoke('e2e_main_on_screen');
  const nav = main.getByRole('navigation', { name: 'Main' });
  await nav.getByRole('button', { name: 'Notes', exact: true }).click();
  const win = helper('yapwindow', yap.child.pid);
  expect(win.hwnd, win.error).toBeTruthy();
  const rightClick = async (locator, at, shift = false) => {
    const box = await locator.boundingBox();
    const args = ['rightclick', win.hwnd, yap.child.pid, box.x + at(box)[0], box.y + at(box)[1]];
    const r = helper(...(shift ? [...args, 'shift'] : args));
    expect(r.error, 'the right-click went out').toBeUndefined();
    return r;
  };

  // Plain UI: an empty part of the Notes view. WebView2 asks; nothing shows —
  // not even Inspect, though this is a debug build.
  let before = (await menus()).menus ?? 0;
  let r = await rightClick(main.locator('.editor'), (b) => [b.width - 60, b.height - 60]);
  await expect.poll(async () => (await menus()).menus).toBeGreaterThan(before);
  let m = (await menus()).last;
  expect(m.items.map((i) => i.name)).toEqual(expect.arrayContaining(['back', 'reload', 'saveAs', 'print']));
  expect(kept(m)).toEqual([]);
  expect(m.inspect).toBe(false);
  expect(m.handled).toBe(true);
  expect(r.popups, 'no window came up').toEqual([]);

  // A developer's Shift+right-click on the same spot: Inspect, and only that.
  before = (await menus()).menus;
  r = await rightClick(main.locator('.editor'), (b) => [b.width - 60, b.height - 60], true);
  await expect.poll(async () => (await menus()).menus).toBeGreaterThan(before);
  m = (await menus()).last;
  expect(m.inspect).toBe(true);
  expect(kept(m)).toEqual(['inspectElement']);
  expect(m.handled).toBe(false);
  expect(r.popups.length, 'the Inspect menu came up').toBeGreaterThan(0);
  expect(r.closed, 'Esc closed it').toBe(true);

  // A text field: its menu comes up, with the edit commands only.
  await main.getByRole('button', { name: 'New note' }).first().click();
  const raw = main.getByPlaceholder('Start writing…');
  await raw.fill('Some text to edit');
  before = (await menus()).menus;
  r = await rightClick(raw, () => [260, 70]);
  m = (await menus()).last;
  expect((await menus()).menus).toBeGreaterThan(before);
  expect(m.editable).toBe(true);
  expect(m.handled).toBe(false);
  expect(m.inspect).toBe(false);
  expect(kept(m)).toEqual(expect.arrayContaining(['cut', 'copy', 'paste']));
  for (const name of kept(m)) expect(EDITING).toContain(name);
  for (const name of NAVIGATES) expect(kept(m)).not.toContain(name);
  expect(r.popups.length, 'the menu came up').toBeGreaterThan(0);
  expect(r.closed, 'Esc closed it').toBe(true);
  // What came up (its window class and size), in the report.
  await test.info().attach('text-field-menu', { body: JSON.stringify({ popups: r.popups, kept: kept(m) }) });
  console.log(`the text field's menu: ${JSON.stringify(r.popups)}, items ${kept(m).join(', ')}`);

  // F12 still opens DevTools in a debug build (closed again at once).
  await main.locator('.editor').click({ position: { x: 40, y: 40 } });
  const devtools = helper('devtools', win.hwnd, yap.child.pid);
  expect(devtools.error, 'F12 went out').toBeUndefined();
  expect(devtools.popups.length, 'DevTools came up').toBeGreaterThan(0);
  expect(devtools.closed, 'DevTools closed again').toBe(true);
  console.log(`F12: ${JSON.stringify(devtools.popups)}`);

  // Tidy up: the note goes.
  const note = (await yap.invoke('notes_list'))[0];
  await yap.invoke('note_delete', { id: note.id });
});
