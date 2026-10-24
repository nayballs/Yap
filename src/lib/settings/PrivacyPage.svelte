<script>
  // Settings → Privacy & data (2026-10-22 review; OpenWhispr's Privacy &
  // Data page, Claude's Privacy): what leaves this PC, feature by feature, as
  // it's set up right now; the dictation history (history.rs; was its own
  // History page — the list itself is Home's feed); the Local API
  // (bridge.rs); logs (Debug logging, was Advanced) and where Yap keeps its
  // data. The feature pages keep one short line and a link here.
  //
  //   cfg:  Settings' config $state (auto-save does the saving).
  //   goto: opens another Settings page.
  //   connections: Network connections (net.rs) open, one level down
  //         (bindable: Settings' `privacy#connections-list` opens it).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Button from '../ui/Button.svelte';
  import Select from '../ui/Select.svelte';
  import LinkRow from '../ui/LinkRow.svelte';
  import ConnectionsView from './ConnectionsView.svelte';
  import { askConfirm } from '../ui/contextMenu.svelte.js';
  import { toast } from '../ui/toast.svelte.js';
  import { showSaveError } from '../saveErrors.js';
  import { account } from '../account.svelte.js';
  import { PROVIDER_DESTINATIONS } from '../core/providers.js';

  let { cfg, goto = null, connections = $bindable(false) } = $props();

  // The network receipts' count for today (net.rs), for the link row.
  let net = $state(null);
  async function loadNet() {
    try {
      net = await invoke('connections_summary');
    } catch {
      net = null;
    }
  }
  const netValue = $derived(net ? `${net.today.count.toLocaleString()} today` : '');
  const netDesc = $derived(
    !net
      ? 'Every connection Yap made in the last 7 days, and why'
      : net.today.count
        ? `${net.today.leaving.toLocaleString()} left this PC today, ${net.today.local.toLocaleString()} stayed on it`
        : 'None today. Every connection Yap makes is listed here, and why'
  );

  let portable = $state(false);
  let logInfo = $state(null); // { dir, file } from log_info
  let historyCount = $state(null);
  // Recent dictation audio (recent_audio.rs): { count, bytes }.
  let audio = $state(null);
  // Chat's screenshots (Snip to Chat, snip/images.rs): { count, bytes }.
  let shots = $state(null);
  async function loadShots() {
    try {
      shots = await invoke('chat_images_usage');
    } catch {
      shots = null;
    }
  }
  // "Keep chat screenshots" (config chatImageDays; 0 = forever).
  const KEEP_SHOTS = [
    { value: 7, label: '7 days' },
    { value: 30, label: '30 days' },
    { value: 90, label: '90 days' },
    { value: 0, label: 'Forever' },
  ];
  const sizeText = (bytes) => {
    const mb = bytes / (1024 * 1024);
    return mb >= 1 ? `${mb.toFixed(mb >= 10 ? 0 : 1)} MB` : `${Math.max(1, Math.round(bytes / 1024))} KB`;
  };
  const shotsDesc = $derived(
    !shots
      ? 'Snip to Chat keeps each screenshot with its chat, on this PC'
      : shots.count
        ? `${shots.count} ${shots.count === 1 ? 'screenshot' : 'screenshots'} · ${sizeText(shots.bytes)}`
        : 'None kept right now'
  );
  onMount(() => {
    invoke('is_portable')
      .then((p) => (portable = !!p))
      .catch(() => {});
    invoke('log_info')
      .then((i) => (logInfo = i))
      .catch(() => {});
    loadCount();
    loadShots();
    loadNet();
    const unNet = listen('yap-connections', loadNet);
    // A dictation saved, an entry deleted from Home, all cleared (history.rs).
    const un = listen('yap-history-changed', loadCount);
    return () => {
      un.then((f) => f());
      unNet.then((f) => f());
    };
  });
  async function loadAudio() {
    try {
      audio = await invoke('recent_audio_status');
    } catch {
      audio = null;
    }
  }
  async function loadCount() {
    loadAudio();
    try {
      const s = await invoke('get_stats');
      historyCount = typeof s?.totalTranscriptions === 'number' ? s.totalTranscriptions : null;
    } catch {
      historyCount = null;
    }
  }

  // Where the words go, by provider id (config: ppProvider, llmScopes.*):
  // providers.js's PROVIDER_DESTINATIONS.
  function goes(provider, what) {
    if (provider === 'ondevice') return 'Stays on this PC (the on-device model)';
    if (provider === 'local') return `${what} go to your own server`;
    return `${what} go to ${PROVIDER_DESTINATIONS[provider] || provider}`;
  }
  // Chat's own model when its tab is on, else the cleanup model's endpoint.
  const chat = $derived(cfg.llmScopes?.chat?.enabled ? cfg.llmScopes.chat : { provider: cfg.ppProvider });
  const leaves = $derived([
    { what: 'Your voice', where: 'Never leaves this PC: speech-to-text runs here', page: 'models' },
    {
      // recent_audio.rs: a day of dictation audio, on this PC only.
      what: 'Recent dictation audio',
      where:
        cfg.keepRecentAudio !== false && cfg.historyEnabled !== false
          ? 'Never leaves this PC: kept here for 24 hours, to run a dictation again'
          : 'Not kept: gone once a dictation is transcribed',
      page: 'privacy#recent-audio',
    },
    {
      // meeting_audio.rs: each meeting's audio, on this PC only.
      what: 'Meeting audio',
      where:
        cfg.meetingKeepAudio !== false
          ? `Never leaves this PC: kept here for ${cfg.meetingAudioDays || 14} ${cfg.meetingAudioDays === 1 ? 'day' : 'days'}, to transcribe a meeting again and play its lines`
          : 'Never leaves this PC: deleted once a meeting is transcribed',
      page: 'meetings#meeting-audio',
    },
    {
      what: 'AI cleanup',
      where: cfg.postProcessEnabled ? goes(cfg.ppProvider, 'Your dictations, as text,') : 'Off',
      page: 'cleanup',
    },
    {
      // screen_terms.rs: the window's text is read for each dictation and
      // never kept; only names a dictation seems to contain go with it to
      // a cleanup model.
      what: 'Names on screen',
      where: !cfg.screenNames
        ? 'Off'
        : cfg.postProcessEnabled && cfg.ppProvider && cfg.ppProvider !== 'ondevice'
          ? `Read on this PC for each dictation. ${goes(cfg.ppProvider, 'The names a dictation seems to contain')} with it, for cleanup`
          : 'Read on this PC for each dictation, never kept or sent',
      page: 'dictation#screen-names',
    },
    {
      what: 'Chat and the Voice Assistant',
      where: chat.provider ? goes(chat.provider, 'Your questions, and the notes they need,') : 'Not set up',
      page: 'cleanup#chat',
    },
    {
      // snip.rs: kept on this PC; one leaves only inside a question you send.
      what: 'Screenshots',
      where: 'Kept on this PC. One goes to the AI model you use only with a question you send',
      page: 'privacy#chat-images',
    },
    {
      what: 'About you',
      where:
        (cfg.aboutName || '').trim() || (cfg.aboutMe || '').trim()
          ? 'Sent with each question, to the AI model you use'
          : 'Your region only, to the AI model you use',
      page: 'general#about-you',
    },
    {
      // memory.rs: kept on this PC, sent with questions to the model in use.
      what: 'Memory',
      where: 'Kept on this PC. What fits a question goes with it to the AI model you use',
      page: 'memory',
    },
    {
      what: 'Web search',
      where: cfg.webSearch
        ? cfg.webSearchNative !== false
          ? "Search words go to the AI model's provider, or Exa, Tavily or Wikipedia"
          : `Search words go to ${cfg.webSearchService === 'tavily' ? 'Tavily' : 'Exa'}, or Wikipedia`
        : 'Off',
      page: 'voice#web-search',
    },
    {
      what: 'Spoken replies',
      where: cfg.ttsProvider === 'elevenlabs' ? 'The words Yap speaks go to ElevenLabs' : 'Made on this PC',
      page: 'voice#voice-provider',
    },
    {
      what: 'AI apps (MCP)',
      where: 'Only the apps you add, reading meetings and notes through Yap. Never dictations',
      page: 'mcp',
    },
    { what: 'Calendar', where: 'Read straight from your calendar. No Yap server in between', page: 'connectors' },
    {
      what: 'Account',
      where: account.status?.signedIn ? 'Signed in: your email, nothing you say or write' : 'Not signed in: Yap never contacts its account service',
      page: 'account',
    },
  ]);

  function clearHistory(e) {
    const btn = e?.currentTarget;
    askConfirm({
      title: 'Clear your dictation history?',
      body: 'Every saved dictation goes from this PC, with the stats in Insights. This can’t be undone.',
      confirmLabel: 'Clear history',
      busyLabel: 'Clearing…',
      returnFocus: btn,
      run: async () => {
        try {
          await invoke('clear_history');
        } catch (err) {
          // Cleared here, but history.json still holds it until Yap can
          // save: said, with Try again.
          if (!showSaveError(err)) {
            toast({ title: "Couldn't clear the history", description: String(err), variant: 'destructive' });
            return false;
          }
        }
        await loadCount();
      },
    });
  }

  function clearAudio(e) {
    const btn = e?.currentTarget;
    askConfirm({
      title: 'Clear recent dictation audio?',
      body: 'The audio Yap kept to run dictations again goes from this PC. Your history stays.',
      confirmLabel: 'Clear audio',
      busyLabel: 'Clearing…',
      returnFocus: btn,
      run: async () => {
        try {
          await invoke('recent_audio_clear');
          await loadAudio();
        } catch (err) {
          toast({ title: "Couldn't clear the audio", description: String(err), variant: 'destructive' });
          return false;
        }
      },
    });
  }
  const audioDesc = $derived.by(() => {
    if (!audio) return 'Deletes the audio Yap kept';
    if (!audio.count) return 'Nothing kept right now';
    const mb = audio.bytes / (1024 * 1024);
    const size = mb >= 1 ? `${mb.toFixed(mb >= 10 ? 0 : 1)} MB` : `${Math.max(1, Math.round(audio.bytes / 1024))} KB`;
    return `${audio.count} ${audio.count === 1 ? 'dictation' : 'dictations'} · ${size}`;
  });

  // The Integrations view keeps its own copy of the Local API's switch.
  function onBridge(on) {
    window.dispatchEvent(new CustomEvent('yap-bridge-changed', { detail: { enabled: on } }));
  }

  let copied = $state(false);
  async function copyLogPath() {
    if (!logInfo?.file) return;
    try {
      await navigator.clipboard.writeText(logInfo.file);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      toast({ title: "Couldn't copy", description: 'Something went wrong copying to the clipboard.', variant: 'destructive' });
    }
  }
  const logName = $derived((logInfo?.file || '').split(/[/\\]/).pop() || '');
