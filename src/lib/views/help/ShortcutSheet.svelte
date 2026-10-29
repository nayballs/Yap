<script>
  // The Help menu's Keyboard shortcuts (and Ctrl+/ anywhere in the main
  // window): every shortcut as it's set now, in one sheet — the global ones
  // in the input hook's own order (`shortcut_sheet`, shell/help.rs, so a
  // binding added there shows up by itself, by the hook's name until it
  // has one here), each key as an amber keycap, more keys beside it; then
  // the saved transforms and the language shortcuts with keys; then the
  // keys inside Yap's window. "Change them" opens Settings → Shortcuts.
  // A dialog: Esc, ✕ or the scrim close it; the focus stays inside and goes
  // back where it was.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';
  import { hotkeyParts } from '../../core/hotkeys.js';

  let { onclose, onopensettings } = $props();

  /** Each action's name and what it does (the hook's key → words). */
  const NAMES = {
    hotkey: { label: 'Dictate', desc: null },
    editHotkey: { label: 'Rewrite selected text', desc: 'Select text, press it and say how to change it' },
    meetingHotkey: { label: 'Meeting notes', desc: 'Start notes on a call; press again to stop and summarise' },
    assistantHotkey: { label: 'Ask the Voice Assistant', desc: 'Ask out loud from any app and hear the answer' },
    readHotkey: { label: 'Read aloud', desc: 'Read the selected text to you' },
    asSaidHotkey: { label: 'Use what I said', desc: 'Swap the cleaned-up paste for your words as said' },
    snipHotkey: { label: 'Snip to Chat', desc: 'Drag a box over any screen and ask about it' },
    pasteLastHotkey: { label: 'Paste last dictation', desc: 'Paste your latest dictation again' },
    copyLastHotkey: { label: 'Copy last dictation', desc: 'Copy your latest dictation' },
    quickNoteHotkey: { label: 'Quick note', desc: 'Jot or dictate a note from any app' },
  };

  /** In Yap's own window. */
  const IN_APP = [
    { keys: ['Ctrl', '/'], label: 'This sheet' },
    { keys: ['Ctrl', 'K'], label: 'Search your dictations on Home' },
    { keys: ['Ctrl', 'F'], label: 'Search Settings, notes or chats' },
    { keys: ['Esc'], label: 'Close a dialog, a menu or Settings' },
  ];

  let sheet = $state(null);
  let error = $state('');
  let dialog = $state(null);
  let opener = null;

  /** "meeting shortcut" → "Meeting shortcut": a binding this page doesn't know yet. */
  function fallbackName(name) {
    const n = String(name || '').trim();
    return n ? n[0].toUpperCase() + n.slice(1) : 'Shortcut';
  }

  const dictateDesc = $derived(
    sheet?.mode === 'pushToTalk' ? 'Hold it and talk; let go to paste. Double-tap for hands-free' : 'Press to start, press again to paste where you’re typing'
  );

  const rows = $derived(
    (sheet?.fixed ?? []).map((b) => {
      const known = NAMES[b.key];
      return {
        key: b.key,
        label: known?.label ?? fallbackName(b.name),
        desc: b.key === 'hotkey' ? dictateDesc : (known?.desc ?? null),
        specs: b.specs,
      };
    })
  );

  onMount(() => {
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    invoke('shortcut_sheet')
      .then((s) => (sheet = s))
      .catch((e) => (error = String(e)));
    tick().then(() => dialog?.focus({ preventScroll: true }));
    return () => opener?.focus?.({ preventScroll: true });
  });

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose?.();
      return;
    }
    if (e.key !== 'Tab' || !dialog) return;
    const items = [...dialog.querySelectorAll('button')].filter((b) => !b.disabled);
    if (!items.length) return;
    const first = items[0];
    const last = items[items.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
</script>

{#snippet keycaps(spec)}
  <span class="keys">
    {#each hotkeyParts(spec) as k, i (i)}
      <kbd>{k}</kbd>
    {/each}
  </span>
{/snippet}

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose?.()}>
  <div class="sheet" bind:this={dialog} role="dialog" aria-modal="true" aria-labelledby="ks-title" tabindex="-1" onkeydown={onKeydown}>
    <header>
      <h2 id="ks-title">Keyboard shortcuts</h2>
      <button type="button" class="x" aria-label="Close" onclick={() => onclose?.()}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6.5 6.5l11 11M17.5 6.5l-11 11" /></svg>
      </button>
    </header>

    <div class="body">
      {#if error}
        <p class="err" role="alert">{error}</p>
      {:else if !sheet}
        <p class="muted">Loading…</p>
      {:else}
        <section aria-labelledby="ks-global">
          <h3 id="ks-global">Anywhere on your PC</h3>
          <ul>
            {#each rows as r (r.key)}
              <li data-shortcut={r.key}>
                <div class="what">
                  <span class="label">{r.label}</span>
                  {#if r.desc}<span class="desc">{r.desc}</span>{/if}
                </div>
                <div class="keyset">
                  {#if r.specs.length}
                    {#each r.specs as spec, i (spec + i)}
                      {#if i > 0}<span class="or">or</span>{/if}
                      {@render keycaps(spec)}
                    {/each}
                  {:else}
                    <span class="unset">Not set</span>
                  {/if}
                </div>
              </li>
            {/each}
          </ul>
        </section>

        {#if sheet.transforms.length}
          <section aria-labelledby="ks-transforms">
            <h3 id="ks-transforms">Transforms</h3>
            <ul>
              {#each sheet.transforms as t (t.key)}
                <li data-shortcut={t.key}>
                  <div class="what"><span class="label">{t.name}</span></div>
                  <div class="keyset">{@render keycaps(t.specs[0])}</div>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        {#if sheet.languages.length}
          <section aria-labelledby="ks-languages">
            <h3 id="ks-languages">Language shortcuts</h3>
            <ul>
              {#each sheet.languages as l (l.key)}
                <li data-shortcut={l.key}>
                  <div class="what"><span class="label">Dictate in {l.name}</span></div>
                  <div class="keyset">{@render keycaps(l.specs[0])}</div>
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        <section aria-labelledby="ks-app">
          <h3 id="ks-app">In Yap's window</h3>
          <ul>
            {#each IN_APP as a (a.label)}
              <li>
                <div class="what"><span class="label">{a.label}</span></div>
                <div class="keyset">
                  <span class="keys">{#each a.keys as k (k)}<kbd>{k}</kbd>{/each}</span>
                </div>
              </li>
            {/each}
          </ul>
        </section>
      {/if}
    </div>

    <footer>
      <button type="button" class="change" onclick={() => onopensettings?.('shortcuts')}>Change them in Settings</button>
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 120;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--yap-scrim);
  }
  .sheet {
    display: flex;
    flex-direction: column;
    width: min(620px, calc(100vw - 48px));
    max-height: calc(100vh - 64px);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-xl);
    background: var(--yap-s1);
    box-shadow: var(--yap-shadow-modal);
    color: var(--yap-fg);
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 18px 20px 8px 24px;
  }
  h2 {
    margin: 0;
    font-family: var(--yap-font-display);
    font-size: 26px;
    font-weight: 600;
  }
  .x {
    display: inline-flex;
    width: 32px;
    height: 32px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--yap-fg-62);
    cursor: pointer;
  }
  .x:hover {
    background: var(--yap-raised-soft);
    color: var(--yap-fg);
  }
  .x svg {
    width: 18px;
    height: 18px;
  }
  .body {
    overflow-y: auto;
    padding: 4px 24px 8px;
  }
  section + section {
    margin-top: 16px;
  }
  h3 {
    margin: 0 0 4px;
    font-size: 11.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--yap-muted);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 9px 0;
    border-bottom: 1px solid var(--yap-rule, var(--yap-border-subtle));
  }
  li:last-child {
    border-bottom: none;
  }
  .what {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .label {
    font-size: 13.5px;
    font-weight: 560;
  }
  .desc {
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .keyset {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 6px;
    flex: 0 0 auto;
    max-width: 55%;
  }
  .keys {
    display: inline-flex;
    gap: 4px;
  }
  kbd {
    min-width: 22px;
    padding: 2px 7px;
    border-radius: 6px;
    background: var(--yap-primary-tint);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    font-weight: 650;
    text-align: center;
  }
  .or {
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .unset {
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .muted {
    color: var(--yap-muted);
    font-size: 13px;
  }
  .err {
    color: var(--yap-danger);
    font-size: 13px;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    padding: 10px 24px 18px;
    border-top: 1px solid var(--yap-border-subtle);
  }
  .change {
    padding: 7px 14px;
    border: 1px solid var(--yap-border);
    border-radius: 8px;
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .change:hover {
    background: var(--yap-raised-soft);
  }
</style>
