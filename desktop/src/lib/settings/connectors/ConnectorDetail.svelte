<script>
  // A connector's page (Settings → Connectors → a connector; the design's
  // §10.5), one level down from Yours:
  // - the header: its mark, its name, "by …" for one from the catalogue, its
  //   status with the button it asks for (Sign in · Cancel · Try again), Sign
  //   out for one signed in through the browser, its switch;
  // - what it reaches: the link, or the exact command line it runs (as the
  //   person approved it), its variables by name (a secret one 🔒), its
  //   headers by name, the sign-in's server and scopes;
  // - its tools grouped Reads · Changes things · Can delete, each Always
  //   allow / Ask first / Off (a group's own control sets them all; a tool
  //   that can delete never offers Always allow), what they cost each Chat
  //   step, and Refresh tools;
  // - Advanced: your own client ID (and secret) for a server Yap has no
  //   registration with; Show details (the server, its MCP version, the last
  //   lines a program wrote, kept in memory only);
  // - when it was last used, and Remove… (asked first; a sign-in revoked).
  // Opening it lists the tools of one that hasn't connected in this run
  // (the design's "when Settings opens its page").
  //
  //   id; entry: its catalogue entry (or null); onback.
  import { invoke } from '@tauri-apps/api/core';
  import Toggle from '../../ui/Toggle.svelte';
  import ConnectorLogo from './ConnectorLogo.svelte';
  import ToolRow, { settingsFor } from './ToolRow.svelte';
  import { toast } from '../../ui/toast.svelte.js';
  import { askConfirm } from '../../ui/contextMenu.svelte.js';
  import { openExternalLink } from '../../core/externalLinks.js';
  import {
    conn,
    connectorById,
    statusAction,
    statusTone,
    signIn,
    cancelSignIn,
    signOut,
    retry,
    setEnabled,
    setTool,
    removeConnector,
    putConnector,
    ago,
    words,
  } from './connectors.svelte.js';

  let { id, entry = null, onback } = $props();

  const c = $derived(connectorById(id));
  const action = $derived(c ? statusAction(c) : null);
  const tone = $derived(c ? statusTone(c) : 'off');
  // A failed attempt's words, when the status line doesn't say them already.
  const trouble = $derived(c && conn.trouble[c.id] && conn.trouble[c.id] !== c.status?.line ? conn.trouble[c.id] : '');
  let busy = $state(false);
  let saving = $state('');
  let now = $state(Date.now() / 1000);
  let details = $state(null);
  let detailsOpen = $state(false);
  let removeBtn = $state(null);
  // The person's own client (Advanced): typed, sent, forgotten.
  let ownId = $state('');
  let ownSecret = $state('');
  let ownError = $state('');

  $effect(() => {
    const t = setInterval(() => (now = Date.now() / 1000), 30_000);
    return () => clearInterval(t);
  });

  // Opened while it hasn't connected in this run: list its tools now.
  let listedOnce = false;
  $effect(() => {
    if (!listedOnce && c?.enabled && c.status?.state === 'idle') {
      listedOnce = true;
      retry(c);
    }
  });

  $effect(() => {
    if (c?.signIn?.ownClientId != null && !ownId) ownId = c.signIn.ownClientId;
  });

  const GROUPS = [
    { kind: 'reads', title: 'Reads', line: 'Looks things up. Nothing changes.' },
    { kind: 'changes', title: 'Changes things', line: 'Creates or updates things in your account.' },
    { kind: 'deletes', title: 'Can delete', line: 'May remove things, or says nothing about what it does. Never Always allow.' },
  ];
  const groups = $derived(GROUPS.map((g) => ({ ...g, tools: (c?.tools ?? []).filter((t) => t.kind === g.kind) })).filter((g) => g.tools.length));
  const offered = $derived((c?.tools ?? []).filter((t) => !t.skipped && t.setting !== 'off'));

  async function run(f) {
    busy = true;
    try {
      return await f();
    } finally {
      busy = false;
    }
  }

  async function set(tool, setting) {
    saving = tool.name;
    const r = await setTool(c, tool.name, setting);
    saving = '';
    if (!r.ok) toast({ title: "Couldn't change that tool", description: r.why, variant: 'destructive' });
  }

  // A group's control sets the tools in it that Yap offers as they are: one
  // it can't offer stays Off, and one whose description reads like orders
  // to the model is only turned on by its own row, on purpose.
  const settable = (t) => !t.skipped && !t.flag;
  /** The group's setting when every tool it sets has the same one. */
  function groupValue(g) {
    const vals = new Set(g.tools.filter(settable).map((t) => t.setting));
    return vals.size === 1 ? [...vals][0] : null;
  }
  async function setGroup(g, setting) {
    saving = g.kind;
    for (const t of g.tools) {
      if (!settable(t) || t.setting === setting) continue;
      if (!settingsFor(t.kind).some((o) => o.value === setting)) continue;
      const r = await setTool(connectorById(id), t.name, setting);
      if (!r.ok) {
        toast({ title: "Couldn't change those tools", description: r.why, variant: 'destructive' });
        break;
      }
    }
    saving = '';
  }

  async function doRetry() {
    const r = await run(() => retry(c));
    if (r?.ok && r.diff) {
      const n = r.diff.added.length;
      const asked = r.diff.askedAgain.length;
      if (n || asked || r.diff.removed.length) {
        const bits = [n ? `${n} new tool${n === 1 ? '' : 's'}` : '', asked ? `${asked} changed since you allowed ${asked === 1 ? 'it' : 'them'}: Ask first again` : '', r.diff.removed.length ? `${r.diff.removed.length} gone` : ''];
        toast({ title: `${c.name}'s tools`, description: bits.filter(Boolean).join(' · ') });
      }
    }
  }

  async function doSignOut() {
    const r = await run(() => signOut(c));
    if (!r.ok) toast({ title: "Couldn't sign out", description: r.why, variant: 'destructive' });
    else if (!r.confirmed) toast({ title: `Signed out of ${c.name}`, description: `${c.name} didn't confirm it ended Yap's access: disconnect Yap on ${c.name}'s own page too.` });
  }

  function askRemove() {
    const name = c.name;
    askConfirm({
      title: `Remove ${name}?`,
      body: `Its tools leave Yap${c.auth === 'oauth' ? `, and Yap signs out of ${name}` : ''}${c.hasSecrets ? ' and forgets its secrets' : ''}. You can add it again at any time.`,
      confirmLabel: 'Remove',
      busyLabel: 'Removing…',
      returnFocus: removeBtn,
      run: async () => {
        const r = await removeConnector(c);
        if (!r.ok) {
          toast({ title: `Couldn't remove ${name}`, description: r.why, variant: 'destructive' });
          return false;
        }
        onback?.();
        return true;
      },
    });
  }

  async function saveClient() {
    ownError = '';
    try {
      const v = await invoke('connector_set_client', { id: c.id, clientId: ownId.trim(), secret: ownSecret.trim() || null });
      ownSecret = '';
      putConnector(v);
      toast({ title: ownId.trim() ? 'Your client ID is saved' : 'Your client ID is forgotten', description: ownId.trim() ? 'Yap uses it the next time you sign in.' : '' });
    } catch (e) {
      ownError = words(e);
    }
  }

  async function loadDetails() {
    try {
      details = await invoke('connector_details', { id: c.id });
    } catch (e) {
      details = { error: words(e) };
    }
  }
  $effect(() => {
    if (detailsOpen && c) loadDetails();
  });
