<script>
  // Settings → General (2026-10-22 review: the hotkey, audio and meetings
  // went to Dictation and Meetings): language and region, what the AI knows
  // about you, startup and updates, the Yap bar (bar.rs), the
  // reading text (Appearance) and tips.
  //
  //   cfg:            Settings' config $state (auto-save does the saving).
  //   barHiddenUntil: ms the bar is hidden until ("Hide the bar for 1 hour"
  //                   in its menu isn't a setting), or null.
  //   goto:           opens another Settings page.
  import { invoke } from '@tauri-apps/api/core';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Select from '../ui/Select.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Button from '../ui/Button.svelte';
  import AboutYouSettings from '../AboutYouSettings.svelte';
  import AppearanceSettings from '../AppearanceSettings.svelte';

  let { cfg, barHiddenUntil = null, goto = null } = $props();

  const BAR_THEMES = [
    { value: 'light', label: 'Light' },
    { value: 'dark', label: 'Dark' },
  ];
  const POSITIONS = [
    { value: 'bottom', label: 'Bottom' },
    { value: 'top', label: 'Top' },
  ];

  // Start on login applies at once (the OS can say no: the switch goes back).
  async function onAutostart(enabled) {
    try {
      await invoke('set_autostart', { enabled });
    } catch {
      cfg.autostart = !enabled;
    }
  }
  function hiddenUntilLabel(ms) {
    return new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
  }
</script>

<div class="page-h">
  <h1>General</h1>
  <p>Your region, startup and updates, the Yap bar and how Yap looks.</p>
</div>

<!-- What the AI models are told about you (about_you.rs). -->
<AboutYouSettings {cfg} {goto} />

<Group title="Startup and updates" id="settings-startup">
  <Row setting="autostart">
    <Toggle bind:checked={cfg.autostart} label="Start Yap when you sign in" desc="Yap waits in the tray, ready for your hotkey" onchange={onAutostart} />
  </Row>
  <Row setting="update-checks">
    <Toggle
      bind:checked={cfg.updateChecksEnabled}
      label="Check for updates automatically"
      desc="Downloads new versions in the background. Yap never restarts without asking"
    />
  </Row>
</Group>

<Group title="Yap bar" id="settings-bar">
  <Row setting="bar">
    <Toggle
      bind:checked={cfg.barEnabled}
      label="Show the Yap bar"
      desc="A small pill above the taskbar. Hover it to dictate or take meeting notes"
      hint="Off: the overlay shows only while you dictate, and Yap's notices come as Windows notifications."
    />
  </Row>
  {#if cfg.barEnabled && barHiddenUntil}
    <Row label={`Hidden until ${hiddenUntilLabel(barHiddenUntil)}`} desc="You hid it from its menu for an hour">
      <Button variant="secondary" size="sm" onclick={() => invoke('bar_action', { action: 'show' }).catch(() => {})}>Show it now</Button>
    </Row>
  {/if}
  <Row setting="bar-fullscreen">
    <Toggle
      bind:checked={cfg.barHideFullscreen}
      label="Hide in fullscreen apps"
      desc="Games and presentations stay clear. It still shows while you record"
      disabled={!cfg.barEnabled}
    />
  </Row>
  <Row label="Colours" desc="Dark stands out on a bright screen" setting="bar-colours">
    <Segmented bind:value={cfg.barTheme} options={BAR_THEMES} label="Bar colours" disabled={!cfg.barEnabled} />
  </Row>
  <Row label="Position" desc="Where the bar and the dictation overlay sit" setting="bar-position">
    <Select bind:value={cfg.overlayPosition} options={POSITIONS} ariaLabel="Position" />
  </Row>
</Group>

<!-- The reading font and size, and Motion, for every window
     (AppearanceSettings.svelte, lib/appearance.js). -->
<AppearanceSettings {cfg} />

<!-- Yap's notice policy (notice.rs): one notice at a time, held while you
     dictate. Tips are the one kind you can switch off. -->
<Group title="Notifications" id="settings-notifications">
  <Row setting="tips">
    <Toggle
      bind:checked={cfg.showTips}
      label="Show tips"
      desc="Short tips, like push-to-talk's double-tap, and the nudge to connect your calendar"
      hint="Questions, alerts and errors always show. Yap shows one at a time, and holds them back while you dictate."
    />
  </Row>
</Group>
