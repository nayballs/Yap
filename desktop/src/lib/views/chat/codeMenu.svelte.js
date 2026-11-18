// **A Code chat's + menu and "/" list** (17 Nov 2026, after the Claude Code app's; agents/menu.rs):
// what the chat's Claude Code offers in its workspace — its slash commands and skills, its MCP servers and
// connectors, its plugins — and the chat's own choices: more folders its runs may use (`--add-dir`), servers
// switched off for it (`--disallowedTools`).
//
// - `code_menu` reads them (a workspace nothing is known of yet is looked at in the background: `looking`, then
//   `yap-code-menu` says it's in). Every run's init keeps them fresh.
// - A new code chat (no conversation yet) chooses for its workspace; its first question makes them its own.
// - `CommandPicker`: the box's "/" list in a Code chat — "/" at the start of the box, then letters, `:` or `-`:
//   the commands whose name holds them, the ones that start with them first; ↑ ↓ move, Enter or Tab put
//   `/name ` in the box, Esc closes.
// - Yap's own `/agents` (18 Nov 2026, `AGENTS_COMMAND`) is in both lists: picked or sent, the chat's Agent panel.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

/** The Code chat's commands after "/" as typed: `/` then its name begun (letters, digits, `-`, `_`, `:`). */
export function commandQuery(text) {
  const m = /^\/([a-z0-9:_-]*)$/i.exec(String(text ?? '').trimStart());
  return m ? m[1].toLowerCase() : null;
}

/**
 * The commands for `query`, best first: its name in full, then names that start with it (a plugin's
 * `plugin:skill` by its skill's part too, and a command's other names), then names that hold it, then lines that
 * do. Pure.
 */
export function matchCommands(commands, query) {
  const q = String(query ?? '').toLowerCase();
  const list = Array.isArray(commands) ? commands : [];
  if (!q) return list;
  const rank = (c) => {
    const n = String(c.name || '').toLowerCase();
    const part = n.includes(':') ? n.slice(n.indexOf(':') + 1) : n;
    // (Its other names count as its name: /review for /code-review.)
    const aliases = (c.aliases || []).map((a) => String(a).toLowerCase());
    if (n === q || aliases.includes(q)) return 0;
    if (n.startsWith(q) || part.startsWith(q) || aliases.some((a) => a.startsWith(q))) return 1;
    if (n.includes(q)) return 2;
    if (String(c.description || '').toLowerCase().includes(q)) return 3;
    return -1;
  };
  return list
    .map((c, i) => ({ c, i, r: rank(c) }))
    .filter((x) => x.r >= 0)
    .sort((a, b) => a.r - b.r || a.i - b.i)
    .map((x) => x.c);
}

/**
 * **Yap's own handling of three of Claude Code's commands** in a Code chat (17 Nov 2026, the Claude Code app's):
 * the words in the box as `{ name: 'clear'|'compact'|'model', arg }`, or null — anything else typed with "/" goes to
 * Claude Code as typed. Pure.
 */
export function codeCommand(text) {
  const m = /^\/(clear|compact|model)(?:\s+([\s\S]*))?$/i.exec(String(text ?? '').trim());
  if (!m) return null;
  const name = m[1].toLowerCase();
  const arg = (m[2] || '').trim();
  // (/clear takes nothing: "/clear the cache" is words for Claude.)
  if (name === 'clear' && arg) return null;
  return { name, arg };
}

/**
 * **The model `/model <name>` means** among `rows` (the provider's list, `{ value, label }`, newest first): its id
 * typed in full, else the first whose id or name holds every word ("haiku", "sonnet 5", "opus-5-5"). null: none. Pure.
 */
