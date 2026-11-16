// **Chat's start screen greeting** (views/chat/ChatThread.svelte, 16 Nov 2026,
// after the Claude app's new-chat screen): "Afternoon, Sam" by the local time,
// with the name About you gives (Settings → General → About you, `aboutName`;
// none: "Good afternoon"). Mostly plain; a small warm set late at night and
// very early, and now and then a weekend touch. One line per day and time band
// (`seedOf`), so it never changes from one render to the next. Pure.

/** The time bands, by the local hour: [band, from hour, to hour (exclusive)]. */
const BANDS = [
  ['late', 0, 5],
  ['early', 5, 7],
  ['morning', 7, 12],
  ['afternoon', 12, 18],
  ['evening', 18, 23],
  ['late', 23, 24],
];

/** Each band's lines: `named` with `{name}` in it, `plain` without a name. */
const LINES = {
  morning: [{ named: 'Morning, {name}', plain: 'Good morning' }],
  afternoon: [{ named: 'Afternoon, {name}', plain: 'Good afternoon' }],
  evening: [{ named: 'Evening, {name}', plain: 'Good evening' }],
  // Late at night (23:00–04:59): warm and short, never about sleep.
  late: [
    { named: 'Burning the midnight oil, {name}?', plain: 'Burning the midnight oil?' },
    { named: 'Still up, {name}?', plain: 'Still up?' },
    { named: 'Night owl mode, {name}', plain: 'Night owl mode' },
    { named: 'The quiet hours, {name}', plain: 'The quiet hours' },
  ],
  // Very early (05:00–06:59).
  early: [
    { named: 'Early start, {name}', plain: 'Early start' },
    { named: 'First light, {name}', plain: 'First light' },
    { named: 'Ahead of the day, {name}', plain: 'Ahead of the day' },
  ],
};

const DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];

/** The name to greet: About you's, on one line, at most 40 characters (cut at a word). '' for none. */
export function greetName(name) {
  const one = String(name ?? '')
    .replace(/\s+/g, ' ')
    .trim();
  if (one.length <= 40) return one;
  const cut = one.slice(0, 40);
  const space = cut.lastIndexOf(' ');
  return (space > 0 ? cut.slice(0, space) : cut).trim();
}

/** The band of local time `date`: 'late' | 'early' | 'morning' | 'afternoon' | 'evening'. */
export function greetingBand(date = new Date()) {
  const h = date.getHours();
  return BANDS.find(([, from, to]) => h >= from && h < to)[0];
}

/**
 * The day and band a greeting is picked for: one night is one night — after
 * midnight it's still the evening before's ("2026-11-14:late" at 01:30 on the 15th).
 */
export function seedOf(date = new Date()) {
  const band = greetingBand(date);
  const d = new Date(date.getFullYear(), date.getMonth(), date.getDate() - (band === 'late' && date.getHours() < 5 ? 1 : 0));
  const day = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  return `${day}:${band}`;
}

/** FNV-1a: a small, steady number from a string. */
function hash(s) {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i += 1) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h;
}

/**
 * The greeting for `name` (About you's, or '') at local time `date`:
 * `{ text, band }`. The same day and band always give the same line.
 */
export function greetingFor(name, date = new Date()) {
  const who = greetName(name);
  const band = greetingBand(date);
  const seed = seedOf(date);
  let lines = LINES[band];
  // A weekend's morning or afternoon, one day in three or so: "Happy Saturday, Sam".
  const day = date.getDay();
  if ((day === 0 || day === 6) && (band === 'morning' || band === 'afternoon') && hash(`${seed}:weekend`) % 3 === 0) {
    lines = [{ named: `Happy ${DAYS[day]}, {name}`, plain: `Happy ${DAYS[day]}` }];
  }
  const line = lines[hash(seed) % lines.length];
  return { text: who ? line.named.replace('{name}', who) : line.plain, band };
}

/** Every line a band can give, named and plain (for tests and the docs). */
export function greetingLines(band) {
  return (LINES[band] || []).map((l) => ({ ...l }));
}
