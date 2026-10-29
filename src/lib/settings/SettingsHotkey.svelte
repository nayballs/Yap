<script>
  // One of the ten global shortcuts, wherever Settings shows it: Yap's
  // keycaps + Change (+ ✕ when it can be unset), checked against the other
  // nine, with the live binding paused while a new one is picked.
  //
  //   cfg:    Settings' config $state (auto-save does the saving).
  //   target: 'hotkey' | 'editHotkey' | 'meetingHotkey' | 'assistantHotkey' |
  //           'readHotkey' | 'asSaidHotkey' | 'snipHotkey' |
  //           'pasteLastHotkey' | 'copyLastHotkey' | 'quickNoteHotkey' | 'sendHotkey'.
  //   more:   also the action's more shortcuts (`moreHotkeys`, Settings →
  //           Shortcuts), each with ✕, and "+ Add another" (at most
  //           MORE_SHORTCUTS) — one key or mouse button more for the same
  //           action, under the same clash rules (input_hook.rs).
  import HotkeyInput from '../ui/HotkeyInput.svelte';
  import {
    MORE_SHORTCUTS,
    moreHotkeys,
    onHotkeyCapturing,
    onMoreCapturing,
    setMoreHotkeys,
    validateHotkey,
    validateMoreHotkey,
  } from './hotkeys.svelte.js';

  let { cfg, target, clearable = false, placeholder = undefined, more = false } = $props();

  const extras = $derived(more ? moreHotkeys(cfg, target) : []);

  // The first one taken away while more are set: the next moves up into
  // its place (an action's first shortcut is the one Yap names).
  $effect(() => {
    if (more && !cfg[target] && extras.length) {
      cfg[target] = extras[0];
      setMoreHotkeys(cfg, target, extras.slice(1));
    }
  });

  function setExtra(index, spec) {
    const list = [...extras];
    if (spec) list[index] = spec;
    else list.splice(index, 1);
    setMoreHotkeys(cfg, target, list);
  }
</script>

<div class="hotkeys" class:stacked={more && (extras.length || cfg[target])}>
  <HotkeyInput
    bind:value={cfg[target]}
    clearable={clearable || extras.length > 0}
    {placeholder}
    validate={(s) => validateHotkey(cfg, target, s)}
    oncapturingchange={(on) => onHotkeyCapturing(cfg, target, on)}
  />
  {#if more}
    {#each extras as spec, i (`${i}:${spec}`)}
      <HotkeyInput
        bind:value={() => spec, (v) => setExtra(i, v)}
        clearable
        validate={(s) => validateMoreHotkey(cfg, target, i, s)}
        oncapturingchange={(on) => onMoreCapturing(cfg, target, i, on)}
      />
    {/each}
    {#if cfg[target] && extras.length < MORE_SHORTCUTS}
      <!-- A new one: picked like any other, then added to the list. -->
      <HotkeyInput
        bind:value={() => '', (v) => v && setExtra(extras.length, v)}
        placeholder="+ Add another"
        validate={(s) => validateMoreHotkey(cfg, target, extras.length, s)}
        oncapturingchange={(on) => onMoreCapturing(cfg, target, extras.length, on)}
      />
    {/if}
  {/if}
</div>

<style>
  .hotkeys {
    display: contents;
  }
  .hotkeys.stacked {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
  }
</style>
