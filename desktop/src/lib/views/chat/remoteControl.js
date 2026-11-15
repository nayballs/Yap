// **Remote Control for a workspace** (2026-11-08): one switch, the same in the
// "Code · garden-planner" card (CodeEnvironment.svelte) and the Code chat's
// summary (SummaryTab.svelte). On means Settings → Phone's switch is on and
// the workspace allows it (`link_workspace_remote`, on unless switched off).
// With Settings → Phone's switch off, turning it on turns that on too, asked
// once with what it allows (REMOTE_ASK).
import { invoke } from '@tauri-apps/api/core';

/** What turning it on everywhere asks first. */
export const REMOTE_ASK = {
  title: 'Turn on Remote Control?',
  body: "Your paired phones ticked for it in Settings → Phone can follow Yap Code's agents, allow their steps and send them work, on your home Wi-Fi. From a phone, agents run at most at Auto, never Bypass permissions, and every step allowed there is recorded.",
  confirmLabel: 'Turn on',
};

/** Its status for workspace `name` (`link_remote_status`), or null. */
export async function remoteStatus(name) {
  try {
    return await invoke('link_remote_status', { workspace: name ?? null });
  } catch {
    return null;
  }
}

/** On for this workspace. Pure. */
export const remoteOn = (status) => !!status?.on && !!status?.workspaceAllowed;

/** This workspace's own switch. */
export const setWorkspaceRemote = (name, on) => invoke('link_workspace_remote', { name, on });

/** Settings → Phone's switch, patched onto a fresh config (as the Integrations view's). */
async function setMaster(next) {
  const fresh = await invoke('get_config');
  await invoke('save_config', { cfg: { ...fresh, linkRemoteControl: next }, base: fresh });
  window.dispatchEvent(new CustomEvent('yap-config-patched', { detail: { linkRemoteControl: next } }));
}

/** On here and in Settings → Phone (after REMOTE_ASK). */
export async function remoteOnEverywhere(name) {
  await setWorkspaceRemote(name, true);
  await setMaster(true);
}

/**
 * **The switch pressed**: off when on; on when Settings → Phone's is on;
 * else `'ask'` (show REMOTE_ASK, then `remoteOnEverywhere`).
 */
export async function toggleRemote(name, status) {
  if (!status) return null;
  if (remoteOn(status)) {
    await setWorkspaceRemote(name, false);
    return 'off';
  }
  if (!status.on) return 'ask';
  await setWorkspaceRemote(name, true);
  return 'on';
}
