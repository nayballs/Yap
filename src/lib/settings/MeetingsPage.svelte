<script>
  // Settings → Meetings (2026-10-22 review, out of General; Wispr Flow's
  // Notetaker page and its order): Call detection (meeting_detect.rs), While
  // a meeting records (meeting_guard.rs, capture.rs, diarize.rs), Notepad
  // (notepad.rs) and Calendar (calendar.rs). The 12 call apps' switches
  // live one level down, behind "Ask about calls in · 8 of 12 apps ›".
  //
  //   cfg:  Settings' config $state (auto-save does the saving).
  //   goto: opens another Settings page.
  //   apps: open on the call apps' list ("meetings#call-apps").
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Select from '../ui/Select.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import LinkRow from '../ui/LinkRow.svelte';
  import CallAppIcon from '../CallAppIcon.svelte';
  import SpeakersToggle from '../SpeakersToggle.svelte';
  import SettingsHotkey from './SettingsHotkey.svelte';
  import { meetingDetect } from '../meetingDetect.svelte.js';
  import { calendar } from '../calendar.svelte.js';
  import { formatHotkeySpec } from '../hotkeys.js';

  let { cfg, goto = null, apps = $bindable(false) } = $props();

  const STYLES = [
    { value: 'popup', label: 'Pop-up' },
    { value: 'quiet', label: 'Quietly' },
  ];
  const STYLE_DESC = {
    popup: 'A card in Yap, or a Windows notification while Yap is in the background',
    // With the Yap bar on, the card goes on the bar instead (bar.rs).
    popupBar: "A card in Yap, or on the Yap bar while you're in another app",
    quiet: 'No pop-up: it waits in the notification centre and the tray menu',
  };
  const CALL_END = [
    { value: 'ask', label: 'Ask me' },
    { value: 'stop', label: 'Stop and summarise' },
  ];
  const CALL_END_DESC = {
    ask: 'Yap asks whether to stop recording and summarise',
    stop: 'Yap stops and writes your action plan, no question asked',
  };
  const LENGTHS = [
    { value: 60, label: '1 hour' },
    { value: 120, label: '2 hours' },
    { value: 180, label: '3 hours' },
    { value: 240, label: '4 hours' },
    { value: 0, label: 'No limit' },
  ];
  const REMINDERS = [
    { value: '15s', label: 'Right before' },
    { value: '1min', label: '1 minute before' },
    { value: '2min', label: '2 minutes before' },
    { value: 'never', label: 'Never' },
  ];

  // A length set outside the list (config.json by hand) still shows as itself.
  const lengthOptions = $derived(
    LENGTHS.some((o) => o.value === cfg.meetingMaxMinutes)
      ? LENGTHS
      : [...LENGTHS, { value: cfg.meetingMaxMinutes, label: `${cfg.meetingMaxMinutes} minutes` }]
  );

  // Each app's effective choice: the saved one (`meetingDetectApps`), else
  // the app's default from meeting_detect.rs (work apps on, personal chat
  // apps off). Flipping one saves it as the person's choice.
  function asks(app) {
    return cfg.meetingDetectApps?.[app.id] ?? app.asksByDefault;
  }
  function setAsks(id, on) {
    cfg.meetingDetectApps = { ...(cfg.meetingDetectApps || {}), [id]: on };
  }
  const asking = $derived(meetingDetect.apps.filter(asks));
  const appsValue = $derived(
    meetingDetect.apps.length ? `${asking.length} of ${meetingDetect.apps.length} apps` : ''
  );

  const shortcutDesc = $derived(
    cfg.meetingHotkey
      ? 'Take notes on a call, and press again to stop and summarise'
      : 'Start and stop meeting notes from any app'
  );
</script>

