// The AI providers Yap's language models run on, in one place (2026-10-24
// code review: the list, the keys and the names had been copied into
// Settings, the scope tabs, onboarding and the Chat model's words, and the
// copies drifted).
//
// - The ids are persisted in config.json and matched by Rust ("ondevice" =
//   local_llm::PROVIDER_ONDEVICE, "local" = a self-hosted server): never
//   change them. Labels are display-only.
// - The keyed providers share ONE API key each across every scope
//   (`cfg.ppApiKeys[provider]`, as OpenWhispr keeps one openai_api_key);
//   cleanup's active key is `cfg.ppApiKey`, a scope's its `apiKey`. A custom
//   endpoint or self-hosted server keeps its key per scope (`apiKeys`).
// - Each surface names them its own way today (the on-device model is
//   "Local" on the Language Models tabs and "On this PC" on a model chip;
//   Gemini is "Google Gemini" where the words go): the sets are below, side
//   by side, so making them one is a change here.
import { PROVIDER_ICONS } from './providerIcons.js';

/**
 * The cloud providers with an API key in the shared store, in Settings'
 * order (Rust's `ai_model::KEYED_PROVIDERS`).
 */
export const KEYED_PROVIDERS = ['groq', 'anthropic', 'openai', 'gemini', 'openrouter'];
export const isKeyed = (p) => KEYED_PROVIDERS.includes(p);

/** The Cloud providers tabs: the keyed providers and a custom endpoint. */
export const CLOUD_PROVIDER_IDS = [...KEYED_PROVIDERS, 'custom'];

/** Each provider's OpenAI-compatible endpoint (Anthropic's /v1/chat/completions layer). */
export const BASE_URLS = {
  groq: 'https://api.groq.com/openai/v1',
  anthropic: 'https://api.anthropic.com/v1',
  openai: 'https://api.openai.com/v1',
  gemini: 'https://generativelanguage.googleapis.com/v1beta/openai/',
  openrouter: 'https://openrouter.ai/api/v1',
  local: 'http://localhost:11434/v1',
};

/**
 * Every provider a cleanup (or a cleanup profile) can run on. Picking one
 * fills in its base URL; "custom" leaves the field editable. The backend
 * only ever uses the base URL.
 */
export const PP_PROVIDERS = [
  { value: 'ondevice', label: 'Local (on this PC)', baseUrl: null },
  { value: 'groq', label: 'Groq', baseUrl: BASE_URLS.groq },
  { value: 'anthropic', label: 'Anthropic (Claude)', baseUrl: BASE_URLS.anthropic },
  { value: 'openai', label: 'OpenAI', baseUrl: BASE_URLS.openai },
  { value: 'gemini', label: 'Google Gemini', baseUrl: BASE_URLS.gemini },
  { value: 'openrouter', label: 'OpenRouter', baseUrl: BASE_URLS.openrouter },
  { value: 'local', label: 'My own server (Ollama · LM Studio)', baseUrl: BASE_URLS.local },
  { value: 'custom', label: 'Custom', baseUrl: null },
];

/**
 * Where a model runs (OpenWhispr's split: a mode, then the provider's
 * tab): one set of names on every Language Models tab. UI-only — the saved
 * state is the provider id (`modeOf`).
 */
export const PROVIDER_MODES = [
  { value: 'cloud', label: 'Cloud providers', desc: 'Groq, OpenAI, Anthropic, Gemini or OpenRouter, on your own API key', kind: 'cloud' },
  { value: 'ondevice', label: 'Local', desc: 'A model on this PC. Nothing leaves it', kind: 'local' },
  { value: 'selfhosted', label: 'Self-hosted', desc: 'Your own server, like Ollama or LM Studio', kind: 'server' },
];

/** The mode a saved provider id is in. */
export function modeOf(provider) {
  return provider === 'ondevice' ? 'ondevice' : provider === 'local' ? 'selfhosted' : 'cloud';
}

/** The Cloud providers mode's tabs, with their marks. */
export const CLOUD_TABS = [
  { value: 'groq', label: 'Groq', icon: PROVIDER_ICONS.groq },
  { value: 'anthropic', label: 'Anthropic', icon: PROVIDER_ICONS.anthropic, mono: true },
  { value: 'openai', label: 'OpenAI', icon: PROVIDER_ICONS.openai, mono: true },
  { value: 'gemini', label: 'Gemini', icon: PROVIDER_ICONS.gemini },
  { value: 'openrouter', label: 'OpenRouter' },
  { value: 'custom', label: 'Custom' },
];

/** Example model ids per provider, as the Model field's hint. */
export const MODEL_HINTS = {
  groq: 'e.g. openai/gpt-oss-20b',
  anthropic: 'e.g. claude-haiku-4-5',
  openai: 'e.g. gpt-5-mini',
  gemini: 'e.g. gemini-3.5-flash',
  openrouter: 'e.g. meta-llama/llama-3.1-8b-instruct',
  local: 'your Ollama / LM Studio model name (e.g. llama3.1)',
  custom: 'the model id your endpoint expects',
};

/** An API key as it shows until you edit it: "gsk…1a2b". */
export function maskKey(key) {
  return !key ? '' : key.length > 8 ? `${key.slice(0, 3)}…${key.slice(-4)}` : '••••••••';
}

