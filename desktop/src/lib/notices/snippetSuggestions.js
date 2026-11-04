// "Type that often? Save it as a snippet" in the main window
// (dictionary/snippet_offer.rs). Rust decides — the same address, sign-off
// or link three times in a week, offered once, at most one offer a day,
// never during a dictation — and this shows its question while the window
// is in front (else it's a card on the Yap bar):
//
// `yap-snippet-suggestion` { id, title, body, say, text, kind }:
//   Save snippet / Not now, and a small "Don't suggest this again" link
//   (no more snippet suggestions; the Dictionary view switches them back
//   on). The answer goes back with `snippet_suggestion_respond` ("save" |
//   "later" | "never" | "expire"): the toast's ✕ is "Not now", running out
//   is no answer at all. After Save, Rust adds the snippet (Settings and the
//   Dictionary view take it); this confirms it, with Open Dictionary.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';

let started = false;
let showView = null;

/**
 * Show snippet suggestions as toasts. Idempotent; the main window calls it
 * with `showView` (a sidebar view, e.g. 'dictionary').
 */
export function initSnippetSuggestions(opts = {}) {
  if (opts.showView) showView = opts.showView;
  if (started) return;
  started = true;
  listen('yap-snippet-suggestion', (e) => askSnippetSuggestion(e.payload));
}

/** Ask about a snippet (`{ id, title, body, say, text, kind }`, snippet_offer.rs `Prompt`) as a toast. */
export function askSnippetSuggestion(s) {
  if (!s?.id || !s.title) return;
  const answer = (a) => invoke('snippet_suggestion_respond', { id: s.id, answer: a });
  const quietly = (a) => answer(a).catch(() => {});
  toast({
    kind: 'tip',
    title: s.title,
    description: s.body,
    chip: 'Dictionary',
    icon: 'dictionary',
    duration: 30_000,
    action: {
      label: 'Save snippet',
      onClick: async () => {
        try {
          const say = await answer('save');
          if (!say) return;
          toast({
            title: 'Snippet saved',
            description: `Say “${say}” and Yap types it. Change the phrase in Dictionary.`,
            variant: 'success',
            chip: 'Dictionary',
            icon: 'dictionary',
            ...(showView ? { action: { label: 'Open Dictionary', onClick: () => showView('dictionary') } } : {}),
          });
        } catch (err) {
          toast({ title: "Couldn't save the snippet", description: String(err), variant: 'destructive' });
        }
      },
    },
    secondary: { label: 'Not now', onClick: () => quietly('later') },
    tertiary: { label: "Don't suggest this again", onClick: () => quietly('never') },
    onClose: () => quietly('later'),
    onExpire: () => quietly('expire'),
  });
}
