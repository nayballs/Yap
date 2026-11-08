// Skills on an install from before the core six (chat/skills.rs; config.rs
// `migrate_once` v7): a config at v6, which ran a Yap with Skills, and one
// of the person's own in skills.json. It keeps the six Skills that were
// built in then in its "/" menus — /risks, /coach and /weekly-recap with
// the core's /followup-email, /todos and /decisions — and the core's three
// new ones join them; the other six stay in More Skills. Its own Skill
// stays, and stays first. The rest of the Skills' tests: skills.spec.js,
// meeting-chips.spec.js.
import { test, expect, openView, openSettings, closeSettings } from './support/fixtures.js';

test.use({
  yapOptions: {
    name: 'skills-existing',
    config: { configVersion: 6 },
    data: {
      'skills.json': {
        skills: [{ id: 's1', name: 'Budget numbers', command: 'budget-numbers', instructions: 'List every amount of money that was said, and who said it.', runsOn: 'both' }],
      },
    },
  },
});

const skillMenu = (page) => page.getByRole('listbox', { name: 'Skills' });

test('keeps the six Skills it had in the menus, beside the core, and its own first', async ({ yap, main, shot }) => {
  // Kept in skills.json, its own Skill with them.
  const stored = yap.readJson('skills.json');
  expect(stored.menu).toEqual({ 'builtin:coach': true, 'builtin:risks': true, 'builtin:weekly-recap': true });
  expect(stored.skills.map((s) => s.command)).toEqual(['budget-numbers']);
  // Its migration (v7) ran; later ones may have run after it.
  expect((await yap.invoke('get_config')).configVersion).toBeGreaterThanOrEqual(7);

  // Chat's "/": its own, then Yap's that run across the meetings.
  await openView(main, 'Chat');
  await main.getByRole('button', { name: 'New chat' }).first().click();
  const box = main.getByRole('textbox', { name: 'Message' });
  await box.fill('/');
  await expect(skillMenu(main).getByRole('option')).toHaveText([
    /^\/budget-numbers/,
    /^\/prep/,
    /^\/standup/,
    /^\/todos/,
    /^\/decisions/,
    /^\/summary-for/,
    /^\/risks/,
    /^\/coach/,
    /^\/weekly-recap/,
  ]);
  await shot(main, '01-chat-skill-menu-kept');
  await box.fill('');

  // Settings: their switches on in More Skills, the rest off.
  const dialog = await openSettings(main, 'Skills');
  const more = dialog.getByRole('group', { name: 'More Skills' });
  const on = (c) => more.getByRole('button', { name: `Show /${c} in the / menu` });
  for (const c of ['risks', 'coach', 'weekly-recap']) await expect(on(c)).toHaveAttribute('aria-pressed', 'true');
  for (const c of ['decision-log', 'open-questions', 'tickets', 'prd', 'one-to-one', 'project-update']) await expect(on(c)).toHaveAttribute('aria-pressed', 'false');
  await expect(dialog.getByRole('group', { name: 'Your Skills' }).locator('[data-skill="budget-numbers"]')).toBeVisible();
  await closeSettings(main);
});
