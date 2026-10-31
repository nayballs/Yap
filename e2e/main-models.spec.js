// src/lib/core/mainModels.js on its own (no app needed): which models Chat's
// model menu lists first ("main models", as the Claude Code app lists the
// latest Opus, Fable, Sonnet and Haiku) and what goes under More models ▸.
// - the newest of each family from the providers with a key: Anthropic's
//   Opus, Fable, Sonnet and Haiku; OpenAI's flagship and small models;
//   Gemini's flagship and Flash; Groq's biggest and smallest GPT-OSS;
//   OpenRouter's for a lab whose own key isn't there;
// - a newer model in a provider's live list takes its family's place; one
//   being retired or no longer offered never does;
// - the on-device model when one is installed, the model in use always;
// - seven rows at most, the model in use kept;
// - nothing from a provider without a key; More models newest first.
// The rows are made from Yap's curated lists (ppModels.js) and a few live
// ones of the tests' own.
import { test, expect } from '@playwright/test';
import { mainModels, moreModels, versionOf, MAX_MAIN } from '../src/lib/core/mainModels.js';
import { PP_CLOUD_MODELS } from '../src/lib/core/ppModels.js';

const row = (provider, model, name, extra = {}) => ({ key: `${provider}:${model}`, provider, model, name, ...extra });
/** A provider's curated list as the pickers' rows. */
const curated = (p) => PP_CLOUD_MODELS[p].models.map((m) => row(p, m.value, m.label));
const names = (rows) => rows.map((r) => r.name);
const QWEN = { key: 'ondevice:Qwen_Qwen3.5-4B-Q4_K_M.gguf', provider: 'ondevice', model: '', file: 'Qwen_Qwen3.5-4B-Q4_K_M.gguf', name: 'Qwen3.5 4B' };
const ALL = ['groq', 'anthropic', 'openai', 'gemini', 'openrouter'];
const everything = () => Object.fromEntries(ALL.map((p) => [p, curated(p)]));

test('Anthropic: the newest Opus, Fable, Sonnet and Haiku; the older ones under More, newest first', () => {
  const rows = { anthropic: curated('anthropic') };
  const main = mainModels({ rows, connected: ['anthropic'] });
  expect(names(main)).toEqual(['Claude Opus 5.5', 'Claude Fable 5.1', 'Claude Sonnet 5.5', 'Claude Haiku 5.5']);
  const more = moreModels({ rows, connected: ['anthropic'], main });
  expect(more.map((g) => g.id)).toEqual(['anthropic']);
  expect(names(more[0].rows)).toEqual([
    'Claude Opus 4.8',
    'Claude Opus 4.7',
    'Claude Sonnet 4.6',
    'Claude Opus 4.6',
    'Claude Haiku 4.5',
    'Claude Opus 4.5',
  ]);
});

test('OpenAI: its flagship and small models; Gemini: its flagship and Flash; Groq: its biggest and smallest GPT-OSS', () => {
  const pick = (p) => names(mainModels({ rows: { [p]: curated(p) }, connected: [p] }));
  expect(pick('openai')).toEqual(['GPT-6 Astra', 'GPT-6 Luna']);
  expect(pick('gemini')).toEqual(['Gemini 4 Argon', 'Gemini 3.8 Flash']);
  expect(pick('groq')).toEqual(['GPT-OSS 120B', 'GPT-OSS 20B']);
  // One GPT-OSS only: one row, not the same one twice.
  expect(names(mainModels({ rows: { groq: [row('groq', 'openai/gpt-oss-20b', 'GPT-OSS 20B')] }, connected: ['groq'] }))).toEqual(['GPT-OSS 20B']);
  // None in a family: the provider's first, as Yap lists it.
  const odd = [row('groq', 'acme/brainy-9', 'Brainy 9'), row('groq', 'allam-2-7b', 'ALLaM-2-7b')];
  expect(names(mainModels({ rows: { groq: odd }, connected: ['groq'] }))).toEqual(['Brainy 9']);
});

