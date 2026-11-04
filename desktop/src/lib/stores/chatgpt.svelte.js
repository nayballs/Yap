// Sign in with ChatGPT's state in this window (ai/chatgpt.rs): Settings'
// ChatGPT tab, Chat's "Using ChatGPT plan" line and its model picker read
// it. Rust owns it: `chatgpt_status` on load, `yap-chatgpt` after every
// change (signed in, signed out, a sign-in that ended). The key store's
// marker follows through `yap-config-saved` like any other saved setting.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export const chatgpt = $state({
  loaded: false,
  signedIn: false,
  /** Signed in with the plan's permission: Yap can use the plan. */
  plan: false,
  waiting: false,
  email: null,
  name: null,
  noticeSeen: false,
  error: null,
  usageUrl: 'https://chatgpt.com/settings/usage',
  /** The last sign-out wasn't confirmed by OpenAI. */
  unconfirmed: false,
  /** A command's own failure (starting the sign-in…). */
  failed: null,
});

let listening = null;

function apply(s) {
  if (!s || typeof s !== 'object') return;
  Object.assign(chatgpt, {
    loaded: true,
    signedIn: !!s.signedIn,
    plan: !!s.plan,
    waiting: !!s.waiting,
    email: s.email ?? null,
    name: s.name ?? null,
    noticeSeen: !!s.noticeSeen,
    error: s.error ?? null,
    usageUrl: s.usageUrl || chatgpt.usageUrl,
  });
  if (s.signedIn) chatgpt.unconfirmed = false;
}

/** Read the state, and follow it from now on. */
export async function loadChatgpt() {
  if (!listening) listening = listen('yap-chatgpt', (e) => apply(e.payload)).catch(() => null);
  await listening;
  try {
    apply(await invoke('chatgpt_status'));
  } catch {
    /* not in Tauri */
  }
}

/** Continue with ChatGPT (`reconsent`: ask the plan's permission again). */
export async function signInChatgpt(reconsent = false) {
  chatgpt.failed = null;
  try {
    await invoke('chatgpt_sign_in', { reconsent });
  } catch (e) {
    chatgpt.failed = String(e);
  }
}

export async function cancelChatgpt() {
  try {
    await invoke('chatgpt_cancel_sign_in');
  } catch {
    /* already over */
  }
}

export async function signOutChatgpt() {
  chatgpt.failed = null;
  try {
    const r = await invoke('chatgpt_sign_out');
    chatgpt.unconfirmed = !r?.confirmed;
  } catch (e) {
    chatgpt.failed = String(e);
  }
}

/** "You're using your ChatGPT plan" → Got it. */
export async function chatgptNoticeSeen() {
  chatgpt.noticeSeen = true;
  try {
    await invoke('chatgpt_notice_seen');
  } catch {
    /* shown again next time */
  }
}
