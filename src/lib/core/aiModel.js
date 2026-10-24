// Naming the AI model a call goes to (ai/ai_model.rs says which provider and
// id; this file says it in words): the Chat view's model chip and its
// picker, and the quiet "GPT-OSS 20B · Groq" line under each answer.
// Short names come from ppModels.js's registry (else the provider's own name
// from its live list, modelCatalog.svelte.js), icons from providerIcons.js.
import { invoke } from '@tauri-apps/api/core';
import { PROVIDER_ICONS } from './providerIcons.js';
import { modelsFor, modelLabel } from '../stores/modelCatalog.svelte.js';
import { PROVIDER_LABELS, KEYED_PROVIDERS as CLOUD_ORDER } from './providers.js';

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

// The providers' short names (PROVIDER_LABELS) and the cloud ones, in
// Settings' order (CLOUD_ORDER), are providers.js's.

/**
 * The model's short name: the on-device model's own, the registry's label
 * ("GPT-OSS 20B"), the provider's own name for it, else the id as the
 * endpoint knows it.
 * `m` = `{ provider, id, name?, onThisPc? }` (chat/chats.rs `ChatModel`).
 */
export function modelName(m) {
  if (!m) return '';
  if (m.name) return m.name;
  return modelLabel(m.provider, m.id) || 'Unknown model';
}

/**
 * The saved model of a cloud provider that its live list no longer has
 * (`{ value, label }`), or null: the chip says "no longer offered".
 */
export function noLongerOffered(m) {
  if (!m || !CLOUD_ORDER.includes(m.provider)) return null;
  return modelsFor(m.provider, m.id).missing;
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
  return PROVIDER_ICONS[provider] ? provider : null;
}

/**
 * The chip's icon id: the provider's mark, else (the on-device model, a
 * self-hosted server) the model family's, else null (Yap's own glyph).
 */
export function modelIcon(m) {
  if (!m) return null;
  return providerIcon(m.provider) ?? familyIcon(m.name, m.id);
}