</script>

{#if c}
  <div class="detail" data-connector={c.id}>
    <button class="back" aria-label="Back to Connectors" onclick={() => onback?.()}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 6-6 6 6 6" /></svg>
      Connectors
    </button>

    <header class="head">
      <ConnectorLogo logo={entry?.logo} color={entry?.color} name={c.name} size={48} />
      <div class="titles">
        <h1>{c.name}</h1>
        <div class="status" data-tone={tone}>
          <span class="dot" aria-hidden="true"></span>
          <span data-testid="connector-status">{c.enabled ? c.status?.line : 'Off: Chat and the Voice Assistant don’t use it.'}</span>
        </div>
        {#if entry}<div class="by">by {entry.publisher}{#if entry.tested}<span class="tested">Tested with Yap</span>{/if}</div>{/if}
        {#if trouble}<p class="trouble" role="alert">{trouble}</p>{/if}
      </div>
      <div class="acts">
        {#if action === 'sign-in'}
          <button class="btn ink" disabled={busy} onclick={() => run(() => signIn(c))}>{c.signIn?.state === 'expired' ? 'Sign in again' : 'Sign in'}</button>
        {:else if action === 'cancel'}
          <button class="btn" onclick={() => cancelSignIn(c)}>Cancel</button>
        {:else if action === 'retry'}
          <button class="btn" disabled={busy} onclick={doRetry}>{busy ? 'Trying…' : 'Try again'}</button>
        {/if}
        {#if c.auth === 'oauth' && c.signIn && c.signIn.state !== 'needs-sign-in' && action !== 'cancel'}
          <button class="btn" disabled={busy} onclick={doSignOut}>Sign out</button>
        {/if}
        <Toggle checked={c.enabled} ariaLabel={`Use ${c.name}`} disabled={busy} onchange={(on) => run(() => setEnabled(c, on))} />
      </div>
    </header>

    <section class="reach" aria-label="What it reaches">
      {#if c.kind === 'local'}
        <div class="k">It runs</div>
        <pre class="cmd" data-testid="connector-command">{c.address}</pre>
        {#if c.env?.length}
          <div class="vars">
            {#each c.env as v (v.name)}
              <span class="var"><code>{v.name}</code>{#if v.secret}<span class="muted">🔒 in Credential Manager</span>{:else if v.value != null}<code class="muted">= {v.value}</code>{/if}</span>
            {/each}
          </div>
        {/if}
        <p class="fine">A program on this PC, with your permissions. Its command can't change: add it again to run something else.</p>
      {:else}
        <div class="k">Address</div>
        <code class="addr" data-testid="connector-address">{c.address}</code>
        {#if c.auth === 'bearer'}<p class="fine">Sends a token (kept in Windows Credential Manager), only to this server.</p>{/if}
        {#if c.headers?.length}<p class="fine">Sends {c.headers.join(', ')} (kept in Windows Credential Manager), only to this server.</p>{/if}
        {#if c.signIn?.state === 'signed-in'}
          <p class="fine">Signed in{c.signIn.host ? ` at ${c.signIn.host}` : ''}{c.signIn.scopes?.length ? ` · ${c.signIn.scopes.join(', ')}` : ''}.</p>
        {/if}
      {/if}
    </section>

    <section class="tools" aria-labelledby="tools-h-{c.id}">
      <div class="toolshead">
        <h2 id="tools-h-{c.id}">Tools</h2>
        <button class="btn sm" disabled={busy || !c.enabled} onclick={doRetry}>{busy ? 'Refreshing…' : 'Refresh tools'}</button>
      </div>
      {#if c.tools?.length}
        <p class="fine" data-testid="connector-cost">
          {offered.length === 1 ? '1 tool is' : `${offered.length} tools are`} on for Chat{#if c.tokens > 0}, about {c.tokens.toLocaleString()} tokens of every Chat step{/if}.
        </p>
        {#each groups as g (g.kind)}
          {@const gv = groupValue(g)}
          <div class="group" data-group={g.kind}>
            <div class="ghead">
              <div>
                <h3>{g.title} <span class="count">{g.tools.length}</span></h3>
                <p class="fine">{g.line}</p>
              </div>
              <div class="choice" role="radiogroup" aria-label={`All of ${g.title}`}>
                {#each settingsFor(g.kind) as o (o.value)}
                  <button type="button" role="radio" class="opt" class:on={gv === o.value} aria-checked={gv === o.value} disabled={!!saving} onclick={() => setGroup(g, o.value)}>{o.label}</button>
                {/each}
              </div>
            </div>
            {#each g.tools as t (t.name)}
              <ToolRow tool={t} busy={!!saving} onset={(s) => set(t, s)} />
            {/each}
          </div>
        {/each}
      {:else}
        <p class="fine">{c.enabled ? 'Its tools show once it connects.' : 'Turn it on to see its tools.'}</p>
      {/if}
    </section>

    <section class="foot">
      <p class="fine" data-testid="connector-used">
        {c.lastUsed ? `Last used ${ago(c.lastUsed, now)}` : 'Not used yet'} · Added {ago(c.added, now)}{c.origin === 'import' ? ' from another app' : ''}
      </p>

      {#if c.signIn}
        <details class="more">
          <summary>Advanced: your own client ID</summary>
          <p class="fine">
            For a server Yap has no app registration with. Register an app with it whose redirect is <code>{c.signIn.redirectUri}</code>, then enter its client ID here. Yap uses it the next time you sign in.
          </p>
          <div class="pair">
            <input class="inp" bind:value={ownId} placeholder="Client ID" aria-label="Your own client ID" autocomplete="off" spellcheck="false" />
            <input class="inp" bind:value={ownSecret} type="password" placeholder={c.signIn.hasOwnSecret ? 'Secret (kept)' : 'Secret, if it has one'} aria-label="Its client secret" autocomplete="off" spellcheck="false" />
            <button class="btn" onclick={saveClient}>Save</button>
          </div>
          {#if ownError}<p class="trouble" role="alert">{ownError}</p>{/if}
        </details>
      {/if}

      <details class="more" bind:open={detailsOpen}>
        <summary>Show details</summary>
        {#if details?.error}
          <p class="trouble">{details.error}</p>
        {:else if details}
          <dl class="facts">
            <dt>Server</dt>
            <dd>{details.server ? `${details.server.name || 'unnamed'} ${details.server.version || ''}` : 'Not connected yet'}</dd>
            {#if details.server?.protocol}<dt>MCP version</dt><dd>{details.server.protocol}</dd>{/if}
            <dt>{c.kind === 'local' ? 'Command' : 'Address'}</dt>
            <dd><code>{details.address}</code></dd>
            {#if details.listedAt}<dt>Tools listed</dt><dd>{ago(details.listedAt, now)}</dd>{/if}
            {#if c.signIn?.host}<dt>Sign-in server</dt><dd>{c.signIn.host}</dd>{/if}
          </dl>
          {#if details.stderr?.length}
            <div class="k">What it last wrote (kept in memory only)</div>
            <pre class="cmd small" data-testid="connector-stderr">{details.stderr.join('\n')}</pre>
          {/if}
        {:else}
          <p class="fine">Loading…</p>
        {/if}
      </details>

      {#if entry?.docs}
        <button class="link" onclick={() => openExternalLink(entry.docs)}>About {entry.name}'s MCP server ↗</button>
      {/if}

      <div class="remove">
        <button class="btn danger" bind:this={removeBtn} onclick={askRemove}>Remove…</button>
      </div>
    </section>
  </div>
{:else}
  <div class="detail">
    <button class="back" onclick={() => onback?.()}>Connectors</button>
    <p class="fine">That connector is gone.</p>
  </div>
{/if}

<style>
  .detail {
    display: flex;
    flex-direction: column;
    gap: 22px;
  }
  .back {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin: -8px 0 -6px -6px;
    padding: 4px 8px 4px 4px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg-62);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .back:hover {
    color: var(--yap-fg);
    background: var(--yap-raised-soft);
  }
  .back svg {
    width: 15px;
    height: 15px;
  }
  .head {
    display: flex;
    align-items: flex-start;
    gap: 14px;
  }
  .titles {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  h1 {
    margin: 0;
    font-size: 22px;
    font-weight: 650;
    line-height: 1.2;
  }
  .status {
    display: flex;
    align-items: baseline;
    gap: 7px;
    font-size: 13px;
    color: var(--yap-fg-80);
  }
  .dot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--yap-fg-62);
  }
  [data-tone='ok'] .dot {
    background: var(--yap-success);
  }
  [data-tone='wait'] .dot {
    background: var(--yap-link);
  }
  [data-tone='needs'] .dot {
    background: var(--yap-warning);
  }
  [data-tone='bad'] .dot {
    background: var(--yap-danger);
  }
  [data-tone='bad'] {
    color: var(--yap-danger);
  }
  [data-tone='off'] .dot {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--yap-fg-62);
  }
  .by {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .tested {
    padding: 0 7px;
    border-radius: 99px;
    border: 1px solid var(--yap-border);
    color: var(--yap-fg-80);
    font-size: 11px;
    font-weight: 600;
    line-height: 17px;
  }
  .acts {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .reach {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 14px 16px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .k {
    font-size: 11.5px;
    font-weight: 650;
    color: var(--yap-muted);
  }
  .cmd {
    margin: 0;
    padding: 8px 10px;
    border-radius: 7px;
    background: var(--yap-s3);
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 12px;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .cmd.small {
    max-height: 180px;
    overflow: auto;
    font-size: 11.5px;
  }
  .addr {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 12.5px;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .vars {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 12px;
    font-size: 12px;
  }
  .var {
    display: inline-flex;
    gap: 5px;
    align-items: baseline;
  }
  code {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 11.5px;
  }
  .muted {
    color: var(--yap-muted);
    font-size: 11.5px;
  }
  .fine {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .trouble {
    margin: 2px 0 0;
    font-size: 12.5px;
    color: var(--yap-danger);
  }
  .tools {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .toolshead {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 650;
  }
  .group {
    margin-top: 10px;
  }
  .ghead {
    display: flex;
    align-items: flex-end;
    justify-content: space-between;
    gap: 16px;
    padding-bottom: 8px;
  }
  h3 {
    margin: 0 0 2px;
    font-size: 13.5px;
    font-weight: 650;
  }
  .count {
    margin-left: 4px;
    font-weight: 600;
    color: var(--yap-muted);
  }
  .choice {
    flex: 0 0 auto;
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-raised-soft);
  }
  .opt {
    height: 26px;
    padding: 0 10px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-fg-62);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }
  .opt.on {
    background: var(--yap-s2);
    color: var(--yap-fg);
    box-shadow: var(--yap-shadow-sm), 0 0 0 1px var(--yap-border-subtle);
  }
  .opt:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .foot {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding-top: 6px;
    border-top: 1px solid var(--yap-rule);
  }
  .more summary {
    font-size: 13px;
    font-weight: 600;
    color: var(--yap-fg-80);
    cursor: pointer;
  }
  .more[open] {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .more[open] summary {
    margin-bottom: 2px;
  }
  .pair {
    display: flex;
    gap: 6px;
  }
  .inp {
    flex: 1 1 0;
    min-width: 0;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
  }
  .inp:focus {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 4px 14px;
    margin: 0;
    font-size: 12.5px;
  }
  dt {
    font-weight: 620;
    color: var(--yap-fg-80);
  }
  dd {
    margin: 0;
    color: var(--yap-muted);
    overflow-wrap: anywhere;
  }
  .link {
    align-self: flex-start;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .remove {
    display: flex;
  }
  .btn {
    flex: 0 0 auto;
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }
  .btn.sm {
    height: 28px;
  }
  .btn:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .btn.ink {
    border-color: var(--yap-ink);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .btn.ink:hover:not(:disabled) {
    background: var(--yap-ink-hover);
  }
  .btn.danger {
    color: var(--yap-danger);
  }
  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .btn:focus-visible,
  .back:focus-visible,
  .link:focus-visible,
  .more summary:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
</style>
