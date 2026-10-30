// The document panel's own helpers (DocPanel.svelte's chunk, loaded the
// first time a document opens, so the main window's stays small): where a
// rendered block's lines are in the note, the text Copy as Markdown copies,
// and Export as Markdown (the Notes view's `note_export`).
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../../ui/toast.svelte.js';
import { isPlaceholder } from '../../core/markdown.js';
import { copyText } from '../../core/itemMenus.js';

/**
 * How many of `content`'s lines come before the first line the panel's page
 * shows (`chatDoc`'s `shownText`: the title's, and the blank lines after
 * it): a rendered block's `data-line` plus this is its line in the note
 * (with nothing streaming). Pure.
 */
export function shownOffset(content, title) {
  const lines = String(content || '').split('\n');
  let at = 0;
  const first = lines[0]?.trim() ?? '';
  if (/^#\s+/.test(first) && first.replace(/^#\s+/, '').trim() === String(title || '').trim()) at = 1;
  while (at < lines.length - 1 && lines[at] === '') at++;
  return at;
}

/** A document's text to copy, as Markdown: its sections still to write left out. Pure. */
export function copyableText(content) {
  return String(content || '')
    .split('\n')
    .filter((l) => !isPlaceholder(l))
    .join('\n')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

/** Copy the panel's document as Markdown (what's typed saved first). */
export async function copyDocument(doc) {
  await doc.flushSave();
  return copyText(copyableText(doc.content), 'Copied as Markdown');
}

/** Export as Markdown: a .md file where you choose. */
export async function exportDocument(doc) {
  if (doc.id == null) return;
  await doc.flushSave();
  const id = doc.id;
  try {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const suggested = (doc.title || 'document').replace(/[\\/:*?"<>|]/g, '-');
    const path = await save({ defaultPath: `${suggested}.md`, filters: [{ name: 'Markdown', extensions: ['md'] }] });
    if (!path) return;
    await invoke('note_export', { id, path });
    toast({ title: 'Document exported', description: path, variant: 'success' });
  } catch (e) {
    toast({ title: 'Export failed', description: String(e), variant: 'destructive' });
  }
}
