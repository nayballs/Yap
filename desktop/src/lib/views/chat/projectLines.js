// The small lines of Chat's Projects page (ProjectsView.svelte): a file's
// kind and size, and when a project or chat last changed. Pure.

import { ocrLine } from './chatFiles.js';

const KINDS = { pdf: 'PDF', word: 'Word', powerpoint: 'PowerPoint', sheet: 'Spreadsheet', markdown: 'Markdown', code: 'Code', text: 'Text' };

/** "PDF", "Markdown"… for a project file's `kind`; a scanned PDF's "PDF, 2 pages read with OCR". */
export function kindLabel(f) {
  const kind = KINDS[f?.kind] || 'File';
  const ocr = ocrLine(f);
  return ocr ? `${kind}, ${ocr}` : kind;
}

/** "12 KB · about 3K tokens": a file's size and what it costs a model. */
export function sizeLabel(f) {
  const b = Number(f?.bytes) || 0;
  const size = b >= 1024 * 1024 ? `${(b / (1024 * 1024)).toFixed(1)} MB` : b >= 1024 ? `${Math.round(b / 1024)} KB` : `${b} bytes`;
  const t = Number(f?.tokens) || 0;
  const tokens = t >= 1000 ? `about ${Math.round(t / 1000)}K tokens` : `${t} tokens`;
  return `${size} · ${tokens}`;
}

/**
 * **How Chat's model reads the files** (`project_knowledge`, chat/projects/knowledge.rs):
 * whole while they fit, else the parts that match each question. '' with no files.
 */
export function knowledgeLine(k, files) {
  if (!files || !k) return '';
  const t = Number(k.tokens) || 0;
  const about = t >= 1000 ? `About ${Math.round(t / 1000)}K tokens` : `About ${t} tokens`;
  if (k.searched) {
    const model = k.model || "Chat's model";
    return `${about}: too much to send ${model} whole with every question, so each question gets the parts of these files that match it.`;
  }
  return `${about}: every chat here reads them whole.`;
}

/** "Today", "Yesterday", "3 days ago", "12 Oct" for a time in milliseconds. */
export function whenLabel(ms, now = Date.now()) {
  if (!ms) return '';
  const d = new Date(ms);
  const start = (t) => new Date(new Date(t).getFullYear(), new Date(t).getMonth(), new Date(t).getDate()).getTime();
  const days = Math.round((start(now) - start(ms)) / 86_400_000);
  if (days <= 0) return 'Today';
  if (days === 1) return 'Yesterday';
  if (days < 7) return `${days} days ago`;
  return d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
}
