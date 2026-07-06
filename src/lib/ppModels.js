// Cleanup ("language model") registry per cloud provider — model lists ported
// from OpenWhispr's modelRegistryData.json cloudProviders (MIT), trimmed to
// Yap's provider set (+ OpenRouter, which OpenWhispr doesn't ship). The FIRST
// model in each list is the default auto-selected when the user switches to
// that provider (OpenWhispr wiring); order the lists best-cleanup-first, not
// biggest-first — dictation cleanup wants fast + cheap.
export const PP_CLOUD_MODELS = {
  groq: {
    keyUrl: 'https://console.groq.com/keys',
    models: [
      { value: 'llama-3.1-8b-instant', label: 'LLaMA 3.1 8B', desc: 'Ultra-fast 560 T/sec, 131K context' },
      { value: 'openai/gpt-oss-20b', label: 'GPT-OSS 20B', desc: 'Fast open-source model, 1000 T/sec' },
      { value: 'openai/gpt-oss-120b', label: 'GPT-OSS 120B', desc: "OpenAI's open-source flagship, 500 T/sec" },
      { value: 'llama-3.3-70b-versatile', label: 'LLaMA 3.3 70B', desc: "Meta's versatile model, 280 T/sec" },
      { value: 'qwen/qwen3-32b', label: 'Qwen3 32B', desc: 'Powerful reasoning model, 131K context' },
      { value: 'meta-llama/llama-4-scout-17b-16e-instruct', label: 'Llama 4 Scout', desc: "Meta's efficient multimodal, 750 T/sec" },
      { value: 'moonshotai/kimi-k2-instruct-0905', label: 'Kimi K2 0905', desc: "Moonshot AI's 1T MoE, 256K context" },
      { value: 'groq/compound', label: 'Compound', desc: "Groq's compound system, 450 T/sec" },
      { value: 'groq/compound-mini', label: 'Compound Mini', desc: 'Fast compound system, 3x lower latency' },
    ],
  },
  anthropic: {
    keyUrl: 'https://console.anthropic.com/settings/keys',
    models: [
      { value: 'claude-haiku-4-5', label: 'Claude Haiku 4.5', desc: 'Fast with near-frontier intelligence' },
      { value: 'claude-sonnet-4-6', label: 'Claude Sonnet 4.6', desc: 'Balanced performance' },
      { value: 'claude-opus-4-8', label: 'Claude Opus 4.8', desc: 'Most capable Claude model, 1M context' },
      { value: 'claude-sonnet-4-5', label: 'Claude Sonnet 4.5', desc: 'Previous Sonnet generation' },
      { value: 'claude-opus-4-7', label: 'Claude Opus 4.7', desc: 'Powerful Opus model, 1M context' },
      { value: 'claude-opus-4-6', label: 'Claude Opus 4.6', desc: 'Previous Opus generation, 1M context' },
      { value: 'claude-opus-4-5', label: 'Claude Opus 4.5', desc: 'Earlier Opus model' },
    ],
  },
  openai: {
    keyUrl: 'https://platform.openai.com/api-keys',
    models: [
      { value: 'gpt-5-mini', label: 'GPT-5 Mini', desc: 'Fast and cost-efficient' },
      { value: 'gpt-5-nano', label: 'GPT-5 Nano', desc: 'Ultra-fast, low latency' },
      { value: 'gpt-5.5', label: 'GPT-5.5', desc: 'Frontier model for complex reasoning, 1M context' },
      { value: 'gpt-5.2', label: 'GPT-5.2', desc: 'Strong reasoning model' },
      { value: 'gpt-4.1', label: 'GPT-4.1', desc: 'Strong baseline, 1M context' },
      { value: 'gpt-4.1-mini', label: 'GPT-4.1 Mini', desc: 'Smaller GPT-4.1 model' },
      { value: 'gpt-4.1-nano', label: 'GPT-4.1 Nano', desc: 'Lowest latency GPT-4.1' },
    ],
  },
  gemini: {
    keyUrl: 'https://aistudio.google.com/app/api-keys',
    models: [
      { value: 'gemini-3.5-flash', label: 'Gemini 3.5 Flash', desc: 'Latest fast, high-capability Gemini model' },
      { value: 'gemini-3.1-pro-preview', label: 'Gemini 3.1 Pro', desc: 'Next-gen flagship model for complex reasoning' },
      { value: 'gemini-3-flash-preview', label: 'Gemini 3 Flash', desc: 'Ultra-fast, high-capability next-gen model' },
      { value: 'gemini-2.5-flash-lite', label: 'Gemini 2.5 Flash Lite', desc: 'Lowest latency and cost' },
      { value: 'gemma-4-31b-it', label: 'Gemma 4 31B', desc: "Google's largest Gemma 4, 31B dense, 256K context" },
      { value: 'gemma-4-26b-a4b-it', label: 'Gemma 4 26B MoE', desc: "Google's Gemma 4 MoE, 4B active params, 256K context" },
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
// output. Keyed by the exact model id used in PP_CLOUD_MODELS.
export const PP_THINKING_MODELS = new Set([
  // Groq
  'qwen/qwen3-32b',
  'openai/gpt-oss-20b',
  'openai/gpt-oss-120b',
  // OpenAI
  'gpt-5.5',
  'gpt-5.2',
  // Gemini
  'gemini-3.5-flash',
  'gemini-3.1-pro-preview',
  'gemini-3-flash-preview',
  // OpenRouter
  'openai/gpt-oss-20b',
  'qwen/qwen3-32b',
]);

/** Whether a model id emits thinking tokens by default. */
export function modelThinks(modelId) {
  return PP_THINKING_MODELS.has(modelId);
}
