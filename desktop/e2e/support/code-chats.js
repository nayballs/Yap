// Code chats in the tests (desktop/src/lib/views/chat/codeChats.svelte.js):
// with Jobs on, Chat's list has a Chat | Code switch at its top, and the
// Code side's "New code chat" opens the workspace picker.
import { expect } from './fixtures.js';

/** The Chat | Code switch (only with Jobs on). */
export const sideSwitch = (main) => main.getByTestId('chat-side-switch');

/**
 * Chat's list on `side` ('Chat' or 'Code'). `jobsOn`: the switch is expected
 * (with Jobs off there's none, and nothing to do).
 */
export async function toSide(main, side, { jobsOn = true } = {}) {
  const sw = sideSwitch(main);
  if (!jobsOn) {
    await expect(sw).toHaveCount(0);
    return;
  }
  await expect(sw).toBeVisible();
  const radio = sw.getByRole('radio', { name: side, exact: true });
  if ((await radio.getAttribute('aria-checked')) !== 'true') await radio.click();
  await expect(radio).toHaveAttribute('aria-checked', 'true');
}

/** The workspace picker, wherever it's open. */
export const workspaceMenu = (main) => main.getByTestId('chat-workspace-menu');

/** A new code chat in the workspace `name`: the Code side, "New code chat", its pick. */
export async function newCodeChat(main, name) {
  await toSide(main, 'Code');
  await main.getByRole('button', { name: 'New code chat' }).click();
  const menu = workspaceMenu(main);
  await expect(menu).toBeVisible();
  await menu.getByRole('menuitemradio', { name, exact: true }).click();
  await expect(menu).toHaveCount(0);
  await expect(main.getByTestId('chat-workspace-chip')).toHaveAttribute('data-workspace', name);
}
