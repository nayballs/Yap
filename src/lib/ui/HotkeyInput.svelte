<script>
  // Hotkey capture input, ported from OpenWhispr's ui/HotkeyInput.tsx:
  // click to record → live modifier chips while you hold → press a key to bind
  // a combo, or hold 2+ modifiers (or a single right-side modifier) ~200 ms and
  // release to bind a modifier-only hotkey. Inline validation warning (4 s),
  // Esc cancels, Backspace clears when `clearable`, mouse extra buttons bind
  // as before. Emits Yap's spec format (see lib/hotkeys.js).
  import {
    MOD_LABELS,
    MOD_CODE_TO_VK,
    eventModifierFamily,
    hotkeyParts,
    parseHotkeySpec,
  } from '../hotkeys.js';

  let {
    value = $bindable(''),
    clearable = false,
    disabled = false,
    placeholder = 'Click to set',
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

  function onMouse(e) {
    if (e.button === 0 || e.button === 2) return; // left/right reserved for UI
    e.preventDefault();
    e.stopPropagation();
    const map = { 1: 3, 3: 4, 4: 5 }; // browser button -> our id
    finalize(`mouse:${map[e.button] ?? e.button + 1}`);
  }

  function clear() {
    value = '';
  }
</script>

<div class="hk" class:disabled>
  {#if capturing}
    <div class="face capturing">
      {#if heldMods.length}
        {#each heldMods as m (m)}
          <span class="chip live">{MOD_LABELS[m]}</span>
        {/each}
        <span class="hint">+ key, or release to set</span>
      {:else}
        <span class="hint">Press a key or hold modifiers… <span class="dim">Esc cancels{clearable ? ' · Backspace clears' : ''}</span></span>
      {/if}
    </div>
  {:else}
    <div class="row">
      <button class="face" onclick={start} {disabled}>
        {#if parts.length}
          {#each parts as p (p)}
            <span class="chip">{p}</span>
          {/each}
          {#if isChord}<span class="holdtag">hold</span>{/if}
        {:else}
          <span class="none">{placeholder}</span>
        {/if}
      </button>
      {#if clearable && value}
        <button class="clear" onclick={clear} title="Remove hotkey" aria-label="Remove hotkey">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m3 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /></svg>
        </button>
      {/if}
    </div>
  {/if}
  {#if warning}
    <p class="warn">
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
    gap: 6px;
  }
  .face {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 120px;
    min-height: 32px;
    justify-content: center;
    padding: 4px 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    transition:
      border-color var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  button.face:hover {
    border-color: var(--yap-primary);
  }
  .face.capturing {
    border-color: var(--yap-primary);
    background: var(--yap-primary-wash);
    cursor: default;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    padding: 2px 7px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-sm);
    background: var(--yap-s2);
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11.5px;
    line-height: 1.4;
  }
  .chip.live {
    border-color: var(--yap-primary);
    color: var(--yap-primary);
    background: transparent;
  }
  .holdtag {
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--yap-muted-55);
  }
  .hint {
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .hint .dim {
    color: var(--yap-muted-55);
  }
  .none {
    color: var(--yap-muted);
  }
  .clear {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted-55);
    cursor: pointer;
    transition: color var(--yap-dur) ease;
  }
  .clear:hover {
    color: var(--yap-danger, #e5484d);
  }
  .clear svg {
    width: 14px;
    height: 14px;
  }
  .warn {
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-warning, #f5a524);
  }
  .warn svg {
    width: 13px;
    height: 13px;
    flex: 0 0 auto;
  }
</style>
