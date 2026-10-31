// The document panel beside Chat's conversation (src-tauri/src/chat/documents.rs):
// which document it shows, its text, the sections being written as they
// stream (`section` events), its two views and the Source box's saves,
// expanded or split, and the split's width (remembered).
//
// - A turn makes or changes a document: `document` events open the panel
//   on it (`fromTurn`), `section` events fill a placeholder as the model
//   writes it (`onSection`), and the note's own `yap-note-changed` brings
//   the text as each section lands (`onNoteChanged`).
// - Two views (2026-10-31, after Claude Code's file view): **Preview**, the
//   page to read, and **Source**, the markdown itself, coloured and
//   editable — it took the Edit box's place (one raw view, not two). Each
//   document's scroll is kept per view while Yap runs; a change Chat makes
//   lands with its blocks marked a moment (`landed`).
// - The person's words win: the Source box saves through
//   `chat_document_save` with the text it started from (`base`), and Rust
//   lays the person's edit over whatever Chat wrote meanwhile (documents.rs
//   `merge3`). A save that comes back while more was typed keeps the newer
//   typing and saves again from the text it was typed over.
// - Undo: the newest change, when it's Chat's (its writing, or a restore it
//   did), goes back to the version before it (notes/versions.rs; itself a
//   version, so it can be undone too).
// - Copy (the whole document as Markdown) and Export as Markdown (the Notes
//   view's `note_export`): `docTools.js`, in the panel's own chunk.
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../../ui/toast.svelte.js';
import { showSaveError } from '../../notices/saveErrors.js';
import { isPlaceholder, pendingSections } from '../../core/markdown.js';
import { closePartial } from '../../core/streaming.js';

/** The origin of the panel's saves (`yap-note-changed`): its own aren't news. */
export const PANEL_ORIGIN = 'chat-panel';

const WIDTH_KEY = 'yapChatDocWidth';
export const MIN_WIDTH = 320;
export const DEFAULT_WIDTH = 460;
/**
 * The least the conversation keeps beside the panel. Less room than that
 * and the panel's least (a narrow window): it covers the conversation
 * instead, ✕ going back.
 */
export const THREAD_MIN = 340;
/**
 * Chat at least this wide keeps its conversation list beside an open
 * document; narrower, the list steps aside for it.
 */
export const LIST_STAYS = 1240;

/** Where each document was scrolled to, per view (`${id}:${view}` → px), while Yap runs. */
const scrolls = new Map();

function loadWidth() {
  try {
    const w = Number(localStorage.getItem(WIDTH_KEY));
    return Number.isFinite(w) && w >= MIN_WIDTH ? w : DEFAULT_WIDTH;
  } catch {
    return DEFAULT_WIDTH;
  }
}

