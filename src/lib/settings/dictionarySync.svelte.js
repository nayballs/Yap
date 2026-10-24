// The dictionary in Settings' config copy and the Dictionary view
// (DictionaryView.svelte) kept converged: both edit `cfg.dictionary` from
// separate components, so events keep the two copies in step instead of
// the last save winning.
// - `yap-dictionary-changed` (window event, the Dictionary view): its list
//   (and its own switches, which ride along in this copy) adopted, marked
//   as received so it isn't echoed back;
// - `yap-dictionary-learned` (Rust, learn.rs: "Add “Kubernetes” to your
//   dictionary?"): saved there, adopted here, and — not marked as
//   received — passed on to the Dictionary view;
// - a change made in Settings (the Commands page's agent name keeps the
//   name in the dictionary) goes out as `yap-dictionary-external`.
import { listen } from '@tauri-apps/api/event';
import { onMount } from 'svelte';

/**
 * Call while the component initialises. `getCfg()` = Settings' config copy
 * (or null); `isLoaded()` = it's set up (nothing goes out before).
 */
export function followDictionary(getCfg, isLoaded) {
  let lastReceivedDictJson = null;

  function onDictChanged(e) {
    const cfg = getCfg();
    const entries = Array.isArray(e.detail) ? e.detail : e.detail?.entries;
    if (!cfg || !Array.isArray(entries)) return;
    lastReceivedDictJson = JSON.stringify(entries);
    cfg.dictionary = entries.map((x) => ({ ...x }));
    // dictionaryFuzzy and learnWords are owned by DictionaryView but ride
    // along in this cfg copy — adopt their new values or our next auto-save
    // would revert the toggles.
    if (typeof e.detail?.fuzzy === 'boolean') cfg.dictionaryFuzzy = e.detail.fuzzy;
    if (typeof e.detail?.learnWords === 'boolean') cfg.learnWords = e.detail.learnWords;
  }
  $effect(() => {
    window.addEventListener('yap-dictionary-changed', onDictChanged);
    return () => window.removeEventListener('yap-dictionary-changed', onDictChanged);
  });

  onMount(() => {
    const un = listen('yap-dictionary-learned', (e) => {
      const cfg = getCfg();
      const entries = e.payload?.entries;
      if (cfg && Array.isArray(entries)) cfg.dictionary = entries.map((x) => ({ ...x }));
    });
    return () => un.then((f) => f());
  });

  // Settings-side edits out to DictionaryView — skipping echoes of updates
  // just received from it.
  $effect(() => {
    const json = JSON.stringify(getCfg()?.dictionary ?? null);
    if (!isLoaded() || json === null || json === lastReceivedDictJson) return;
    window.dispatchEvent(new CustomEvent('yap-dictionary-external', { detail: JSON.parse(json) }));
  });
}
