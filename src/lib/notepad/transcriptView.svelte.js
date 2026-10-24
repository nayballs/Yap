// What the Transcript tab shows, kept while another tab is up (a tab's body
// is made again on every switch, notepad/Notepad.svelte): the search, the
// lines the mic picked up from the speakers, whether the log follows the
// newest line, the speaker menu, the line being fixed, and the copy ticks.
// One per notepad; another note starts it afresh (`reset`).

const HINT_KEY = 'yapNotepadHintGone';

function readFlag(key) {
  try {
    return localStorage.getItem(key) === '1';
  } catch {
    return false;
  }
}

export class TranscriptView {
  showEcho = $state(false);
  searchOpen = $state(false);
  query = $state('');
  /** Follow the newest line, unless the person scrolled up to read. */
  followLog = $state(true);
  /** The menu on a speaker's name (a group's: its lines) or on one line's
   *  person button: `{ key, voice, lines, start }` (SpeakerMenu.svelte). */
  speakerMenu = $state(null);
  /** The line whose words are being fixed: `{ i, text }` (notes/note_edits.rs). */
  editingLine = $state(null);
  /** The line just copied (its ✓), and the whole transcript's. */
  copiedLine = $state(null);
  transcriptCopied = $state(false);
  /** The tip about the live transcript, dismissible for good. */
  hintGone = $state(readFlag(HINT_KEY));

  searchQuery = $derived(this.searchOpen ? this.query.trim() : '');

  /** Another note: the search, the echo lines and the menus start afresh. */
  reset() {
    this.showEcho = false;
    this.followLog = true;
    this.searchOpen = false;
    this.query = '';
    this.speakerMenu = null;
  }

  closeSearch() {
    this.searchOpen = false;
    this.query = '';
  }

  toggleSpeakerMenu(key, voice, lines, start = 'menu') {
    this.speakerMenu = this.speakerMenu?.key === key ? null : { key, voice, lines, start };
  }

  dismissHint() {
    this.hintGone = true;
    try {
      localStorage.setItem(HINT_KEY, '1');
    } catch {
      /* private storage: just this session */
    }
  }
}