{#if apps}
  <!-- One level down: every call app, with its own switch. -->
  <button type="button" class="page-back" onclick={() => (apps = false)}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 6-6 6 6 6" /></svg>
    Meetings
  </button>
  <div class="page-h">
    <h1>Ask about calls in</h1>
    <p>Work apps to start with. Personal ones like Discord and WhatsApp stay quiet unless you switch them on.</p>
  </div>
  <Group title="Call apps" id="settings-call-apps-list">
    <div class="applist" role="group" aria-label="Ask about calls in">
      {#each meetingDetect.apps as app (app.id)}
        <div class="approw">
          <CallAppIcon app={app.id} disabled={!cfg.meetingDetection} />
          <Toggle
            bind:checked={() => asks(app), (on) => setAsks(app.id, on)}
            label={app.label}
            disabled={!cfg.meetingDetection}
          />
        </div>
      {/each}
    </div>
  </Group>
  {#if !cfg.meetingDetection}
    <p class="set-note">Call detection is off, so Yap doesn't ask about any of them.</p>
  {/if}
{:else}
  <div class="page-h">
    <h1>Meetings</h1>
    <p>When Yap offers to take notes, what happens while it records, and the notepad.</p>
  </div>

  <Group title="Call detection" id="settings-meetings">
    <Row setting="call-detection">
      <Toggle
        bind:checked={cfg.meetingDetection}
        label="Detect calls and offer to take notes"
        desc="Nothing records until you click Record notes"
        hint="Yap reads Windows' own record of which app is using your microphone, on this PC only."
      />
    </Row>
    <LinkRow
      label="Ask about calls in"
      desc="Work apps to start with. Personal ones stay quiet"
      value={appsValue}
      setting="call-apps"
      onclick={() => (apps = true)}
    >
      {#snippet extra()}
        <span class="tiles" aria-hidden="true">
          {#each asking.slice(0, 5) as app (app.id)}
            <span class="tile"><CallAppIcon app={app.id} size={16} /></span>
          {/each}
          {#if asking.length > 5}<span class="tile more">+{asking.length - 5}</span>{/if}
        </span>
      {/snippet}
    </LinkRow>
    <Row
      label="How Yap asks"
      setting="call-prompt-style"
      desc={cfg.meetingDetectStyle !== 'quiet' && cfg.barEnabled
        ? STYLE_DESC.popupBar
        : (STYLE_DESC[cfg.meetingDetectStyle] ?? STYLE_DESC.popup)}
    >
      <Segmented bind:value={cfg.meetingDetectStyle} options={STYLES} label="How Yap asks" disabled={!cfg.meetingDetection} />
    </Row>
    <Row setting="auto-start">
      <Toggle
        bind:checked={cfg.meetingAutoStart}
        label="Start notes automatically after 10 seconds"
        desc="The prompt counts down, and you can cancel. Not when Yap asks quietly"
        disabled={!cfg.meetingDetection}
      />
    </Row>
  </Group>

  <Group title="While a meeting records" id="settings-recording">
    <Row label="Maximum length" desc="Yap stops and writes your action plan, with a warning 5 minutes before" setting="max-length">
      <Select bind:value={cfg.meetingMaxMinutes} options={lengthOptions} ariaLabel="Maximum recording length" />
    </Row>
    <Row label="When a call ends" desc={CALL_END_DESC[cfg.meetingCallEnd] ?? CALL_END_DESC.ask} setting="call-end">
      <Select bind:value={cfg.meetingCallEnd} options={CALL_END} ariaLabel="When a call ends" disabled={!cfg.meetingDetection} />
    </Row>
    <Row setting="end-when-quiet">
      <Toggle
        bind:checked={cfg.meetingEndWhenQuiet}
        label="End notes when the meeting goes quiet"
        desc="After 10 minutes with nobody speaking, Yap counts down 30 seconds, then stops"
        hint="Anyone speaking cancels it, and Keep going gives you another 10 minutes. Yap also asks “Still in your meeting?” when a calendar meeting runs over, and an hour in."
      />
    </Row>
    <!-- The screen-share tip's "Update settings" lands here (yap-settings-goto
         "meetings#screen-sharing", and the old "general#screen-sharing"). -->
    <div id="settings-screen-sharing">
      <Row setting="hide-from-capture">
        <Toggle
          bind:checked={cfg.meetingHideFromCapture}
          label="Hide Yap from screen sharing"
          desc="The notepad and the bar stay on your screen, out of every capture"
          hint="While a meeting records, Windows leaves them out of Teams, Zoom and Meet screen shares, the Snipping Tool and OBS."
        />
      </Row>
    </div>
    <!-- Speaker diarization (diarize.rs): the same switch as in
         Speech-to-Text, where the models are. -->
    <Row setting="speakers">
      <SpeakersToggle
        bind:enabled={cfg.meetingDiarization}
        bind:model={cfg.diarizationModel}
        place="meetings"
        onopenmodels={() => goto?.('models#speakers')}
      />
    </Row>
    <!-- Names for the voices (speakers/naming.rs): cards to confirm. -->
    <Row setting="speaker-names">
      <Toggle
        bind:checked={cfg.meetingSpeakerNames}
        label="Suggest who's speaking"
        desc="Names for the voices, from what's said and who was invited. You confirm each one."
        hint="When someone says “Priya, can you…?” and a voice answers, Yap asks “Speaker 2 is Priya?”. When a meeting ends, the AI you use for meetings can suggest more, and Yap checks each against the transcript. Nothing listens to how anyone sounds."
      />
    </Row>
  </Group>

  <Group title="Notepad" id="settings-notepad">
    <Row setting="open-notepad">
      <Toggle
        bind:checked={cfg.meetingOpenNotepad}
        label="Open the notepad when a meeting starts"
        desc="Your notes, the live transcript and the summary, beside your call"
        hint="It opens without taking the focus from your call. Closing it never stops the recording."
      />
    </Row>
    <Row setting="split-screen">
      <Toggle
        bind:checked={cfg.meetingSplitScreen}
        label="Split the screen when joining"
        desc={cfg.meetingOpenNotepad
          ? "Moves the call's window left of the notepad, so you see both"
          : 'Needs the notepad to open when a meeting starts'}
        disabled={!cfg.meetingOpenNotepad}
      />
    </Row>
    <Row setting="live-transcript">
      <Toggle
        bind:checked={cfg.meetingLiveTranscript}
        label="Show live transcript"
        desc="The words appear in the notepad while a meeting records"
      />
    </Row>
    <Row label="Meeting shortcut" desc={shortcutDesc} setting="meeting-shortcut">
      <SettingsHotkey {cfg} target="meetingHotkey" clearable />
    </Row>
  </Group>

  <Group title="Calendar" id="settings-calendar">
    <Row label="Remind me before scheduled meetings" desc="A card with Join & take notes" setting="meeting-reminder">
      <Select bind:value={cfg.meetingReminder} options={REMINDERS} ariaLabel="Notify before scheduled meetings start" />
    </Row>
    <!-- calendar/brief.rs: on the reminder card, in Meetings and the notepad. -->
    <Row setting="meeting-brief">
      <Toggle
        bind:checked={cfg.meetingBrief}
        label="Prepare a brief before meetings"
        desc="Last time with these people, open items and the agenda, each line linked to its note"
        hint="Made on this PC 15 minutes before a calendar meeting, from your meeting notes and what Yap remembers. A short summary on top comes from your meeting AI model (Language Models → Note Formatting) when it's a cloud model or 4B and up; it never adds what the notes don't say."
      />
    </Row>
    <LinkRow
      label="Calendars"
      desc={calendar.connections.length ? 'Meeting names and attendees come from them' : 'Connect Google, Outlook or any calendar'}
      value={calendar.connections.length ? calendar.connections.map((c) => c.label).join(', ') : 'None connected'}
      setting="calendars"
      onclick={() => goto?.('connectors')}
    />
  </Group>

  <p class="set-note consent-line">Recording a call? Let people know you're taking notes.</p>
{/if}

<style>
  /* Two columns of [icon] [name] [switch], a hairline over each (the
     list's first sits on the group's own). */
  .applist {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(max(220px, calc((100% - 40px) / 2)), 1fr));
    column-gap: 40px;
    margin-top: -1px;
  }
  .approw {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 48px;
    border-top: 1px solid var(--yap-rule);
  }
  .approw :global(.toggle-row) {
    flex: 1;
    min-width: 0;
  }
  .tiles {
    display: inline-flex;
    align-items: center;
  }
  .tile {
    display: inline-grid;
    place-items: center;
    width: 22px;
    height: 22px;
    margin-left: -5px;
    border-radius: 6px;
    background: var(--yap-s2);
    box-shadow: 0 0 0 2px var(--yap-s1), 0 0 0 3px var(--yap-border-subtle);
  }
  .tile:first-child {
    margin-left: 0;
  }
  .tile.more {
    width: 26px;
    font-size: 10px;
    font-weight: 700;
    color: var(--yap-fg-62);
    background: var(--yap-raised);
  }
  .consent-line {
    margin-top: -12px;
  }
</style>
