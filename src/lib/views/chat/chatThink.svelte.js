// Chat's Think button (src-tauri/src/chat/think.rs): the next question is
// thought through — the on-device Qwen thinks before it answers (up to about
// 1,500 tokens). For one question: it goes off again once that question is
// sent. Only for the model on this PC (`chat_think_info`, asked again
// whenever Chat's model changes): a cloud model's thinking is its effort,
// chosen in the model picker (src-tauri/src/ai/effort.rs).
import { invoke } from '@tauri-apps/api/core';

export const think = $state({
  /** The next question is thought through. */
  on: false,
  /** Chat's model can think it through (the button shows). */
  can: false,
  /** "here" (the model runs on this PC) | "cloud". */
  place: 'here',
});

let asked = 0;

/** What Chat's model can do now (after a model switch). */
export async function refreshThink() {
  const n = ++asked;
  try {
    const info = await invoke('chat_think_info');
    if (n !== asked) return;
    think.can = !!info?.can;
    think.place = info?.place || 'here';
    if (!think.can) think.on = false;
  } catch {
    if (n === asked) think.can = false;
  }
}

/** The question being sent takes the button's state; the button goes off. */
export function takeThink() {
  const on = think.on && think.can;
  think.on = false;
  return on;
}

/** "Thought for 6 s" (or "Thought it through" when the time isn't known). */
export function thoughtLabel(ms) {
  if (!ms || ms < 500) return 'Thought it through';
  const s = Math.round(ms / 1000);
  return s < 60 ? `Thought for ${s} s` : `Thought for ${Math.floor(s / 60)} min ${s % 60} s`;
}
