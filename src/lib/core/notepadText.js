// Pure helpers for the meeting notepad (Notepad.svelte) and the Notes view's
// transcript: what the copy, save and email actions put out, how a
// transcript is laid out, and what each line's speaker is called (the
// note's speaker map, src-tauri/src/speakers.rs, read the same way).
import { markdownToText } from './markdown.js';
import { shortMicName } from './mics.js';

/** The voice of the person using Yap (the mic). */
export const YOU = 'you';

/**
 * The voice a line is (speakers.rs `voice_of`): its `speaker`, else its side
 * (`source`, every line nobody relabelled), through any merges in the map.
 */
export function speakerId(speakers, seg) {
  let id = seg?.speaker || seg?.source || 'them';
  for (let i = 0; i < 16; i += 1) {
    const into = speakers?.[id]?.mergedInto;
    if (!into || into === id) break;
    id = into;
  }
  return id;
}

/** What an unnamed voice is called: "You", "Them", "Speaker 2". */
export function defaultSpeakerName(id) {
  if (id === YOU) return 'You';
  if (id === 'them') return 'Them';
  const n = /^them-(\d+)$/.exec(id || '');
  return n ? `Speaker ${n[1]}` : 'Someone';
}

/**
 * What voice `id` (resolved) is called (speakers.rs `name_of`/`display`):
 * `{ id, name, label, guess, named, locked }`. A calendar or AI name nobody
 * confirmed is a guess, labelled "Priya?". "You" is always You.
 */
export function speakerInfo(speakers, id) {
  if (id === YOU) return { id, name: 'You', label: 'You', guess: false, named: false, locked: false };
  const s = speakers?.[id];
  const named = !!s?.name?.trim();
  const guess = named && !s.locked && (s.source === 'calendar' || s.source === 'ai');
  const name = named ? s.name.trim() : defaultSpeakerName(id);
  return { id, name, label: guess ? `${name}?` : name, guess, named, locked: !!s?.locked };
}

/** How the transcript labels line `seg`: "You", "Them", "Priya", "Priya?". */
export const speakerLabel = (speakers, seg) => speakerInfo(speakers, speakerId(speakers, seg)).label;
const who = speakerLabel;

/**
 * The voices of a note, resolved (speakers.rs `voices`): those its lines are
 * in, in order, then any other the map knows — but not "Them" once the
 * voice separation told every call line apart (a 1:1 guess it had went to
 * a voice).
 */
export function noteVoices(note) {
  const out = [];
  const add = (id) => !out.includes(id) && out.push(id);
  for (const s of note?.transcript || []) if (!s.echo && !s.dictated && !s.failed && s.mic == null) add(speakerId(note.speakers, s));
  const toldApart = !out.includes('them') && out.some((id) => id.startsWith('them-'));
  for (const id of Object.keys(note?.speakers || {})) {
    const v = speakerId(note.speakers, { speaker: id });
    if (!(toldApart && v === 'them')) add(v);
  }
  return out;
}

/**
 * The same line, arriving twice (`yap-meeting-segment` after a fetch that
 * already had it, or after the voice separation split it: its first part
 * keeps the line's side, date and place).
 */
export function hasLine(transcript, seg) {
  if (!seg || seg.startMs == null || !Array.isArray(transcript)) return false;
  for (let i = transcript.length - 1, n = 0; i >= 0 && n < 64; i -= 1, n += 1) {
    const s = transcript[i];
    if (s.source === seg.source && s.ts === seg.ts && s.startMs === seg.startMs) return true;
  }
  return false;
}

/**
 * A transcript after the voice separation labelled lines (`yap-note-changed`
 * from speakers/labels.rs: `lineSplice` `{ at, remove, lines }` — lines
 * split where the speaker changed, so the ones after moved — and
 * `lineSpeakers` for every line after it), or `null` when this copy isn't
 * the one it changed (a line arrived meanwhile: fetch the note).
 */
