// Skills for every box that takes one (src-tauri/src/chat/skills.rs): the
// list (Yap's own and the person's, re-read on `yap-skills-changed`), and
// a box's `/` menu (`SkillPicker`): open while the box holds "/" and a
// command so far, ↑ ↓ to move, Enter to run the highlighted Skill, Tab to
// put it in the box and add words, Esc to close it. Pure matching is in
// core/skills.js; the menu is components/SkillMenu.svelte.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { matchSkills, slashQuery, splitCommand, skillFor, promptChips } from '../core/skills.js';

/** `list`: every Skill (built-ins first); `loaded`: read at least once. */
export const skills = $state({ list: [], loaded: false });

let started = false;

/** Read the Skills (again). */
export async function refreshSkills() {
  try {
    skills.list = (await invoke('skills_list')) || [];
    skills.loaded = true;
  } catch {
    /* keep what's there */
  }
}

/** Load them once per window and follow Settings' changes. */
export function initSkills() {
  if (started) return;
  started = true;
  refreshSkills();
  listen('yap-skills-changed', () => refreshSkills()).catch(() => {});
}

/** The one-tap prompts (Chat, Ask across meetings). */
export function chips() {
  return promptChips(skills.list);
}

/** A box's `/` menu. `where`: 'meeting' | 'meetings'. */
export class SkillPicker {
  text = $state('');
  active = $state(0);
  /** Esc closed it, for the text as it is. */
  dismissed = $state(false);
  query = $derived(slashQuery(this.text));
  items = $derived(this.query == null || this.dismissed ? [] : matchSkills(skills.list, this.query, this.where));
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
      this.dismissed = true;
      return true;
    }
    if ((e.key === 'Enter' && !e.shiftKey) || e.key === 'Tab') {
      e.preventDefault();
      return { skill: this.items[Math.min(this.active, n - 1)], run: e.key === 'Enter' };
    }
    return null;
  }

  /** What sending `text` runs: `{ skill, words }`, or null (an ordinary question). */
  parse(text) {
    const c = splitCommand(text);
    if (!c) return null;
    const skill = skillFor(skills.list, c.command, this.where);
    return skill ? { skill, words: c.words } : null;
  }
}
