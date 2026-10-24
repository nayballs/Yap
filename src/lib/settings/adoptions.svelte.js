// What other surfaces save, taken into Settings' config copy one setting
// at a time — or the copy's next auto-save would put the old value back.
// The older one-off adoptions; `yap-config-saved` (configSync.svelte.js)
// covers every save since 2026-10-23, and they agree. (The dictionary's
// are dictionarySync.svelte.js's.)
import { listen } from '@tauri-apps/api/event';
import { onMount } from 'svelte';
import { adoptProviderKey } from '../providers.js';
import { SCOPE_DEFAULTS } from './scopes.js';
import { lm } from './languageModels.svelte.js';
import { refreshLocalLlm } from './localLlm.svelte.js';

/** Call while the component initialises; `getCfg()` = Settings' config copy (or null). */
export function followOutsideChanges(getCfg) {
  // A view outside Settings saved a config field (the Integrations view's
  // Local API switch; the e2e suite's setConfig): adopt it.
  function onConfigPatched(e) {
    const cfg = getCfg();
    if (cfg && e.detail && typeof e.detail === 'object') {
      Object.assign(cfg, e.detail);
      // The scope tabs read their scope once (ScopeProviderConfig).
      if (e.detail.llmScopes) lm.chatScopeRev += 1;
    }
  }
  // The Chat model picker connected a provider (ai_model.rs
  // `set_provider_key` saved it): the key where this copy keeps it too —
  // the shared store, cleanup's own when cleanup is on that provider, the
  // Chat scope's (providers.js `adoptProviderKey`).
  function onProviderKeySaved(e) {
    const cfg = getCfg();
    const { provider: p, key } = e.detail || {};
    if (!cfg || !p || !key) return;
    if (adoptProviderKey(cfg, p, key)) lm.chatScopeRev += 1;
  }
  $effect(() => {
    window.addEventListener('yap-config-patched', onConfigPatched);
    window.addEventListener('yap-provider-key-saved', onProviderKeySaved);
    return () => {
      window.removeEventListener('yap-config-patched', onConfigPatched);
      window.removeEventListener('yap-provider-key-saved', onProviderKeySaved);
    };
  });

  onMount(() => {
    const uns = [
      // "Don't ask for Teams" on a call prompt (meeting_detect.rs): the
      // app's choice in `meetingDetectApps`, as Meetings → "Ask about calls
      // in" saves it; "Stop asking about calls?" → Stop asking, or "Turn
      // back on".
      listen('yap-meeting-detect-choice', (e) => {
        const cfg = getCfg();
        const c = e.payload;
        if (cfg && c?.app && typeof c.asks === 'boolean') {
          cfg.meetingDetectApps = { ...(cfg.meetingDetectApps || {}), [c.app]: c.asks };
        }
        if (cfg && typeof c?.enabled === 'boolean') cfg.meetingDetection = c.enabled;
      }),
      // The on-device model's context was set (Language Models or the Chat
      // model picker: local_llm_set_context saved it); and its state read
      // again as its restart waits and ends.
      listen('yap-local-context-changed', (e) => {
        const cfg = getCfg();
        if (cfg && typeof e.payload?.localContext === 'number') cfg.localContext = e.payload.localContext;
      }),
      listen('yap-local-llm-changed', () => refreshLocalLlm()),
      // The Chat view's model chip switched the Chat scope's model
      // (ai_model.rs chat_set_model saved it): its endpoint adopted, the
      // prompt kept this copy's. The Chat tab remounts (`lm.chatScopeRev`):
      // its provider pills read the scope once.
      listen('yap-chat-model-changed', (e) => {
        const cfg = getCfg();
        // The picker chose another of the on-device models: the one for
        // every feature (the sidecar serves one).
        if (cfg && typeof e.payload?.ppLocalModel === 'string') {
          cfg.ppLocalModel = e.payload.ppLocalModel;
          refreshLocalLlm();
        }
        const s = e.payload?.scope;
        if (!cfg || !s || typeof s !== 'object') return;
        if (!cfg.llmScopes || typeof cfg.llmScopes !== 'object') cfg.llmScopes = {};
        const cur = cfg.llmScopes.chat || {};
        // The whole saved scope (every field, so the tab's bindings have
        // values), with this copy's keys and prompt kept.
        cfg.llmScopes.chat = {
          disableThinking: false,
          ...cur,
          ...s,
          apiKeys: { ...(cur.apiKeys || {}), ...(s.apiKeys || {}) },
          prompt: cur.prompt || s.prompt || SCOPE_DEFAULTS.chat.prompt,
        };
        lm.chatScopeRev += 1;
      }),
      // Voice Assistant → Spoken replies (tts.rs): the Chat view's 🔊 saves
      // "Speak replies", its volume slider the volume (tts_volume_set), the
      // reading card's − / + the speed (read_aloud.rs).
      listen('yap-voice-replies', (e) => {
        const cfg = getCfg();
        if (cfg && typeof e.payload?.on === 'boolean') cfg.speakReplies = e.payload.on;
      }),
      listen('yap-voice-volume', (e) => {
        const cfg = getCfg();
        if (cfg && Number.isFinite(e.payload?.volume)) cfg.ttsVolume = e.payload.volume;
      }),
      listen('yap-voice-rate', (e) => {
        const cfg = getCfg();
        if (cfg && Number.isFinite(e.payload?.rate)) cfg.ttsRate = e.payload.rate;
      }),
      // The notepad's consent message, edited there (notepad.rs).
      listen('yap-consent-message-changed', (e) => {
        const cfg = getCfg();
        if (cfg && typeof e.payload?.message === 'string') cfg.meetingConsentMessage = e.payload.message;
      }),
      // "Turn off the bar" in the bar's own menu (bar.rs).
      listen('yap-bar-changed', (e) => {
        const cfg = getCfg();
        if (cfg && typeof e.payload?.enabled === 'boolean') cfg.barEnabled = e.payload.enabled;
      }),
    ];
    return () => uns.forEach((u) => u.then((f) => f()));
  });
}
