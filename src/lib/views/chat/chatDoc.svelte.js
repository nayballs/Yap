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
//   did), goes back to the version before Chat's last turn — every version
//   that turn made (`undoTarget`) — never to half of it (notes/versions.rs;
//   itself a version, so it can be undone too).
// - Copy (the whole document as Markdown) and Export as Markdown (the Notes
//   view's `note_export`): `docTools.js`, in the panel's own chunk.
// - **Stream-in** (2026-10-31): each section on its way to the page is
//   `live` (key → its text as it came); the panel lets it flow in at its
//   own pace (DocPanel, docStream.js), so it reads line by line instead of
//   in the bursts it comes in. A section that lands stays live as saved
//   (`settled`) until the panel has all of it shown (`release`); while a
//   turn writes, one a provider sent whole is live too.
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../../ui/toast.svelte.js';
import { showSaveError } from '../../notices/saveErrors.js';
import { isPlaceholder, pendingSections } from '../../core/markdown.js';
import { sectionText } from './docSections.js';

export { sectionRange } from './docSections.js';

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

/** A version Chat made: its writing, or a restore it did (`restore_document`). */
function chatsOwn(v) {
  return v?.kind === 'chat' || (v?.kind === 'restored' && v?.label === 'Chat');
}

/**
 * What the panel's Undo goes back to (`versions`: the document's notes
 * versions, newest first): the version before Chat's last turn. A turn
 * writes a document a call at a time — its outline, then each section —
 * and each is a version, all marked with the turn (`turn`,
 * notes/versions.rs `in_turn`): going back one call brought the last
 * section's placeholder back ("1 section not written yet", 31 Oct 2026).
 * `null`: the newest change isn't Chat's, or that turn made the document
 * (nothing of it before). A version without a turn (an older Yap's) is a
 * turn of its own. Pure.
 */
export function undoTarget(versions) {
  const newest = versions?.[0];
  if (!chatsOwn(newest)) return null;
  let i = 1;
  if (newest.turn) while (i < versions.length && versions[i].turn === newest.turn) i++;
  return versions[i] ?? null;
}

/** `key` among `keys`, as documents.rs matches a section's key (case aside). */
const hasKey = (keys, key) => keys.some((k) => k.trim().toLowerCase() === String(key).trim().toLowerCase());

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
  // known yet); 'writing' a section or 'editing' its blocks (edit_document,
  // restore_document).
  let busy = $state(false);
  let writing = $state(null);
  let action = $state('writing');
  // Each section on its way to the page: its text as it came (key → text).
  // One that landed (`settled`) leaves when the panel has it all shown.
  let live = $state({});
  const settled = new Set();
  // The newest change is Chat's (Undo shows).
  let canUndo = $state(false);
  let width = $state(loadWidth());
  // Counts the changes that landed from outside the panel (Chat's, Undo's):
  // the page marks the blocks new since the one before.
  let landed = $state(0);

  const pending = $derived(pendingSections(content));

  /** Nothing on its way to the page any more (another document, the panel closed). */
  function stopFlow() {
    settled.clear();
    live = {};
  }

  /** The panel has all of section `key` shown: once it has landed, it leaves. */
  function release(key) {
    if (!settled.has(key)) return;
    settled.delete(key);
    live = Object.fromEntries(Object.entries(live).filter(([k]) => k !== key));
  }

  async function load() {
    const want = id;
    if (want == null) return;
    try {
      const n = await invoke('note_get', { id: want });
      if (id !== want) return;
      gone = false;
      title = n.title || '';
      // A change that came this way first (a turn's `document` event can
      // beat the note's own `yap-note-changed`): marked as it lands too.
      if (content && (n.content || '') !== content) landed += 1;
      take(n.content || '');
      refreshUndo();
    } catch {
      if (id === want) gone = true;
    }
  }

  /** The document's text now (Rust's): shown, and the Source box's too unless typing is waiting to be saved. */
  function take(text) {
    const was = content;
    content = text;
    const lines = text.split('\n');
    const waiting = (k) => lines.some((l) => isPlaceholder(l, k));
    const next = { ...live };
    let changed = false;
    // A section that landed (or was rewritten) stays live as saved until
    // the panel has it all shown — unless it changed again since it landed
    // (a later edit): then it's shown as it is now, the change marked.
    for (const k of Object.keys(live)) {
      if (waiting(k)) continue;
      const saved = sectionText(text, k, live[k]);
      if (settled.has(k) && saved === live[k]) continue;
      changed = true;
      if (saved == null || settled.has(k)) {
        delete next[k];
        settled.delete(k);
      } else {
        next[k] = saved;
        settled.add(k);
      }
    }
    // While a turn writes: a section a provider sent whole flows in too.
    if (busy) {
      for (const { key } of pendingSections(was)) {
        if (waiting(key) || hasKey(Object.keys(next), key)) continue;
        const saved = sectionText(text, key);
        if (saved == null) continue;
        next[key] = saved;
        settled.add(key);
        changed = true;
      }
    }
    if (changed) live = next;
    if (!dirty) {
      draft = text;
      base = text;
    }
  }

  /** The document's notes versions, newest first. */
  async function notesVersions(noteId) {
    return ((await invoke('note_versions', { id: noteId })) || []).filter((v) => v.field === 'notes');
  }

  async function refreshUndo() {
    const want = id;
    if (want == null) return;
    try {
      const notes = await notesVersions(want);
      if (id !== want) return;
      canUndo = undoTarget(notes) != null;
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
      stopFlow();
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
    settled.delete(ev.key);
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
    // What's still flowing in finishes (as saved, once `load` takes it), then leaves.
    for (const k of Object.keys(live)) settled.add(k);
    if (open) load();
    else stopFlow();
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
    stopFlow();
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

  /** Undo: back to the version before Chat's last turn (itself a version). */
  async function undo() {
    if (id == null) return;
    await flushSave();
    const noteId = id;
    try {
      const target = undoTarget(await notesVersions(noteId));
      if (!target) return;
      const r = await invoke('note_version_restore', { id: noteId, n: target.n, origin: PANEL_ORIGIN });
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
    get live() { return live; },
    get streaming() { return busy || Object.keys(live).length > 0; },
    isSettled: (key) => settled.has(key),
    release,
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