export function withLineSplice(transcript, change) {
  const splice = change?.lineSplice;
  if (!splice || !Array.isArray(transcript) || !Array.isArray(change.lineSpeakers)) return null;
  const { at, remove, lines } = splice;
  if (!Array.isArray(lines) || at < 0 || at + remove > transcript.length) return null;
  if (transcript.length - remove + lines.length !== change.lineSpeakers.length) return null;
  const first = transcript[at];
  if (remove > 0 && (first?.source !== lines[0]?.source || first?.ts !== lines[0]?.ts || first?.startMs !== lines[0]?.startMs)) {
    return null;
  }
  return withLineSpeakers([...transcript.slice(0, at), ...lines, ...transcript.slice(at + remove)], change.lineSpeakers);
}

/**
 * A transcript with each line's voice from a `yap-note-changed` after a
 * speaker edit (`lineSpeakers`: `[speaker, locked, echo]` per line; `echo`
 * since "It was me" hides a voice's repeats of your lines), or `null` when
 * the lengths differ (a line arrived meanwhile: fetch the note).
 */
export function withLineSpeakers(transcript, lineSpeakers) {
  if (!Array.isArray(lineSpeakers) || !Array.isArray(transcript) || lineSpeakers.length !== transcript.length) {
    return null;
  }
  return transcript.map((s, i) => {
    const entry = lineSpeakers[i] || [];
    const [speaker, locked] = entry;
    const out = { ...s, speaker: speaker && speaker !== s.source ? speaker : '', speakerLocked: !!locked };
    if (entry.length > 2) out.echo = !!entry[2];
    return out;
  });
}

/**
 * The name cards still to answer (speakers/naming.rs): `[{ voice, name,
 * me, confidence, why }]`, surest first — voices with a suggestion that
 * wasn't put off ("Not now"), not merged into another, with lines of
 * their own.
 */
export function pendingSuggestions(note) {
  const voices = new Set(noteVoices(note));
  return Object.entries(note?.speakers || {})
    .filter(([id, s]) => s?.suggestion && !s.suggestion.later && !s.mergedInto && voices.has(id))
    .map(([voice, s]) => ({ voice, ...s.suggestion }))
    .sort((a, b) => (b.confidence || 0) - (a.confidence || 0));
}

/**
 * The voice flags still to answer (speakers/flags.rs): two voices that may
 * be one (`{ kind: 'alike', voice, with, score }`) and a voice that may be
 * two people (`{ kind: 'split', voice, moved, kept, score }`), not put off
 * ("Not now"), each voice with lines of its own — the pairs first.
 */
export function pendingFlags(note) {
  const onLines = new Set(
    (note?.transcript || [])
      .filter((s) => !s.echo && !s.dictated && !s.failed && s.mic == null)
      .map((s) => speakerId(note.speakers, s))
  );
  const out = [];
  for (const [voice, s] of Object.entries(note?.speakers || {})) {
    if (s?.mergedInto || !onLines.has(voice)) continue;
    if (s.alike && !s.alike.later) {
      const other = speakerId(note.speakers, { speaker: s.alike.with });
      if (other !== voice && onLines.has(other)) out.push({ kind: 'alike', voice, with: other, score: s.alike.score || 0 });
    }
    if (s.split && !s.split.later && s.split.lines?.length) {
      out.push({ kind: 'split', voice, moved: s.split.lines.length, kept: s.split.kept || 0, score: s.split.score || 0 });
    }
  }
  return out.sort((a, b) => (a.kind === b.kind ? (b.score || 0) - (a.score || 0) : a.kind === 'alike' ? -1 : 1));
}

/** "Speaker 2 and Speaker 4 may be one voice" / "Speaker 3 may be two people". */
export function flagQuestion(note, flag) {
  const who = cardVoice(note, flag.voice);
  return flag.kind === 'alike' ? `${who} and ${cardVoice(note, flag.with)} may be one voice` : `${who} may be two people`;
}

/** The flag card's small print. */
export function flagWhy(flag) {
  if (flag.kind === 'alike') return 'They sound alike. Listen to both, and merge them if they’re one person.';
  const lines = flag.moved === 1 ? '1 of their lines sounds' : `${flag.moved} of their lines sound`;
  return `${lines} like someone else. Split makes those a new speaker.`;
}

/** What a card calls the voice it asks about: "Speaker 2" (not the guess on it). */
function cardVoice(note, voice) {
  const info = speakerInfo(note?.speakers, voice);
  return info.guess ? defaultSpeakerName(voice) : info.label;
}

