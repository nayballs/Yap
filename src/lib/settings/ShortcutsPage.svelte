<script>
  // Settings → Shortcuts (2026-10-22 review; OpenWhispr's Hotkeys page): the
  // seven global shortcuts together (dictation/input_hook.rs), each in Yap's keycaps,
  // with a clash said where it happens. Their own pages show them too
  // (Dictation, Commands, Voice Assistant, Meetings): the same setting. The
  // saved transforms' shortcuts are a link to Commands → Transforms.
  //
  //   cfg:  Settings' config $state (auto-save does the saving).
  //   goto: opens another Settings page.
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import LinkRow from '../ui/LinkRow.svelte';
  import SettingsHotkey from './SettingsHotkey.svelte';
  import { transformName } from './transforms.js';
  import { languageShortcuts, shortcutName } from './languages.svelte.js';

  let { cfg, goto = null } = $props();

  // Language shortcuts (My languages, dictation/languages.rs) live with your
  // languages (Dictation): a link.
  const languageKeys = $derived(languageShortcuts(cfg).filter((s) => s.hotkey));
  const languagesValue = $derived(
    languageKeys.length === 0 ? 'None set' : languageKeys.length === 1 ? shortcutName(languageKeys[0]) : `${languageKeys.length} shortcuts`
  );

  // Saved transforms' shortcuts live with the transforms (Commands): a link.
  const withKeys = $derived((cfg?.transforms || []).filter((t) => t.hotkey));
  const transformsValue = $derived(
    withKeys.length === 0
      ? 'None set'
      : withKeys.length === 1
        ? transformName(withKeys[0])
        : `${withKeys.length} shortcuts`
  );
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
  <Row
    label="Read aloud"
    desc="Select text in any app and press it: Yap reads it to you, word by word on the Yap bar"
    hint="Press it again, or Esc, to stop. 🔊 on the Yap bar does the same."
    setting="shortcut-read"
  >
    <SettingsHotkey {cfg} target="readHotkey" clearable />
  </Row>
  <Row label="Use what I said" desc="Right after a dictation, swap what Yap pasted for your words before AI cleanup" setting="shortcut-as-said">
    <SettingsHotkey {cfg} target="asSaidHotkey" clearable />
  </Row>
  <Row label="Meeting notes" desc="Take notes on a call, and press again to stop and summarise" setting="shortcut-meeting">
    <SettingsHotkey {cfg} target="meetingHotkey" clearable />
  </Row>
  <Row
    label="Snip to Chat"
    desc="Drag a box over any screen: the picture goes into Chat for you to ask about"
    hint="Esc or right-click cancels. ✂ on the Yap bar and the tray's Snip to Chat do the same. Windows never lets an app capture its administrator prompts, and video protected from copying comes out black."
    setting="shortcut-snip"
  >
    <SettingsHotkey {cfg} target="snipHotkey" clearable />
  </Row>
  <LinkRow
    label="Language shortcuts"
    desc="Dictate in a language of yours, or in one and paste another, each on a key of its own"
    value={languagesValue}
    setting="shortcut-languages"
    onclick={() => goto?.('dictation#language-shortcuts')}
  />
  <LinkRow
    label="Transforms"
    desc="Polish, Shorter, Translate and your own, each on a key of its own"
    value={transformsValue}
    setting="shortcut-transforms"
    onclick={() => goto?.('commands#transforms')}
  />
</Group>

<p class="set-note">
  Two shortcuts can't share a key, or fire on one press: Yap says which one has it.
  How the dictation hotkey records (toggle or push-to-talk) is in
  {#if goto}<button type="button" class="set-link" onclick={() => goto('dictation#recording-mode')}>Dictation</button>{:else}Dictation{/if}.
</p>