test('a newer model in the live list takes its family’s place; one being retired or gone never does', () => {
  const now = Math.floor(Date.now() / 1000);
  const rows = {
    anthropic: [
      ...curated('anthropic'),
      // What Anthropic lists tomorrow: a dated snapshot counts as its alias.
      row('anthropic', 'claude-opus-6', 'Claude Opus 6', { created: now }),
      row('anthropic', 'claude-sonnet-6-20270115', 'Claude Sonnet 6', { created: now }),
      row('anthropic', 'claude-haiku-6', 'Claude Haiku 6', { created: now, deprecated: true }),
    ],
  };
  const main = mainModels({ rows, connected: ['anthropic'] });
  expect(names(main)).toEqual(['Claude Opus 6', 'Claude Fable 5.1', 'Claude Sonnet 6', 'Claude Haiku 5.5']);
  // Opus and Sonnet 5.5 move to More, the newest there first (Haiku 6
  // too: it can still be picked there).
  const more = moreModels({ rows, connected: ['anthropic'], main });
  expect(names(more[0].rows).slice(0, 4)).toEqual(['Claude Haiku 6', 'Claude Sonnet 5.5', 'Claude Opus 5.5', 'Claude Opus 4.8']);
  // A provider's dates order More when every row has one.
  const dated = moreModels({
    rows: { groq: [row('groq', 'b', 'B', { created: 2 }), row('groq', 'a', 'A', { created: 3 }), row('groq', 'c', 'C', { created: 1 })] },
    connected: ['groq'],
    main: [],
  });
  expect(names(dated[0].rows)).toEqual(['A', 'B', 'C']);
  // No longer offered (the provider's live list dropped it): not a main one.
  const gone = { anthropic: curated('anthropic').map((r) => (r.model === 'claude-fable-5-1' ? { ...r, missing: true } : r)) };
  expect(names(mainModels({ rows: gone, connected: ['anthropic'] }))).toEqual(['Claude Opus 5.5', 'Claude Sonnet 5.5', 'Claude Haiku 5.5']);
});

test('the model in use is always in, even an older one, last; the on-device model when installed', () => {
  const rows = { anthropic: curated('anthropic').map((r) => (r.model === 'claude-opus-4-8' ? { ...r, current: true } : r)) };
  const current = rows.anthropic.find((r) => r.current);
  expect(names(mainModels({ rows, connected: ['anthropic'], current }))).toEqual([
    'Claude Opus 5.5',
    'Claude Fable 5.1',
    'Claude Sonnet 5.5',
    'Claude Haiku 5.5',
    'Claude Opus 4.8',
  ]);
  // On this PC after the cloud's; the model in use after that.
  const main = mainModels({ rows, connected: ['anthropic'], current, onDevice: QWEN });
  expect(names(main)).toEqual(['Claude Opus 5.5', 'Claude Fable 5.1', 'Claude Sonnet 5.5', 'Claude Haiku 5.5', 'Qwen3.5 4B', 'Claude Opus 4.8']);
  // …and not under More.
  expect(names(moreModels({ rows, connected: ['anthropic'], main })[0].rows)).not.toContain('Claude Opus 4.8');
  // A main one in use: once.
  const opus = { ...rows.anthropic.find((r) => r.model === 'claude-opus-5-5'), current: true };
  expect(names(mainModels({ rows, connected: ['anthropic'], current: opus }))).toHaveLength(4);
  // The on-device one in use: once.
  expect(names(mainModels({ rows, connected: ['anthropic'], current: QWEN, onDevice: QWEN }))).toEqual([
    'Claude Opus 5.5',
    'Claude Fable 5.1',
    'Claude Sonnet 5.5',
    'Claude Haiku 5.5',
    'Qwen3.5 4B',
  ]);
  // One the provider no longer offers, still in use: shown, last.
  const old = row('anthropic', 'claude-opus-3', 'claude-opus-3', { current: true, missing: true });
  expect(names(mainModels({ rows: { anthropic: [old, ...curated('anthropic')] }, connected: ['anthropic'], current: old })).at(-1)).toBe('claude-opus-3');
  // Nothing connected, nothing installed: the model in use alone (a server on this PC).
  const server = row('local', 'llama3.1:8b', 'llama3.1:8b', { current: true });
  expect(names(mainModels({ current: server }))).toEqual(['llama3.1:8b']);
});

test('nothing from a provider without a key', () => {
  const rows = everything();
  const main = mainModels({ rows, connected: ['groq'], onDevice: QWEN });
  expect(main.map((r) => r.provider)).toEqual(['groq', 'groq', 'ondevice']);
  const more = moreModels({ rows, connected: ['groq'], pc: [QWEN], main });
  expect(more.map((g) => g.id)).toEqual(['groq']);
  expect(names(more[0].rows)).toEqual(['Qwen3.8 27B']);
  expect(mainModels({ rows, connected: [] })).toEqual([]);
});

