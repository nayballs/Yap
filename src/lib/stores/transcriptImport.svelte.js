// Import a transcript… (TranscriptImportDialog.svelte, the main window
// mounts it once): Upload's "Import a transcript…", a transcript file
// dropped on Upload, and the Meetings view's ⋯ open it.
//
// Rust reads and keeps the files (meetings/import.rs): `transcript_import_pick`
// (Windows' dialog, several at once) or `transcript_import_add` (a drop's
// paths) answer with a dry preview of each — its format, the speakers
// found, lines, length, the date and where it came from, which speaker is
// you — or why it can't be read. The person can fix the date, say which
// speaker is them and name it; `transcript_import_run` makes a meeting note
// of each, then Rust writes their titles and summaries one after another
// (the action-plan job, `yap-meeting-summary`).
import { invoke } from '@tauri-apps/api/core';
import { toast } from '../ui/toast.svelte.js';

/** What the dialog reads: the transcript files offered in Upload and the picker. */
export const TRANSCRIPT_EXTENSIONS = ['vtt', 'srt', 'txt', 'docx', 'md', 'markdown'];

export const transcriptImport = $state({
  open: false,
  /**
   * The files read, in order: Rust's preview of each (`id`, `name`, `error`,
   * `formatLabel`, `title`, `start`, `startFrom`, `timeKnown`, `lengthMs`,
   * `estimated`, `lines`, `speakers`, `me`, `notes`, `transcriptDays`) and
   * the person's choices: `when` (a datetime-local value), `you` (a
   * speaker's name, '' for none) and `ownTitle` (a title of their own).
   */
  items: [],
  /** Reading files (the picker, a drop). */
  adding: false,
  importing: false,
  /** Why the picker or a drop failed as a whole. */
  error: '',
});

let opener = null;
let nav = { openNote: () => {} };

/** The main window says how to open a note in Notes. */
export function initTranscriptImport({ openNote } = {}) {
  if (openNote) nav.openNote = openNote;
}

/** Whether `name` is a file the import reads (by its extension). */
export function isTranscriptFile(name) {
  const ext = (String(name).split('.').pop() || '').toLowerCase();
  return TRANSCRIPT_EXTENSIONS.includes(ext);
}

/** Open the dialog, with `paths` already in it (a drop) or empty. */
export function openTranscriptImport(paths = null) {
  if (!transcriptImport.open) {
    const focused = document.activeElement;
    opener = focused instanceof HTMLElement && focused !== document.body ? focused : null;
    transcriptImport.error = '';
    transcriptImport.open = true;
  }
  if (paths?.length) addFiles(paths);
}

/** What had the focus when the dialog opened (it goes back there). */
export function importOpener() {
  return opener && document.contains(opener) ? opener : null;
}

/** Close it (not mid-import): nothing waits in Rust any more. */
export function closeTranscriptImport() {
  if (transcriptImport.importing) return;
  transcriptImport.open = false;
  transcriptImport.items = [];
  transcriptImport.error = '';
  invoke('transcript_import_clear').catch(() => {});
}

const pad = (n) => String(n).padStart(2, '0');

