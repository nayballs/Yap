// Pure helpers for the meeting notepad (Notepad.svelte): what its copy, save
// and email actions put out, and how its transcript is laid out.
import { markdownToText } from './markdown.js';

const who = (seg) => (seg.source === 'you' ? 'You' : 'Them');

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
    for (const s of lines) out += `**${who(s)}:** ${s.text}\n\n`;
  }
  return out.trimEnd() + '\n';
}

/** The note as plain text (headings and emphasis dropped, tasks as ☐). */
export function noteText(note) {
  return markdownToText(noteMarkdown(note));
}

/** The transcript as "You: …" / "Them: …" lines. */
export function transcriptText(transcript) {
  return spoken(transcript)
    .map((s) => `${who(s)}: ${s.text}`)
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
    : [note?.content?.trim(), transcriptText(note?.transcript)].filter(Boolean).join('\n\n');
  if (body.length > MAIL_BODY_MAX) body = `${body.slice(0, MAIL_BODY_MAX).trimEnd()}…`;
  return `mailto:?subject=${encodeURIComponent(title)}&body=${encodeURIComponent(body)}`;
}

/**
 * What the Transcript tab shows, in order: speaker groups of lines (one chat
 * bubble each), a paused divider wherever a recording stopped (`breaks`: the
 * transcript's length at each stop), and a marker where the person dictated
 * mid-meeting (a `dictated` segment). Echo lines only with `showEcho`. With a
 * search `query`, only the lines that match it, in their groups, without
 * dividers.
 *   { kind: 'group', key, source, echo, lines: [{ text, i }] }
 *   { kind: 'pause', key } | { kind: 'dictated', key }
 */
export function transcriptItems(transcript, breaks = [], { showEcho = false, query = '' } = {}) {
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
      if (group && group.source === s.source && group.echo === echo) {
        group.lines.push({ text: s.text, i });
      } else {
        group = { kind: 'group', key: `g${i}`, source: s.source, echo, lines: [{ text: s.text, i }] };
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

/** "m:ss", or "h:mm:ss" past the hour. */
export function clock(secs) {
  const s = Math.max(0, Math.round(secs || 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}
