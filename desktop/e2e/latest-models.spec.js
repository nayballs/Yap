// desktop/src/lib/core/latestModels.js on its own (no app needed): how Chat's model
// picker splits the selected provider's list into Latest (the newest of
// each family, as the Claude Code app lists the latest Opus, Fable, Sonnet
// and Haiku) and More models (the rest, newest first):
// - Anthropic's Opus, Fable, Sonnet and Haiku; OpenAI's flagship and small
//   tiers; Gemini's flagship and Flash; Groq's biggest and smallest
//   GPT-OSS; OpenRouter's for the labs it carries;
// - a newer model in a provider's live list takes its family's place; one
//   being retired or no longer offered never does;
// - the model in use stays where the split puts it (the picker opens More
//   models when it's there);
// - More models newest first: the provider's dates, else the id's version;
// - a provider none of whose models is in a family isn't split.
// The rows are made from Yap's curated lists (ppModels.js) and a few live
// ones of the tests' own.
import { test, expect } from '@playwright/test';
import { latestModels, newestFirst, versionOf } from '../src/lib/core/latestModels.js';
import { PP_CLOUD_MODELS } from '../src/lib/core/ppModels.js';

const row = (provider, model, name, extra = {}) => ({ key: `${provider}:${model}`, provider, model, name, ...extra });
/** A provider's curated list as the picker's rows. */
const curated = (p) => PP_CLOUD_MODELS[p].models.map((m) => row(p, m.value, m.label));
const names = (rows) => rows.map((r) => r.name);
const split = (p, rows = curated(p)) => {
  const s = latestModels(p, rows);
  return s && { latest: names(s.latest), older: names(s.older) };
};

test('Anthropic: the newest Opus, Fable, Sonnet and Haiku; the older ones under More, newest first', () => {
  expect(split('anthropic')).toEqual({
    latest: ['Claude Opus 5.5', 'Claude Fable 5.2', 'Claude Sonnet 5.5', 'Claude Haiku 5.5'],
    older: ['Claude Fable 5.1', 'Claude Opus 4.8', 'Claude Opus 4.7', 'Claude Sonnet 4.6', 'Claude Opus 4.6', 'Claude Haiku 4.5', 'Claude Opus 4.5'],
  });
});

test('OpenAI: its flagship and small tiers; Gemini: its flagship and Flash; Groq: its biggest and smallest GPT-OSS', () => {
  expect(split('openai')).toEqual({
    latest: ['GPT-6 Astra', 'GPT-6 Luna'],
    older: ['GPT-6.1 Sol', 'GPT-5.6 Luna', 'GPT-5.6 Terra', 'GPT-5.6 Sol', 'GPT-5.5', 'GPT-5.4 Mini', 'GPT-5 Mini'],
  });
  expect(split('gemini').latest).toEqual(['Gemini 4 Argon', 'Gemini 3.8 Flash']);
  // Newest first by the id's version (Google's list has no dates); Gemma last.
  expect(split('gemini').older.slice(0, 3)).toEqual(['Gemini 3.6 Flash', 'Gemini 3.5 Flash-Lite', 'Gemini 3.5 Flash']);
  expect(split('gemini').older.slice(-2)).toEqual(['Gemma 4 31B', 'Gemma 4 26B MoE']);
  expect(split('groq')).toEqual({ latest: ['GPT-OSS 120B', 'GPT-OSS 20B'], older: ['Qwen3.8 27B'] });
  // One GPT-OSS only: one row, not the same one twice.
  expect(split('groq', [row('groq', 'openai/gpt-oss-20b', 'GPT-OSS 20B'), row('groq', 'qwen/qwen3.8-27b', 'Qwen3.8 27B')])).toEqual({
    latest: ['GPT-OSS 20B'],
    older: ['Qwen3.8 27B'],
  });
  // A later GPT-OSS generation takes both places.
  const gen2 = [...curated('groq'), row('groq', 'openai/gpt-oss-2-200b', 'GPT-OSS 2 200B'), row('groq', 'openai/gpt-oss-2-40b', 'GPT-OSS 2 40B')];
  expect(split('groq', gen2).latest).toEqual(['GPT-OSS 2 200B', 'GPT-OSS 2 40B']);
});

test('a provider none of whose models is in a family is not split', () => {
  const odd = [row('groq', 'acme/brainy-9', 'Brainy 9'), row('groq', 'allam-2-7b', 'ALLaM-2-7b')];
  expect(latestModels('groq', odd)).toBeNull();
  expect(latestModels('groq', [])).toBeNull();
  expect(latestModels('nobody', curated('anthropic'))).toBeNull();
});

test('a newer model in the live list takes its family’s place; one being retired or gone never does', () => {
  const now = Math.floor(Date.now() / 1000);
  const rows = [
    ...curated('anthropic'),
    // What Anthropic lists tomorrow: a dated snapshot counts as its alias.
    row('anthropic', 'claude-opus-6', 'Claude Opus 6', { created: now }),
    row('anthropic', 'claude-sonnet-6-20270115', 'Claude Sonnet 6', { created: now }),
    row('anthropic', 'claude-haiku-6', 'Claude Haiku 6', { created: now, deprecated: true }),
  ];
  const s = split('anthropic', rows);
  expect(s.latest).toEqual(['Claude Opus 6', 'Claude Fable 5.2', 'Claude Sonnet 6', 'Claude Haiku 5.5']);
  // Opus and Sonnet 5.5 move to More, the newest there first (Haiku 6
  // too: it can still be picked there).
  expect(s.older.slice(0, 5)).toEqual(['Claude Haiku 6', 'Claude Sonnet 5.5', 'Claude Opus 5.5', 'Claude Fable 5.1', 'Claude Opus 4.8']);
  // No longer offered (the provider's live list dropped it): not a latest
  // one, but still listed; the Fable before it takes the family's place.
  const gone = curated('anthropic').map((r) => (r.model === 'claude-fable-5-2' ? { ...r, missing: true } : r));
  expect(split('anthropic', gone).latest).toEqual(['Claude Opus 5.5', 'Claude Fable 5.1', 'Claude Sonnet 5.5', 'Claude Haiku 5.5']);
  expect(split('anthropic', gone).older).toContain('Claude Fable 5.2');
  // Every row once, in one list or the other.
  const all = latestModels('anthropic', rows);
  expect([...all.latest, ...all.older].map((r) => r.key).sort()).toEqual(rows.map((r) => r.key).sort());
});