// ---- Names ----

/** Short names: the Chat model's chip and picker (aiModel.js), Settings' link rows. */
export const PROVIDER_LABELS = {
  groq: 'Groq',
  anthropic: 'Anthropic',
  openai: 'OpenAI',
  gemini: 'Gemini',
  openrouter: 'OpenRouter',
  ondevice: 'On this PC',
  local: 'Self-hosted',
  custom: 'Custom',
};

/** The Commands, Note Formatting and Chat tabs' Prompt Studio ("Test with …"). */
export const SCOPE_PROVIDER_LABELS = {
  groq: 'Groq',
  anthropic: 'Anthropic',
  openai: 'OpenAI',
  gemini: 'Gemini',
  openrouter: 'OpenRouter',
  ondevice: 'Local',
  local: 'Self-hosted',
  custom: 'Custom',
};

/** Dictation cleanup's: its Prompt Studio, the missing-key banner, where the text goes. */
export const CLEANUP_PROVIDER_LABELS = {
  groq: 'Groq',
  anthropic: 'Anthropic',
  openai: 'OpenAI',
  gemini: 'Google Gemini',
  openrouter: 'OpenRouter',
  custom: 'Custom endpoint',
  local: 'Self-hosted',
  ondevice: 'Local',
};

/** Where the words go, in a sentence ("go to your own server"): Dictation, Privacy & data. */
export const PROVIDER_DESTINATIONS = {
  groq: 'Groq',
  anthropic: 'Anthropic',
  openai: 'OpenAI',
  gemini: 'Google Gemini',
  openrouter: 'OpenRouter',
  custom: 'your custom endpoint',
  local: 'your own server',
};

// ---- The key store (cfg = Settings' config copy) ----

/** A keyed provider's key: the shared store's, else cleanup's while it's on that provider. */
export function sharedKey(cfg, p) {
  return cfg.ppApiKeys?.[p] || (cfg.ppProvider === p ? cfg.ppApiKey : '') || '';
}

/**
 * Cleanup's active key into the store, on the way out of a provider (the
 * backend only reads the active `ppApiKey`; each provider's stays here).
 */
export function stashCleanupKey(cfg) {
  if (cfg.ppProvider !== 'ondevice') {
    cfg.ppApiKeys = { ...cfg.ppApiKeys, [cfg.ppProvider]: cfg.ppApiKey };
  }
}

/**
 * On load: the active key is authoritative for the active provider (a key
 * edited right before the window closed; configs from before the store).
 * With the store empty and a key saved while on a keyless provider
 * (on-device), it belonged to the last cloud provider used (`cloudProvider`):
 * pre-store configs had ONE key. Never leave a lone key un-stashed — a later
 * provider switch would restore '' over it and the auto-save would keep the
 * wipe.
 */
export function creditCleanupKey(cfg, cloudProvider) {
  if (!cfg.ppApiKeys || typeof cfg.ppApiKeys !== 'object') cfg.ppApiKeys = {};
  if (cfg.ppApiKey) {
    if (cfg.ppProvider !== 'ondevice') cfg.ppApiKeys[cfg.ppProvider] = cfg.ppApiKey;
    else if (Object.keys(cfg.ppApiKeys).length === 0) cfg.ppApiKeys[cloudProvider] = cfg.ppApiKey;
  }
}

/**
 * A scope's keys and the shared store: a keyed provider's key a scope kept
 * locally moves up (never over an existing one), and a scope on a keyed
 * provider without a key takes the store's — a scope pointed at a provider
 * you've already keyed just works.
 */
export function liftScopeKeys(cfg, s) {
  for (const [p, k] of Object.entries(s.apiKeys || {})) {
    if (k && isKeyed(p) && !cfg.ppApiKeys[p]) cfg.ppApiKeys[p] = k;
  }
  if (isKeyed(s.provider)) {
    if (s.apiKey && !cfg.ppApiKeys[s.provider]) cfg.ppApiKeys[s.provider] = s.apiKey;
    else if (!s.apiKey) s.apiKey = cfg.ppApiKeys[s.provider] || '';
  }
}

/**
 * A key saved outside Settings (the Chat model picker's `set_provider_key`):
 * where this copy keeps it too — the store, cleanup's own while it's on that
 * provider, the Chat scope's. True when the Chat scope took it.
 */
export function adoptProviderKey(cfg, p, key) {
  cfg.ppApiKeys = { ...(cfg.ppApiKeys || {}), [p]: key };
  if (cfg.ppProvider === p) cfg.ppApiKey = key;
  const chat = cfg.llmScopes?.chat;
  if (chat && (chat.provider === p || (chat.apiKeys && p in chat.apiKeys))) {
    if (chat.provider === p) chat.apiKey = key;
    if (chat.apiKeys && p in chat.apiKeys) chat.apiKeys = { ...chat.apiKeys, [p]: key };
    return true;
  }
  return false;
}

/** Cleanup is on, on a keyed provider, with no key: the red badge, and its banner. */
export function cleanupNeedsKey(cfg) {
  return !!cfg?.postProcessEnabled && isKeyed(cfg.ppProvider) && !(cfg.ppApiKey || '').trim();
}