/** "Speaker 2 is Priya?" / "Speaker 3 is you?" */
export function suggestionQuestion(note, card) {
  const who = cardVoice(note, card.voice);
  return card.me ? `${who} is you?` : `${who} is ${card.name}?`;
}

/** Why Yap thinks so, in a line (the card's small print). */
export function suggestionWhy(note, card) {
  const why = card.why || {};
  const who = cardVoice(note, card.voice);
  const by = why.by ? speakerInfo(note?.speakers, why.by).label : 'Someone';
  const quote = why.quote ? `“${why.quote}”` : '';
  const first = (card.name || '').split(/\s+/)[0];
  switch (why.kind) {
    case 'introduced':
      return `${who} said ${quote}`;
    case 'asked':
      return `${by} asked ${quote} and ${who} answered`;
    case 'thanked':
      return `${by} said ${quote} right after ${who} spoke`;
    case 'invite':
      return `${first} is the only other person invited`;
    case 'app':
      return `The call app showed ${first} talking while ${who} spoke`;
    case 'model':
      return quote ? `The AI matched ${quote}` : `The AI matched it from the transcript`;
    case 'echo':
      return `It repeats what you said: your voice may be reaching the call audio`;
    default:
      return '';
  }
}

/** Spoken lines worth sharing: no echo (the call through the speakers), no markers. */
const spoken = (transcript) => (transcript || []).filter((s) => !s.echo && !s.dictated && !s.failed && s.mic == null);

/**
 * The note as Markdown, laid out as "Save as .md" writes it (commands.rs
 * `note_export`): the title, attendees, the summary (with the typed notes
 * after it) or the typed notes, then the transcript.
 */
export function noteMarkdown(note) {
  const title = note?.title?.trim() || 'Untitled note';
  let out = `# ${title}\n\n`;
  if (note?.participants?.length) out += `Attendees: ${note.participants.join(', ')}\n\n`;
  const summary = note?.enhancedContent?.trim();
  const typed = note?.content?.trim();
  if (summary) {
    out += `${summary}\n`;
    if (typed) out += `\n---\n\n## My thoughts\n\n${typed}\n`;
  } else if (typed) {
    out += `${typed}\n`;
  }
  const lines = spoken(note?.transcript);
  if (lines.length) {
    out += '\n## Meeting Transcript\n\n';
    for (const s of lines) out += `**${who(note.speakers, s)}:** ${s.text}\n\n`;
  }
  return out.trimEnd() + '\n';
}

/** The note as plain text (headings and emphasis dropped, tasks as ☐). */
export function noteText(note) {
  return markdownToText(noteMarkdown(note));
}

/** The transcript as "You: …" / "Them: …" lines (or the speakers' names). */
export function transcriptText(transcript, speakers = {}) {
  return spoken(transcript)
    .map((s) => `${who(speakers, s)}: ${s.text}`)
    .join('\n');
}

/** Mail clients cut long mailto: links; keep the body well under that. */
const MAIL_BODY_MAX = 1800;

/**
 * A mailto: link for the note: the summary as text, or (none yet) the typed
 * notes and the transcript, cut short to fit a mail client's link limit.
 */
