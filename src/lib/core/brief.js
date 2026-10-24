// A meeting's pre-meeting brief (calendar/brief.rs) in one line, for the
// places that show it folded: the Yap bar's call card, the notepad's strip
// over My thoughts, the Meetings view. "Last met Thu 16 Oct · 2 open items",
// or "First meeting with these people".

/** "Thu 16 Oct". */
function day(secs) {
  return new Date(secs * 1000).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
}

/** How many open items the brief lists. */
export function openCount(brief) {
  return (brief?.parts ?? []).filter((p) => p.kind === 'open').reduce((n, p) => n + p.lines.length, 0);
}

/** The brief's one-line teaser. */
export function briefTeaser(brief) {
  if (!brief) return '';
  if (brief.first) {
    return brief.parts?.find((p) => p.kind === 'last')?.lines?.[0]?.text || 'First meeting with these people';
  }
  const last = brief.parts?.find((p) => p.kind === 'last')?.lines?.find((l) => l.cites.length);
  const source = last && brief.sources?.find((s) => s.n === last.cites[0]);
  const bits = [];
  if (source?.date) bits.push(`Last met ${day(source.date)}`);
  const open = openCount(brief);
  if (open) bits.push(`${open} open ${open === 1 ? 'item' : 'items'}`);
  return bits.join(' · ') || 'What to know going in';
}

/** A brief worth comparing: its made time, fingerprint and writing state. */
export function briefStamp(brief) {
  return brief ? `${brief.madeTs}:${brief.fingerprint}:${brief.writing ? 1 : 0}` : '';
}
