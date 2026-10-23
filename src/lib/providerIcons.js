// Brand/provider logos, ported from OpenWhispr (MIT — src/assets/icons/providers).
// Imported as URLs by Vite and rendered via <img src>. Monochrome logos (drawn in
// black) render as-is on the light theme; under [data-yap-theme='dark'] (the
// dark theme, lib/appearance.js) they invert to white — OpenWhispr's
// `.icon-monochrome` recipe.
// ProviderMark.svelte draws the monochrome ones as a mask in `currentColor`
// instead, so they follow the text colour on any theme.
//
// openrouter.svg is OpenRouter's mark as Simple Icons publishes it (CC0 1.0,
// https://simpleicons.org; the trademark stays OpenRouter's), the same glyph
// the onboarding's brands/openrouter.svg uses.
import openai from '../assets/providers/openai.svg';
import openaiOss from '../assets/providers/openai-oss.svg';
import nvidia from '../assets/providers/nvidia.svg';
import anthropic from '../assets/providers/anthropic.svg';
import claude from '../assets/providers/claude.svg';
import gemini from '../assets/providers/gemini.svg';
import gemma from '../assets/providers/gemma.svg';
import groq from '../assets/providers/groq.svg';
import llama from '../assets/providers/llama.svg';
import mistral from '../assets/providers/mistral.svg';
import openrouter from '../assets/providers/openrouter.svg';
import qwen from '../assets/providers/qwen.svg';
import xai from '../assets/providers/xai.svg';

export const PROVIDER_ICONS = {
  openai,
  'openai-oss': openaiOss,
  nvidia,
  anthropic,
  claude,
  gemini,
  gemma,
  groq,
  llama,
  mistral,
  openrouter,
  qwen,
  xai,
  whisper: openai,
};

// Logos drawn in solid black that must be inverted on the dark theme.
export const MONOCHROME_PROVIDERS = new Set(['openai', 'openai-oss', 'whisper', 'anthropic', 'xai', 'openrouter']);

// Logos drawn in `currentColor` that carry a brand colour of their own:
// ProviderMark.svelte fills them with it.
export const ICON_TINTS = { claude: '#d97757' };

// STT engine family (models.js `engine`) → provider icon id. Engines without a
// truthful brand icon (Cohere, SenseVoice, GigaAM, Moonshine) fall back to a
// generic glyph in the UI.
export const ENGINE_PROVIDER = {
  Parakeet: 'nvidia',
  Canary: 'nvidia',
  Whisper: 'openai',
  // The speaker model (diarization.svelte.js).
  Nemotron: 'nvidia',
};