export function mailtoUrl(note) {
  const title = note?.title?.trim() || 'Meeting notes';
  let body = note?.enhancedContent?.trim()
    ? markdownToText(note.enhancedContent)
    : [note?.content?.trim(), transcriptText(note?.transcript, note?.speakers)].filter(Boolean).join('\n\n');
  if (body.length > MAIL_BODY_MAX) body = `${body.slice(0, MAIL_BODY_MAX).trimEnd()}…`;
  return `mailto:?subject=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
}

/**
 * What the Transcript tab shows, in order: speaker groups of lines (one chat
 * bubble each; a group per run of one voice, named from the note's
 * `speakers` map), a paused divider wherever a recording stopped (`breaks`:
 * the transcript's length at each stop), and a marker where the person
 * dictated mid-meeting (a `dictated` segment), and a "Couldn't transcribe
 * 0:42–1:12" line where a chunk failed (a `failed` segment, meetings/meeting.rs; line
 * `i` of the transcript), and "Your mic was disconnected" where the
 * meeting's microphone went and "You" moved to the next one (a `mic`
 * segment: its name, or "" with none left). Echo lines only with `showEcho`. With a search
 * `query`, only the lines that match it, in their groups, without dividers.
 *   { kind: 'group', key, source ('you' | 'them': the user or someone else),
 *     speaker (the voice), label, guess, echo, lines: [{ text, i, edited,
 *     original }] } — `edited`: the person fixed its words (`original`: what
 *     Yap heard)
 *   { kind: 'pause', key } | { kind: 'dictated', key } | { kind: 'failed', key, i }
 *   | { kind: 'mic', key, next } (`next`: the microphone's short name, or '')
 */
export function transcriptItems(transcript, breaks = [], { showEcho = false, query = '', speakers = {} } = {}) {
  const q = query.trim().toLowerCase();
  const stops = new Set(breaks || []);
  const items = [];
  let group = null;
  (transcript || []).forEach((s, i) => {
    if (s.dictated) {
      if (!q) items.push({ kind: 'dictated', key: `d${i}` });
      group = null;
    } else if (s.mic != null) {
      if (!q) items.push({ kind: 'mic', key: `m${i}`, next: s.mic ? shortMicName(s.mic) : '' });
      group = null;
    } else if (s.failed) {
      if (!q) items.push({ kind: 'failed', key: `f${i}`, i });
      group = null;
    } else if ((!s.echo || showEcho) && (!q || (s.text || '').toLowerCase().includes(q))) {
      const echo = !!s.echo;
      const speaker = speakerId(speakers, s);
      const line = { text: s.text, i, edited: !!s.edited, original: s.original || '' };
      if (group && group.speaker === speaker && group.echo === echo) {
        group.lines.push(line);
      } else {
        const { label, guess } = speakerInfo(speakers, speaker);
        const source = speaker === YOU ? 'you' : 'them';
        group = { kind: 'group', key: `g${i}`, source, speaker, label, guess, echo, lines: [line] };
        items.push(group);
      }
    }
    if (stops.has(i + 1) && !q) {
      items.push({ kind: 'pause', key: `p${i + 1}` });
      group = null;
    }
  });
  return items;
}

/**
 * A line of a speaker group as a chat bubble: which corner is tight. The
 * first of a run is tight bottom-left, the last tight top-left, the ones in
 * between both (Wispr Flow's grouped bubbles).
 */
export function bubbleShape(index, count) {
  if (count <= 1 || index === 0) return 'first';
  return index === count - 1 ? 'last' : 'mid';
}

/** `text` split around matches of `query` (case-insensitive), for highlighting. */
export function highlightParts(text, query) {
  const q = query.trim();
  if (!q) return [{ text, hit: false }];
  const parts = [];
  const lower = text.toLowerCase();
  const needle = q.toLowerCase();
  let at = 0;
  for (;;) {
    const found = lower.indexOf(needle, at);
    if (found < 0) break;
    if (found > at) parts.push({ text: text.slice(at, found), hit: false });
    parts.push({ text: text.slice(found, found + needle.length), hit: true });
    at = found + needle.length;
  }
  if (at < text.length) parts.push({ text: text.slice(at), hit: false });
  return parts;
}

/** "5 Oct, 22:32" (en-GB), "Oct 5, 10:32 PM" (en-US): the locale's own. */
export function dateLine(ts) {
  const d = new Date((ts || 0) * 1000);
  const day = d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
  const time = d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  return `${day}, ${time}`;
}

/** What "Copy consent message" copies until the person edits it. */
export const DEFAULT_CONSENT_MESSAGE =
  "Hi all, I'm taking notes with Yap, which transcribes this call on my computer. Let me know if you'd rather I didn't.";

/** The consent message as saved (`meetingConsentMessage`; empty = the default). */
export function consentMessage(saved) {
  return saved?.trim() || DEFAULT_CONSENT_MESSAGE;
}

/** What to save for an edited consent message: '' for the default (or nothing). */
export function consentToSave(text) {
  const t = (text || '').trim();
  return t === DEFAULT_CONSENT_MESSAGE ? '' : t;
}

/**
 * The one-tap AI helpers on a meeting (meetings/meeting_assist.rs): the chip's label
 * (also the question bubble) and the line shown while it works. `catchUp` is
 * "What did I miss?" (`meeting_catch_up`); the others go to `meeting_assist`
 * with their key as the `kind`.
 */
export const MEETING_HELPERS = {
  catchUp: { label: 'What did I miss?', busy: 'Catching you up…' },
  questions: { label: 'Questions to ask', busy: 'Thinking of questions…' },
  followUpEmail: { label: 'Follow-up email', busy: 'Drafting the email…' },
  blindSpots: { label: 'Gaps and risks', busy: 'Looking for loose ends…' },
};

/**
 * Which helpers a meeting note offers: while it records, catching up and
 * questions to ask; after it, the follow-up email and the gaps, once there's
 * something to go on (talk, typed notes or a summary).
 */
export function meetingHelpers(note, recording) {
  if (!note) return [];
  if (recording) return ['catchUp', 'questions'];
  const material = spoken(note.transcript).length || note.content?.trim() || note.enhancedContent?.trim();
  return material ? ['followUpEmail', 'blindSpots'] : [];
}

// ---- fixing notes by hand (noteEdits.js, src-tauri/src/notes/note_edits.rs, notes/tasks.rs) ----

/** Line `i` of a transcript as `transcript_line_edit` finds it again (`LineRef`). */
export function lineRef(transcript, i) {
  const s = transcript?.[i] || {};
  return { index: i, source: s.source, ts: s.ts, startMs: s.startMs ?? null, text: s.text ?? '' };
}

/**
 * A transcript with another window's line edit in it (`yap-note-changed`'s
 * `lineEdit { index, line }`), or `null` when this copy isn't the one it
 * changed (lines moved meanwhile: fetch the note).
 */
export function withLineEdit(transcript, edit) {
  if (!Array.isArray(transcript) || !edit?.line) return null;
  const old = transcript[edit.index];
  const line = edit.line;
  if (!old || old.source !== line.source || old.ts !== line.ts || (old.startMs ?? null) !== (line.startMs ?? null)) return null;
  const next = transcript.slice();
  next[edit.index] = { ...old, text: line.text, edited: !!line.edited, original: line.original || '' };
  return next;
}

// A checkbox line: "- [ ] words" (also "*", "+", "•"), any indent.
const TASK_LINE = /^(\s*[-*+•][ \t]+\[)([ xX])(\][ \t]+)(.*)$/;

/** Line `line`'s task words, as notes/tasks.rs reads them ('' when it's no task). */
export function taskText(md, line) {
  const m = TASK_LINE.exec((md || '').split('\n')[line] ?? '');
  return m ? m[4].trim() : '';
}

/** `md` with the checkbox on `line` ticked (`done`) or not: the instant change on screen. */
export function withTaskMark(md, line, done) {
  const lines = (md || '').split('\n');
  const m = TASK_LINE.exec(lines[line] ?? '');
  if (!m) return md;
  lines[line] = `${m[1]}${done ? 'x' : ' '}${m[3]}${m[4]}`;
  return lines.join('\n');
}

/**
 * Talk lines dated after the summary read the meeting: what "New since
 * this summary · Update" counts (0 on older notes, whose summary's time
 * isn't known).
 */
export function newSinceSummary(note) {
  if (!note?.summaryTs || !note.enhancedContent?.trim()) return 0;
  return (note.transcript || []).filter((s) => !s.echo && !s.dictated && s.text?.trim() && s.ts > note.summaryTs).length;
}

/** "3 new lines since this summary" / "1 new line since this summary". */
export function newSinceLine(n) {
  return `${n} new ${n === 1 ? 'line' : 'lines'} since this summary`;
}

/** "m:ss", or "h:mm:ss" past the hour. */
/**
 * A failed chunk's marker as its line says it: "Couldn't transcribe
 * 0:42–1:12" (its stretch on its recording's clock).
 */
export function failedText(seg) {
  return `Couldn't transcribe ${clock((seg?.startMs || 0) / 1000)}–${clock((seg?.endMs || 0) / 1000)}`;
}

export function clock(secs) {
  const s = Math.max(0, Math.round(secs || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}
