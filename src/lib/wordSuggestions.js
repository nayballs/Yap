// "Suggest words from your corrections" in the main window (learn.rs).
// Rust watches the field a dictation went into and decides what's worth
// learning; this shows its question while the window is on screen (else
// it's a card on the Yap bar):
//
// `yap-word-suggestion` { id, title, body, never, words: [{ heard, term }] }:
//   "Add “Kubernetes” to your dictionary? Yap heard “cube and eighties”."
//   Add / Not now, and a small "Don't suggest this" link. The answer goes
//   back with `word_suggestion_respond` ("add" | "later" | "never" |
//   "expire"): the card's ✕ is "Not now", running out is no answer at all.
//   After Add, Rust saves the words and sends `yap-dictionary-learned`
//   (Settings adopts it, so its auto-save keeps them); this confirms it.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from './ui/toast.svelte.js';

let started = false;
let showView = () => {};

/** "“Kubernetes” is in your dictionary" / "Added 2 words to your dictionary". */
export function addedTitle(added) {
  return added.length === 1 ? `“${added[0]}” is in your dictionary` : `Added ${added.length} words to your dictionary`;
}

/**
 * Show word suggestions as toasts. Idempotent; the main window calls it
 * with `showView` (a sidebar view, e.g. 'dictionary').
 */
export function initWordSuggestions(opts = {}) {
  if (opts.showView) showView = opts.showView;
  if (started) return;
  started = true;
  listen('yap-word-suggestion', (e) => ask(e.payload));
}

function ask(s) {
  if (!s?.id || !s.title) return;
  const answer = (a) => invoke('word_suggestion_respond', { id: s.id, answer: a });
  const quietly = (a) => answer(a).catch(() => {});
  toast({
    kind: 'tip',
    title: s.title,
    description: s.body,
    chip: 'Dictionary',
    icon: 'dictionary',
    duration: 30_000,
    action: {
      label: 'Add',
      onClick: async () => {
        try {
          const added = await answer('add');
          if (!added?.length) return;
          toast({
            title: addedTitle(added),
            description: 'Yap writes it that way from now on.',
            variant: 'success',
            chip: 'Dictionary',
            icon: 'dictionary',
            action: { label: 'Open Dictionary', onClick: () => showView('dictionary') },
          });
        } catch (err) {
          toast({ title: "Couldn't add it to your dictionary", description: String(err), variant: 'destructive' });
        }
      },
    },
    secondary: { label: 'Not now', onClick: () => quietly('later') },
    tertiary: { label: s.never || "Don't suggest this", onClick: () => quietly('never') },
    onClose: () => quietly('later'),
    onExpire: () => quietly('expire'),
  });
}
