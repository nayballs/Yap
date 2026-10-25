// The meeting's summary, edited in place (notes/note_edits.rs): Edit turns it into
// its markdown, saved 600 ms after typing stops; Done (or Esc, another note,
// Generate summary) saves what's left. Kept while another tab is up, and
// read by the notepad's loads: words typed and not yet saved are never
// replaced by an older copy (`dirty`).
import { saveSummary } from '../core/noteEdits.js';
import { toast } from '../ui/toast.svelte.js';
import { LABEL } from './actions.js';

export class SummaryEditor {
  editing = $state(false);
  draft = $state('');
  /** Typed and not yet saved: an event or a read never puts older words back. */
  dirty = false;
  #timer = null;
  #note;
  #changing;

  /** `note()`: the note shown; `changing(field)`: the notepad's change count. */
  constructor({ note, changing }) {
    this.#note = note;
    this.#changing = changing;
  }

  /** Edit: the summary's markdown in the box. False with no note. */
  start() {
    const note = this.#note();
    if (!note) return false;
    this.draft = note.enhancedContent || '';
    this.editing = true;
    return true;
  }

  /** A keystroke: saved once typing stops. */
  queueSave() {
    this.dirty = true;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.flush(), 600);
  }

  async flush() {
    clearTimeout(this.#timer);
    this.#timer = null;
    const note = this.#note();
    if (!this.dirty || !note) return;
    this.dirty = false;
    const id = note.id;
    const text = this.draft;
    try {
      const s = await saveSummary(id, text, LABEL);
      const now = this.#note();
      if (now?.id !== id) return;
      this.#changing('summary');
      if (!this.dirty) now.enhancedContent = s.enhancedContent;
      now.summaryEditedMs = s.summaryEditedMs;
      now.summaryTs = s.summaryTs;
      if ('summaryAi' in s) now.summaryAi = s.summaryAi || '';
      if ('summaryRating' in s) now.summaryRating = s.summaryRating ?? null;
    } catch (e) {
      toast({ title: "Couldn't save the summary", description: String(e), variant: 'destructive' });
    }
  }

  /** Done: what was typed shows at once, and is saved. */
  finish() {
    if (!this.editing) return;
    const note = this.#note();
    if (this.dirty && note) note.enhancedContent = this.draft;
    this.flush();
    this.editing = false;
  }
}
