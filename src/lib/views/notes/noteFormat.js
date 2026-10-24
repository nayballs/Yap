// How the Notes view says a note's name, dates and times (pure helpers).

/** Title fallback = first 6 words of content (OpenWhispr's fallback). */
export function displayTitle(n) {
  if (n.title?.trim()) return n.title;
  const words = (n.preview || n.content || '').trim().split(/\s+/).filter(Boolean);
  if (!words.length) return 'Untitled note';
  return words.slice(0, 6).join(' ') + (words.length > 6 ? '…' : '');
}

/** "5 Oct": the date chip, and a list row's date beyond today. */
export function chipDate(ts) {
  return new Date((ts || 0) * 1000).toLocaleDateString(undefined, {
    day: 'numeric',
    month: 'short',
  });
}

/** Relative time for list rows (OpenWhispr's "now" badge style). */
export function relTime(ts) {
  const diff = Math.floor(Date.now() / 1000) - ts;
  if (diff < 60) return 'now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m`;
  const d = new Date(ts * 1000);
  if (d.toDateString() === new Date().toDateString()) {
    return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  }
  return chipDate(ts);
}

/** A list row's date: the time today, else "Oct 5". */
export function dateOf(ts) {
  const d = new Date(ts * 1000);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) {
    return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  }
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

/** A recording's running time: "4:05", "1:02:09". */
export function fmtElapsed(s) {
  const m = Math.floor(s / 60);
  const sec = String(s % 60).padStart(2, '0');
  return m >= 60 ? `${Math.floor(m / 60)}:${String(m % 60).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}

/** A point in the meeting: "40:12", "1:02:09". */
export function clock(secs) {
  const s = Math.max(0, Math.round(secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}
