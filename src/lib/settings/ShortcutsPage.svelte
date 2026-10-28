<script>
  // Settings → Shortcuts (2026-10-22 review; OpenWhispr's Hotkeys page): the
  // ten global shortcuts together (dictation/input_hook.rs), each in Yap's keycaps,
  // with a clash said where it happens, and "+ Add another" for more keys
  // for the same action (Wispr Flow's, 2026-10-25: `moreHotkeys`). Their own
  // pages show the first one too (Dictation, Commands, Voice Assistant,
  // Meetings): the same setting. The saved transforms' shortcuts are a link
  // to Commands → Transforms.
  //
  //   cfg:  Settings' config $state (auto-save does the saving).
  //   goto: opens another Settings page.
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import LinkRow from '../ui/LinkRow.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import SettingsHotkey from './SettingsHotkey.svelte';
  import { validateHotkey } from './hotkeys.svelte.js';
  import { toast } from '../ui/toast.svelte.js';
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
    <SettingsHotkey {cfg} target="hotkey" more />
  </Row>
  <Row label="Rewrite selected text" desc="Select text, press it and say how to change it" setting="shortcut-rewrite">
    <SettingsHotkey {cfg} target="editHotkey" clearable more />
  </Row>
  <Row label="Ask the Voice Assistant" desc="Ask out loud from any app and hear the answer" setting="shortcut-assistant">
    <SettingsHotkey {cfg} target="assistantHotkey" clearable more />
  </Row>
  <Row
    label="Read aloud"
    desc="Select text in any app and press it: Yap reads it to you, word by word on the Yap bar"
    hint="Press it again, or Esc, to stop. 🔊 on the Yap bar does the same."
    setting="shortcut-read"
  >
    <SettingsHotkey {cfg} target="readHotkey" clearable more />
  </Row>
  <Row label="Use what I said" desc="Right after a dictation, swap what Yap pasted for your words before AI cleanup" setting="shortcut-as-said">
    <SettingsHotkey {cfg} target="asSaidHotkey" clearable more />
  </Row>
  <Row
    label="Paste last dictation"
    desc="Paste your latest dictation again where you're typing, for a paste that went astray"
    setting="shortcut-paste-last"
  >
    <SettingsHotkey {cfg} target="pasteLastHotkey" clearable more />
  </Row>
  <Row
    label="Copy last dictation"
    desc="Copy your latest dictation without pasting it"
    hint="Like Yap's pastes, it stays out of Windows' clipboard history (Win+V). The tray menu has it too."
    setting="shortcut-copy-last"
  >
    <SettingsHotkey {cfg} target="copyLastHotkey" clearable more />
  </Row>
  <Row
    label="Quick note"
    desc="Jot or dictate a note from any app, in a small window. Press it again to put it away"
    hint="Your quick notes are saved as you type, in the Quick notes folder in Notes. The tray menu has New quick note too."
    setting="shortcut-quick-note"
  >
    <SettingsHotkey {cfg} target="quickNoteHotkey" clearable more />
  </Row>
  <Row label="Meeting notes" desc="Take notes on a call, and press again to stop and summarise" setting="shortcut-meeting">
    <SettingsHotkey {cfg} target="meetingHotkey" clearable more />
  </Row>
  <Row
    label="Snip to Chat"
    desc="Drag a box over any screen: the picture goes into Chat for you to ask about"
    hint="Esc or right-click cancels. ✂ on the Yap bar and the tray's Snip to Chat do the same. With Print Screen, Yap snips instead of Windows while it runs. Windows never lets an app capture its administrator prompts, and video protected from copying comes out black."
    setting="shortcut-snip"
  >
    <SettingsHotkey {cfg} target="snipHotkey" clearable more />
  </Row>
  {#if cfg.snipHotkey !== 'kb:44'}
    <Row>
      <p class="set-note">
        Rather snip with Print Screen?
        <button
          class="set-link"
          type="button"
          onclick={() => {
            const err = validateHotkey(cfg, 'snipHotkey', 'kb:44');
            if (err) toast({ title: "Couldn't use Print Screen", description: err, variant: 'destructive' });
            else cfg.snipHotkey = 'kb:44';
          }}>Use Print Screen</button
        >
      </p>
    </Row>
  {/if}
  <!-- Every snip to Chat on the clipboard too, as Windows' Snipping Tool
       does (snip.rs; out of Win+V history). -->
  <Row setting="snip-copy">
    <Toggle
      bind:checked={cfg.snipCopy}
      label="Copy every snip to the clipboard"
      desc="Paste it anywhere with Ctrl+V, as well as asking about it in Chat"
      hint="Like Yap's pastes, it stays out of Windows' clipboard history (Win+V). Pictures you paste or drop into Chat aren't copied again."
    />
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
  Each can have up to three more keys or mouse buttons: "+ Add another" (F9 and a mouse's side button, say).
  Two shortcuts can't share a key, or fire on one press: Yap says which one has it.
  How the dictation hotkey records (toggle or push-to-talk) is in
  {#if goto}<button type="button" class="set-link" onclick={() => goto('dictation#recording-mode')}>Dictation</button>{:else}Dictation{/if}.
</p>
