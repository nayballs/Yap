// Which AI models read images: the "Reads images" tag in the Chat model
// picker and Settings' model lists (Snip to Chat, 2026-10-25). Asked of
// Rust (`chat_models_read_images`), so the tag says exactly what Chat goes
// by when it sends a screenshot (snip/vision.rs: a refusal this session,
// then the provider's own list, then the family rules — every Claude and
// Gemini chat model, OpenAI's GPT-4o/4.1/5.x/6 and o-series, ids that say
// "vision"/"vl"; never the on-device model).
//
// `readsImages(provider, model)` answers from what's known — true, false,
// or null (not known yet, or nobody says) — and asks for what isn't, every
// model a picker shows in one call a moment later; the tag appears then.
import { invoke } from '@tauri-apps/api/core';

/** "provider␀model" → true | false | null. */
export const visionTags = $state({ known: {} });

const pending = new Map();
let timer = null;

const keyOf = (provider, model) => `${provider}\u0000${model}`;

async function flush() {
  timer = null;
  const batch = [...pending.entries()];
  pending.clear();
  if (!batch.length) return;
  let got = [];
  try {
    got = (await invoke('chat_models_read_images', { models: batch.map(([, m]) => m) })) ?? [];
  } catch {
    /* not in Tauri: no tags */
  }
  const next = { ...visionTags.known };
  batch.forEach(([k], i) => (next[k] = got[i] ?? null));
  visionTags.known = next;
}

/** Whether `model` on `provider` reads images (null: not known yet, or nobody says). */
export function readsImages(provider, model) {
  if (!provider || !model) return null;
  const k = keyOf(provider, model);
  if (k in visionTags.known) return visionTags.known[k];
  if (!pending.has(k)) {
    pending.set(k, { provider, model });
    timer ??= setTimeout(flush, 0);
  }
  return null;
}

/** Ask again (a picker opening: a provider's list, or a refusal, may have changed it). */
export function forgetVisionTags() {
  visionTags.known = {};
}
