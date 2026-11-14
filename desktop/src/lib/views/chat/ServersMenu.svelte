<script>
  // **Dev servers** (14 Nov 2026; the maintainer, on the Claude Code app's
  // Dev servers menu in its browser pane: "Can we add this to Yap, please?"):
  // a Code chat's workspace's `.claude/launch.json` servers, from the page
  // tab's bar — each with ▶ start / ■ stop and its state, Stop all, and Add
  // server (a name, a command and a port written to launch.json). A server
  // that's ready opens in a tab beside the chat (`onopen`). Rust has the rest
  // (agents/servers.rs): starting, its port, its output, never outliving Yap.
  // Its Claude starts them too, with `preview_start` (chat/pages/preview.rs).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';

  /** `workspace`: the Code chat's folder; `onopen(url, name)`: show a server's page. */
  let { workspace, onopen = () => {} } = $props();

  let open = $state(false);
  let data = $state({ configs: [], skipped: [], exists: false, hasComments: false });
  let error = $state('');
  /** Servers being started (name → true), until Rust says. */
  let starting = $state({});
  let adding = $state(false);
  let form = $state({ name: '', command: '', port: '' });
  let formError = $state('');
  let menuEl = $state(null);
  let btnEl = $state(null);

  const running = $derived(data.configs.filter((c) => c.server && (c.server.state === 'ready' || c.server.state === 'starting')));

  async function load() {
    if (!workspace) return;
    try {
      data = await invoke('dev_servers', { workspace });
      error = '';
    } catch (e) {
      error = String(e);
    }
  }

  onMount(() => {
    let un = null;
    let alive = true;
    listen('yap-dev-servers', () => {
      if (open) load();
    }).then((u) => (alive ? (un = u) : u()));
    const away = (e) => {
      if (open && menuEl && !menuEl.contains(e.target) && btnEl && !btnEl.contains(e.target)) open = false;
    };
    window.addEventListener('pointerdown', away, true);
    return () => {
      alive = false;
      un?.();
      window.removeEventListener('pointerdown', away, true);
    };
  });

  function toggle() {
    open = !open;
    if (open) {
      adding = false;
      load();
    }
  }

  async function start(c) {
    starting = { ...starting, [c.name]: true };
    error = '';
    try {
      const info = await invoke('dev_server_start', { workspace, name: c.name });
      await load();
      open = false;
      onopen(info.url, info.name);
    } catch (e) {
      error = String(e);
      await load();
    } finally {
      const { [c.name]: _, ...rest } = starting;
      starting = rest;
    }
  }

  async function stop(c) {
    if (!c.server) return;
    try {
      await invoke('dev_server_stop', { id: c.server.id });
    } catch (e) {
      error = String(e);
    }
    await load();
  }

  async function stopAll() {
    try {
      await invoke('dev_servers_stop_all', { workspace });
    } catch (e) {
      error = String(e);
    }
    await load();
  }

  /** "npm run dev -- --port 4000" → the command and its arguments (quotes keep spaces). */
  function splitLine(line) {
    const out = [];
    const re = /"([^"]*)"|'([^']*)'|(\S+)/g;
    let m;
    while ((m = re.exec(line))) out.push(m[1] ?? m[2] ?? m[3]);
    return out;
  }

  async function add() {
    formError = '';
    const parts = splitLine(form.command.trim());
    const port = Number(form.port);
    if (!form.name.trim() || !parts.length) {
      formError = 'Give it a name and a command.';
      return;
    }
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      formError = 'Give the port it listens on (1–65535).';
      return;
    }
    try {
      await invoke('dev_server_add', { workspace, name: form.name.trim(), command: parts[0], args: parts.slice(1), port });
      adding = false;
      form = { name: '', command: '', port: '' };
      await load();
    } catch (e) {
      formError = String(e);
    }
  }

  function stateOf(c) {
    if (starting[c.name]) return 'starting';
    return c.server?.state || 'stopped';
  }
  function said(c) {
    const s = stateOf(c);
    if (s === 'ready') return `Running on ${c.server.port}${c.server.movedFrom ? ` (${c.server.movedFrom} was taken)` : ''}`;
    if (s === 'starting') return 'Starting…';
    if (s === 'failed') return `Stopped: ${c.server.why || 'it ended'}`;
    return `Port ${c.port}`;
  }
</script>

