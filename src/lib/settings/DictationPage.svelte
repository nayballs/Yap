<script>
  // Settings → Dictation (2026-10-22 review, out of General and Advanced):
  // how you start dictating, which microphone Yap listens to, what shows
  // while you talk, and how your words land in other apps (pasting.md).
  //
  //   cfg:        Settings' config $state (auto-save does the saving).
  //   devices:    microphone names (list_audio_devices).
  //   outputs:    output device names (list_output_devices).
  //   onshowview: leaves Settings for a main-window view ("dictionary").
  import { invoke } from '@tauri-apps/api/core';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Select from '../ui/Select.svelte';
  import Slider from '../ui/Slider.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import LinkRow from '../ui/LinkRow.svelte';
  import SettingsHotkey from './SettingsHotkey.svelte';
  import { fillHotkey, isCopilotKey } from '../hotkeys.js';

  let { cfg, devices = [], outputs = [], onshowview = null } = $props();

  const micOptions = $derived([{ value: null, label: 'System default' }, ...devices.map((d) => ({ value: d, label: d }))]);
  const outputOptions = $derived([{ value: null, label: 'System default' }, ...outputs.map((d) => ({ value: d, label: d }))]);

  const MODES = [
    { value: 'toggle', label: 'Toggle' },
    { value: 'pushToTalk', label: 'Push-to-talk' },
  ];
  // The dictation capsule's ✕ / ✓ (bar.rs → DictationCapsule.svelte).
  const BUTTONS = [
    { value: 'hover', label: 'When you point at it' },
    { value: 'always', label: 'Always' },
    { value: 'off', label: 'Off' },
  ];
  const BUTTONS_DESC = {
    hover: '✕ to cancel and ✓ to paste appear when you point at the bar',
    always: '✕ to cancel and ✓ to paste show the whole time you dictate',
    off: 'Cancel from the tray icon instead. Undo still shows after a cancel',
  };
  const SEND_KEYS = [
    { value: 'enter', label: 'Enter' },
    { value: 'ctrlEnter', label: 'Ctrl + Enter' },
    { value: 'shiftEnter', label: 'Shift + Enter' },
  ];

  const dict = $derived.by(() => {
    const list = Array.isArray(cfg.dictionary) ? cfg.dictionary : [];
    const snippets = list.filter((e) => e?.snippet).length;
    const corrections = list.length - snippets;
    const n = (k, one, many) => `${k} ${k === 1 ? one : many}`;
    return list.length ? `${n(corrections, 'correction', 'corrections')} · ${n(snippets, 'snippet', 'snippets')}` : 'Empty';
  });
</script>

<div class="page-h">
  <h1>Dictation</h1>
  <p>How you start dictating, which microphone Yap listens to, and how your words land in other apps.</p>
</div>

<Group title="Start and stop" id="settings-hotkey">
  <Row label="Hotkey" desc="Press it in any app to start and stop dictating" setting="hotkey">
    <SettingsHotkey {cfg} target="hotkey" />
  </Row>
  <Row
    label="Recording mode"
    setting="recording-mode"
    desc={cfg.recordingMode === 'pushToTalk'
      ? isCopilotKey(cfg.hotkey)
        ? // Many laptops send the Copilot key's press and release together,
          // so holding it may not keep a recording going.
          'Hold the Copilot key while you talk. If your laptop sends it as a single tap, double-tap it to record hands-free, or use Toggle.'
        : fillHotkey('Hold {key} while you talk. Double-tap it to keep recording hands-free, then press it once more to stop.', cfg.hotkey)
      : 'Press to start and stop, or hold the key while you talk'}
  >
    <Segmented bind:value={cfg.recordingMode} options={MODES} label="Recording mode" />
  </Row>
</Group>