/** Unix seconds as a datetime-local value, in this PC's time. */
export function toLocalInput(secs) {
  const d = new Date(secs * 1000);
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** A datetime-local value as unix seconds (null when it isn't one). */
export function fromLocalInput(value) {
  const t = new Date(value).getTime();
  return Number.isFinite(t) ? Math.floor(t / 1000) : null;
}

/** "32 min", "1 h 5 min", "under a minute"; "about …" when the file has no times. */
export function lengthText(ms, estimated = false) {
  const min = Math.round((ms || 0) / 60000);
  let text;
  if (min < 1) text = 'under a minute';
  else if (min < 60) text = `${min} min`;
  else text = `${Math.floor(min / 60)} h${min % 60 ? ` ${min % 60} min` : ''}`;
  return estimated && min >= 1 ? `about ${text}` : text;
}

/** Where the date came from, as the dialog says it. */
export function whenFrom(item) {
  switch (item.startFrom) {
    case 'the transcript':
      return item.timeKnown ? 'From the transcript' : 'Date from the transcript: check the time';
    case "the file's name":
      return item.timeKnown ? "From the file's name" : "Date from the file's name: check the time";
    case "its folder's name":
      return "From its folder's name";
    case "the document's date":
      return "From the document's date: check it";
    case "the file's date":
      return "From when the file was saved: check it";
    default:
      return 'Pick when the meeting was';
  }
}

/** "Delete meeting transcripts after N days" will take this one's lines at once. */
export function retentionWarning(item, now = Date.now() / 1000) {
  const days = item.transcriptDays || 0;
  const start = fromLocalInput(item.when);
  if (!days || start == null) return '';
  const newest = start + (item.lengthMs || 0) / 1000;
  if (newest >= now - days * 86400) return '';
  return `Your settings delete meeting transcripts after ${days} days, so this meeting's lines will go within the hour. Its summary and notes stay.`;
}

function take(previews) {
  const now = Math.floor(Date.now() / 1000);
  for (const p of previews || []) {
    transcriptImport.items.push({
      ...p,
      when: toLocalInput(p.start ?? now),
      you: p.me || '',
      ownTitle: '',
    });
  }
}

/** "Choose files…": Windows' dialog, several at once. */
export async function chooseFiles() {
  if (transcriptImport.adding || transcriptImport.importing) return;
  transcriptImport.adding = true;
  transcriptImport.error = '';
  try {
    take(await invoke('transcript_import_pick'));
  } catch (e) {
    transcriptImport.error = String(e);
  } finally {
    transcriptImport.adding = false;
  }
}

/** Files dropped (their paths, from Tauri's drop). */
export async function addFiles(paths) {
  if (!paths?.length || transcriptImport.importing) return;
  transcriptImport.adding = true;
  transcriptImport.error = '';
  try {
    take(await invoke('transcript_import_add', { paths }));
  } catch (e) {
    transcriptImport.error = String(e);
  } finally {
    transcriptImport.adding = false;
  }
}

/** Take a file out of the list. */
export function removeItem(id) {
  transcriptImport.items = transcriptImport.items.filter((i) => i.id !== id);
  invoke('transcript_import_forget', { id }).catch(() => {});
}

/** The files that can be imported. */
export function readyItems() {
  return transcriptImport.items.filter((i) => !i.error && fromLocalInput(i.when) != null);
}

/**
 * Import every readable file: a meeting note each. The dialog closes when
 * they all went in; a file that didn't stays in it with why.
 */
export async function runImport() {
  const ready = readyItems();
  if (!ready.length || transcriptImport.importing) return;
  transcriptImport.importing = true;
  try {
    const outcomes = await invoke('transcript_import_run', {
      items: ready.map((i) => ({ id: i.id, start: fromLocalInput(i.when), me: i.you || null, title: i.ownTitle.trim() || null })),
    });
    const made = outcomes.filter((o) => o.noteId != null);
    const failed = new Map(outcomes.filter((o) => o.error).map((o) => [o.id, o.error]));
    // What went in leaves the list (and the files it couldn't read, said
    // already); what failed now stays, with why.
    transcriptImport.items = transcriptImport.items
      .filter((i) => failed.has(i.id))
      .map((i) => ({ ...i, error: failed.get(i.id) }));
    if (made.length) {
      const first = made[0];
      toast({
        title: made.length === 1 ? 'Meeting imported' : `${made.length} meetings imported`,
        description:
          made.length === 1
            ? `“${first.title}” is with your meetings. Its summary is on its way.`
            : 'They are with your meetings. Their summaries are on their way.',
        variant: 'success',
        action: { label: 'Open', onClick: () => nav.openNote(first.noteId) },
      });
    }
    transcriptImport.importing = false;
    if (!transcriptImport.items.length) closeTranscriptImport();
  } catch (e) {
    transcriptImport.error = String(e);
  } finally {
    transcriptImport.importing = false;
  }
}
