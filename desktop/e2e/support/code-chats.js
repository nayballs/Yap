// Code chats in the tests (desktop/src/lib/views/chat/codeChats.svelte.js):
// with Yap Code on, Chat's list has a Chat | Code switch at its top, and the
// Code side's "New code chat" opens the workspace picker.
import { expect } from './fixtures.js';

/** The Chat | Code switch (only with Yap Code on). */
export const sideSwitch = (main) => main.getByTestId('chat-side-switch');

/**
 * Chat's list on `side` ('Chat' or 'Code'). `agentsOn`: the switch is expected
 * (with Yap Code off there's none, and nothing to do).
 */
export async function toSide(main, side, { agentsOn = true } = {}) {
  const sw = sideSwitch(main);
  if (!agentsOn) {
    await expect(sw).toHaveCount(0);
    return;
  }
  await expect(sw).toBeVisible();
  const radio = sw.getByRole('radio', { name: side, exact: true });
  if ((await radio.getAttribute('aria-checked')) !== 'true') await radio.click();
  await expect(radio).toHaveAttribute('aria-checked', 'true');
}

/**
 * The Chat side, when Yap Code is on (else nothing to do): for a spec that turns
 * Yap Code on in some tests and expects "New chat" in the next (a chat that
 * started an agent is a Code chat, and Chat followed it to the Code side).
 */
export async function plainSide(main) {
  const cfg = await main.evaluate(() => window.__TAURI_INTERNALS__.invoke('get_config'));
  if (cfg?.agentsEnabled) await toSide(main, 'Chat');
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
