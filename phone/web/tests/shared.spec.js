// The desktop's own files the phone's page loads as they are (build.rs
// `LINK_SHARED`: Chat's picker code at core/, the logos at providers/) stay
// loadable on a phone: each module imports only others the PC serves,
// never Tauri, Svelte or an asset; and the marks the picker draws are the
// ones served. No browser.
import fs from 'node:fs';
import path from 'node:path';
import { test, expect } from '@playwright/test';
import { SHARED, PAGE_DIR } from './support/harness.js';

const modules = Object.keys(SHARED).filter((p) => p.startsWith('core/'));
const imports = (src) => [...src.matchAll(/^\s*(?:import|export)\s[^'"]*?from\s*['"]([^'"]+)['"]/gm)].map((m) => m[1]);

test('every shared module imports only the others the PC serves, and nothing of the app', () => {
  expect(modules).toEqual(expect.arrayContaining(['core/chatPicker.js', 'core/modelCatalog.js', 'core/latestModels.js', 'core/ppModels.js']));
  for (const m of modules) {
    const src = fs.readFileSync(SHARED[m], 'utf8');
    for (const spec of imports(src)) {
      const served = path.posix.join(path.posix.dirname(m), spec);
      expect(SHARED[served], `${m} imports ${spec}, which the PC doesn't serve (build.rs LINK_SHARED)`).toBeTruthy();
    }
    expect(src, m).not.toMatch(/@tauri-apps|\.svelte['"]|\$state\(|\$derived|import\.meta\.env|\.svg['"]/);
  }
});

test("the page's own modules import only what's served", () => {
  for (const f of ['link.js', 'picker.js']) {
    const src = fs.readFileSync(path.join(PAGE_DIR, f), 'utf8');
    for (const spec of imports(src)) {
      const rel = path.posix.normalize(spec.replace(/^\.\//, ''));
      expect(fs.existsSync(path.join(PAGE_DIR, rel)) || !!SHARED[rel], `${f} imports ${spec}`).toBe(true);
    }
  }
});

test('the marks the picker draws are the ones the PC serves', async () => {
  const served = Object.keys(SHARED)
    .filter((p) => p.startsWith('providers/'))
    .map((p) => p.slice('providers/'.length, -'.svg'.length))
    .sort();
  const picker = fs.readFileSync(path.join(PAGE_DIR, 'picker.js'), 'utf8');
  const listed = JSON.parse(picker.match(/const SERVED = new Set\((\[[^\]]+\])\)/)[1].replace(/'/g, '"')).sort();
  expect(listed).toEqual(served);
  // Every family and provider mark Chat's picker can give a row or a tab.
  const { familyIcon, providerIcon, ICON_FILES } = await import(`file://${SHARED['core/modelIcons.js'].replace(/\\/g, '/')}`);
  const shown = new Set(
    [
      ...['groq', 'gemini', 'anthropic', 'chatgpt', 'openai', 'openrouter'].map(providerIcon),
      ...['GPT-OSS 20B', 'Qwen3.5 4B', 'Gemma 4', 'Gemini 3.6 Flash', 'Llama 3.3', 'Mistral Small', 'Claude Haiku', 'Grok 4', 'GPT-6 Luna'].map((n) => familyIcon(n)),
    ].map((i) => ICON_FILES[i]),
  );
  for (const file of shown) expect(served, file).toContain(file);
});
