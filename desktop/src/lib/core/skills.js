// Skills (desktop/src-tauri/src/chat/skills.rs): saved instructions run by typing
// `/name` — in a meeting's Ask box on that meeting, in Chat and Ask across
// meetings on the meetings. Pure helpers, shared by every box that takes a
// Skill (stores/skills.svelte.js keeps the list), and the one-tap prompts
// (four of the core six carry a chip and the question it sends).
//
// Shown or not: each Skill in the list says whether it's in the `/` menus
// and one-tap prompts (`shown`: the core six and your own to begin with,
// the other nine once switched on in Settings → Skills) and whether it's one
// of the core six (`core`). Only the menus and the prompts follow `shown`:
// a Skill's whole command typed and sent runs it either way (`skillFor`).

/** Where a box runs Skills: 'meeting' (one meeting's Ask) or 'meetings' (Chat, Ask across meetings). */
export function runsHere(skill, where) {
  const on = skill?.runsOn || 'both';
  return on === 'both' || on === where;
}

/** In the `/` menus and one-tap prompts now (a Skill without the field — a test's — is). */
export function isShown(skill) {
  return skill?.shown !== false;
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
 * The Skills the `/` menu lists for `query` in a box that runs `where` (the
 * ones shown): the command typed in full first, then those whose command
 * starts with it, then those whose command or name holds it; Yap's own
 * before the person's within each. The whole command of one off the menu
 * typed: none, so Enter sends it as typed and it runs (never another that
 * happens to hold the words, /deal-risks for /risks).
 */
export function matchSkills(skills, query, where) {
  const q = String(query ?? '').toLowerCase();
  const all = (skills || []).filter((s) => runsHere(s, where));
  const here = all.filter(isShown);
  if (!q) return here;
  if (all.some((s) => s.command === q && !isShown(s))) return [];
  const exact = here.filter((s) => s.command === q);
  const starts = here.filter((s) => s.command !== q && s.command.startsWith(q));
  const holds = here.filter((s) => !s.command.startsWith(q) && (s.command.includes(q) || s.name.toLowerCase().includes(q)));
  return [...exact, ...starts, ...holds];
}

/**
 * The `/` menu as it's drawn. Just "/": the Skills shown here in two
 * groups, yours then Yap's. A command begun: one list, the best match first
 * (`matchSkills`), so Enter runs what was typed. `items` is the order ↑ ↓
 * move through; each group says where it starts in it. Pure.
 * @returns {{ items: object[], groups: { label: string, start: number, items: object[] }[] }}
 */
export function skillMenu(skills, query, where) {
  const found = matchSkills(skills, query, where);
  if (query) return { items: found, groups: found.length ? [{ label: '', start: 0, items: found }] : [] };
  const mine = found.filter((s) => !s.builtin);
  const yaps = found.filter((s) => s.builtin);
  const groups = [];
  if (mine.length) groups.push({ label: 'Your Skills', start: 0, items: mine });
  if (yaps.length) groups.push({ label: "Yap's Skills", start: mine.length, items: yaps });
  return { items: [...mine, ...yaps], groups };
}

/**
 * Settings → Skills' three groups: yours, Yap's core six ("Yap's Skills")
 * and the rest of Yap's ("More Skills"), each in the list's order. Pure.
 */
export function skillGroups(skills) {
  const list = skills || [];
  return {
    mine: list.filter((s) => !s.builtin),
    yaps: list.filter((s) => s.builtin && s.core),
    more: list.filter((s) => s.builtin && !s.core),
  };
}

/** The Skill `/command` runs in a box that runs `where`, or null — shown in the menus or not. */
export function skillFor(skills, command, where) {
  const c = String(command ?? '').toLowerCase();
  return (skills || []).find((s) => s.command === c && runsHere(s, where)) ?? null;
}

/** What a run shows as the person's question: "/decisions last week". */
export function asked(skill, words = '') {
  return `/${skill.command}${words ? ` ${words}` : ''}`;
}

/**
 * A Skill that needs words to run on (/project-update: which project; its
 * `asks`): picked from the `/` menu, it goes in the box for them.
 */
export function needsWords(skill) {
  return !!skill?.asks;
}

/** A built-in that writes a document when it runs in Chat (/tickets, /prd). */
export function writesDocument(skill) {
  return !!skill?.document;
}

/** The most one-tap prompts shown at once. */
export const MAX_PROMPTS = 3;

/**
 * The one-tap prompts (Chat, Ask across meetings): the first three of the
 * built-ins shown with a chip — /prep's only with a calendar connected
 * (`calendar`), where it finds the next meeting itself; without one it could
 * only ask which meeting.
 */
export function promptChips(skills, { calendar = true } = {}) {
  return (skills || [])
    .filter((s) => s.builtin && s.chip && s.question && isShown(s) && (calendar || s.gather !== 'prep'))
    .slice(0, MAX_PROMPTS);
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

/** Where a Skill runs, as a sentence (Settings: an open Skill, the form's hint). */
export function whereItRuns(on) {
  return on === 'meeting'
    ? "In a meeting's Ask box, on that meeting."
    : on === 'meetings'
      ? 'In Chat and Ask across meetings, on all your meetings.'
      : "In a meeting's Ask box, and in Chat across your meetings.";
}

/** A Skill's line in the `/` menu: what it does, else (one of yours without a description) its name. */
export function menuLine(skill) {
  return String(skill?.description || '').trim() || String(skill?.name || '');
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
