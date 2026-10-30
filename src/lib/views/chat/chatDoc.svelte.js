// The document panel beside Chat's conversation (src-tauri/src/chat/documents.rs):
// which document it shows, its text, the sections being written as they
// stream (`section` events), the Edit box and its saves, expanded or split,
// and the split's width (remembered).
//
// - A turn makes or changes a document: `document` events open the panel
//   on it (`fromTurn`), `section` events fill a placeholder as the model
//   writes it (`onSection`), and the note's own `yap-note-changed` brings
//   the text as each section lands (`onNoteChanged`).
// - The person's words win: the Edit box saves through `chat_document_save`
//   with the text it started from (`base`), and Rust lays the person's edit
//   over whatever Chat wrote meanwhile (documents.rs `merge3`). A save that
//   comes back while more was typed keeps the newer typing and saves again
//   from the text it was typed over.
// - Undo: the newest change, when it's Chat's, goes back to the version
//   before it (notes/versions.rs; itself a version, so it can be undone too).
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

function loadWidth() {
  try {
    const w = Number(localStorage.getItem(WIDTH_KEY));
    return Number.isFinite(w) && w >= MIN_WIDTH ? w : DEFAULT_WIDTH;
  } catch {
    return DEFAULT_WIDTH;
  }
}

/**
 * A document's text as the panel shows it: without its first line when
 * that's the title (the header says it), and each section still being
 * written in its placeholder's place (`live`: key → its text so far). Pure.
 */
export function shownText(content, title, live = {}) {
  let lines = String(content || '').split('\n');
  const first = lines[0]?.trim() ?? '';
  if (/^#\s+/.test(first) && first.replace(/^#\s+/, '').trim() === String(title || '').trim()) {
    lines = lines.slice(1);
  }
  const keys = Object.keys(live);
  if (keys.length) {
    lines = lines.map((line) => {
      const key = keys.find((k) => isPlaceholder(line, k));
      if (key == null) return line;
      const text = live[key] ?? '';
      // Its own heading when it wrote one; else the key's.
      const own = /^#{1,2}\s+\S/.test(text.trimStart());
      return own ? closePartial(text.trimStart().replace(/^#\s/, '## ')) : `## ${key}\n\n${closePartial(text)}`;
    });
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
  let editing = $state(false);
  let draft = $state('');
  // The text the Edit box started from, or last took in.
  let base = '';
  let dirty = false;
  let saveTimer = null;
  let saving = null;
  // A turn is writing it now; the section on the way (its key, '' not
  // known yet); each section's text as it streams.
  let busy = $state(false);
  let writing = $state(null);
  let live = $state({});
  // The newest change is Chat's (Undo shows).
  let canUndo = $state(false);
  let width = $state(loadWidth());

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

  /** The document's text now (Rust's): shown, and the Edit box's too unless typing is waiting to be saved. */
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

  async function refreshUndo() {
    const want = id;
    if (want == null) return;
    try {
      const versions = (await invoke('note_versions', { id: want })) || [];
      if (id !== want) return;
      const notes = versions.filter((v) => v.field === 'notes');
      canUndo = notes.length > 1 && notes[0].kind === 'chat';
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
      editing = false;
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
    if (!open || id !== ev.noteId) show(ev.noteId);
    writing = ev.key ?? '';
    live = { ...live, [ev.key]: ev.text ?? '' };
  }

  /** A tool call began (`tool` event): a section is on its way. */
  function onTool(ev) {
    if (open && ev?.name === 'write_section') {
      busy = true;
      writing = writing ?? '';
    }
  }

  /** The turn is over: nothing is being written; the text as saved. */
  function turnEnded() {
    if (!busy) return;
    busy = false;
    writing = null;
    live = {};
    if (open) load();
  }

  /** Another window (or Chat's tools) changed a note (`yap-note-changed`). */
  function onNoteChanged(p) {
    if (!p || p.id !== id || p.origin === PANEL_ORIGIN) return;
    if (typeof p.title === 'string') title = p.title;
    if (typeof p.content === 'string') take(p.content);
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
    editing = false;
  }

  function toggleEdit() {
    if (editing) {
      flushSave();
      editing = false;
      return;
    }
    if (!dirty) {
      draft = content;
      base = content;
    }
    editing = true;
  }

  /** The Edit box changed. */
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
      if (versions.length < 2 || versions[0].kind !== 'chat') return;
      const r = await invoke('note_version_restore', { id: noteId, n: versions[1].n, origin: PANEL_ORIGIN });
      if (id === noteId && r?.field === 'notes') {
        dirty = false;
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
    get editing() { return editing; },
    get draft() { return draft; },
    get busy() { return busy; },
    get writing() { return writing; },
    get canUndo() { return canUndo; },
    get width() { return width; },
    show,
    fromTurn,
    onSection,
    onTool,
    turnEnded,
    onNoteChanged,
    onNoteDeleted,
    close,
    toggleEdit,
    input,
    flushSave,
    undo,
    openInNotes,
    setWidth,
    keepWidth,
  };
}