export function modelFor(rows, name) {
  const want = String(name ?? '').trim().toLowerCase();
  if (!want) return null;
  const list = Array.isArray(rows) ? rows : [];
  const exact = list.find((r) => String(r.value).toLowerCase() === want);
  if (exact) return exact;
  const words = want.split(/[\s._-]+/).filter(Boolean);
  const holds = (r) => {
    const hay = `${r.value} ${r.label}`.toLowerCase().replace(/[._-]/g, ' ');
    return words.every((w) => hay.includes(w));
  };
  return list.find(holds) ?? null;
}

/**
 * **Yap's own `/agents`** (18 Nov 2026): the chat's Agent panel — its helpers and agents, with the crew, cost, tokens
 * and time — opened or brought forward; nothing is sent to Claude Code (its own `/agents` runs only in its terminal,
 * so its list leaves it out). Listed among the built-in commands, in a Code chat's "/" list and Slash commands ›.
 */
export const AGENTS_COMMAND = Object.freeze({ name: 'agents', description: 'Show the agents and helpers working in this chat', hint: '', builtin: true, skill: false, plugin: '', yap: 'agents' });

/**
 * Claude Code's commands with Yap's own laid in: `/agents` among the built-in ones in its alphabetical place, in place
 * of any `agents` of Claude Code's (a name twice would be one row twice). Pure.
 */
export function withYapCommands(commands) {
  const list = (Array.isArray(commands) ? commands : []).filter((c) => String(c?.name || '').toLowerCase() !== AGENTS_COMMAND.name);
  const at = list.findIndex((c) => c.builtin && String(c.name || '').toLowerCase() > AGENTS_COMMAND.name);
  const end = list.findLastIndex((c) => c.builtin);
  const i = at >= 0 ? at : end >= 0 ? end + 1 : list.length;
  return [...list.slice(0, i), AGENTS_COMMAND, ...list.slice(i)];
}

/** `/agents` typed on its own (any case, spaces around): Yap's. With words after it, they're words for Claude. Pure. */
export function isAgentsCommand(text) {
  return /^\/agents$/i.test(String(text ?? '').trim());
}

/**
 * **The agent whose tab `/agents` shows** in chat `conversation` (`list`: every agent, newest first): the agent tab
 * showing now, else one still in the panel's row (`doc.extra` less `doc.hidden`), else the chat's newest — the panel
 * lists all of the chat's either way. null: none in this chat yet. Pure.
 */
export function panelAgentFor(list, conversation, doc = null) {
  if (conversation == null) return null;
  const mine = (Array.isArray(list) ? list : []).filter((a) => a?.conversation === conversation);
  if (!mine.length) return null;
  const has = (id) => id != null && mine.some((a) => a.id === id);
  if (doc?.showing === 'agent' && has(doc.agent?.id)) return doc.agent.id;
  const hidden = new Set(doc?.hidden || []);
  const tab = (doc?.extra || []).filter((t) => t?.kind === 'agent' && !hidden.has(t.key) && has(t.id)).at(-1);
  return tab?.id ?? mine[0].id;
}

