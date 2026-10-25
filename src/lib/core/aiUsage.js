// The AI usage tracker in words (ai/usage.rs): Chat's context ring and its
// panel (views/chat/ContextRing.svelte, ContextPanel.svelte) and Insights'
// AI usage card (views/insights/AiUsageCard.svelte). Counts only: what each
// AI answer said it took, counted on this PC.
import { modelName, providerName } from './aiModel.js';
import { fmtTokens as fmtContext } from './localContext.js';

export { fmtContext };

/** What each feature is called (ai/usage.rs `Feature::id`). */
export const FEATURE_LABELS = {
  cleanup: 'AI cleanup',
  chat: 'Chat',
  assistant: 'Voice Assistant',
  commands: 'Commands',
  transforms: 'Transforms',
  translation: 'Translation',
  meetings: 'Meeting notes',
  notes: 'Notes',
  memory: 'Memory',
  speakers: 'Speaker names',
  brief: 'Meeting briefs',
  search: 'Search by meaning',
  other: 'Other',
};

export function featureLabel(id) {
  return FEATURE_LABELS[id] || id || 'Other';
}

/** A count of tokens: "840", "1.2K", "12.3K", "123K", "1.2M". */
export function fmtCount(n) {
  const v = Math.max(0, Math.round(Number(n) || 0));
  if (v < 1000) return String(v);
  if (v < 100_000) return `${(v / 1000).toFixed(1).replace(/\.0$/, '')}K`;
  if (v < 1_000_000) return `${Math.round(v / 1000)}K`;
  return `${(v / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`;
}

/** "1 token", "1,240 tokens". */
export function tokensWord(n) {
  const v = Math.round(Number(n) || 0);
  return `${v.toLocaleString()} ${v === 1 ? 'token' : 'tokens'}`;
}

/** An estimate in millionths of a dollar: "$0.42", "$0.0021", "under $0.0001". */
export function fmtCost(micros) {
  const usd = (Number(micros) || 0) / 1_000_000;
  if (usd === 0) return '$0.00';
  if (usd < 0.0001) return 'under $0.0001';
  if (usd < 0.01) return `$${usd.toFixed(4).replace(/0+$/, '')}`;
  if (usd < 100) return `$${usd.toFixed(2)}`;
  return `$${Math.round(usd).toLocaleString()}`;
}

/** An estimate as said: "≈ $0.42", "under $0.0001". */
export function aboutCost(micros) {
  const c = fmtCost(micros);
  return c.startsWith('under') ? c : `≈ ${c}`;
}

/** The share of the context used, 0–100+ (whole numbers). */
export function percentOf(used, context) {
  const c = Number(context) || 0;
  if (c <= 0) return 0;
  return Math.round(((Number(used) || 0) / c) * 100);
}

/** The ring's colour: amber, warmer as it fills (tokens only). */
export function ringColor(pct) {
  if (pct >= 95) return 'var(--yap-danger)';
  if (pct >= 80) return 'color-mix(in srgb, var(--yap-danger) 55%, var(--yap-primary))';
  return 'var(--yap-primary)';
}

/** "38% of context used" (an estimate: "About 38%…"). */
export function ringLabel(use) {
  if (!use?.context) return '';
  const pct = percentOf(use.used, use.context);
  return `${use.estimate || !use.contextKnown ? 'About ' : ''}${pct}% of context used`;
}

/** A model row's name and where it ran: { name, where }. */
export function modelRow(m) {
  const model = { provider: m.provider, id: m.model, name: m.name || '', onThisPc: !!m.here };
  const name = modelName(model);
  return { name: name === 'Unknown model' ? m.model : name, where: providerName(model) };
}

/** What a model's tokens cost, as a row says it. */
export function costLine(m) {
  if (m.here) return 'free, on your PC';
  if (m.costMicros == null) return 'no price known';
  return aboutCost(m.costMicros);
}

/**
 * A stretch's split: "4.4K on this PC · 1.2K cloud · ≈ $0.01", "1.2K cloud",
 * "free, on your PC" (all on this PC), "none yet".
 */
export function splitLine(t) {
  if (!t?.calls) return 'none yet';
  if (!t.cloud) return 'free, on your PC';
  return [t.here ? `${fmtCount(t.here)} on this PC` : '', `${fmtCount(t.cloud)} cloud`, t.costMicros > 0 ? aboutCost(t.costMicros) : '']
    .filter(Boolean)
    .join(' · ');
}

/** "2026-10-25" as the person reads a date ("25 Oct 2026"). */
export function fmtDay(day) {
  const [y, m, d] = String(day || '').split('-').map(Number);
  if (!y || !m || !d) return day || '';
  return new Date(y, m - 1, d).toLocaleDateString(undefined, { day: 'numeric', month: 'short', year: 'numeric' });
}

/** "120 tokens/s" ('' unknown). */
export function speedLine(perSecond) {
  const v = Number(perSecond) || 0;
  if (!v) return '';
  return `${v >= 100 ? Math.round(v) : v.toFixed(1).replace(/\.0$/, '')} tokens/s`;
}

/** The empty summary (before the first read). */
export const EMPTY_TOTALS = { days: 1, tokens: 0, prompt: 0, answer: 0, calls: 0, unreported: 0, here: 0, cloud: 0, costMicros: 0, unpriced: 0, models: [], features: [] };
