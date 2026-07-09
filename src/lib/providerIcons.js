// Brand/provider logos, ported from OpenWhispr (MIT — src/assets/icons/providers).
// Imported as URLs by Vite and rendered via <img src>. Monochrome logos (drawn in
// black) render as-is on the light theme; under a [data-yap-theme='dark'] scope
// (currently unused) they invert to white — OpenWhispr's `.icon-monochrome` recipe.
import openai from '../assets/providers/openai.svg';
import nvidia from '../assets/providers/nvidia.svg';
import anthropic from '../assets/providers/anthropic.svg';
import claude from '../assets/providers/claude.svg';
import gemini from '../assets/providers/gemini.svg';
import groq from '../assets/providers/groq.svg';
import llama from '../assets/providers/llama.svg';
import mistral from '../assets/providers/mistral.svg';
import qwen from '../assets/providers/qwen.svg';
import xai from '../assets/providers/xai.svg';

export const PROVIDER_ICONS = {
  openai,
  nvidia,
  anthropic,
  claude,
  gemini,
  groq,
  llama,
  mistral,
  qwen,
  xai,
  whisper: openai,
};

// Logos drawn in solid black that must be inverted on the dark theme.
export const MONOCHROME_PROVIDERS = new Set(['openai', 'whisper', 'anthropic', 'xai']);

// STT engine family (models.js `engine`) → provider icon id. Engines without a
// truthful brand icon (Cohere, SenseVoice, GigaAM, Moonshine) fall back to a
// generic glyph in the UI.
export const ENGINE_PROVIDER = {
  Parakeet: 'nvidia',
  Canary: 'nvidia',
  Whisper: 'openai',
};

export function getProviderIcon(id) {
  return PROVIDER_ICONS[id];
}
