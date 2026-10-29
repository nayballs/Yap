// Feature tips in the main window (dictation/tips.rs): the first time a
// dictation shows off something Yap did by itself — a self-correction
// caught, a spoken list laid out, an email framed, a snippet expanded, a
// word fixed from the dictionary — one short tip, once each, ever. Rust
// decides (after the paste, never during a dictation, "Show tips" on) and
// counts it; this shows it while the window is in front (else it's a card
// on the Yap bar).
//
// `yap-feature-tip` { kind, title, body }: a toast of kind "tip"
// (ui/noticePolicy.js: held while a dictation runs, dropped if it waits too
// long unseen); a snippet's or the dictionary's has Open Dictionary.
import { listen } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';

let started = false;
let showView = null;

/** Show feature tips as toasts. Idempotent; the main window calls it with `showView`. */
export function initFeatureTips(opts = {}) {
  if (opts.showView) showView = opts.showView;
  if (started) return;
  started = true;
  listen('yap-feature-tip', (e) => showFeatureTip(e.payload));
}

/** A feature tip (`{ kind, title, body }`, tips.rs `Tip`) as a toast. */
export function showFeatureTip(t) {
  if (!t?.title) return;
  const dictionary = t.kind === 'snippet' || t.kind === 'dictionary';
  toast({
    kind: 'tip',
    key: `feature-tip:${t.kind}`,
    title: t.title,
    description: t.body || '',
    chip: 'Tip',
    icon: dictionary ? 'dictionary' : undefined,
    duration: 12_000,
    action: dictionary && showView ? { label: 'Open Dictionary', onClick: () => showView('dictionary') } : null,
  });
}
