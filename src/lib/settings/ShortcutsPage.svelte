<script>
  // Settings → Shortcuts (2026-10-22 review; OpenWhispr's Hotkeys page): the
  // four global shortcuts together (input_hook.rs), each in Yap's keycaps,
  // with a clash said where it happens. Their own pages show them too
  // (Dictation, Commands, Voice Assistant, Meetings): the same setting.
  //
  //   cfg:  Settings' config $state (auto-save does the saving).
  //   goto: opens another Settings page.
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import SettingsHotkey from './SettingsHotkey.svelte';

  let { cfg, goto = null } = $props();
</script>

<div class="page-h">
  <h1>Shortcuts</h1>
  <p>Every global shortcut in one place. Each works in any app, even with Yap's window closed.</p>
</div>

<Group title="Global shortcuts" id="settings-shortcuts">
  <Row label="Dictate" desc="Start and stop dictating where you're typing" setting="shortcut-dictation">
    <SettingsHotkey {cfg} target="hotkey" />
  </Row>
  <Row label="Rewrite selected text" desc="Select text, press it and say how to change it" setting="shortcut-rewrite">
    <SettingsHotkey {cfg} target="editHotkey" clearable />
  </Row>
  <Row label="Ask the Voice Assistant" desc="Ask out loud from any app and hear the answer" setting="shortcut-assistant">
    <SettingsHotkey {cfg} target="assistantHotkey" clearable />
  </Row>
  <Row label="Meeting notes" desc="Take notes on a call, and press again to stop and summarise" setting="shortcut-meeting">
    <SettingsHotkey {cfg} target="meetingHotkey" clearable />
  </Row>
</Group>

<p class="set-note">
  Two shortcuts can't share a key, or fire on one press: Yap says which one has it.
  How the dictation hotkey records (toggle or push-to-talk) is in
  {#if goto}<button type="button" class="set-link" onclick={() => goto('dictation#recording-mode')}>Dictation</button>{:else}Dictation{/if}.
</p>
