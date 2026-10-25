<script>
  // Hotkey capture input, ported from OpenWhispr's ui/HotkeyInput.tsx:
  // click to record → live modifier chips while you hold → press a key to bind
  // a combo, or hold 2+ modifiers (or a single right-side modifier) ~200 ms and
  // release to bind a modifier-only hotkey. Inline validation warning (4 s),
  // Esc cancels, Backspace clears when `clearable`, mouse extra buttons bind
  // as before. Emits Yap's spec format (see lib/core/hotkeys.js).
  import {
    COPILOT_KEY,
    MOD_LABELS,
    MOD_CODE_TO_VK,
    eventModifierFamily,
    hotkeyParts,
    parseHotkeySpec,
  } from '../core/hotkeys.js';

  let {
    value = $bindable(''),
    clearable = false,
    disabled = false,
    placeholder = 'Set a shortcut',
    // validate(spec) → error string to reject, or null/undefined to accept
    validate = null,
    // called with true/false when capturing starts/stops — Settings uses it to
    // pause the live global binding while the user picks a new one
    oncapturingchange = null,
  } = $props();

  const HOLD_MS = 200; // OpenWhispr MODIFIER_HOLD_THRESHOLD_MS

  let capturing = $state(false);
  let heldMods = $state([]); // families currently held, in press order
  let warning = $state(null);
  let warnTimer = null;
  let chordAt = 0; // when the first modifier of the current chord went down
  let sawNonModifier = false; // a normal key was pressed during the chord
  let singleRightCode = null; // e.code if exactly one right-side modifier held

  const parts = $derived(hotkeyParts(value));
  const isChord = $derived(parseHotkeySpec(value).kind === 'mods');

  function warn(msg) {
    warning = msg;
    clearTimeout(warnTimer);
    warnTimer = setTimeout(() => (warning = null), 4000);
  }

  function setCapturing(on) {
    capturing = on;
    mouseMode = false;
    heldMods = [];
    chordAt = 0;
    sawNonModifier = false;
    singleRightCode = null;
    oncapturingchange?.(on);
  }

  function start() {
    if (disabled || capturing) return;
    setCapturing(true);
    window.addEventListener('keydown', onKeyDown, true);
    window.addEventListener('keyup', onKeyUp, true);
    window.addEventListener('mousedown', onMouse, true);
  }

  function stop() {
    window.removeEventListener('keydown', onKeyDown, true);
    window.removeEventListener('keyup', onKeyUp, true);
    window.removeEventListener('mousedown', onMouse, true);
    setCapturing(false);
  }

  function finalize(spec) {
    if (validate) {
      const err = validate(spec);
      if (err) {
        warn(err);
        heldMods = [];
        chordAt = 0;
        sawNonModifier = false;
        singleRightCode = null;
        return; // stay capturing so the user can try again
      }
    }
    // A refused key's warning goes with the key that's taken.
    warning = null;
    clearTimeout(warnTimer);
    value = spec;
    stop();
  }

  function onKeyDown(e) {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') return stop();
    if (clearable && e.key === 'Backspace') {
      value = '';
      return stop();
    }
    const fam = eventModifierFamily(e);
    if (fam) {
      if (!heldMods.includes(fam)) {
        if (!heldMods.length) {
          chordAt = performance.now();
          sawNonModifier = false;
          singleRightCode = e.code.endsWith('Right') ? e.code : null;
        } else {
          singleRightCode = null; // two+ held — no longer a single
        }
        heldMods = [...heldMods, fam];
      }
      return;
    }
    // Non-modifier key → bare key or combo, from the event's modifier state.
    sawNonModifier = true;
    if (!e.keyCode) return;
    const mods = [];
    if (e.ctrlKey) mods.push('ctrl');
    if (e.altKey) mods.push('alt');
    if (e.shiftKey) mods.push('shift');
    if (e.metaKey) mods.push('win');
    finalize(mods.length ? `kb:${mods.join('+')}+${e.keyCode}` : `kb:${e.keyCode}`);
  }

  function onKeyUp(e) {
    // Print Screen: Windows sends the page only its release, never a press.
    if (capturing && e.keyCode === 44 && !eventModifierFamily(e)) {
      e.preventDefault();
      e.stopPropagation();
      const mods = [];
      if (e.ctrlKey) mods.push('ctrl');
      if (e.altKey) mods.push('alt');
      if (e.shiftKey) mods.push('shift');
      if (e.metaKey) mods.push('win');
      return finalize(mods.length ? `kb:${mods.join('+')}+44` : 'kb:44');
    }
    const fam = eventModifierFamily(e);
    if (!fam || !capturing) return;
    e.preventDefault();
    e.stopPropagation();
    // Releasing a chord that was held ≥200 ms with no normal key in between
    // captures it as a modifier-only hotkey (OpenWhispr semantics): 2+
    // modifiers, or exactly one right-side modifier.
    const heldLong = chordAt && performance.now() - chordAt >= HOLD_MS;
    if (!sawNonModifier && heldLong) {
      if (heldMods.length >= 2) return finalize(`mods:${heldMods.join('+')}`);
      if (heldMods.length === 1 && singleRightCode) {
        const vk = MOD_CODE_TO_VK[singleRightCode];
        if (vk) return finalize(`kb:${vk}`);
      }
    }
    heldMods = heldMods.filter((f) => f !== fam);
    if (!heldMods.length) {
      chordAt = 0;
      sawNonModifier = false;
      singleRightCode = null;
    }
  }

  // The guided mouse set-up (Wispr Flow's, by behaviour): "Press the mouse
  // button you want to use" — a side button or the middle one; left and
  // right are refused, saying why (they're for clicking).
  let mouseMode = $state(false);

  /** Which mouse buttons Yap can use (input_hook.rs): the middle one, Back, Forward. */
  const MOUSE_IDS = { 1: 3, 3: 4, 4: 5 }; // browser button -> our id

  function onMouse(e) {
    const own = e.target?.closest?.('.capture button');
    if (e.button === 0 || e.button === 2) {
      // Left/right are for clicking: the strip's own buttons still work.
      if (mouseMode && !own) {
        e.preventDefault();
        e.stopPropagation();
        warn('Left and right click are for clicking. Press a side button or the middle button (the wheel).');
      }
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    const id = MOUSE_IDS[e.button];
    if (!id) {
      warn("Yap can use the middle button and the two side buttons. For another, have your mouse's own app send a key, then press that key.");
      return;
    }
    finalize(`mouse:${id}`);
  }

  function clear() {
    value = '';
  }
</script>

<div class="hk" class:disabled>
  {#if capturing}
    <div class="capture" role="status" aria-live="polite">
      {#if mouseMode}
        <span class="hint">Press the mouse button you want to use <span class="dim">· a side button or the middle one · Esc cancels</span></span>
        <button class="copilot" onclick={() => (mouseMode = false)} title="Pick a key instead">Keys</button>
      {:else if heldMods.length}
        {#each heldMods as m (m)}
          <kbd class="key live">{MOD_LABELS[m]}</kbd>
        {/each}
        <span class="hint">+ a key, or let go to set</span>
      {:else}
        <span class="hint">Press a key or hold modifiers <span class="dim">· Esc cancels{clearable ? ' · Backspace clears' : ''}</span></span>
        <!-- Windows may open Copilot before Yap's window sees the key, so
             it's offered as a button too (hotkeys.js COPILOT_KEY). -->
        <button
          class="copilot"
          onclick={() => finalize(COPILOT_KEY)}
          title="The Copilot key on new Windows laptops (it sends Win + Shift + F23)">Copilot key</button>
        <!-- A mouse's side button or middle button, guided. -->
        <button class="copilot" onclick={() => (mouseMode = true)} title="Use a mouse button: a side button or the middle one">Mouse button</button>
      {/if}
    </div>
  {:else}
    <div class="row">
      {#if parts.length}
        <!-- The keys, in Yap's keycaps (the Home greeting's), and Change:
             one button, so the whole thing is the target. -->
        <button class="face" onclick={start} {disabled} title="Change the shortcut">
          <span class="keys">
            {#each parts as p (p)}
              <kbd class="key">{p}</kbd>
            {/each}
            {#if isChord}<span class="holdtag">hold</span>{/if}
          </span>
          <span class="change">Change</span>
        </button>
        {#if clearable}
          <button class="clear" onclick={clear} title="Remove the shortcut" aria-label="Remove hotkey">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
          </button>
        {/if}
      {:else}
        <button class="set" onclick={start} {disabled}>{placeholder}</button>
      {/if}
    </div>
  {/if}
  {#if warning}
    <p class="warn" role="alert">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /><path d="M12 9v4M12 17h.01" /></svg>
      {warning}
    </p>
  {/if}
</div>

<style>
  .hk {
    display: flex;
    flex-direction: column;
    gap: 6px;
    align-items: flex-end;
  }
  .hk.disabled {
    opacity: 0.55;
    pointer-events: none;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .face {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    min-height: 32px;
    padding: 3px 4px 3px 3px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg);
    font: inherit;
    cursor: pointer;
  }
  .keys {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  /* Yap's keycap: the amber key, as in Home's greeting. */
  .key {
    display: inline-grid;
    place-items: center;
    min-width: 26px;
    height: 24px;
    padding: 0 7px;
    box-sizing: border-box;
    font-family: inherit;
    font-size: 11.5px;
    font-weight: 700;
    color: var(--yap-key-fg);
    background: var(--yap-key-bg);
    border: 1px solid var(--yap-key-border);
    border-bottom-width: 2px;
    border-radius: 6px;
    white-space: nowrap;
  }
  .key.live {
    background: var(--yap-s2);
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }
  .change {
    padding: 0 8px;
    height: 28px;
    display: inline-flex;
    align-items: center;
    border-radius: var(--yap-r);
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg-62);
    transition:
      background var(--yap-dur) ease,
      color var(--yap-dur) ease;
  }
  .face:hover .change {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .face:focus-visible,
  .set:focus-visible,
  .clear:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .set {
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .set:hover {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .capture {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-height: 32px;
    padding: 0 12px;
    border: 1px solid var(--yap-primary-line);
    border-radius: var(--yap-r);
    background: var(--yap-primary-wash);
  }
  .copilot {
    margin-left: 4px;
    height: 24px;
    padding: 0 9px;
    border: 1px solid var(--yap-border);
    border-radius: 6px;
    background: var(--yap-s2);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }
  .copilot:hover {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .copilot:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .holdtag {
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--yap-muted);
  }
  .hint {
    font-size: 12px;
    color: var(--yap-fg-80);
  }
  .hint .dim {
    color: var(--yap-muted);
  }
  .clear {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg-62);
    cursor: pointer;
    transition:
      color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .clear:hover {
    color: var(--yap-danger);
    background: var(--yap-s3);
  }
  .clear svg {
    width: 13px;
    height: 13px;
  }
  .warn {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    max-width: 320px;
    font-size: 12px;
    line-height: 1.4;
    color: var(--yap-danger);
  }
  .warn svg {
    width: 13px;
    height: 13px;
    flex: 0 0 auto;
  }
</style>