test('the model in use stays where it is: in Latest, or under More models', () => {
  const inUse = (model) => curated('anthropic').map((r) => (r.model === model ? { ...r, current: true } : r));
  const older = latestModels('anthropic', inUse('claude-opus-4-8'));
  expect(names(older.latest)).toEqual(['Claude Opus 5.5', 'Claude Fable 5.2', 'Claude Sonnet 5.5', 'Claude Haiku 5.5']);
  expect(older.older.find((r) => r.current)?.name).toBe('Claude Opus 4.8');
  const latest = latestModels('anthropic', inUse('claude-sonnet-5-5'));
  expect(latest.latest.find((r) => r.current)?.name).toBe('Claude Sonnet 5.5');
  expect(latest.older.some((r) => r.current)).toBe(false);
});

test('More models newest first: by the provider’s dates when every row has one, else by the id’s version', () => {
  const dated = [row('groq', 'b', 'B', { created: 2 }), row('groq', 'a', 'A', { created: 3 }), row('groq', 'c', 'C', { created: 1 })];
  expect(names(newestFirst('groq', dated))).toEqual(['A', 'B', 'C']);
  // A live Anthropic list: Haiku 4.5 came out after Opus 4.1.
  const live = [
    row('anthropic', 'claude-opus-4-1-20250805', 'Claude Opus 4.1', { created: 1754352000 }),
    row('anthropic', 'claude-haiku-4-5-20251001', 'Claude Haiku 4.5', { created: 1759276800 }),
    row('anthropic', 'claude-3-5-haiku-20241022', 'Claude Haiku 3.5', { created: 1729555200 }),
  ];
  expect(names(newestFirst('anthropic', live))).toEqual(['Claude Haiku 4.5', 'Claude Opus 4.1', 'Claude Haiku 3.5']);
  // One without a date: by the version, then the date.
  expect(names(newestFirst('anthropic', [...live, row('anthropic', 'claude-opus-4', 'Claude Opus 4')]))).toEqual([
    'Claude Haiku 4.5',
    'Claude Opus 4.1',
    'Claude Opus 4',
    'Claude Haiku 3.5',
  ]);
  // No dates and no versions: Yap's order.
  expect(names(newestFirst('groq', [row('groq', 'x', 'X'), row('groq', 'y', 'Y')]))).toEqual(['X', 'Y']);
});

test('OpenRouter: the families of the labs it carries, Anthropic’s, OpenAI’s, then Google’s', () => {
  const or = [
    row('openrouter', 'meta-llama/llama-3.3-70b-instruct', 'LLaMA 3.3 70B'),
    row('openrouter', 'google/gemini-3.8-flash', 'Gemini 3.8 Flash'),
    row('openrouter', 'openai/gpt-5-mini', 'GPT-5 Mini'),
    row('openrouter', 'openai/gpt-6-astra', 'GPT-6 Astra'),
    row('openrouter', 'openai/gpt-oss-120b', 'GPT-OSS 120B'),
    row('openrouter', 'anthropic/claude-sonnet-5.5', 'Claude Sonnet 5.5'),
    row('openrouter', 'anthropic/claude-opus-5.5:thinking', 'Claude Opus 5.5 (thinking)'),
    row('openrouter', 'anthropic/claude-opus-5.5', 'Claude Opus 5.5'),
    row('openrouter', 'anthropic/claude-opus-4.8', 'Claude Opus 4.8'),
  ];
  // The plain id over its ":thinking" variant.
  expect(split('openrouter', or)).toEqual({
    latest: ['Claude Opus 5.5', 'Claude Sonnet 5.5', 'GPT-6 Astra', 'GPT-5 Mini', 'Gemini 3.8 Flash'],
    older: ['Claude Opus 5.5 (thinking)', 'Claude Opus 4.8', 'LLaMA 3.3 70B', 'GPT-OSS 120B'],
  });
  // Yap's own OpenRouter list: its Claude and its GPT up front.
  expect(split('openrouter')).toEqual({
    latest: ['Claude Haiku 4.5', 'GPT-5 Mini'],
    older: ['LLaMA 3.3 70B', 'GPT-OSS 20B', 'Qwen3 32B'],
  });
});

test('the version an id says', () => {
  expect(versionOf('anthropic', 'claude-opus-4-5-20251101')).toEqual([4, 5]);
  expect(versionOf('anthropic', 'claude-sonnet-5')).toEqual([5, 0]);
  expect(versionOf('anthropic', 'claude-3-5-sonnet-20241022')).toEqual([3, 5]);
  expect(versionOf('openrouter', 'anthropic/claude-opus-4.5:thinking')).toEqual([4, 5]);
  expect(versionOf('openai', 'gpt-6.1-sol')).toEqual([6, 1]);
  expect(versionOf('gemini', 'models/gemini-3.1-pro-preview')).toEqual([3, 1]);
  expect(versionOf('groq', 'openai/gpt-oss-120b')).toEqual([0, 0]);
});
