// "You've dictated 1,000 words" in the main window (dictation/milestones.rs).
// Rust decides — 100, 500, 1,000 and 2,000 words, then further apart; once
// each; after the dictation, never during one or a meeting; a tip "Show
// tips" switches off — and this shows it while the window is in front
// (else it's a card on the Yap bar):
//
// `yap-milestone` { words, title, body }: a tip toast with See Insights.
// `yap-open-insights`: the bar card's See Insights (the window is up).
import { listen } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';

let started = false;
let showView = null;

/** Show milestones as toasts. Idempotent; the main window passes `showView`. */
export function initMilestones(opts = {}) {
  if (opts.showView) showView = opts.showView;
  if (started) return;
  started = true;
  listen('yap-milestone', (e) => showMilestone(e.payload));
  listen('yap-open-insights', () => showView?.('insights'));
}

/** A milestone (`{ words, title, body }`) as a toast. */
export function showMilestone(m) {
  if (!m?.title) return;
  toast({
    kind: 'tip',
    title: m.title,
    description: m.body,
    chip: 'Insights',
    duration: 10_000,
    ...(showView ? { action: { label: 'See Insights', onClick: () => showView('insights') } } : {}),
  });
}
