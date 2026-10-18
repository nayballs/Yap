// Cleanup ("language model") registry per cloud provider — model lists ported
// from OpenWhispr's modelRegistryData.json cloudProviders (MIT), trimmed to
// Yap's provider set (+ OpenRouter, which OpenWhispr doesn't ship). The FIRST
// model in each list is the default auto-selected when the user switches to
// that provider (OpenWhispr wiring); order the lists best-cleanup-first, not
// biggest-first — dictation cleanup wants fast + cheap.
//
// This is Yap's CURATED list: the recommended order and wording. What each
// provider really offers comes from the provider itself (model_catalog.rs,
// asked with the key the person saved) and is merged in by modelCatalog.js
// for every picker: a curated model the provider no longer lists is hidden
// (and the next one becomes the default), and one it lists that isn't here
// shows after these, marked "New" when it came out after CURATED_AT. With
// no key, offline, or in a test run, exactly this list shows.
//
// Reviewing it: check each provider's docs, update the lists, set
// CURATED_AT to the day, and refresh `seen` for providers whose model list
// carries no dates (Gemini) from a live list.

/** The day these lists were last checked against the providers (UTC). */
export const CURATED_AT = '2026-10-18';

export const PP_CLOUD_MODELS = {
  groq: {
    keyUrl: 'https://console.groq.com/keys',
    // Groq retired its Llama/Qwen/Kimi/Compound models for non-Enterprise
    // keys in 2026; config.rs migrates saved selections to the GPT-OSS two.
    models: [
      { value: 'openai/gpt-oss-20b', label: 'GPT-OSS 20B', desc: 'Fast open-source model, 1000 T/sec' },
      { value: 'openai/gpt-oss-120b', label: 'GPT-OSS 120B', desc: "OpenAI's open-source flagship, 500 T/sec" },
      { value: 'qwen/qwen3.8-27b', label: 'Qwen3.8 27B', desc: 'Preview: reasoning model that also reads images, 450 T/sec' },
    ],
  },
  anthropic: {
    keyUrl: 'https://console.anthropic.com/settings/keys',
    // Anthropic lists older models by their dated ids
    // (claude-haiku-4-5-20251001); the aliases here still match them.
    models: [
      { value: 'claude-haiku-4-5', label: 'Claude Haiku 4.5', desc: 'Fast with near-frontier intelligence' },
      { value: 'claude-haiku-5-5', label: 'Claude Haiku 5.5', desc: 'Newest Haiku: fastest and lowest-cost Claude' },
      { value: 'claude-sonnet-5-5', label: 'Claude Sonnet 5.5', desc: 'Newest Sonnet: balanced speed and intelligence' },
      { value: 'claude-opus-5-5', label: 'Claude Opus 5.5', desc: 'Newest Opus: the most capable Claude for most work' },
      { value: 'claude-fable-5-1', label: 'Claude Fable 5.1', desc: 'For the hardest reasoning and long tasks; slower' },
      { value: 'claude-sonnet-4-6', label: 'Claude Sonnet 4.6', desc: 'Balanced performance' },
      { value: 'claude-opus-4-8', label: 'Claude Opus 4.8', desc: 'Previous Opus generation, 1M context' },
      { value: 'claude-opus-4-7', label: 'Claude Opus 4.7', desc: 'Powerful Opus model, 1M context' },
      { value: 'claude-opus-4-6', label: 'Claude Opus 4.6', desc: 'Previous Opus generation, 1M context' },
      { value: 'claude-opus-4-5', label: 'Claude Opus 4.5', desc: 'Earlier Opus model' },
    ],
  },
  openai: {
    keyUrl: 'https://platform.openai.com/api-keys',
    // GPT-5 Mini is first no longer: OpenAI shuts it down on 11 Dec 2026
    // (its deprecations page). GPT-5.6 Luna costs about the same.
    models: [
      { value: 'gpt-5.6-luna', label: 'GPT-5.6 Luna', desc: 'Fast and low-cost, for high-volume tasks' },
      { value: 'gpt-5-mini', label: 'GPT-5 Mini', desc: 'Fast and cost-efficient; OpenAI retires it on 11 Dec 2026' },
      { value: 'gpt-5.6-terra', label: 'GPT-5.6 Terra', desc: 'Balanced intelligence and cost' },
      { value: 'gpt-5.6-sol', label: 'GPT-5.6 Sol', desc: 'Strong reasoning for complex work' },
      { value: 'gpt-6-luna', label: 'GPT-6 Luna', desc: 'Newest generation at the lowest cost, 1M context' },
      { value: 'gpt-6.1-sol', label: 'GPT-6.1 Sol', desc: 'Newest generation, balanced, 1M context' },
      { value: 'gpt-6-astra', label: 'GPT-6 Astra', desc: "OpenAI's most capable model, 1M context" },
      { value: 'gpt-5.5', label: 'GPT-5.5', desc: 'Frontier model for complex reasoning, 1M context' },
      { value: 'gpt-5.4-mini', label: 'GPT-5.4 Mini', desc: 'Smaller, faster GPT-5.4' },
    ],
  },
  gemini: {
    keyUrl: 'https://aistudio.google.com/app/api-keys',
    // Gemini 3.5 Flash was first until Google deprecated it (8 Oct 2026); its
    // calls now go to Gemini 3.6 Flash, which leads instead.
    models: [
      { value: 'gemini-3.6-flash', label: 'Gemini 3.6 Flash', desc: 'Fast everyday Flash (Gemini 3.5 Flash now runs on it)' },
      { value: 'gemini-3.8-flash', label: 'Gemini 3.8 Flash', desc: 'Newest Flash, strongest on long multi-step tasks' },
      { value: 'gemini-4-argon', label: 'Gemini 4 Argon', desc: "Google's newest flagship: deep reasoning, 1M context" },
      { value: 'gemini-3.5-flash-lite', label: 'Gemini 3.5 Flash-Lite', desc: 'Lowest latency and cost' },
      { value: 'gemini-3.1-flash-lite', label: 'Gemini 3.1 Flash-Lite', desc: 'Low cost, near-frontier quality' },
      { value: 'gemini-3.1-pro-preview', label: 'Gemini 3.1 Pro', desc: 'Next-gen flagship model for complex reasoning' },
      { value: 'gemini-3-flash-preview', label: 'Gemini 3 Flash', desc: 'Ultra-fast, high-capability next-gen model' },
      { value: 'gemini-3.5-flash', label: 'Gemini 3.5 Flash', desc: 'Deprecated by Google: it now answers with Gemini 3.6 Flash' },
      { value: 'gemma-4-31b-it', label: 'Gemma 4 31B', desc: "Google's largest Gemma 4, 31B dense, 256K context" },
      { value: 'gemma-4-26b-a4b-it', label: 'Gemma 4 26B MoE', desc: "Google's Gemma 4 MoE, 4B active params, 256K context" },
    ],
    // Google's list gives no release dates, so "New" can't come from a date:
    // these are the chat models it listed on CURATED_AT besides the curated
    // ones; anything else it lists later is new.
    seen: [
      'gemini-2.5-flash',
      'gemini-2.5-pro',
      'gemini-2.5-flash-lite',
      'gemini-3.1-flash-lite-preview',
      'gemini-3.7-flash',
    ],
  },
  openrouter: {
    keyUrl: 'https://openrouter.ai/settings/keys',
    models: [
      { value: 'openai/gpt-5-mini', label: 'GPT-5 Mini', desc: 'Fast and cost-efficient' },
      { value: 'anthropic/claude-haiku-4.5', label: 'Claude Haiku 4.5', desc: 'Fast with near-frontier intelligence' },
      { value: 'meta-llama/llama-3.3-70b-instruct', label: 'LLaMA 3.3 70B', desc: "Meta's versatile open model" },
      { value: 'openai/gpt-oss-20b', label: 'GPT-OSS 20B', desc: 'Fast open-source model' },
      { value: 'qwen/qwen3-32b', label: 'Qwen3 32B', desc: 'Powerful open reasoning model' },
    ],
  },
};

