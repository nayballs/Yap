// The little the Chat model chip (ModelChip.svelte) needs before a picker
// loads: the on-device models' status (`local`, read as the chip opens) and
// the Recent list (the last three Chat models used, kept in this webview).
// Everything else the pickers list is chatModels.svelte.js's, loaded with
// the menu or the provider rail (their own chunks: the main window's stays
// under Vite's 725 kB limit).
import { invoke } from '@tauri-apps/api/core';

/** local_llm_status, the download under way (a curated id) and its progress. */
export const local = $state({ status: null, downloading: '', progress: null, error: '', errorFor: '' });

export async function refreshLocal() {
  try {
    local.status = await invoke('local_llm_status');
  } catch {
    /* not in Tauri */
  }
}

export function sameUrl(a, b) {
  const t = (s) => String(s || '').trim().replace(/\/+$/, '').toLowerCase();
  return !!a && t(a) === t(b);
}

// ------------------------------------------------------------- recent

const RECENT_KEY = 'yapChatRecentModels';
const sameEntry = (a, b) =>
  a.provider === b.provider && (a.model || '') === (b.model || '') && (a.file || '') === (b.file || '') && sameUrl(a.baseUrl || '-', b.baseUrl || '-');

/** The last three Chat models used, newest first. */
export function readRecent() {
  try {
    const v = JSON.parse(localStorage.getItem(RECENT_KEY) || '[]');
    return Array.isArray(v) ? v.filter((e) => e && typeof e.provider === 'string').slice(0, 3) : [];
  } catch {
    return [];
  }
}

/** A picker's row → what Recent keeps of it. */
export function recentEntry(row) {
  return { provider: row.provider, model: row.model || '', file: row.file || '', baseUrl: row.baseUrl || '' };
}

/** A Chat model used (picked, or the one in use when the picker opened). */
export function pushRecent(entry) {
  if (!entry?.provider) return;
  const list = [entry, ...readRecent().filter((e) => !sameEntry(e, entry))].slice(0, 3);
  try {
    localStorage.setItem(RECENT_KEY, JSON.stringify(list));
  } catch {
    /* a convenience only */
  }
}
