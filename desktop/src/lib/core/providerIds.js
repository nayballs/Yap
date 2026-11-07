// The AI providers' ids and short names, with nothing else in the file:
// providers.js re-exports them (the one place the app imports them from),
// and Yap Link's phone page loads this file as it is (`desktop/src-tauri/build.rs`
// serves it at core/providerIds.js), so it must stay pure — no imports, no
// Tauri, no Svelte, no assets (`phone/web/README.md`, "Chat's own
// modules").

/**
 * The cloud providers with an API key in the shared store, in Settings'
 * order (Rust's `ai_model::KEYED_PROVIDERS`).
 */
export const KEYED_PROVIDERS = ['groq', 'anthropic', 'openai', 'gemini', 'openrouter', 'chatgpt', 'claude-plan'];

/**
 * Sign in with ChatGPT (ai/chatgpt.rs): a keyed provider whose key store
 * holds this marker while signed in with the plan's permission — never a
 * key the person pastes. Its calls use the sign-in's token.
 */
export const CHATGPT = 'chatgpt';

/**
 * Chat on your Claude plan (ai/claude_plan.rs): the person's own Claude
 * Code, signed in with their own Claude account, answers. A keyed provider
 * whose key store holds a marker while Claude Code is signed in here; every
 * Language Models scope since 7 Nov 2026 (Chat, Dictation cleanup, Commands,
 * Note formatting).
 */
export const CLAUDE_PLAN = 'claude-plan';

/**
 * OpenAI's models, two ways (2026-11-02: one "ChatGPT" entry instead of the
 * OpenAI and ChatGPT tabs, in Settings and Chat's model picker alike): the
 * person's ChatGPT plan first (Sign in with ChatGPT, provider "chatgpt"),
 * then an OpenAI API key ("openai"). UI only: both ids stay what config.json,
 * Rust, usage and effort know; a scope is always on exactly one of them.
 */
export const GPT_TAB = 'gpt';
export const GPT_PROVIDERS = [CHATGPT, 'openai'];
/** The ChatGPT entry's two-way switch, when both are connected. */
export const GPT_WAYS = [
  { value: CHATGPT, label: 'Your plan' },
  { value: 'openai', label: 'API key' },
];

/**
 * Claude's models, two ways (2026-11-07, as ChatGPT's): the person's Claude
 * plan first (their own Claude Code, provider "claude-plan"), then an
 * Anthropic API key ("anthropic"). The tab keeps Anthropic's id.
 */
export const CLAUDE_TAB = 'anthropic';
export const CLAUDE_PROVIDERS = [CLAUDE_PLAN, 'anthropic'];
/** The Claude entry's two-way switch, when both are connected. */
export const CLAUDE_WAYS = [
  { value: CLAUDE_PLAN, label: 'Your plan' },
  { value: 'anthropic', label: 'API key' },
];

/**
 * **The entries with two ways in** (a plan, or an API key), by tab: which
 * provider is each way, and the switch. One table for ChatGPT and Claude.
 */
export const TAB_WAYS = {
  [GPT_TAB]: { plan: CHATGPT, key: 'openai', ways: GPT_WAYS, name: 'ChatGPT' },
  [CLAUDE_TAB]: { plan: CLAUDE_PLAN, key: 'anthropic', ways: CLAUDE_WAYS, name: 'Claude' },
};

/** The two-way entry `p` is a way of (`TAB_WAYS`' tab), else null. */
export function waysTabOf(p) {
  return Object.keys(TAB_WAYS).find((t) => TAB_WAYS[t].plan === p || TAB_WAYS[t].key === p) ?? null;
}

/** A plan's way (its key store holds a marker, never a key). */
export const isPlan = (p) => p === CHATGPT || p === CLAUDE_PLAN;

/** Short names: the Chat model's chip and picker (aiModel.js), Settings' link rows. */
export const PROVIDER_LABELS = {
  groq: 'Groq',
  anthropic: 'Anthropic',
  openai: 'OpenAI',
  chatgpt: 'ChatGPT',
  'claude-plan': 'Your Claude plan',
  gemini: 'Gemini',
  openrouter: 'OpenRouter',
  ondevice: 'On this PC',
  local: 'Self-hosted',
  custom: 'Custom',
};
