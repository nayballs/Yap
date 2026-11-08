// **Yap's crew**: who a model is when Yap draws it as a little pixel character
// (the Agent panel's rows: views/chat/AgentPanel.svelte; the drawings
// themselves in views/chat/crew/sprites.js). One character a model family, all
// Yap's own — the Claude models are Yap's speech bubble in four builds, the
// others creatures of their own, never a company's mascot or logo shape —
// and what each row's character holds: its Mode's prop. Pure (no Svelte, no
// Tauri, no assets), so any surface — the phone one day — can ask who a model
// is: `characterFor(provider, model)`.
//
// The idea of a character a row, walking while it works with a prop that
// moves, follows savvy-progress's agents panel (MIT,
// github.com/JohnnyVizz/claude-kit); the characters are Yap's own.

/** **The crew**, by id: its name (what an avatar's label says). */
export const CREW = {
  haiku: { name: 'Haiku' },
  sonnet: { name: 'Sonnet' },
  opus: { name: 'Opus' },
  fable: { name: 'Fable' },
  gpt: { name: 'GPT' },
  gemini: { name: 'Gemini' },
  grok: { name: 'Grok' },
  llama: { name: 'Llama' },
  qwen: { name: 'Qwen' },
  mistral: { name: 'Mistral' },
  deepseek: { name: 'DeepSeek' },
  local: { name: 'On-device model' },
  generic: { name: 'Model' },
};

/** The Claude models' characters, by the word in their id (the most specific first). */
const CLAUDE = [
  [/haiku/, 'haiku'],
  [/opus/, 'opus'],
  [/fable/, 'fable'],
  [/sonnet/, 'sonnet'],
];

/** Every other family, by its model ids' words (as core/modelIcons.js tells the marks apart). */
const FAMILIES = [
  [/deepseek/, 'deepseek'],
  [/gpt[-_ ]?oss|\bgpt|chatgpt|openai|(^|[/:_ -])o[1-9](\b|-|$)/, 'gpt'],
  [/gemini|gemma/, 'gemini'],
  [/grok/, 'grok'],
  [/llama/, 'llama'],
  [/qwen|qwq/, 'qwen'],
  [/mistral|mixtral|magistral|ministral|devstral|codestral/, 'mistral'],
];

/** A provider's character when its model's id names no family. */
const PROVIDERS = {
  anthropic: 'sonnet',
  'claude-plan': 'sonnet',
  openai: 'gpt',
  chatgpt: 'gpt',
  gemini: 'gemini',
  xai: 'grok',
  mistral: 'mistral',
  deepseek: 'deepseek',
  ondevice: 'local',
};

/**
 * **Who a model is**: its character's id. The model on this PC (provider
 * "ondevice") is always the on-device one, whatever its family; a Claude
 * model by its tier ("claude-opus-5-5" → opus), Sonnet when the id says no
 * tier; any other by its family's words ("gpt-5-mini" → gpt, "o4-mini" →
 * gpt, "gemma-4-31b" → gemini, "llama3.1" → llama); else its provider's;
 * else the generic one.
 */
export function characterFor(provider, model) {
  const p = String(provider || '').toLowerCase();
  const id = String(model || '').toLowerCase();
  if (p === 'ondevice') return 'local';
  if (id.includes('claude') || p === 'anthropic' || p === 'claude-plan') {
    return CLAUDE.find(([re]) => re.test(id))?.[1] ?? 'sonnet';
  }
  const family = FAMILIES.find(([re]) => re.test(id))?.[1];
  if (family) return family;
  const bare = CLAUDE.find(([re]) => re.test(id))?.[1];
  if (bare) return bare;
  return PROVIDERS[p] ?? 'generic';
}

/** A character's name ("Opus"); the generic one's for an id there isn't. */
export const crewName = (id) => (CREW[id] ?? CREW.generic).name;

/**
 * **The prop a Mode carries**: plan (a magnifying glass), auto (a racing
 * flag), manual (a wrench), edits (a pencil), bypass (a space helmet) — the
 * Mode's tone's own word (core/agentPanel.js `modeTone`).
 */
export function propOf(mode) {
  if (mode === 'bypassPermissions' || mode === 'bypass') return 'bypass';
  if (mode === 'acceptEdits' || mode === 'edits') return 'edits';
  if (mode === 'plan' || mode === 'manual') return mode;
  return 'auto';
}

/** The poses: working (walks, its prop moving), needs (stops, looks up, "!"), done (sits, ✓), stopped (grey), failed (a red badge), queued (faint). */
export const POSES = ['working', 'needs', 'done', 'stopped', 'failed', 'queued'];

const POSE_WORDS = { working: 'working', needs: 'needs you', done: 'done', stopped: 'stopped', failed: "didn't finish", queued: 'queued' };

/** **An avatar's label**: "Opus, working, Auto mode". */
export function crewLabel(character, pose, modeWord) {
  const parts = [crewName(character), POSE_WORDS[pose] ?? POSE_WORDS.working];
  if (modeWord) parts.push(`${modeWord} mode`);
  return parts.join(', ');
}
