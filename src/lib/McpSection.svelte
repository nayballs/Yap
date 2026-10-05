<script>
  // Settings → MCP (Connections): Wispr Flow's Settings → MCP page, local.
  // One light card per AI app, in Wispr's order (Claude, ChatGPT, Gemini,
  // Cursor), each with "Add to …": Yap edits that app's own config file
  // (src-tauri/src/mcp_clients.rs) so the app starts `yap.exe mcp`
  // (mcp.rs). Then "All other apps": one-click rows for the other apps Yap
  // knows, plus the command and JSON for anything else. The apps read
  // meetings and notes through the running Yap's Local API, so the page says
  // so, with a switch, when that's off.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { toast } from './ui/toast.svelte.js';
  import Toggle from './ui/Toggle.svelte';
  import claudeIcon from '../assets/providers/claude.svg';
  import openaiIcon from '../assets/providers/openai.svg';
  import geminiIcon from '../assets/providers/gemini.svg';
  import cursorIcon from '../assets/providers/cursor.svg';

  // Settings' own config copy, which Settings auto-saves: the "save notes"
  // switch and "Turn on the Local API" edit it (saving starts the API).
  let { cfg = $bindable() } = $props();

  // Wispr's four cards, in its order; every other app goes under "All other apps".
  const FEATURED = ['claude', 'chatgpt', 'gemini', 'cursor'];
  const ICONS = { claude: claudeIcon, 'claude-code': claudeIcon, chatgpt: openaiIcon, gemini: geminiIcon, cursor: cursorIcon };
  const ALLOW = {
    claude: 'Allow Claude to access your meeting notes and transcripts',
    chatgpt: 'Allow ChatGPT to access your meeting notes and transcripts',
    gemini: 'Allow Gemini CLI to access your meeting notes and transcripts',
    cursor: 'Allow Cursor to access your meeting notes and transcripts',
  };
  // The honest small print.
  const NOTE = {
    chatgpt: "In the ChatGPT desktop app and Codex. ChatGPT on the web can't reach apps on your PC.",
  };

  let info = $state(null); // { commandLine, snippet, devBuild, clients: [...] }
  let failed = $state('');
  let busy = $state({}); // app id → true while adding/removing
  let bridge = $state({ known: false, running: true, starting: false });

  const featured = $derived(info ? FEATURED.map((id) => info.clients.find((c) => c.id === id)).filter(Boolean) : []);
  const others = $derived(info ? info.clients.filter((c) => !FEATURED.includes(c.id)) : []);

  async function load() {
    try {
      info = await invoke('mcp_clients_status');
      failed = '';
    } catch (e) {
      failed = String(e);
    }
  }

  async function refreshBridge() {
    try {
      bridge.running = (await invoke('bridge_status')).running === true;
      bridge.known = true;
    } catch {
      /* best-effort */
    }
  }

  // Settings stays mounted while its modal is hidden, so look again each time
  // the page comes into view (an app installed meanwhile, the Local API
  // switched in Integrations…), as AccountSection does.
  let root = $state(null);
  onMount(() => {
    const seen = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        load();
        refreshBridge();
      }
    });
    seen.observe(root);
    return () => seen.disconnect();
  });

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  async function turnOnBridge() {
    bridge.starting = true;
    try {
      // Saved straight away (Settings' copy may already say "on" if it was
      // switched off elsewhere), then mirrored so its auto-save agrees.
      const fresh = await invoke('get_config');
      await invoke('save_config', { cfg: { ...fresh, bridgeEnabled: true } });
      cfg.bridgeEnabled = true;
      for (let i = 0; i < 20 && !bridge.running; i++) {
        await refreshBridge();
        if (!bridge.running) await sleep(150);
      }
      if (!bridge.running) throw new Error('Check Integrations → Local API.');
    } catch (e) {
      toast({ title: "The Local API didn't start", description: String(e?.message ?? e), variant: 'destructive' });
    } finally {
      bridge.starting = false;
    }
  }

  async function change(client, add) {
    busy = { ...busy, [client.id]: true };
    try {
      const row = await invoke(add ? 'mcp_client_add' : 'mcp_client_remove', { id: client.id });
      info.clients = info.clients.map((c) => (c.id === row.id ? row : c));
      if (add) {
        toast({ title: `Added to ${client.name}`, description: client.restart, variant: 'success' });
      } else {
        toast({ title: `Removed from ${client.name}`, variant: 'success' });
      }
    } catch (e) {
      toast({
        title: add ? `Couldn't add Yap to ${client.name}` : `Couldn't remove Yap from ${client.name}`,
        description: String(e),
        variant: 'destructive',
      });
      await load();
    } finally {
      busy = { ...busy, [client.id]: false };
    }
  }

  async function copy(text, what) {
    try {
      await navigator.clipboard.writeText(text);
      toast({ title: `${what} copied`, variant: 'success' });
    } catch {
      toast({ title: "Couldn't copy to clipboard", variant: 'destructive' });
    }
  }
