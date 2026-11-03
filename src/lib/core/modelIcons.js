// Which mark a provider or a model family gets (the Chat model chip, its
// picker, Settings' model lists), by id: the logos themselves are
// src/assets/providers/<id>.svg, imported as URLs in providerIcons.js. Pure —
// no imports, no Tauri, no Svelte, no assets — because Yap Link's phone page
// loads this file as it is (`src-tauri/build.rs` serves it at
// core/modelIcons.js, the logos at providers/<id>.svg).

/** The marks there are, by id (each `src/assets/providers/<id>.svg`; "whisper" is OpenAI's). */
export const ICON_FILES = {
  openai: 'openai',
  'openai-oss': 'openai-oss',
  nvidia: 'nvidia',
  anthropic: 'anthropic',
  claude: 'claude',
  gemini: 'gemini',
  gemma: 'gemma',
  groq: 'groq',
  llama: 'llama',
  mistral: 'mistral',
  openrouter: 'openrouter',
  qwen: 'qwen',
  xai: 'xai',
  whisper: 'openai',
};

// Logos drawn in solid black that must be inverted on the dark theme.
export const MONOCHROME_PROVIDERS = new Set(['openai', 'openai-oss', 'whisper', 'anthropic', 'xai', 'openrouter']);

// Logos drawn in `currentColor` that carry a brand colour of their own:
// ProviderMark.svelte fills them with it.
export const ICON_TINTS = { claude: '#d97757' };

/** A model family's mark, by its name or id (the most specific first). */
const FAMILY_ICONS = [
  [/gpt[-_ ]?oss/i, 'openai-oss'],
  [/qwen|qwq/i, 'qwen'],
  [/gemma/i, 'gemma'],
  [/gemini/i, 'gemini'],
  [/llama/i, 'llama'],
  [/mistral|mixtral|magistral|ministral|devstral|codestral/i, 'mistral'],
  [/claude/i, 'claude'],
  [/grok/i, 'xai'],
  [/\bgpt|openai/i, 'openai'],
];

/**
 * The icon id of a model's family from its name or id ("GPT-OSS 20B" →
 * "openai-oss", "Gemma 4 31B" → "gemma", "Qwen3.5 4B" → "qwen"), or null:
 * the picker's rows show the model's own mark, the rail and the chip the
 * provider's.
 */
export function familyIcon(...texts) {
  const hay = texts.filter(Boolean).join(' ');
  return FAMILY_ICONS.find(([re]) => re.test(hay))?.[1] ?? null;
}

/** A cloud provider's mark (Anthropic's is Claude's, as the picker's rail shows it). */
export function providerIcon(provider) {
  if (provider === 'anthropic') return 'claude';
  if (provider === 'chatgpt') return 'openai';
  return ICON_FILES[provider] ? provider : null;
}