// Reasoning models that emit "thinking" tokens by default. Drives the "Disable
// thinking output" toggle's visibility (like OpenWhispr's supportsThinking flag);
// when the toggle is on the backend strips <think>…</think> blocks from the
// output. Keyed by the exact model id used in PP_CLOUD_MODELS. A model the
// provider's own list says reasons (modelCatalog.js) counts too.
export const PP_THINKING_MODELS = new Set([
  // Groq
  'openai/gpt-oss-20b',
  'openai/gpt-oss-120b',
  'qwen/qwen3.8-27b',
  // OpenAI
  'gpt-5.6-luna',
  'gpt-5.6-terra',
  'gpt-5.6-sol',
  'gpt-6-luna',
  'gpt-6.1-sol',
  'gpt-6-astra',
  'gpt-5.5',
  'gpt-5.4-mini',
  // Gemini
  'gemini-3.6-flash',
  'gemini-3.8-flash',
  'gemini-4-argon',
  'gemini-3.5-flash-lite',
  'gemini-3.1-flash-lite',
  'gemini-3.5-flash',
  'gemini-3.1-pro-preview',
  'gemini-3-flash-preview',
  // OpenRouter
  'openai/gpt-oss-20b',
  'qwen/qwen3-32b',
]);

/** Whether a model id emits thinking tokens by default (the registry's say). */
export function modelThinks(modelId) {
  return PP_THINKING_MODELS.has(modelId);
}
