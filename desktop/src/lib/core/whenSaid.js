// **When a message was said** (16 Nov 2026, the Claude app's rows under a
// message: "12 minutes ago"): its time in words, and in full for a hover.
// Pure, no imports: Yap Link's phone page reads it as the desktop does
// (desktop/src-tauri/build.rs `LINK_SHARED`).

const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/**
 * `secs` (unix seconds) in words, as of `now` (seconds): "just now", "1 minute
 * ago", "12 minutes ago", "1 hour ago", "5 hours ago", "yesterday", "3 days
 * ago", then the day ("4 Nov", "4 Nov 2025" another year). '' with no time.
 */
export function whenSaid(secs, now = Date.now() / 1000) {
  const t = Number(secs);
  if (!Number.isFinite(t) || t <= 0) return '';
  const d = Math.max(0, now - t);
  if (d < MINUTE) return 'just now';
  if (d < HOUR) {
    const m = Math.floor(d / MINUTE);
    return m === 1 ? '1 minute ago' : `${m} minutes ago`;
  }
  if (d < DAY) {
    const h = Math.floor(d / HOUR);
    return h === 1 ? '1 hour ago' : `${h} hours ago`;
  }
  if (d < 2 * DAY) return 'yesterday';
  if (d < 7 * DAY) return `${Math.floor(d / DAY)} days ago`;
  const at = new Date(t * 1000);
  const year = at.getFullYear() === new Date(now * 1000).getFullYear() ? '' : ` ${at.getFullYear()}`;
  return `${at.getDate()} ${MONTHS[at.getMonth()]}${year}`;
}

/** `secs` in full, for a hover: "Mon 16 Nov 2026, 09:42" in this PC's own clock. '' with no time. */
export function whenFull(secs) {
  const t = Number(secs);
  if (!Number.isFinite(t) || t <= 0) return '';
  const at = new Date(t * 1000);
  const day = at.toLocaleDateString([], { weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' });
  const time = at.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  return `${day}, ${time}`;
}
