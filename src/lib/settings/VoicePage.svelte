<script>
  // Settings → Voice Assistant (chat/assistant.rs + voice/tts.rs): ask Yap's Chat out
  // loud and hear the answer. Not Commands, which type. The assistant
  // shortcut, Read aloud (voice/read_aloud.rs), opening apps (OpenAppsSettings),
  // web search (WebSearchSettings), spoken replies in Chat, the voice
  // (VoiceSettings) and a link to its model in Language Models.
  //
  //   cfg:          Settings' config $state (auto-save does the saving).
  //   goto:         opens another Settings page.
  //   offerKokoro:  the "Download Kokoro" offer open (bindable: Settings
  //                 keeps it while you're on another page).
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import LinkRow from '../ui/LinkRow.svelte';
  import VoiceSettings from './VoiceSettings.svelte';
  import OpenAppsSettings from './OpenAppsSettings.svelte';
  import WebSearchSettings from './WebSearchSettings.svelte';
  import KokoroOffer from './KokoroOffer.svelte';
  import { kokoro } from './kokoro.svelte.js';
  import { formatHotkeySpec, fillHotkey } from '../core/hotkeys.js';
  import SettingsHotkey from './SettingsHotkey.svelte';
  import PageExplain from './PageExplain.svelte';
  import { scopeModelLabel } from './scopes.js';
  import { localLlm } from './localLlm.svelte.js';

  let { cfg, goto, offerKokoro = $bindable(false) } = $props();

  // Switching replies on while Kokoro isn't downloaded offers it
  // (KokoroOffer), Windows voices speaking meanwhile. Not while a cloud
  // voice (ElevenLabs) speaks: Kokoro is only its stand-in.
  function setSpeakReplies(on) {
    cfg.speakReplies = on;
    offerKokoro = on && cfg.ttsProvider !== 'elevenlabs' && cfg.ttsEngine !== 'windows' && kokoro.inBuild && !kokoro.installed;
  }
  const SPEAK_WHEN = [
    { value: 'voice', label: 'When I asked by voice' },
    { value: 'always', label: 'Always' },
  ];
  const SPEAK_WHEN_DESC = {
    voice: 'In Chat, answers to questions you dictate are read aloud; typed ones stay quiet',
    always: 'Every answer in Chat is read aloud',
  };
</script>

<div class="page-h">
  <h1>Voice Assistant</h1>
  <p>Ask Yap something by voice and hear the answer, like a voice assistant.</p>
</div>
<PageExplain ask="Want Yap to write something into the app you're in?" to="Commands" onclick={() => goto('commands')}>
  {#if cfg.assistantHotkey}
    Press {formatHotkeySpec(cfg.assistantHotkey)} in any app and ask <q>What's on my notes for the launch?</q>: Yap answers out loud, and the conversation is in Chat.
  {:else}
    Set a shortcut below, press it in any app and ask <q>What's on my notes for the launch?</q>: Yap answers out loud, and the conversation is in Chat.
  {/if}
  {#if cfg.openApps}Or say <q>Open Spotify</q>, and Yap opens it.{/if}
  {#if cfg.webSearch}Or ask <q>Where do I find Gale in Baldur's Gate 3?</q>, and Yap looks it up on the web.{/if}
</PageExplain>
<Group title="Assistant shortcut" id="settings-assistant-shortcut">
  <Row
    label="Assistant shortcut"
    desc={cfg.recordingMode === 'pushToTalk'
      ? 'Hold it while you ask, or double-tap it to ask hands-free. It never types into your app'
      : 'Press it, ask, press it again. It never types into your app'}
    hint="Press it while Yap speaks to cut in. Answers are read aloud with Speak replies on, and shown on the Yap bar otherwise. Paused during meetings and calls."
    setting="assistant-shortcut"
  >
    <SettingsHotkey {cfg} target="assistantHotkey" clearable />
  </Row>
  {#if !cfg.assistantHotkey}
    <Row>
      <p class="set-note">Set an assistant shortcut to ask Yap from any app, even while its window is closed.</p>
    </Row>
  {/if}
  <Row setting="answer-card">
    <Toggle
      bind:checked={cfg.assistantAnswerCard}
      label="Show the answer on the Yap bar"
      desc="Your question and its answer, above the bar while Yap reads it out"
      hint="Off: just the voice, with ■ on the bar to stop it. An answer Yap couldn't read aloud still shows."
    />
  </Row>
</Group>
<!-- Read aloud, anywhere (voice/read_aloud.rs): any selected text, in
     the voice below, with the reading card on the Yap bar. -->
<Group title="Read aloud" id="settings-read-aloud">
  <Row
    label="Read aloud shortcut"
    desc="Select text in any app and press it: Yap reads it in the voice below, each word lit up on the Yap bar as it's said"
    hint="Press it again, or Esc, to stop. 🔊 on the Yap bar and the tray's Read selection aloud do the same, and notes and Chat answers have Read aloud too. The text is in your reading font (General → Appearance)."
    setting="read-shortcut"
  >
    <SettingsHotkey {cfg} target="readHotkey" clearable />
  </Row>
</Group>
<!-- "Open Teams": apps installed on this PC (apps.rs). -->
<OpenAppsSettings {cfg} />
<!-- Looking things up on the web (web.rs), for Chat too. -->
<WebSearchSettings {cfg} {goto} />
<Group title="Spoken replies in Chat" id="settings-spoken-replies">
  <Row setting="speak-replies">
    <Toggle
      bind:checked={() => cfg.speakReplies, setSpeakReplies}
      label="Speak replies"
      desc="Yap reads its answers aloud, in Chat and from the assistant shortcut. Off, Yap never speaks"
      hint="The same switch as Voice replies in Chat."
    />
    <KokoroOffer bind:open={offerKokoro} {cfg} />
  </Row>
  <Row label="When to speak" desc={SPEAK_WHEN_DESC[cfg.speakWhen] ?? SPEAK_WHEN_DESC.voice} setting="speak-when">
    <Segmented bind:value={cfg.speakWhen} options={SPEAK_WHEN} label="When to speak" disabled={!cfg.speakReplies} />
  </Row>
  <Row setting="talk-back">
    <Toggle
      bind:checked={cfg.talkBackHandsFree}
      label="Talk back hands-free"
      desc="A question you dictate into Chat is sent when you stop, and the answer read out"
      hint={fillHotkey('Press the mic or {key} to ask the next one: it cuts Yap off mid-answer.', cfg.hotkey)}
      disabled={!cfg.speakReplies}
    />
  </Row>
</Group>
<Group title="Voice" id="settings-voice">
  <VoiceSettings {cfg} />
</Group>
<Group title="Model" id="settings-assistant-model">
  <LinkRow
    label="Model"
    desc="The same as Chat's, in Language Models"
    value={scopeModelLabel(cfg, 'chat', localLlm.status.model)}
    setting="assistant-model"
    onclick={() => goto({ page: 'cleanup', scope: 'chat' })}
  />
</Group>
