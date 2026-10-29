// Skills (src-tauri/src/chat/skills.rs): saved instructions run by typing
// `/name` — in a meeting's Ask box on that meeting, in Chat and Ask across
// meetings on the meetings. Pure helpers, shared by every box that takes a
// Skill (stores/skills.svelte.js keeps the list), and the one-tap prompts
// (three built-ins carry a chip and the question it sends).

/** Where a box runs Skills: 'meeting' (one meeting's Ask) or 'meetings' (Chat, Ask across meetings). */
export function runsHere(skill, where) {
  const on = skill?.runsOn || 'both';
  return on === 'both' || on === where;
}

/**
 * The `/` being typed at the start of a box: the command so far ('' just
 * after the slash), or null when the box isn't starting a Skill (no slash,
 * or words already after the command).
 */
export function slashQuery(text) {
  const m = /^\/([a-z0-9-]*)$/i.exec(String(text ?? '').trimStart());
  return m ? m[1].toLowerCase() : null;
}

/** `"/decisions last week"` → `{ command: 'decisions', words: 'last week' }`; null without a command. */
export function splitCommand(text) {
  const m = /^\/([a-z0-9-]+)(?:\s+([\s\S]*))?$/i.exec(String(text ?? '').trim());
  return m ? { command: m[1].toLowerCase(), words: (m[2] || '').trim() } : null;
}

/**
 * The Skills the `/` menu lists for `query` in a box that runs `where`:
 * those whose command starts with it first, then those whose command or
 * name holds it; Yap's own before the person's within each.
 */
export function matchSkills(skills, query, where) {
  const q = String(query ?? '').toLowerCase();
  const here = (skills || []).filter((s) => runsHere(s, where));
  if (!q) return here;
  const starts = here.filter((s) => s.command.startsWith(q));
  const holds = here.filter((s) => !s.command.startsWith(q) && (s.command.includes(q) || s.name.toLowerCase().includes(q)));
  return [...starts, ...holds];
}

/** The Skill `/command` runs in a box that runs `where`, or null. */
export function skillFor(skills, command, where) {
  const c = String(command ?? '').toLowerCase();
  return (skills || []).find((s) => s.command === c && runsHere(s, where)) ?? null;
}

/** What a run shows as the person's question: "/decisions last week". */
export function asked(skill, words = '') {
  return `/${skill.command}${words ? ` ${words}` : ''}`;
}

/** The one-tap prompts (Chat, Ask across meetings): the built-ins with a chip. */
export function promptChips(skills) {
  return (skills || []).filter((s) => s.builtin && s.chip && s.question);
}

/**
 * An answer that cites the meeting, as it's copied: without its "[2]"s
 * (pills in Yap, noise in an email or a document). Pure.
 */
export function withoutCites(text) {
  return String(text ?? '').replace(/ ?\[\d{1,2}(?:\s*[,–-]\s*\d{1,2})*\]/g, '');
}

/** Where a Skill runs, in words (the menu and Settings). */
export function runsOnLabel(on) {
  return on === 'meeting' ? 'One meeting' : on === 'meetings' ? 'Across meetings' : 'One meeting or across them';
}

/**
 * A command made from a name, for a new Skill ("Customer follow-up" →
 * "customer-follow-up"). Pure.
 */
export function commandFromName(name) {
  return String(name ?? '')
    .toLowerCase()
    .normalize('NFKD')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-+|-+$/g, '')
    .slice(0, 32)
    .replace(/-+$/g, '');
}

/** Why `command` can't be one (Settings checks as it's typed; Rust checks again), or ''. */
export function commandProblem(command, skills, ownId = null) {
  const c = String(command ?? '').trim().replace(/^\//, '');
  if (!c) return 'Add a command.';
  if (c.length < 2 || c.length > 32 || !/^[a-z0-9]+(-[a-z0-9]+)*$/.test(c)) {
    return 'Use 2 to 32 lowercase letters and numbers, with single hyphens between words.';
  }
  if ((skills || []).some((s) => s.builtin && s.command === c)) return `/${c} is one of Yap's own Skills. Pick another.`;
  if ((skills || []).some((s) => !s.builtin && s.command === c && s.id !== ownId)) return `You already have a Skill with /${c}.`;
  return '';
}