/** "about 54k" for a token count. Pure. */
export function roughTokens(n) {
  const v = Number(n) || 0;
  if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(v >= 10_000_000 ? 0 : 1)}M`;
  if (v >= 1000) return `${Math.round(v / 1000)}k`;
  return String(v);
}

/** The groups drawn: the person's own (their commands, skills, their plugins'), then Claude Code's own. Pure. */
export function commandGroups(commands) {
  const list = Array.isArray(commands) ? commands : [];
  const yours = list.filter((c) => !c.builtin);
  const builtin = list.filter((c) => c.builtin);
  const groups = [];
  if (yours.length) groups.push({ label: 'Commands and skills', items: yours, start: 0 });
  if (builtin.length) groups.push({ label: 'Built in', items: builtin, start: yours.length });
  return groups;
}

/**
 * The plugins as the menu lists them, each with its commands (`plugin:skill`): one with several says "12 skills ›",
 * one with one says that one's line, one with none its own. `query` keeps those whose name, line or skills hold it.
 * Pure.
 */
export function pluginRows(offered, query = '') {
  const q = String(query ?? '').trim().toLowerCase();
  const commands = offered?.commands ?? [];
  return (offered?.plugins ?? [])
    .map((p) => {
      const skills = commands.filter((c) => c.plugin === p.name);
      return { ...p, skills, line: skills.length === 1 ? skills[0].description || p.description : p.description };
    })
    .filter((p) => !q || p.name.toLowerCase().includes(q) || (p.line || '').toLowerCase().includes(q) || p.skills.some((s) => s.name.toLowerCase().includes(q)));
}

/** The servers as the menu lists them, `query` keeping those whose name holds it. Pure. */
export function serverRows(offered, off, query = '') {
  const q = String(query ?? '').trim().toLowerCase();
  const isOff = new Set(off ?? []);
  return (offered?.servers ?? [])
    .filter((s) => !q || s.label.toLowerCase().includes(q) || s.name.toLowerCase().includes(q))
    .map((s) => ({ ...s, on: !isOff.has(s.name) }));
}

/** A server's state in a word or two, when it isn't simply connected. */
export const SERVER_STATUS = { 'needs-auth': 'Needs sign-in', failed: 'Failed', pending: 'Starting', disabled: 'Off in Claude Code' };

const sameSpot = (a, b) => String(a || '').replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase() === String(b || '').replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase();

/**
 * The menu's state for the chat shown: `follow(conversation, workspace)` as it changes (ChatView / Composer), then
 * `load()` when the menu opens or "/" is typed.
 */
export class CodeMenu {
  conversation = $state(null);
  workspace = $state(null);
  offered = $state(null);
  looking = $state(false);
  folders = $state([]);
  serversOff = $state([]);
  loaded = $state(false);
  error = $state('');
  #seq = 0;
  #un = null;

  /** The chat's /clears (`{at, ts, agents}`): the thread's dividers. */
  clears = $state([]);
  #choiceSeq = 0;

  constructor() {
    listen('yap-code-menu', (e) => {
      const p = e.payload || {};
      const mine = (p.conversation != null && p.conversation === this.conversation) || (p.workspace && sameSpot(p.workspace, this.workspace));
      if (!mine) return;
      // (Its choices always — a /clear's divider, a folder's "+1" —, and the menu's lists when it has them.)
      this.loadChoices();
      if (this.loaded) this.load();
    }).then((u) => (this.#un = u));
  }

  /** The chat shown: its conversation (null: a new chat) and workspace path. Read again when it changes. */
  follow(conversation, workspace) {
    const same = conversation === this.conversation && sameSpot(workspace, this.workspace);
    if (same) return;
    this.conversation = conversation ?? null;
    this.workspace = workspace ?? null;
    this.offered = null;
    this.folders = [];
    this.serversOff = [];
    this.clears = [];
    this.loaded = false;
    this.error = '';
    this.loadChoices();
  }

  /** The chat's own choices only (its folders, servers off, /clears): no look at what Claude Code offers. */
  async loadChoices() {
    if (this.conversation == null && !this.workspace) return;
    const seq = ++this.#choiceSeq;
    try {
      const c = await invoke('code_chat_choices', { conversation: this.conversation, workspace: this.workspace });
      if (seq !== this.#choiceSeq) return;
      this.folders = c?.folders ?? [];
      this.serversOff = c?.serversOff ?? [];
      this.clears = c?.clears ?? [];
    } catch {
      /* an older Yap, or not a Code chat: nothing chosen */
    }
  }

  /** Read what's offered and the chat's choices (a look starts in the background when nothing's known). */
  async load() {
    if (this.conversation == null && !this.workspace) return;
    const seq = ++this.#seq;
    try {
      const m = await invoke('code_menu', { conversation: this.conversation, workspace: this.workspace });
      if (seq !== this.#seq) return;
      this.offered = m.offered ?? null;
      this.looking = !!m.looking;
      this.folders = m.folders ?? [];
      this.serversOff = m.serversOff ?? [];
      this.error = '';
    } catch (e) {
      if (seq !== this.#seq) return;
      this.error = String(e?.message ?? e);
    } finally {
      if (seq === this.#seq) this.loaded = true;
    }
  }

  /** A server on or off for this chat. */
  async setServer(name, on) {
    const before = this.serversOff;
    this.serversOff = on ? before.filter((s) => s !== name) : [...before.filter((s) => s !== name), name];
    try {
      this.serversOff = await invoke('code_menu_server', { conversation: this.conversation, workspace: this.workspace, server: name, on });
      return { ok: true };
    } catch (e) {
      this.serversOff = before;
      return { ok: false, why: String(e?.message ?? e) };
    }
  }

  /** Add folder: Windows' folder picker (a test run: the folder it named), checked and added. */
  async addFolder() {
    try {
      const rows = await invoke('code_chat_folder_add', { conversation: this.conversation, workspace: this.workspace });
      if (rows) this.folders = rows;
      return { ok: true, added: !!rows };
    } catch (e) {
      return { ok: false, why: String(e?.message ?? e) };
    }
  }

  /** A folder taken off this chat. */
  async removeFolder(path) {
    try {
      this.folders = await invoke('code_chat_folder_remove', { conversation: this.conversation, workspace: this.workspace, path });
      return { ok: true };
    } catch (e) {
      return { ok: false, why: String(e?.message ?? e) };
    }
  }

  /** Claude Code's commands here, with Yap's own `/agents` among them (before Claude Code has answered too). */
  get commands() {
    return withYapCommands(this.offered?.commands ?? []);
  }

  destroy() {
    this.#un?.();
  }
}

/**
 * **The box's "/" list in a Code chat** (as Claude Code's own box has it): open while the box holds "/" and a
 * command begun, the chat's commands matching. `menu`: its CodeMenu (read when the list first opens).
 */
export class CommandPicker {
  text = $state('');
  active = $state(0);
  /** Esc closed it, for the text as it is. */
  dismissed = $state(false);
  /** On only in a Code chat (`setOn`). */
  on = $state(false);
  query = $derived(this.on ? commandQuery(this.text) : null);
  items = $derived(this.query == null || this.dismissed ? [] : matchCommands(this.menu.commands, this.query));
  /** Just "/": grouped; a name begun: one list, the best match first. */
  groups = $derived(this.query ? [{ label: '', items: this.items, start: 0 }] : commandGroups(this.items));
  open = $derived(this.query != null && !this.dismissed);

  constructor(menu) {
    this.menu = menu;
  }

  setOn(on) {
    this.on = !!on;
  }

  /** The box's text now. */
  update(text) {
    if (text === this.text) return;
    const was = this.query;
    this.text = text;
    this.active = 0;
    this.dismissed = false;
    // First opened: what the chat offers, read (a look starts when nothing's known).
    if (was == null && this.query != null && !this.menu.loaded) this.menu.load();
  }

  close() {
    this.dismissed = true;
  }

  /** A key in the box while it's open: ↑ ↓ move (true), Esc closes (true), Enter or Tab → the command. null: not its key. */
  key(e) {
    if (!this.open || e.isComposing) return null;
    const n = this.items.length;
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      this.close();
      return true;
    }
    if (!n) return null;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      this.active = (this.active + (e.key === 'ArrowDown' ? 1 : n - 1)) % n;
      return true;
    }
    if ((e.key === 'Enter' && !e.shiftKey && !e.ctrlKey) || e.key === 'Tab') {
      const pick = this.items[Math.min(this.active, n - 1)];
      // A command typed in full (or one of its other names): Enter sends it as typed (Tab still puts it in the box).
      const names = [pick?.name, ...(pick?.aliases || [])].map((n) => String(n || '').toLowerCase());
      if (e.key === 'Enter' && names.includes(this.query)) return null;
      e.preventDefault();
      return { command: pick };
    }
    return null;
  }
}