<div class="servers">
  <button
    bind:this={btnEl}
    type="button"
    class="pagebtn"
    class:on={running.length > 0}
    onclick={toggle}
    aria-label="Dev servers"
    aria-expanded={open}
    title="Dev servers (this workspace's .claude/launch.json)"
    data-testid="chat-dev-servers"
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <rect x="4" y="4" width="16" height="7" rx="1.5" /><rect x="4" y="13" width="16" height="7" rx="1.5" /><path d="M8 7.5h.01M8 16.5h.01M12 7.5h4M12 16.5h4" />
    </svg>
    {#if running.length > 0}<span class="dot" aria-hidden="true"></span>{/if}
  </button>
  {#if open}
    <div bind:this={menuEl} class="menu" role="dialog" aria-label="Dev servers" data-testid="chat-dev-servers-menu">
      <div class="head">
        <span class="title">Dev servers</span>
        {#if running.length > 0}
          <button type="button" class="link" onclick={stopAll} data-testid="chat-dev-servers-stop-all">Stop all</button>
        {/if}
      </div>
      {#if data.configs.length === 0 && !adding}
        <p class="empty">
          {data.exists ? 'launch.json has no server Yap can use.' : 'No dev servers yet.'} Add one, or ask Claude in the chat to set one up.
        </p>
      {/if}
      <ul class="list">
        {#each data.configs as c (c.name)}
          {@const s = stateOf(c)}
          <li class="row" data-testid="chat-dev-server" data-name={c.name} data-state={s}>
            <span class="state {s}" aria-hidden="true"></span>
            <span class="what">
              {#if s === 'ready'}
                <button type="button" class="name link" onclick={() => { open = false; onopen(c.server.url, c.name); }} title={`Open ${c.server.url} beside the chat`}>{c.name}</button>
              {:else}
                <span class="name">{c.name}</span>
              {/if}
              <span class="sub" title={c.line}>{said(c)}</span>
            </span>
            {#if s === 'ready' || s === 'starting'}
              <button type="button" class="act" onclick={() => stop(c)} disabled={s === 'starting' && !c.server} aria-label={`Stop ${c.name}`} title="Stop" data-testid="chat-dev-server-stop">
                <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor" /></svg>
              </button>
            {:else}
              <button type="button" class="act" onclick={() => start(c)} aria-label={`Start ${c.name}`} title={`Start: ${c.line}`} data-testid="chat-dev-server-start">
                <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 6l10 6-10 6z" fill="currentColor" /></svg>
              </button>
            {/if}
          </li>
        {/each}
      </ul>
      {#if data.skipped?.length}
        <p class="skipped" title={data.skipped.join('\n')}>{data.skipped.length} more in launch.json Yap can't use.</p>
      {/if}
      {#if error}
        <p class="error" role="alert" data-testid="chat-dev-servers-error">{error}</p>
      {/if}
      {#if adding}
        <form class="add" onsubmit={(e) => { e.preventDefault(); add(); }} data-testid="chat-dev-server-add-form">
          <input type="text" placeholder="Name (web)" bind:value={form.name} aria-label="Name" spellcheck="false" />
          <input type="text" placeholder="Command (npm run dev)" bind:value={form.command} aria-label="Command" spellcheck="false" />
          <input type="text" inputmode="numeric" placeholder="Port (5173)" bind:value={form.port} aria-label="Port" />
          {#if formError}<p class="error" role="alert">{formError}</p>{/if}
          <div class="buttons">
            <button type="button" class="link" onclick={() => (adding = false)}>Cancel</button>
            <button type="submit" class="primary">Add</button>
          </div>
        </form>
      {:else if data.hasComments}
        <p class="empty">launch.json has comments: ask Claude in the chat to add a server, so they stay.</p>
      {:else}
        <button type="button" class="addbtn" onclick={() => { adding = true; formError = ''; }} data-testid="chat-dev-server-add">+ Add server</button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .servers {
    position: relative;
    display: inline-flex;
  }
  .pagebtn {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
  }
  .pagebtn:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .pagebtn svg {
    width: 16px;
    height: 16px;
  }
  .dot {
    position: absolute;
    top: 4px;
    right: 4px;
    width: 6px;
    height: 6px;
    border-radius: var(--yap-r-full);
    background: var(--yap-success);
  }
  .menu {
    position: absolute;
    top: calc(100% + 6px);
    right: 0;
    z-index: 30;
    width: 280px;
    padding: 8px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: var(--yap-r-lg);
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    box-shadow: var(--yap-shadow-menu);
    font-size: 13px;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 2px 6px 6px;
  }
  .title {
    font-weight: 600;
    color: var(--yap-paper-muted);
    font-size: 12px;
  }
  .list {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px;
    border-radius: var(--yap-r);
  }
  .row:hover {
    background: var(--yap-paper-s1);
  }
  .state {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: var(--yap-r-full);
    background: var(--yap-paper-faint);
  }
  .state.ready {
    background: var(--yap-success);
  }
  .state.starting {
    background: var(--yap-warning);
  }
  .state.failed {
    background: var(--yap-danger);
  }
  .what {
    display: flex;
    flex-direction: column;
    min-width: 0;
    flex: 1;
  }
  .name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
  }
  .sub {
    color: var(--yap-paper-muted);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .act {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-ink);
    cursor: pointer;
  }
  .act:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .act svg {
    width: 14px;
    height: 14px;
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--yap-link);
    cursor: pointer;
    font: inherit;
  }
  .name.link {
    color: var(--yap-paper-ink);
  }
  .name.link:hover {
    color: var(--yap-link);
    text-decoration: underline;
  }
  .empty,
  .skipped {
    margin: 4px 6px 8px;
    color: var(--yap-paper-muted);
    font-size: 12px;
    line-height: 1.4;
  }
  .error {
    margin: 4px 6px;
    color: var(--yap-paper-danger);
    font-size: 12px;
    line-height: 1.4;
    white-space: pre-wrap;
    max-height: 160px;
    overflow: auto;
  }
  .addbtn {
    width: 100%;
    margin-top: 4px;
    padding: 6px;
    border: 1px dashed var(--yap-paper-rule);
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
    font: inherit;
    text-align: left;
  }
  .addbtn:hover {
    color: var(--yap-paper-ink);
    background: var(--yap-paper-s1);
  }
  .add {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 6px;
    padding: 6px;
  }
  .add input {
    padding: 6px 8px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: var(--yap-r);
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
  }
  .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 12px;
    align-items: center;
  }
  .primary {
    padding: 5px 12px;
    border: none;
    border-radius: var(--yap-r);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    cursor: pointer;
    font: inherit;
  }
  .primary:hover {
    background: var(--yap-ink-hover);
  }
</style>
