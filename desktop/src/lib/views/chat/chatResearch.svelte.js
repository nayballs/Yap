// Chat's Research switch (desktop/src-tauri/src/chat/research.rs, 16 Nov 2026; the Claude app's
// Research mode): while it's on, each question sent from the box is researched — a short plan first,
// then many searches and page reads, then a report as a document beside the chat. It stays on until
// it's turned off, as the Claude app's does (for this run of Yap; never saved). Chat's model must be
// able to research (`chat_research_info`: web search on, a model that calls tools or your Claude plan),
// asked again whenever Chat's model changes.
import { invoke } from '@tauri-apps/api/core';

export const research = $state({
  /** Questions from the box are researched. */
  on: false,
  /** Chat's model can research now (else `why` says why not). */
  can: false,
  why: '',
});

let asked = 0;

/** What Chat's model and settings allow now (after a model switch, or a settings change). */
export async function refreshResearch() {
  const n = ++asked;
  try {
    const info = await invoke('chat_research_info');
    if (n !== asked) return;
    research.can = !!info?.can;
    research.why = info?.why || '';
  } catch {
    if (n === asked) {
      research.can = false;
      research.why = '';
    }
  }
}

/** Whether the question being sent from the box is researched. */
export function researchIt() {
  return research.on && research.can;
}
