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
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Button from '../ui/Button.svelte';
  import { askConfirm } from '../ui/contextMenu.svelte.js';
  import { toast } from '../ui/toast.svelte.js';
  import { account } from '../account.svelte.js';

  let { cfg, goto = null } = $props();

  let portable = $state(false);
  let logInfo = $state(null); // { dir, file } from log_info
  let historyCount = $state(null);
  onMount(() => {
    invoke('is_portable')
      .then((p) => (portable = !!p))
      .catch(() => {});
    invoke('log_info')
      .then((i) => (logInfo = i))
      .catch(() => {});
    loadCount();
    // A dictation saved, an entry deleted from Home, all cleared (history.rs).
    const un = listen('yap-history-changed', loadCount);
    return () => un.then((f) => f());
  });
  async function loadCount() {
    try {
      const s = await invoke('get_stats');
      historyCount = typeof s?.totalTranscriptions === 'number' ? s.totalTranscriptions : null;
    } catch {
      historyCount = null;
    }
  }

  // Where the words go, by provider id (config: ppProvider, llmScopes.*).
  const PROVIDER_NAMES = {
    groq: 'Groq',
    anthropic: 'Anthropic',
    openai: 'OpenAI',
    gemini: 'Google Gemini',
    openrouter: 'OpenRouter',
    custom: 'your custom endpoint',
    local: 'your own server',
  };
  function goes(provider, what) {
    if (provider === 'ondevice') return 'Stays on this PC (the on-device model)';
    if (provider === 'local') return `${what} go to your own server`;
    return `${what} go to ${PROVIDER_NAMES[provider] || provider}`;
  }
  // Chat's own model when its tab is on, else the cleanup model's endpoint.
  const chat = $derived(cfg.llmScopes?.chat?.enabled ? cfg.llmScopes.chat : { provider: cfg.ppProvider });
  const leaves = $derived([
    { what: 'Your voice', where: 'Never leaves this PC: speech-to-text runs here', page: 'models' },
    {
      what: 'AI cleanup',
      where: cfg.postProcessEnabled ? goes(cfg.ppProvider, 'Your dictations, as text,') : 'Off',
      page: 'cleanup',
    },
    {
      what: 'Chat and the Voice Assistant',
      where: chat.provider ? goes(chat.provider, 'Your questions, and the notes they need,') : 'Not set up',
      page: 'cleanup#chat',
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
          await loadCount();
        } catch (err) {
          toast({ title: "Couldn't clear the history", description: String(err), variant: 'destructive' });
          return false;
        }
      },
    });
  }

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