</script>

{#snippet logo(c)}
  {#if ICONS[c.id]}
    <img class="logo" src={ICONS[c.id]} alt="" aria-hidden="true" />
  {:else if c.id === 'vscode'}
    <svg class="logo glyph" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m8 7-5 5 5 5" /><path d="m16 7 5 5-5 5" /></svg>
  {:else}
    <svg class="logo glyph" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M3 9c3-3 6 3 9 0s6 3 9 0" /><path d="M3 15c3-3 6 3 9 0s6 3 9 0" /></svg>
  {/if}
{/snippet}

{#snippet action(c)}
  {#if c.status === 'missing'}
    <span class="muted">Not installed</span>
  {:else if c.status === 'added'}
    <span class="added" title={c.elsewhere ? 'Added by another copy of Yap' : c.paths.join('\n')}>Added ✓</span>
    <button class="remove" aria-label="Remove Yap from {c.name}" disabled={busy[c.id]} onclick={() => change(c, false)}>Remove</button>
  {:else if c.status === 'outdated'}
    <button class="add" disabled={busy[c.id]} onclick={() => change(c, true)}>Update {c.name}</button>
  {:else}
    <button class="add" disabled={busy[c.id]} onclick={() => change(c, true)}>
      {busy[c.id] ? 'Adding…' : `Add to ${c.name}`}
    </button>
  {/if}
{/snippet}

<div class="mcp" bind:this={root}>
{#if bridge.known && !bridge.running}
  <div class="warn" role="note">
    <span>Your AI apps reach Yap through its Local API, which is off.</span>
    <button class="ink" disabled={bridge.starting} onclick={turnOnBridge}>
      {bridge.starting ? 'Turning on…' : 'Turn on the Local API'}
    </button>
  </div>
{/if}

{#if failed}
  <p class="error">{failed}</p>
{:else if !info}
  <p class="loading">Looking for your AI apps…</p>
{:else}
  <ul class="apps" aria-label="AI apps">
    {#each featured as c (c.id)}
      <li class="app" aria-label={c.name}>
        <div class="who">
          <div class="nameline">
            {@render logo(c)}
            <span class="name">{c.name}</span>
          </div>
          <div class="allow">{ALLOW[c.id]}</div>
          {#if NOTE[c.id]}<div class="note">{NOTE[c.id]}</div>{/if}
          {#if c.problem}
            <div class="problem" title={c.paths.join('\n')}>
              {c.problem}. You can add Yap by hand with the JSON below.
            </div>
          {/if}
        </div>
        <div class="act">{@render action(c)}</div>
      </li>
    {/each}
  </ul>

  <section class="other" aria-labelledby="mcp-other-apps">
    <h2 id="mcp-other-apps">All other apps:</h2>
    <ul class="rows" aria-label="Other AI apps">
      {#each others as c (c.id)}
        <li class="row" aria-label={c.name}>
          {@render logo(c)}
          <div class="who">
            <span class="name">{c.name}</span>
            <span class="detail">{c.detail}</span>
            {#if c.problem}
              <span class="problem" title={c.paths.join('\n')}>{c.problem}. You can add Yap by hand with the JSON below.</span>
            {/if}
          </div>
          <div class="act">{@render action(c)}</div>
        </li>
      {/each}
    </ul>
    <p class="hint">
      Anything else that runs local MCP servers: point it at this command, or paste the JSON into
      its MCP settings.
    </p>
    <div class="kv">
      <span class="k">Command</span>
      <code class="v">{info.commandLine}</code>
      <button class="mini" aria-label="Copy command" onclick={() => copy(info.commandLine, 'Command')}>Copy</button>
    </div>
    <div class="kv top">
      <span class="k">JSON</span>
      <pre class="code">{info.snippet}</pre>
      <button class="mini" aria-label="Copy JSON" onclick={() => copy(info.snippet, 'JSON')}>Copy</button>
    </div>
    {#if info.devBuild}
      <p class="dev">
        Development build: AI apps will start this copy of Yap, which can't be rebuilt while one of
        them is running it.
      </p>
    {/if}
  </section>

  <div class="writes">
    <Toggle
      label="Let AI apps save notes to Yap"
      desc="Adds a tool for saving notes (“save this summary to Yap”). Off: they can only read. Restart the AI app after changing this."
      bind:checked={cfg.mcpAllowWrites}
    />
  </div>

  <p class="privacy">
    Your AI apps read your notes through Yap on this PC, with no Yap server in between. What an app
    reads, it sends to its own AI service (Anthropic, OpenAI, Google…) like anything else you ask
    it.
  </p>
{/if}
</div>

<style>
  .mcp {
    min-height: 1px; /* something for the visibility observer to see */
  }
  .warn {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    margin: 0 0 16px;
    padding: 12px 14px;
    border-radius: var(--yap-r-lg);
    background: color-mix(in srgb, var(--yap-warning) 9%, var(--yap-s2));
    border: 1px solid color-mix(in srgb, var(--yap-warning) 28%, transparent);
    font-size: 12.5px;
    color: var(--yap-fg-80);
  }
  .error {
    font-size: 12.5px;
    color: var(--yap-danger);
  }
  .loading {
    font-size: 12.5px;
    color: var(--yap-muted-55);
  }

  /* Wispr's MCP cards: light, rounded, a white button on the right. */
  .apps {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .app {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 18px 20px;
    border-radius: var(--yap-r-lg);
    background: var(--yap-raised-soft);
    border: 1px solid var(--yap-border-subtle);
  }
  .who {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .nameline {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .logo {
    width: 15px;
    height: 15px;
    flex: 0 0 auto;
  }
  .glyph {
    color: var(--yap-fg-62);
  }
  .name {
    font-size: 13.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .allow {
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .note,
  .detail {
    font-size: 11.5px;
    color: var(--yap-muted-70);
  }
  .problem {
    font-size: 11.5px;
    color: var(--yap-warning);
    overflow-wrap: anywhere;
    line-height: 1.45;
  }
  .act {
    display: flex;
    align-items: center;
    gap: 12px;
    flex: 0 0 auto;
    white-space: nowrap;
  }
  .add {
    height: 34px;
    padding: 0 16px;
    font: inherit;
    font-size: 13px;
    font-weight: 500;
    color: var(--yap-fg);
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    cursor: pointer;
    transition:
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease;
  }
  .add:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
    background: var(--yap-s1);
  }
  .added {
    font-size: 13px;
    font-weight: 600;
    color: var(--yap-success);
  }
  .remove {
    background: none;
    border: none;
    padding: 2px 0;
    font: inherit;
    font-size: 12px;
    color: var(--yap-muted);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .remove:hover:not(:disabled) {
    color: var(--yap-danger);
  }
  .muted {
    font-size: 12px;
    color: var(--yap-muted-55);
  }
  .add:disabled,
  .remove:disabled,
  .ink:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .ink {
    flex: 0 0 auto;
    height: 30px;
    padding: 0 13px;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-ink-fg, #fff);
    background: var(--yap-ink, var(--yap-primary));
    border: none;
    border-radius: var(--yap-r);
    cursor: pointer;
  }
  .ink:hover:not(:disabled) {
    background: var(--yap-ink-hover, var(--yap-primary-hover));
  }

  /* All other apps */
  .other {
    margin-top: 26px;
  }
  .other h2 {
    margin: 0 0 10px;
    font-size: 13.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .rows {
    list-style: none;
    margin: 0;
    padding: 0;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .row {
    display: grid;
    grid-template-columns: 16px minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 12px;
    padding: 11px 16px;
  }
  .row + .row {
    border-top: 1px solid var(--yap-border-subtle);
  }
  .row .who {
    gap: 1px;
  }
  .row .add {
    height: 30px;
    padding: 0 13px;
    font-size: 12px;
  }
  .hint {
    margin: 14px 0 0;
    font-size: 12px;
    color: var(--yap-muted-70);
    line-height: 1.5;
  }
  .kv {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 10px;
  }
  .kv.top {
    align-items: flex-start;
  }
  .k {
    flex: 0 0 62px;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    color: var(--yap-muted-55);
    padding-top: 2px;
  }
  .v,
  .code {
    flex: 1 1 auto;
    min-width: 0;
    font-size: 11.5px;
    color: var(--yap-fg);
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: 6px;
    user-select: text;
  }
  .v {
    padding: 4px 8px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .code {
    margin: 0;
    padding: 9px 11px;
    line-height: 1.5;
    overflow-x: auto;
    white-space: pre;
  }
  .mini {
    flex: 0 0 auto;
    background: none;
    border: 1px solid var(--yap-border);
    color: var(--yap-muted);
    border-radius: 6px;
    padding: 4px 10px;
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }
  .mini:hover {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }
  .dev {
    margin: 10px 0 0;
    font-size: 11.5px;
    color: var(--yap-warning);
  }

  .writes {
    margin-top: 26px;
    padding: 14px 16px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .privacy {
    margin: 14px 2px 0;
    font-size: 11.5px;
    color: var(--yap-muted-70);
    line-height: 1.55;
  }
</style>
