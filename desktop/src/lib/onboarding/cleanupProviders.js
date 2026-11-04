// The brands onboarding draws (superwhisper-style coloured squares), and the
// cloud providers its AI cleanup step offers.
import brandAnthropic from '../../assets/brands/anthropic.svg';
import brandOpenai from '../../assets/brands/openai.svg';
import brandMeta from '../../assets/brands/meta.svg';
import brandGoogle from '../../assets/brands/google.svg';
import brandQwen from '../../assets/brands/qwen.svg';
import brandOllama from '../../assets/brands/ollama.svg';
import brandOpenrouter from '../../assets/brands/openrouter.svg';

// Brand badges (superwhisper-style coloured squares). `icon` = white SVG
// (simple-icons, CC0); brands without one fall back to a letter badge.
export const BRANDS = {
  anthropic: { icon: brandAnthropic, color: '#D97757' },
  openai: { icon: brandOpenai, color: '#10A37F' },
  meta: { icon: brandMeta, color: '#0866FF' },
  google: { icon: brandGoogle, color: '#4285F4' },
  qwen: { icon: brandQwen, color: '#615CED' },
  ollama: { icon: brandOllama, color: '#374151' },
  openrouter: { icon: brandOpenrouter, color: '#6467F2' },
  microsoft: { letter: 'P', color: '#0078D4' }, // Phi
  groq: { letter: 'G', color: '#F55036' },
  custom: { letter: '⚙', color: '#4b5563' },
};

/** Which brand a curated local model belongs to, by id prefix. */
export function brandForModel(id) {
  if (id.startsWith('llama')) return BRANDS.meta;
  if (id.startsWith('qwen')) return BRANDS.qwen;
  if (id.startsWith('gemma')) return BRANDS.google;
  if (id.startsWith('phi')) return BRANDS.microsoft;
  return BRANDS.custom;
}

// Cloud (bring-your-own-key) fields. Base URLs/default models mirror the
// Settings provider presets.
// Anthropic works through its OpenAI-compatible /v1/chat/completions layer,
// so it plugs into Yap's existing cleanup client like any other provider.
export const CLOUD_PROVIDERS = [
  { value: 'groq', label: 'Groq (free tier)', brand: BRANDS.groq, baseUrl: 'https://api.groq.com/openai/v1', model: 'openai/gpt-oss-20b' },
  { value: 'anthropic', label: 'Anthropic (Claude)', brand: BRANDS.anthropic, baseUrl: 'https://api.anthropic.com/v1', model: 'claude-haiku-4-5' },
  { value: 'openai', label: 'OpenAI', brand: BRANDS.openai, baseUrl: 'https://api.openai.com/v1', model: 'gpt-4o-mini' },
  { value: 'openrouter', label: 'OpenRouter', brand: BRANDS.openrouter, baseUrl: 'https://openrouter.ai/api/v1', model: 'meta-llama/llama-3.1-8b-instruct' },
  { value: 'local', label: 'My own server (Ollama · LM Studio)', brand: BRANDS.ollama, baseUrl: 'http://localhost:11434/v1', model: 'llama3.1' },
  { value: 'custom', label: 'Custom endpoint', brand: BRANDS.custom, baseUrl: '', model: '' },
];

/** "820 MB", "2.5 GB". */
export function fmtSize(mb) {
  return mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${mb} MB`;
}
