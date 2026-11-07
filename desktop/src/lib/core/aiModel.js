// Naming the AI model a call goes to (ai/ai_model.rs says which provider and
// id; this file says it in words): the Chat view's model chip and its
// picker, and the quiet "GPT-OSS 20B · Groq" line under each answer.
// Short names come from ppModels.js's registry (else the provider's own name
// from its live list, modelCatalog.svelte.js), icons from providerIcons.js.
import { invoke } from '@tauri-apps/api/core';
import { familyIcon, providerIcon } from './modelIcons.js';
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

/** Millionths of a dollar as an answer's line says them: "$0.004", "$0.0004", "under $0.0001". */
function dollars(micros) {
  const usd = (Number(micros) || 0) / 1_000_000;
  if (usd < 0.0001) return 'under $0.0001';
  if (usd < 0.01) return `$${usd.toFixed(4).replace(/0+$/, '')}`;
  return `$${usd.toFixed(2)}`;
}

/**
 * **An answer's model line, with how it was paid** (Chat's byline): Claude
 * on your Claude plan "Claude Haiku 5.5 · Your Claude plan · ≈ $0.004 on the
 * API" (Claude Code's estimate of what the plan saved), on an Anthropic key
 * "Claude Haiku 5.5 · API key · $0.004" (the list price); any other model as
 * `modelLine`. `tokens`: the answer's (`costMicros`, the turn's estimate).
 */
export function paidLine(m, tokens) {
  if (!m) return '';
  const cost = tokens?.costMicros;
  const known = typeof cost === 'number' && cost >= 0;
  if (m.provider === 'claude-plan') {
    return [modelName(m), 'Your Claude plan', known && cost > 0 ? `≈ ${dollars(cost)} on the API` : ''].filter(Boolean).join(' · ');
  }
  if (m.provider === 'anthropic') {
    return [modelName(m), 'API key', known ? dollars(cost) : ''].filter(Boolean).join(' · ');
  }
  return modelLine(m);
}

/**
 * Where the model runs, in one line: "Runs on this PC: nothing leaves it",
 * "Runs in Groq's cloud", "Runs on my-server:8080".
 */
export function whereItRuns(m, host = '') {
  if (!m) return '';
  if (m.provider === 'ondevice') return 'Runs on this PC: nothing leaves it';
  if (m.provider === 'claude-plan') return "Runs in Anthropic's cloud on your Claude plan, through Claude Code on this PC";
  if (m.onThisPc) return host ? `Runs on this PC (${host})` : 'Runs on this PC';
  if (CLOUD_ORDER.includes(m.provider)) return `Runs in ${PROVIDER_LABELS[m.provider]}'s cloud`;
  return host ? `Runs on ${host}` : 'Runs on your own server';
}

// A model family's mark and a provider's (modelIcons.js, pure: Yap Link's
// phone page shares it).
export { familyIcon, providerIcon };

/**
 * The chip's icon id: the provider's mark, else (the on-device model, a
 * self-hosted server) the model family's, else null (Yap's own glyph).
 */
export function modelIcon(m) {
  if (!m) return null;
  return providerIcon(m.provider) ?? familyIcon(m.name, m.id);
}
