// "You spoke French · Add it to My languages?" in the main window
// (dictation/languages/nudge.rs). Rust decides — a Whisper model sure of a
// language the person didn't pick, at most one offer in ten minutes, never
// during a dictation — and this shows its question while the window is in
// front (else it's a card on the Yap bar):
//
// `yap-language-nudge` { id, code, language, title, body, add, never }:
//   Add French / Not now, and a small "Don't ask about French" link. The
//   answer goes back with `language_nudge_respond` ("add" | "later" |
//   "never" | "expire"): the toast's ✕ is "Not now", running out is no
//   answer at all. After Add, Rust saves My languages (Settings adopts the
//   save); this confirms it, with Open settings.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';

let started = false;
let openSettings = null;

/**
 * Show language offers as toasts. Idempotent; the main window calls it with
 * `openSettings(target)` (Settings at "dictation#languages").
 */
export function initLanguageNudge(opts = {}) {
  if (opts.openSettings) openSettings = opts.openSettings;
  if (started) return;
  started = true;
  listen('yap-language-nudge', (e) => askLanguageNudge(e.payload));
}

/** Ask about a language (`{ id, code, language, title, body, add, never }`, nudge.rs `Prompt`). */
export function askLanguageNudge(n) {
  if (!n?.id || !n.title) return;
  const answer = (a) => invoke('language_nudge_respond', { id: n.id, answer: a });
  const quietly = (a) => answer(a).catch(() => {});
  toast({
    kind: 'tip',
    title: n.title,
    description: n.body,
    chip: 'Languages',
    icon: 'language',
    duration: 30_000,
    action: {
      label: n.add || 'Add it',
      onClick: async () => {
        try {
          const added = await answer('add');
          if (!added) return;
          toast({
            title: `${n.language} is in My languages`,
            description: 'Yap tells it apart from your other languages from now on.',
            variant: 'success',
            chip: 'Languages',
            icon: 'language',
            ...(openSettings ? { action: { label: 'Open settings', onClick: () => openSettings('dictation#languages') } } : {}),
          });
        } catch (err) {
          toast({ title: "Couldn't add it to My languages", description: String(err), variant: 'destructive' });
        }
      },
    },
    secondary: { label: 'Not now', onClick: () => quietly('later') },
    tertiary: { label: n.never || "Don't ask about it", onClick: () => quietly('never') },
    onClose: () => quietly('later'),
    onExpire: () => quietly('expire'),
  });
}
