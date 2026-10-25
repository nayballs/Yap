<script>
  // One of the nine global shortcuts, wherever Settings shows it: Yap's
  // keycaps + Change (+ ✕ when it can be unset), checked against the other
  // eight, with the live binding paused while a new one is picked.
  //
  //   cfg:    Settings' config $state (auto-save does the saving).
  //   target: 'hotkey' | 'editHotkey' | 'meetingHotkey' | 'assistantHotkey' |
  //           'readHotkey' | 'asSaidHotkey' | 'snipHotkey' |
  //           'pasteLastHotkey' | 'copyLastHotkey'.
  import HotkeyInput from '../ui/HotkeyInput.svelte';
  import { onHotkeyCapturing, validateHotkey } from './hotkeys.svelte.js';

  let { cfg, target, clearable = false, placeholder = undefined } = $props();
</script>

<HotkeyInput
  bind:value={cfg[target]}
  {clearable}
  {placeholder}
  validate={(s) => validateHotkey(cfg, target, s)}
  oncapturingchange={(on) => onHotkeyCapturing(cfg, target, on)}
/>
