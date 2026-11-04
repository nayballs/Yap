// Skills for every box that takes one (desktop/src-tauri/src/chat/skills.rs): the
// list (Yap's own and the person's, each with `shown` — in the `/` menus —
// and `core`, re-read on `yap-skills-changed`), Settings' "Show in the /
// menu" switch, and a box's `/` menu (`SkillPicker`): open while the box
// holds "/" and a command so far, ↑ ↓ to move, Enter to run the highlighted
// Skill, Tab to put it in the box and add words, Esc to close it. It lists
// the Skills shown; one that isn't still runs when its whole command is
// sent (`parse`). Pure matching is in core/skills.js; the menu is
// components/SkillMenu.svelte.
import { invoke } from '@tauri-apps/api/core';
import { emit, listen } from '@tauri-apps/api/event';
import { slashQuery, splitCommand, skillFor, promptChips, needsWords, skillMenu } from '../core/skills.js';

/** `list`: every Skill (built-ins first); `loaded`: read at least once. */
export const skills = $state({ list: [], loaded: false });

let started = false;
// The main window's own way to a Settings page (ControlPanel); elsewhere the
// main window is shown and told.
let nav = { openSettings: null };

/** Read the Skills (again). */
export async function refreshSkills() {
  try {
    skills.list = (await invoke('skills_list')) || [];
    skills.loaded = true;
  } catch {
    /* keep what's there */
  }
}

/** Load them once per window and follow Settings' changes. The main window passes `openSettings(section)`. */
export function initSkills(opts = {}) {
  nav = { ...nav, ...opts };
  if (started) return;
  started = true;
  refreshSkills();
  listen('yap-skills-changed', () => refreshSkills()).catch(() => {});
}

/**
 * Settings' "Show in the / menu" for Skill `id`: at once here, then saved
 * (every window follows). On an error the list is read again — as Rust has
 * it: refused, as it was; not on disk yet (a full disk), kept, Rust saving
 * it with the next change — and the error thrown.
 */
export async function setShown(id, shown) {
  const s = skills.list.find((k) => k.id === id);
  if (s) s.shown = shown;
  try {
    await invoke('skill_show', { id, shown });
  } catch (e) {
    await refreshSkills();
    throw e;
  }
}

/** Settings → Skills at More Skills (the `/` menu's way there), from any window. */
export async function openMoreSkills() {
  const at = 'skills#skills-more';
  if (nav.openSettings) {
    nav.openSettings(at);
    return;
  }
  await invoke('open_settings').catch(() => {});
  emit('yap-open-settings', at).catch(() => {});
}

/** The one-tap prompts (Chat, Ask across meetings). `calendar`: one is connected. */
export function chips({ calendar = true } = {}) {
  return promptChips(skills.list, { calendar });
}

const NO_MENU = { items: [], groups: [] };

/** A box's `/` menu. `where`: 'meeting' | 'meetings'. */
export class SkillPicker {
  text = $state('');
  active = $state(0);
  /** Esc closed it, for the text as it is. */
  dismissed = $state(false);
  query = $derived(slashQuery(this.text));
  menu = $derived(this.query == null || this.dismissed ? NO_MENU : skillMenu(skills.list, this.query, this.where));
  items = $derived(this.menu.items);
  /** The groups drawn: "Your Skills" and "Yap's Skills" for just "/", one list once a command is begun. */
  groups = $derived(this.menu.groups);
  open = $derived(this.items.length > 0);

  constructor(where) {
    this.where = where;
    initSkills();
  }

  /** The box's text now. */
  update(text) {
    if (text === this.text) return;
    this.text = text;
    this.active = 0;
    this.dismissed = false;
  }

  /** Close the menu for the text as it is (Esc, or More Skills). */
  close() {
    this.dismissed = true;
  }

  /**
   * A key in the box while the menu is open: ↑ ↓ move (true), Esc closes
   * (true), Enter → `{ skill, run: true }`, Tab → `{ skill, run: false }`.
   * `null`: not the menu's key (the box handles it).
   */
  key(e) {
    if (!this.open || e.isComposing) return null;
    const n = this.items.length;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      this.active = (this.active + (e.key === 'ArrowDown' ? 1 : n - 1)) % n;
      return true;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      this.close();
      return true;
    }
    if ((e.key === 'Enter' && !e.shiftKey) || e.key === 'Tab') {
      e.preventDefault();
      const skill = this.items[Math.min(this.active, n - 1)];
      // One that needs words to run on (/project-update: which project):
      // Enter puts it in the box for them, as Tab does.
      return { skill, run: e.key === 'Enter' && !needsWords(skill) };
    }
    return null;
  }

  /** What sending `text` runs: `{ skill, words }`, or null (an ordinary question). Shown in the menu or not. */
  parse(text) {
    const c = splitCommand(text);
    if (!c) return null;
    const skill = skillFor(skills.list, c.command, this.where);
    return skill ? { skill, words: c.words } : null;
  }
}
