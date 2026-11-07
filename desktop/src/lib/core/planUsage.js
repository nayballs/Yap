// Plan usage limits in words (ai/claude_plan/usage.rs → Chat's usage ring,
// views/chat/PlanUsageRing.svelte, and Settings' plan card): a limit's label
// ("Weekly · all models", "Weekly · Fable", "Current session"), when it
// resets in the person's local time ("Resets Sat 3:00 AM", only the time
// when it's today), the ring's fill and colour (the highest active limit,
// coloured by the answer's severity), extra usage in the answer's currency
// and how full a plan conversation is. Pure.

/** A limit's label: its period, then all models or its model. */
export function limitLabel(l) {
  return [l?.title, l?.scope].filter(Boolean).join(' · ');
}

/** A percent as shown: whole, never below 0. */
export function pct(p) {
  return Math.max(0, Math.round(Number(p) || 0));
}

/** Whole days from `now`'s date to `d`'s, in local time. */
function daysAhead(d, now) {
  const a = new Date(d.getFullYear(), d.getMonth(), d.getDate());
  const b = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  return Math.round((a - b) / 86_400_000);
}

/**
 * When a limit resets (Unix seconds), in local time: "Resets 3:00 PM" today,
 * "Resets Sat 3:00 AM" within the week, "Resets 20 Nov, 3:00 AM" after.
 * '' when it isn't known.
 */
export function resetsLabel(at, now = new Date()) {
  if (at == null || !Number.isFinite(Number(at))) return '';
  const d = new Date(Number(at) * 1000);
  const time = d.toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
  const days = daysAhead(d, now);
  if (days <= 0) return `Resets ${time}`;
  if (days < 7) return `Resets ${d.toLocaleDateString(undefined, { weekday: 'short' })} ${time}`;
  return `Resets ${d.toLocaleDateString(undefined, { day: 'numeric', month: 'short' })}, ${time}`;
}

const RANK = { normal: 0, warning: 1, critical: 2 };

/** A severity Yap knows ("normal", "warning", "critical"). */
export function severityOf(s) {
  return s in RANK ? s : 'normal';
}

/**
 * The ring: filled to the highest active limit's percent (every limit when
 * none says it's active), coloured by the worst of their severities. `null`:
 * no limits, no ring.
 */
export function ringOf(usage) {
  const rows = Array.isArray(usage?.limits) ? usage.limits : [];
  if (!rows.length) return null;
  const active = rows.filter((r) => r.active !== false);
  const pool = active.length ? active : rows;
  const percent = Math.max(...pool.map((r) => pct(r.percent)));
  const severity = pool.map((r) => severityOf(r.severity)).reduce((a, b) => (RANK[b] > RANK[a] ? b : a), 'normal');
  return { percent, severity };
}

/**
 * A severity's colour, on Chat's context ring's scale (core/aiUsage.js
 * `ringColor`): amber, warmer for a warning, red when critical. Theme tokens:
 * light and dark.
 */
export function severityColor(s) {
  switch (severityOf(s)) {
    case 'critical':
      return 'var(--yap-danger)';
    case 'warning':
      return 'color-mix(in srgb, var(--yap-danger) 55%, var(--yap-primary))';
    default:
      return 'var(--yap-primary)';
  }
}

/** An amount in its currency ("£37.50"); a code Intl doesn't know: "37.50 XYZ". */
export function money(n, currency) {
  const v = Number(n) || 0;
  try {
    return new Intl.NumberFormat(undefined, { style: 'currency', currency }).format(v);
  } catch {
    return `${v.toFixed(2)} ${currency || ''}`.trim();
  }
}

/** "Extra usage · £0.00 of £37.50" (no limit set: "· £4.20 used"); '' when off. */
export function extraLine(extra) {
  if (!extra || !extra.currency) return '';
  const used = money(extra.used, extra.currency);
  return extra.limit != null ? `Extra usage · ${used} of ${money(extra.limit, extra.currency)}` : `Extra usage · ${used} used`;
}

/**
 * How full a plan conversation is ("This chat · 16% of context"; "about" when
 * the model's window is Anthropic's usual, not its own word). '' unknown.
 */
export function contextLine(fill) {
  if (!fill || !(Number(fill.window) > 0)) return '';
  const p = pct((Number(fill.used) / Number(fill.window)) * 100);
  return `This chat · ${fill.known ? '' : 'about '}${p}% of context`;
}

/** The ring button's name: "Plan usage: 23% used". */
export function ringName(ring) {
  return ring ? `Plan usage: ${ring.percent}% used` : '';
}