/** A section's text as it streams, under its own heading when it wrote one, else the key's. */
function liveBlock(key, text) {
  const own = /^#{1,2}\s+\S/.test(text.trimStart());
  return own ? closePartial(text.trimStart().replace(/^#\s/, '## ')) : `## ${key}\n\n${closePartial(text)}`;
}

/**
 * Section `key`'s lines in `lines`, `[start, end)`: its `##` heading to the
 * next `#`/`##` heading or placeholder (blank lines at its end left out),
 * as documents.rs `section_range` finds it; `null` if there's none.
 */
export function sectionRange(lines, key) {
  const want = String(key).trim().replace(/\s+/g, ' ').toLowerCase();
  const start = lines.findIndex((l) => {
    const m = /^\s*##\s+(.*?)\s*$/.exec(l);
    return !!m && m[1].replace(/#+$/, '').trim().replace(/\s+/g, ' ').toLowerCase() === want;
  });
  if (start < 0) return null;
  let end = start + 1;
  while (end < lines.length && !/^\s*#{1,2}\s+\S/.test(lines[end]) && !isPlaceholder(lines[end])) end++;
  while (end > start + 1 && !lines[end - 1].trim()) end--;
  return [start, end];
}

/**
 * A document's text as the panel shows it: without its first line when
 * that's the title (the panel shows it), and each section being written in
 * its place as it streams (`live`: key → its text so far) — a placeholder's
 * place (`write_section`), the section it rewrites, or a new one at the end
 * (`rewrite_section`). Pure.
 */
export function shownText(content, title, live = {}) {
  let lines = String(content || '').split('\n');
  const first = lines[0]?.trim() ?? '';
  if (/^#\s+/.test(first) && first.replace(/^#\s+/, '').trim() === String(title || '').trim()) {
    lines = lines.slice(1);
  }
  for (const key of Object.keys(live)) {
    const block = liveBlock(key, live[key] ?? '');
    const at = lines.findIndex((l) => isPlaceholder(l, key));
    if (at >= 0) {
      lines.splice(at, 1, block);
      continue;
    }
    const range = sectionRange(lines, key);
    if (range) lines.splice(range[0], range[1] - range[0], block);
    else lines.push('', block);
  }
  return lines.join('\n').replace(/^\n+/, '');
}

/**
 * The panel's state. `onopennote({id})`: ControlPanel's Open in Notes, as
 * the view has it now.
 */
export function createDocPanel({ onopennote = () => null } = {}) {
  let open = $state(false);
  let id = $state(null);
  let title = $state('');
  let content = $state('');
  let gone = $state(false);
  let expanded = $state(false);
  // 'preview' (the page) or 'source' (the markdown, editable).
  let view = $state('preview');
  let draft = $state('');
  // The text the Source box started from, or last took in.
  let base = '';
  let dirty = false;
  let saveTimer = null;
  let saving = null;
  // A turn is writing it now; the section on the way (its key, '' not
  // known yet); each section's text as it streams; 'writing' a section or
  // 'editing' its blocks (edit_document, restore_document).
  let busy = $state(false);
  let writing = $state(null);
  let live = $state({});
  let action = $state('writing');
  // The newest change is Chat's (Undo shows).
  let canUndo = $state(false);
  let width = $state(loadWidth());
  // Counts the changes that landed from outside the panel (Chat's, Undo's):
  // the page marks the blocks new since the one before.
  let landed = $state(0);

  const pending = $derived(pendingSections(content));
  const shown = $derived(shownText(content, title, live));

  async function load() {
    const want = id;
    if (want == null) return;
    try {
      const n = await invoke('note_get', { id: want });
      if (id !== want) return;
      gone = false;
      title = n.title || '';
      take(n.content || '');
      refreshUndo();
    } catch {
      if (id === want) gone = true;
    }
  }

  /** The document's text now (Rust's): shown, and the Source box's too unless typing is waiting to be saved. */
  function take(text) {
    content = text;
    // The sections that have landed aren't live any more.
    const left = Object.fromEntries(Object.entries(live).filter(([k]) => text.split('\n').some((l) => isPlaceholder(l, k))));
    if (Object.keys(left).length !== Object.keys(live).length) live = left;
    if (!dirty) {
      draft = text;
      base = text;
    }
  }

  /** A version Chat made: its writing, or a restore it did (`restore_document`). */
  function chatsOwn(v) {
    return v?.kind === 'chat' || (v?.kind === 'restored' && v?.label === 'Chat');
  }

  async function refreshUndo() {
    const want = id;
    if (want == null) return;
    try {
      const versions = (await invoke('note_versions', { id: want })) || [];
      if (id !== want) return;
      const notes = versions.filter((v) => v.field === 'notes');
      canUndo = notes.length > 1 && chatsOwn(notes[0]);
    } catch {
      canUndo = false;
    }
  }

  /** Show note `noteId` in the panel (the pill, a turn's document). */
  function show(noteId, opts = {}) {
    if (noteId == null) return;
    if (id !== noteId) {
      flushSave();
      id = noteId;
      title = opts.title ?? '';
      content = '';
      draft = '';
      base = '';
      dirty = false;
      view = 'preview';
      live = {};
      gone = false;
    }
    open = true;
    load();
  }

  /** A turn made or changed a document of this chat (`document` event): the panel shows it. */
  function fromTurn(ev) {
    busy = true;
    show(ev.noteId, { title: ev.title });
  }

  /** A section as it's written (`section` event). */
  function onSection(ev) {
    if (ev?.noteId == null) return;
    busy = true;
    action = 'writing';
    if (!open || id !== ev.noteId) show(ev.noteId);
    writing = ev.key ?? '';
    live = { ...live, [ev.key]: ev.text ?? '' };
  }

  /** A tool call began (`tool` event): a section on its way, or an edit of its blocks. */
  function onTool(ev) {
    if (!open) return;
    if (ev?.name === 'write_section' || ev?.name === 'rewrite_section') {
      busy = true;
      action = 'writing';
      writing = writing ?? '';
    } else if (ev?.name === 'edit_document' || ev?.name === 'restore_document') {
      busy = true;
      action = 'editing';
      writing = '';
    }
  }

  /** The turn is over: nothing is being written; the text as saved. */
  function turnEnded() {
    if (!busy) return;
    busy = false;
    writing = null;
    action = 'writing';
    live = {};
    if (open) load();
  }

  /** Another window (or Chat's tools) changed a note (`yap-note-changed`). */
  function onNoteChanged(p) {
    if (!p || p.id !== id || p.origin === PANEL_ORIGIN) return;
    if (typeof p.title === 'string') title = p.title;
    if (typeof p.content === 'string') {
      // A change landed: its blocks marked a moment on the page.
      if (p.content !== content) landed += 1;
      take(p.content);
    }
    refreshUndo();
  }

  /** A note was deleted (`yap-note-deleted`): this one says so. */
  function onNoteDeleted(p) {
    const gid = p?.id ?? p;
    if (gid != null && gid === id) gone = true;
  }

  function close() {
    flushSave();
    open = false;
    expanded = false;
    view = 'preview';
  }

  /** Preview (the page) or Source (the markdown, editable). */
  function setView(v) {
    const next = v === 'source' ? 'source' : 'preview';
    if (next === view) return;
    if (view === 'source') flushSave();
    if (next === 'source' && !dirty) {
      draft = content;
      base = content;
    }
    view = next;
  }

  function toggleEdit() {
    setView(view === 'source' ? 'preview' : 'source');
  }

  /** Where this document was scrolled to in view `v` (0: the top). */
  function scrollFor(v = view) {
    return id == null ? 0 : (scrolls.get(`${id}:${v}`) ?? 0);
  }

  /** Keep where it's scrolled to in view `v`. */
  function keepScroll(top, v = view) {
    if (id != null) scrolls.set(`${id}:${v}`, Math.max(0, Math.round(top)));
  }

  /** The Source box changed. */
  function input(text) {
    draft = text;
    dirty = true;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(flushSave, 600);
  }

  /** Save what's typed now (if anything is), merged with Chat's writing in Rust. */
  function flushSave() {
    if (saveTimer == null) return saving ?? Promise.resolve();
    clearTimeout(saveTimer);
    saveTimer = null;
    if (id == null || !dirty) return Promise.resolve();
    const sent = draft;
    const from = base;
    const noteId = id;
    const run = (saving ?? Promise.resolve()).then(async () => {
      try {
        const merged = await invoke('chat_document_save', { id: noteId, base: from, content: sent, origin: PANEL_ORIGIN });
        if (id !== noteId) return;
        content = merged;
        if (draft === sent) {
          // Chat's writing meanwhile, in the box too.
          draft = merged;
          base = merged;
          dirty = false;
        } else {
          // More was typed meanwhile: the next save lays that over what's saved.
          base = sent;
          clearTimeout(saveTimer);
          saveTimer = setTimeout(flushSave, 300);
        }
        refreshUndo();
      } catch (e) {
        if (!showSaveError(e)) toast({ title: "Couldn't save the document", description: String(e), variant: 'destructive' });
      }
    });
    saving = run.finally(() => {
      if (saving === run) saving = null;
    });
    return run;
  }

  /** Undo: back to the version before Chat's newest change (itself a version). */
  async function undo() {
    if (id == null) return;
    await flushSave();
    const noteId = id;
    try {
      const versions = ((await invoke('note_versions', { id: noteId })) || []).filter((v) => v.field === 'notes');
      if (versions.length < 2 || !chatsOwn(versions[0])) return;
      const r = await invoke('note_version_restore', { id: noteId, n: versions[1].n, origin: PANEL_ORIGIN });
      if (id === noteId && r?.field === 'notes') {
        dirty = false;
        landed += 1;
        take(r.text);
      }
      refreshUndo();
      toast({ kind: 'done', chip: 'Chat', title: "Chat's last change undone", description: 'It is in the document’s Version history (Open in Notes).' });
    } catch (e) {
      if (!showSaveError(e)) toast({ title: "Couldn't undo", description: String(e), variant: 'destructive' });
    }
  }

  /** Open in Notes: the note itself, in the Notes view. */
  async function openInNotes() {
    if (id == null) return;
    await flushSave();
    onopennote()?.({ id, line: null, ts: null, field: 'content' });
  }

  function setWidth(w, max = Infinity) {
    width = Math.round(Math.max(MIN_WIDTH, Math.min(w, max)));
  }

  /** The split as it is now, kept for next time (this window's convenience). */
  function keepWidth() {
    try {
      localStorage.setItem(WIDTH_KEY, String(width));
    } catch {
      /* private storage refused: the default next time */
    }
  }

  return {
    get open() { return open; },
    get id() { return id; },
    get title() { return title; },
    get content() { return content; },
    get shown() { return shown; },
    get pending() { return pending; },
    get gone() { return gone; },
    get expanded() { return expanded; },
    set expanded(v) { expanded = !!v; },
    get view() { return view; },
    get editing() { return view === 'source'; },
    get draft() { return draft; },
    get busy() { return busy; },
    get writing() { return writing; },
    get action() { return action; },
    get canUndo() { return canUndo; },
    get width() { return width; },
    get landed() { return landed; },
    show,
    fromTurn,
    onSection,
    onTool,
    turnEnded,
    onNoteChanged,
    onNoteDeleted,
    close,
    setView,
    toggleEdit,
    scrollFor,
    keepScroll,
    input,
    flushSave,
    undo,
    openInNotes,
    setWidth,
    keepWidth,
  };
}
