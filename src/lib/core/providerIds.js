// The AI providers' ids and short names, with nothing else in the file:
// providers.js re-exports them (the one place the app imports them from),
// and Yap Link's phone page loads this file as it is (`src-tauri/build.rs`
// serves it at core/providerIds.js), so it must stay pure — no imports, no
// Tauri, no Svelte, no assets (`src-tauri/link-page/README.md`, "Chat's own
// modules").

/**
 * The cloud providers with an API key in the shared store, in Settings'
 * order (Rust's `ai_model::KEYED_PROVIDERS`).
 */
export const KEYED_PROVIDERS = ['groq', 'anthropic', 'openai', 'gemini', 'openrouter', 'chatgpt'];

/**
 * Sign in with ChatGPT (ai/chatgpt.rs): a keyed provider whose key store
 * holds this marker while signed in with the plan's permission — never a
 * key the person pastes. Its calls use the sign-in's token.
 */
export const CHATGPT = 'chatgpt';

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

/** Short names: the Chat model's chip and picker (aiModel.js), Settings' link rows. */
export const PROVIDER_LABELS = {
  groq: 'Groq',
  anthropic: 'Anthropic',
  openai: 'OpenAI',
  chatgpt: 'ChatGPT',
  gemini: 'Gemini',
  openrouter: 'OpenRouter',
  ondevice: 'On this PC',
  local: 'Self-hosted',
  custom: 'Custom',
};