<Group title="Microphone and sound" id="settings-microphone">
  <Row label="Microphone" desc="Changes apply straight away" setting="microphone">
    <Select
      bind:value={cfg.inputDevice}
      options={micOptions}
      ariaLabel="Microphone"
      wide
      onchange={() => invoke('set_input_device', { device: cfg.inputDevice || null }).catch(() => {})}
    />
  </Row>
  <Row label="Chime when recording starts and stops" setting="chime">
    <Slider
      inline
      bind:value={cfg.audioFeedbackVolume}
      min={0}
      max={1}
      step={0.05}
      label="Chime volume"
      disabled={!cfg.soundEnabled}
      format={(v) => `${Math.round(Number(v) * 100)}%`}
    />
    <Toggle bind:checked={cfg.soundEnabled} ariaLabel="Chime when recording starts and stops" />
  </Row>
  <Row label="Chime plays on" desc="The speakers or headset for the chime" setting="chime-output">
    <Select bind:value={cfg.outputDevice} options={outputOptions} ariaLabel="Chime plays on" wide disabled={!cfg.soundEnabled} />
  </Row>
  <!-- Other apps' sound while you dictate (mute.rs, media_pause.rs): after
       the start chime, back after the stop chime. -->
  <Row setting="quiet-other-apps">
    <Toggle
      bind:checked={cfg.quietOtherApps}
      label="Silence other apps while you dictate"
      desc="Music, videos and games go quiet, then come back as they were"
      hint="Yap's chime still plays, and calls are never silenced: not Teams, Zoom or a meeting in your browser."
    />
  </Row>
  <Row setting="pause-media">
    <Toggle
      bind:checked={cfg.pauseMedia}
      label="Pause music and videos while you dictate"
      desc="What's playing pauses, and plays again when you're done"
    />
  </Row>
</Group>

<Group title="While you dictate" id="settings-while-dictating">
  <Row setting="live-preview">
    <Toggle bind:checked={cfg.streamingPartials} label="Live preview on the Yap bar" desc="Your words show as you speak. What's pasted is always the final text" />
  </Row>
  <Row
    label="Cancel and finish buttons"
    desc={BUTTONS_DESC[cfg.barDictationButtons] ?? BUTTONS_DESC.off}
    hint="Never while you hold the push-to-talk key: letting go finishes it."
    setting="dictation-buttons"
  >
    <Select bind:value={cfg.barDictationButtons} options={BUTTONS} ariaLabel="Cancel and finish buttons" />
  </Row>
</Group>

<Group title="Typing into apps" id="settings-typing">
  <Row setting="smart-spacing">
    <Toggle
      bind:checked={cfg.smartSpacing}
      label="Match the text around the cursor"
      desc="Spaces, capitals and full stops fit where you're typing"
      hint="Yap reads a little of the text beside your cursor, on this PC only."
    />
  </Row>
  <Row setting="chat-full-stop">
    <Toggle
      bind:checked={cfg.chatNoFullStop}
      label="Leave the full stop off chat messages"
      desc="In Slack, Teams or WhatsApp, a short message ends without one"
    />
  </Row>
  <Row setting="trailing-space">
    <Toggle
      bind:checked={cfg.appendTrailingSpace}
      label="Append trailing space"
      desc={cfg.smartSpacing ? "Only where Yap can't read the text around the cursor" : 'A space after each dictation'}
    />
  </Row>
  <Row setting="auto-submit">
    <Toggle
      bind:checked={cfg.autoSubmit}
      label="Press Enter after pasting"
      desc={cfg.terminalMode
        ? 'Sends the message for you. In a terminal, only for coding tools like Claude Code'
        : 'Sends the message for you'}
    />
  </Row>
  <Row setting="voice-enter">
    <Toggle
      bind:checked={cfg.voiceEnter}
      label="Say “press enter” to send"
      desc="End a dictation with “press enter” and Yap presses the key for you"
    />
  </Row>
  {#if cfg.autoSubmit || cfg.voiceEnter}
    <Row label="Send key" desc="The key Yap presses to send" setting="send-key">
      <Select bind:value={cfg.autoSubmitKey} options={SEND_KEYS} ariaLabel="Send key" />
    </Row>
  {/if}
  <Row setting="terminal-mode">
    <Toggle
      bind:checked={cfg.terminalMode}
      label="Safer pasting into terminals"
      desc="One line, and Enter only when you say “press enter”, so nothing runs by itself"
      hint="In Windows Terminal, PowerShell and other terminals. Claude Code and Codex in Windows Terminal keep line breaks."
    />
  </Row>
  <Row setting="restore-clipboard">
    <Toggle
      bind:checked={cfg.restoreClipboard}
      label="Restore clipboard after paste"
      desc="Puts back what you'd copied. Dictations never show up in Win+V history"
    />
  </Row>
  <LinkRow
    label="Dictionary"
    desc="Words Yap should spell your way, and snippets"
    value={dict}
    setting="dictionary"
    onclick={() => onshowview?.('dictionary')}
  />
</Group>
