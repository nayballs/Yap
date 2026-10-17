// Naming the AI model a call goes to (ai_model.rs says which provider and
// id; this file says it in words): the Chat view's model chip, the quiet
// "GPT-OSS 20B · Groq" line under each answer, and the chip's switch list.
// Short names come from ppModels.js's registry, icons from providerIcons.js.
import { invoke } from '@tauri-apps/api/core';
import { PP_CLOUD_MODELS } from './ppModels.js';
import { PROVIDER_ICONS, MONOCHROME_PROVIDERS } from './providerIcons.js';

/**
 * The model a scope's next call goes to ("chat": the Chat scope, as
 * note_ask uses it; "notes": the meeting helpers' Note Formatting scope),
 * or null. The Ask bars ask alongside their call and keep it with the answer.
 */
export function nextModel(scope) {
  return invoke('ai_model_status', { scope })
    .then((s) => (s?.ready ? s.model : null))
    .catch(() => null);
}

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

/** Cloud providers, in Settings' order (the switch lists them so). */
const CLOUD_ORDER = ['groq', 'anthropic', 'openai', 'gemini', 'openrouter'];

/**
 * The model's short name: the on-device model's own, the registry's label
 * ("GPT-OSS 20B"), else the id as the endpoint knows it.
 * `m` = `{ provider, id, name?, onThisPc? }` (chats.rs `ChatModel`).
 */
export function modelName(m) {
  if (!m) return '';
  if (m.name) return m.name;
  const own = PP_CLOUD_MODELS[m.provider]?.models.find((x) => x.value === m.id);
  return own?.label || m.id || 'Unknown model';
}

/** "Groq", "On this PC" (the on-device model, or a server on localhost)… */
export function providerName(m) {
  if (!m) return '';
  // A self-hosted or custom server on localhost (Ollama, LM Studio…).
  if ((m.provider === 'local' || m.provider === 'custom') && m.onThisPc) return 'On this PC';
  return PROVIDER_LABELS[m.provider] || m.provider || '';
}

/** "GPT-OSS 20B · Groq" */
export function modelLine(m) {
  if (!m) return '';
  return [modelName(m), providerName(m)].filter(Boolean).join(' · ');
}

/**
 * Where the model runs, in one line: "Runs on this PC: nothing leaves it",
 * "Runs in Groq's cloud", "Runs on my-server:8080".
 */
export function whereItRuns(m, host = '') {
  if (!m) return '';
  if (m.provider === 'ondevice') return 'Runs on this PC: nothing leaves it';
  if (m.onThisPc) return host ? `Runs on this PC (${host})` : 'Runs on this PC';
  if (CLOUD_ORDER.includes(m.provider)) return `Runs in ${PROVIDER_LABELS[m.provider]}'s cloud`;
  return host ? `Runs on ${host}` : 'Runs on your own server';
}

/** Brand icons for a model family, when the provider has none of its own. */
const FAMILY_ICONS = [
  [/qwen/i, 'qwen'],
  [/llama/i, 'llama'],
  [/mistral|mixtral/i, 'mistral'],
  [/gemma|gemini/i, 'gemini'],
  [/claude/i, 'claude'],
  [/gpt|openai/i, 'openai'],
];

/** `{ id, src, mono }` for the provider's icon (or the model family's), or null. */
export function modelIcon(m) {
  if (!m) return null;
  let id = PROVIDER_ICONS[m.provider] ? m.provider : null;
  // OpenRouter routes to many families; name none of them by its logo.
  if (!id && m.provider !== 'openrouter') {
    const hay = `${m.name || ''} ${m.id || ''}`;
    id = FAMILY_ICONS.find(([re]) => re.test(hay))?.[1] ?? null;
  }
  return id ? { id, src: PROVIDER_ICONS[id], mono: MONOCHROME_PROVIDERS.has(id) } : null;
}

/**
 * The chip's switch: the registry's models of every provider with a key,
 * then the on-device model when it's installed; the current model ticked
 * (and listed first when it's none of those, a custom or self-hosted one).
 * Each item: `{ key, provider, model, label, tools, current }`.
 */
export function switchGroups(status) {
  const current = status?.ready ? status.model : null;
  const isCurrent = (provider, model) =>
    !!current && current.provider === provider && (provider === 'ondevice' || current.id === model);
  const groups = [];
  for (const p of CLOUD_ORDER) {
    if (!status?.keyed?.includes(p)) continue;
    const reg = PP_CLOUD_MODELS[p]?.models ?? [];
    const items = reg.map((m) => ({
      key: `${p}:${m.value}`,
      provider: p,
      model: m.value,
      label: m.label,
      tools: true,
      current: isCurrent(p, m.value),
    }));
    // The current model of this provider, when the registry doesn't list it.
    if (current?.provider === p && !reg.some((m) => m.value === current.id)) {
      items.unshift({ key: `${p}:${current.id}`, provider: p, model: current.id, label: current.id, tools: true, current: true });
    }
    groups.push({ id: p, label: PROVIDER_LABELS[p], items });
  }
  if (status?.onDevice) {
    groups.push({
      id: 'ondevice',
      label: 'On this PC',
      items: [
        {
          key: 'ondevice',
          provider: 'ondevice',
          model: '',
          label: status.onDevice.name,
          tools: status.onDevice.tools,
          current: isCurrent('ondevice', ''),
        },
      ],
    });
  }
  if (current && !groups.some((g) => g.items.some((i) => i.current))) {
    groups.unshift({
      id: 'current',
      label: providerName(current),
      items: [
        {
          key: `current:${current.provider}:${current.id}`,
          provider: current.provider,
          model: current.id,
          label: modelName(current),
          tools: status.tools,
          current: true,
          fixed: true,
        },
      ],
    });
  }
  return groups;
}
