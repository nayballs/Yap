// Brand/provider logos, ported from OpenWhispr (MIT — its src/assets/icons/providers).
// Imported as URLs by Vite and rendered via <img src>. Monochrome logos (drawn in
// black) render as-is on the light theme; under [data-yap-theme='dark'] (the
// dark theme, lib/core/appearance.js) they invert to white — OpenWhispr's
// `.icon-monochrome` recipe.
// ProviderMark.svelte draws the monochrome ones as a mask in `currentColor`
// instead, so they follow the text colour on any theme.
//
// openrouter.svg is OpenRouter's mark as Simple Icons publishes it (CC0 1.0,
// https://simpleicons.org; the trademark stays OpenRouter's), the same glyph
// the onboarding's brands/openrouter.svg uses.
import openai from '../../assets/providers/openai.svg';
import openaiOss from '../../assets/providers/openai-oss.svg';
import nvidia from '../../assets/providers/nvidia.svg';
import anthropic from '../../assets/providers/anthropic.svg';
import claude from '../../assets/providers/claude.svg';
import gemini from '../../assets/providers/gemini.svg';
import gemma from '../../assets/providers/gemma.svg';
import groq from '../../assets/providers/groq.svg';
import llama from '../../assets/providers/llama.svg';
import mistral from '../../assets/providers/mistral.svg';
import openrouter from '../../assets/providers/openrouter.svg';
import qwen from '../../assets/providers/qwen.svg';
import xai from '../../assets/providers/xai.svg';

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

// Which marks are monochrome (drawn in black, inverted on the dark theme) or
// tinted (drawn in `currentColor`, filled with a brand colour), and which
// mark a provider or model family gets, live in modelIcons.js (pure: Yap
// Link's phone page loads it, and these same files, as they are).
export { MONOCHROME_PROVIDERS, ICON_TINTS } from './modelIcons.js';

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
