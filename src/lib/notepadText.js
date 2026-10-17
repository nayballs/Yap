// Pure helpers for the meeting notepad (Notepad.svelte) and the Notes view's
// transcript: what the copy, save and email actions put out, how a
// transcript is laid out, and what each line's speaker is called (the
// note's speaker map, src-tauri/src/speakers.rs, read the same way).
import { markdownToText } from './markdown.js';

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
 * in, in order, then any other the map knows.
 */
export function noteVoices(note) {
  const out = [];
  const add = (id) => !out.includes(id) && out.push(id);
  for (const s of note?.transcript || []) if (!s.echo && !s.dictated) add(speakerId(note.speakers, s));
  for (const id of Object.keys(note?.speakers || {})) add(speakerId(note.speakers, { speaker: id }));
  return out;
}

/**
 * A transcript with each line's voice from a `yap-note-changed` after a
 * speaker edit (`lineSpeakers`: `[speaker, locked]` per line), or `null`
 * when the lengths differ (a line arrived meanwhile: fetch the note).
 */
export function withLineSpeakers(transcript, lineSpeakers) {
  if (!Array.isArray(lineSpeakers) || !Array.isArray(transcript) || lineSpeakers.length !== transcript.length) {
    return null;
  }
  return transcript.map((s, i) => {
    const [speaker, locked] = lineSpeakers[i] || [];
    return { ...s, speaker: speaker && speaker !== s.source ? speaker : '', speakerLocked: !!locked };
  });
}

/** Spoken lines worth sharing: no echo (the call through the speakers), no markers. */
const spoken = (transcript) => (transcript || []).filter((s) => !s.echo && !s.dictated);

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
    if (typed) out += `\n---\n\n## Raw notes\n\n${typed}\n`;
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
 * dictated mid-meeting (a `dictated` segment). Echo lines only with
 * `showEcho`. With a search `query`, only the lines that match it, in their
 * groups, without dividers.
 *   { kind: 'group', key, source ('you' | 'them': the user or someone else),
 *     speaker (the voice), label, guess, echo, lines: [{ text, i }] }
 *   { kind: 'pause', key } | { kind: 'dictated', key }
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
    } else if ((!s.echo || showEcho) && (!q || (s.text || '').toLowerCase().includes(q))) {
      const echo = !!s.echo;
      const speaker = speakerId(speakers, s);
      if (group && group.speaker === speaker && group.echo === echo) {
        group.lines.push({ text: s.text, i });
      } else {
        const { label, guess } = speakerInfo(speakers, speaker);
        const source = speaker === YOU ? 'you' : 'them';
        group = { kind: 'group', key: `g${i}`, source, speaker, label, guess, echo, lines: [{ text: s.text, i }] };
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
 * The one-tap AI helpers on a meeting (meeting_assist.rs): the chip's label
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

/** "m:ss", or "h:mm:ss" past the hour. */
export function clock(secs) {
  const s = Math.max(0, Math.round(secs || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}