</script>

{#if connections}
  <ConnectionsView onback={() => ((connections = false), loadNet())} />
{:else}
<div class="page-h">
  <h1>Privacy & data</h1>
  <p>What leaves this PC, what Yap keeps on it, and how to clear it.</p>
</div>

<Group title="What leaves this PC" id="settings-leaves" desc="As you've set Yap up now. Everything else stays here.">
  <div class="leaves" role="table" aria-label="What leaves this PC">
    {#each leaves as l (l.what)}
      <div class="leave" role="row">
        <span class="what" role="rowheader">{l.what}</span>
        <span class="where" role="cell">{l.where}</span>
        <span role="cell">
          {#if goto}
            <button type="button" class="open" aria-label={`${l.what} settings`} onclick={() => goto(l.page)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
            </button>
          {/if}
        </span>
      </div>
    {/each}
  </div>
</Group>

<Group title="Network connections" id="settings-connections">
  <LinkRow label="Network connections" desc={netDesc} value={netValue} setting="connections" onclick={() => (connections = true)} />
</Group>

<Group title="History" id="settings-history">
  <Row setting="history">
    <Toggle
      bind:checked={cfg.historyEnabled}
      label="Keep dictation history"
      desc="Your dictations, kept only on this PC. They show in Home and Insights"
    />
  </Row>
  <Row
    label="Clear history"
    desc={historyCount != null ? `${historyCount} ${historyCount === 1 ? 'dictation' : 'dictations'} saved` : 'Removes every saved dictation'}
    setting="clear-history"
  >
    <Button variant="danger" size="sm" onclick={clearHistory}>Clear history…</Button>
  </Row>
  <Row setting="recent-audio">
    <Toggle
      bind:checked={cfg.keepRecentAudio}
      label="Keep recent dictation audio"
      desc="The last 24 hours, on this PC only, so you can run a dictation again from Home"
      hint="For a paste that didn't land, a dictation Yap couldn't transcribe, or one that came out wrong. Deleting a dictation deletes its audio. Only while history is on."
      disabled={cfg.historyEnabled === false}
      onchange={loadAudio}
    />
  </Row>
  <Row label="Clear recent audio" desc={audioDesc} setting="clear-recent-audio">
    <Button variant="secondary" size="sm" disabled={!audio?.count} onclick={clearAudio}>Clear audio…</Button>
  </Row>
</Group>

<Group title="Chat screenshots" id="settings-chat-images">
  <Row
    label="Keep chat screenshots"
    desc={shotsDesc}
    hint="Snip to Chat keeps each screenshot with its chat, on this PC only. Deleting a chat deletes its screenshots; after this long they're removed, and the chat says so."
    setting="chat-images"
  >
    <Select bind:value={cfg.chatImageDays} options={KEEP_SHOTS} ariaLabel="Keep chat screenshots" onchange={() => setTimeout(loadShots, 1500)} />
  </Row>
</Group>

<Group title="Local API" id="settings-local-api">
  <Row setting="local-api">
    <Toggle
      bind:checked={cfg.bridgeEnabled}
      label="Let apps on this PC use Yap's data"
      desc="Scripts and AI apps on this PC can read your notes, meetings and history while Yap runs"
      hint="Loopback only, with a token in ~/.yap/cli-bridge.json. AI apps added in AI apps (MCP) need it."
      onchange={onBridge}
    />
  </Row>
</Group>

<Group title="Logs and data" id="settings-logs">
  <Row setting="debug-logging">
    <Toggle
      bind:checked={cfg.debugLogging}
      label="Debug logging"
      desc={cfg.debugLogging ? 'Writing extra detail: turn it off when you’re done' : 'More detail in the log, for a bug report'}
      hint="Logs never hold API keys or what you dictate, type or ask. With Debug logging on they also hold your dictations and the AI’s text: turn it off, or skim the log, before sharing one."
    />
  </Row>
  <Row label="Log file" desc={logName || 'No log yet'} setting="logs">
    {#if logInfo?.file}
      <Button variant="ghost" size="sm" onclick={copyLogPath}>{copied ? 'Copied' : 'Copy path'}</Button>
    {/if}
    <Button variant="secondary" size="sm" onclick={() => invoke('open_logs_folder').catch(() => {})}>Open logs folder</Button>
  </Row>
  <Row label="Data folder" setting="data-folder">
    {#snippet desc()}
      Settings, notes, models and history live in
      {#if portable}the <code>Data</code> folder next to <code>yap.exe</code> (portable){:else}<code>%APPDATA%\yap</code>{/if}
    {/snippet}
  </Row>
</Group>
{/if}

<style>
  .leaves {
    display: flex;
    flex-direction: column;
  }
  .leave {
    display: grid;
    grid-template-columns: 190px minmax(0, 1fr) 28px;
    align-items: center;
    column-gap: 16px;
    min-height: 44px;
    padding: 8px 0;
    border-top: 1px solid var(--yap-rule);
  }
  .leave:first-child {
    border-top: none;
  }
  .what {
    font-size: 13px;
    font-weight: 560;
    color: var(--yap-fg);
  }
  .where {
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--yap-muted);
  }
  .open {
    display: inline-grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg-62);
    cursor: pointer;
  }
  .open:hover {
    background: var(--yap-raised-soft);
    color: var(--yap-fg);
  }
  .open:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .open svg {
    width: 15px;
    height: 15px;
  }
  code {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11.5px;
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--yap-raised-soft);
    border: 1px solid var(--yap-border-subtle);
    color: var(--yap-fg-80);
  }
</style>
