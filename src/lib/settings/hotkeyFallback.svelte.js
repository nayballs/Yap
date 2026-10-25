// The in-page hotkey fallback: when one of Yap's own windows has the focus,
// the global low-level hook never sees the hotkey (WebView2 front-runs the
// hook chain; log-proven 2026-07-05). The page gets the keydown normally,
// so drive the pipeline directly — combo-aware via lib/core/hotkeys.js, matching
// the Rust hook's semantics. Settings runs it for the main window's whole
// life (it's always mounted); it stands down while a shortcut is being
// picked (settings/hotkeys.svelte.js).
//
// The meeting shortcut (Win+Alt+M) goes first: a combo is more specific than
// a bare dictation key; then the Voice Assistant's shortcut, Read aloud,
// Snip to Chat, Copy last and the language shortcuts (My languages). Down and up go to
// `dictation_key` / `assistant_key` / `language_key`, the hook's own paths:
// the recording mode (and push-to-talk's tap / double-tap) is Rust's to
// decide.
import { invoke } from '@tauri-apps/api/core';
import {
  actionMatchesKeydown as downOn,
  actionMatchesKeyup as upOn,
  hotkeyMatchesKeydown,
  hotkeyMatchesKeyup,
} from '../core/hotkeys.js';
import { readPageSelection } from '../stores/speech.svelte.js';
import { hotkeyCapture } from './hotkeys.svelte.js';
import { languageShortcuts } from './languages.svelte.js';

/** Call while the component initialises; `getCfg()` = Settings' config copy (or null). */
export function followHotkeyFallback(getCfg) {
  function onKeyDown(e) {
    // (Not while a shortcut is being picked: settings/hotkeys.svelte.js.)
    if (hotkeyCapture.on || e.repeat) return;
    const cfg = getCfg();
    if (downOn(e, cfg, 'meetingHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      invoke('meeting_shortcut', { origin: 'settings' }).catch(() => {});
      return;
    }
    if (downOn(e, cfg, 'assistantHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      invoke('assistant_key', { pressed: true }).catch(() => {});
      return;
    }
    // Read aloud: what's selected on this page (voice/read_aloud.rs).
    if (downOn(e, cfg, 'readHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      readPageSelection();
      return;
    }
    // Snip to Chat (snip.rs): Yap's own windows leave the shot.
    if (downOn(e, cfg, 'snipHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      invoke('snip_start', { origin: 'shortcut' }).catch(() => {});
      return;
    }
    // Copy last transcript (pasting/last.rs): onto the clipboard here too.
    // (Paste last has no fallback: it pastes into another app, as "Use
    // what I said" and the transforms do.)
    if (downOn(e, cfg, 'copyLastHotkey')) {
      e.preventDefault();
      e.stopPropagation();
      invoke('copy_last_transcript').catch(() => {});
      return;
    }
    // A language shortcut (My languages, dictation/languages.rs): a dictation in its
    // language, down and up as the dictation key's.
    const lang = languageShortcuts(cfg).find((s) => hotkeyMatchesKeydown(e, s.hotkey));
    if (lang) {
      e.preventDefault();
      e.stopPropagation();
      invoke('language_key', { id: lang.id, pressed: true }).catch(() => {});
      return;
    }
    if (!downOn(e, cfg, 'hotkey')) return;
    e.preventDefault();
    e.stopPropagation();
    invoke('dictation_key', { pressed: true }).catch(() => {});
  }
  function onKeyUp(e) {
    if (hotkeyCapture.on) return;
    const cfg = getCfg();
    if (upOn(e, cfg, 'assistantHotkey')) {
      e.preventDefault();
      invoke('assistant_key', { pressed: false }).catch(() => {});
      return;
    }
    const lang = languageShortcuts(cfg).find((s) => hotkeyMatchesKeyup(e, s.hotkey));
    if (lang) {
      e.preventDefault();
      invoke('language_key', { id: lang.id, pressed: false }).catch(() => {});
      return;
    }
    if (!upOn(e, cfg, 'hotkey')) return;
    e.preventDefault();
    invoke('dictation_key', { pressed: false }).catch(() => {});
  }
  $effect(() => {
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
    return () => {
      window.removeEventListener('keydown', onKeyDown, true);
      window.removeEventListener('keyup', onKeyUp, true);
    };
  });
}
