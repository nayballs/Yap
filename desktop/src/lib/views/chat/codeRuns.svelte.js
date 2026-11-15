// **A Code chat's ▶ Runs** (15 Nov 2026; desktop/src-tauri/src/agents/run_block.rs):
// each run of a one-line shell block, its output as it comes
// (`yap-code-run`), kept per conversation while the window lives — the
// Runs tab beside the chat (RunsTab.svelte) and the summary's Background
// tasks read them.
//
// `runCommand` is ▶'s click: Rust says whether it asks first (anything but
// Bypass permissions); asked, the person's yes starts it (Rust refuses a run
// that wasn't asked); the Runs tab opens on it.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { askConfirm } from '../../ui/contextMenu.svelte.js';
import { toast } from '../../ui/toast.svelte.js';

/** The newest output a run keeps here (characters: run_block.rs `OUTPUT_KEEP`). */
const KEEP = 200_000;

export const codeRuns = $state({
  /** Conversation id → its runs, oldest first (`RunInfo`, `output` the text so far). */
  by: {},
});

let listening = false;

/** A run as Rust said it, its output kept. */
function upsert(run) {
  if (!run?.id) return;
  const conv = String(run.conversation);
  const list = codeRuns.by[conv] || [];
  const i = list.findIndex((r) => r.id === run.id);
  if (i < 0) codeRuns.by[conv] = [...list, { ...run, output: run.output ?? '' }];
  else codeRuns.by[conv] = list.map((r, k) => (k === i ? { ...r, ...run, output: run.output ?? r.output } : r));
}

/** New output for run `id`. */
function append(conversation, id, text) {
  const conv = String(conversation);
  const list = codeRuns.by[conv];
  if (!list) return;
  codeRuns.by[conv] = list.map((r) => {
    if (r.id !== id) return r;
    let output = (r.output || '') + text;
    let cut = r.cut;
    if (output.length > KEEP) {
      output = output.slice(output.length - KEEP);
      cut = true;
    }
    return { ...r, output, cut };
  });
}

/** Follow every run's news (once per window). */
export function watchRuns() {
  if (listening) return;
  listening = true;
  listen('yap-code-run', (e) => {
    const p = e.payload;
    if (!p?.id) return;
    if (p.run) upsert(p.run);
    if (typeof p.text === 'string') append(p.conversation, p.id, p.text);
  }).catch(() => {});
}

/** Code chat `conversation`'s runs this session, read from Rust (a chat opened again). */
export async function loadRuns(conversation) {
  if (conversation == null) return;
  watchRuns();
  try {
    const list = await invoke('code_runs', { conversation });
    if (Array.isArray(list)) codeRuns.by[String(conversation)] = list.map((r) => ({ ...r, output: r.output ?? '' }));
  } catch {
    /* none */
  }
}

/** Code chat `conversation`'s runs (oldest first). */
export const runsOf = (conversation) => (conversation == null ? [] : codeRuns.by[String(conversation)] || []);

/** Stop run `id`. */
export async function stopRun(id) {
  try {
    await invoke('code_run_stop', { id });
  } catch (e) {
    toast({ title: "Couldn't stop it", description: String(e?.message ?? e), variant: 'destructive' });
  }
}

async function start(conversation, command, lang, asked, onstarted) {
  try {
    const run = await invoke('code_run_start', { conversation, command, lang, asked });
    upsert({ ...run, output: '' });
    onstarted?.(run);
    return true;
  } catch (e) {
    toast({ title: "Couldn't run it", description: String(e?.message ?? e), variant: 'destructive' });
    return false;
  }
}

/**
 * **▶ Run** a block (`command`, its fence's `lang`) in Code chat
 * `conversation`: asked first unless its Mode is Bypass permissions;
 * `onstarted(run)` once it runs (the Runs tab shows it).
 */
export async function runCommand(conversation, { command, lang }, onstarted = null) {
  if (conversation == null) return;
  watchRuns();
  let check;
  try {
    check = await invoke('code_run_check', { conversation, lang });
  } catch (e) {
    toast({ title: "Couldn't run it", description: String(e?.message ?? e), variant: 'destructive' });
    return;
  }
  const line = String(command || '')
    .trim()
    .replace(/^(\$|>|PS>)\s+/, '');
  if (!check.asks) {
    await start(conversation, command, lang, false, onstarted);
    return;
  }
  askConfirm({
    title: 'Run this command?',
    body: `“${line.length > 300 ? `${line.slice(0, 300)}…` : line}” runs in ${check.workspace} with ${check.ranIn}${check.note ? ` (${check.note.replace(/\.$/, '')})` : ''}. Yap asks first because this chat's Mode is ${check.mode}; under Bypass permissions it runs straight away.`,
    confirmLabel: 'Run',
    busyLabel: 'Starting…',
    danger: false,
    run: () => start(conversation, command, lang, true, onstarted),
  });
}