test("OpenRouter: a lab's families when its own key isn't there", () => {
  const or = [
    row('openrouter', 'anthropic/claude-opus-5.5', 'Claude Opus 5.5'),
    row('openrouter', 'anthropic/claude-sonnet-5.5', 'Claude Sonnet 5.5'),
    row('openrouter', 'openai/gpt-6-astra', 'GPT-6 Astra'),
    row('openrouter', 'openai/gpt-5-mini', 'GPT-5 Mini'),
    row('openrouter', 'openai/gpt-oss-120b', 'GPT-OSS 120B'),
    row('openrouter', 'google/gemini-3.8-flash', 'Gemini 3.8 Flash'),
    row('openrouter', 'meta-llama/llama-3.3-70b-instruct', 'LLaMA 3.3 70B'),
  ];
  const alone = mainModels({ rows: { openrouter: or }, connected: ['openrouter'] });
  expect(names(alone)).toEqual(['Claude Opus 5.5', 'Claude Sonnet 5.5', 'GPT-6 Astra', 'GPT-5 Mini', 'Gemini 3.8 Flash']);
  // With Anthropic's own key: Claude from Anthropic, the rest through OpenRouter.
  const both = mainModels({ rows: { anthropic: curated('anthropic'), openrouter: or }, connected: ['anthropic', 'openrouter'] });
  expect(both.map((r) => `${r.name} · ${r.provider}`)).toEqual([
    'Claude Opus 5.5 · anthropic',
    'Claude Fable 5.1 · anthropic',
    'Claude Sonnet 5.5 · anthropic',
    'Claude Haiku 5.5 · anthropic',
    'GPT-6 Astra · openrouter',
    'GPT-5 Mini · openrouter',
    'Gemini 3.8 Flash · openrouter',
  ]);
});

test('a full menu: seven rows at most, each provider’s first families in turn, the model in use kept', () => {
  const rows = everything();
  const main = mainModels({ rows, connected: ALL, onDevice: QWEN });
  expect(main).toHaveLength(MAX_MAIN);
  expect(names(main)).toEqual(['Claude Opus 5.5', 'Claude Sonnet 5.5', 'GPT-6 Astra', 'GPT-6 Luna', 'Gemini 4 Argon', 'GPT-OSS 120B', 'Qwen3.5 4B']);
  // A small one in use: kept, in its place.
  const small = { ...rows.groq.find((r) => r.model === 'openai/gpt-oss-20b'), current: true };
  expect(names(mainModels({ rows, connected: ALL, onDevice: QWEN, current: small }))).toEqual([
    'Claude Opus 5.5',
    'Claude Sonnet 5.5',
    'GPT-6 Astra',
    'Gemini 4 Argon',
    'GPT-OSS 120B',
    'GPT-OSS 20B',
    'Qwen3.5 4B',
  ]);
  // An older one in use: last, still seven.
  const older = { ...rows.anthropic.find((r) => r.model === 'claude-opus-4-8'), current: true };
  const withOld = mainModels({ rows, connected: ALL, onDevice: QWEN, current: older });
  expect(withOld).toHaveLength(MAX_MAIN);
  expect(names(withOld).at(-1)).toBe('Claude Opus 4.8');
  // Everything left out is under More.
  const more = moreModels({ rows, connected: ALL, main });
  const shown = new Set([...main, ...more.flatMap((g) => g.rows)].map((r) => r.key));
  for (const p of ALL) for (const r of rows[p]) expect(shown.has(r.key)).toBe(true);
  expect(more.map((g) => g.id)).toEqual(['anthropic', 'openai', 'gemini', 'groq', 'openrouter']);
});

test("More models: this PC's other models and the servers, after the providers", () => {
  const big = { ...QWEN, key: 'ondevice:qwen2.5-1.5b.gguf', name: 'Qwen2.5 1.5B Instruct' };
  const mine = row('local', 'llama3.1:8b', 'llama3.1:8b');
  const away = row('custom', 'big-model', 'big-model');
  const more = moreModels({
    rows: { groq: curated('groq') },
    connected: ['groq'],
    pc: [QWEN, big, mine],
    servers: [{ id: 'server:https://llm.example.invalid/v1', label: 'llm.example.invalid', rows: [away] }],
    main: [QWEN],
  });
  expect(more.map((g) => g.id)).toEqual(['groq', 'pc', 'server:https://llm.example.invalid/v1']);
  expect(names(more[1].rows)).toEqual(['Qwen2.5 1.5B Instruct', 'llama3.1:8b']);
  expect(more[2].label).toBe('llm.example.invalid');
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
