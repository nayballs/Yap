<script>
  // What became of a connector just added (+ Add's dialogs, Discover's
  // sheet), live from the store: connected — the server's own name and its
  // tools —; a server that wants a sign-in — Sign in, then "Finish signing
  // in in your browser" with Cancel —; or why it didn't connect, with Try
  // again. It stays added either way: its page can fix it later.
  //
  //   id: the connector's; entry: its catalogue entry, if any.
  import ConnectorLogo from './ConnectorLogo.svelte';
  import { connectorById, statusAction, signIn, cancelSignIn, retry, conn } from './connectors.svelte.js';

  let { id, entry = null, autoSignIn = false } = $props();

  const c = $derived(connectorById(id));
  const action = $derived(c ? statusAction(c) : null);
  const connected = $derived(c?.status?.state === 'connected');
  const offered = $derived((c?.tools ?? []).filter((t) => !t.skipped));
  const host = $derived(c?.signIn?.host || hostOf(c?.address));
  // A failed attempt's words, when the status line doesn't say them already.
  const trouble = $derived(c && conn.trouble[c.id] && conn.trouble[c.id] !== c.status?.line ? conn.trouble[c.id] : '');
  let busy = $state(false);
  let started = false;

  function hostOf(a) {
    try {
      return new URL(a).host;
    } catch {
      return '';
    }
  }

  async function run(f) {
    busy = true;
    try {
      await f();
    } finally {
      busy = false;
    }
  }

  // Discover's Connect: the browser opens at once (the person asked for it).
  $effect(() => {
    if (autoSignIn && !started && c && action === 'sign-in') {
      started = true;
      run(() => signIn(c));
    }
  });
</script>

{#if c}
  <div class="result" aria-live="polite" data-state={c.status?.state} data-signin={c.signIn?.state ?? ''}>
    <div class="who">
      <ConnectorLogo logo={entry?.logo} color={entry?.color} name={c.name} size={36} />
      <div class="text">
        <div class="name">{c.name}</div>
        <div class="line" class:bad={action === 'retry'}>{c.status?.line}</div>
      </div>
    </div>

    {#if connected}
      <p class="ok">
        Connected to <b>{c.server?.name || c.name}</b>{#if c.server?.version}&nbsp;<span class="ver">{c.server.version}</span>{/if} · {offered.length === 1 ? '1 tool' : `${offered.length} tools`}
      </p>
      {#if offered.length}
        <ul class="tools" aria-label="Its tools">
          {#each offered.slice(0, 10) as t (t.name)}<li>{t.title || t.name}</li>{/each}
          {#if offered.length > 10}<li class="more">+{offered.length - 10} more</li>{/if}
        </ul>
      {/if}
    {:else if action === 'sign-in'}
      <p class="say">{host ? `${host} asks you to sign in, in your browser.` : 'This server asks you to sign in, in your browser.'} Yap keeps the sign-in in Windows Credential Manager.</p>
      <div class="btns"><button class="btn ink" disabled={busy} onclick={() => run(() => signIn(c))}>Sign in</button></div>
    {:else if action === 'cancel'}
      <p class="say">Finish signing in in your browser{host ? ` (${host})` : ''}. Yap is waiting for it.</p>
      <div class="btns"><button class="btn" onclick={() => cancelSignIn(c)}>Cancel</button></div>
    {:else if action === 'retry'}
      <p class="say">It's added, but it didn't connect. Fix what the line says, then Try again, or remove it from its page.</p>
      <div class="btns"><button class="btn" disabled={busy} onclick={() => run(() => retry(c))}>{busy ? 'Trying…' : 'Try again'}</button></div>
    {/if}
    {#if trouble}<p class="trouble" role="alert">{trouble}</p>{/if}
  </div>
{/if}

<style>
  .result {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .who {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .text {
    min-width: 0;
  }
  .name {
    font-size: 14px;
    font-weight: 650;
  }
  .line {
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .line.bad,
  .trouble {
    color: var(--yap-danger);
  }
  .ok,
  .say,
  .trouble {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
  }
  .ver {
    color: var(--yap-muted);
    font-size: 12px;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .tools li {
    padding: 2px 8px;
    border-radius: 99px;
    background: var(--yap-raised-soft);
    border: 1px solid var(--yap-border-subtle);
    font-size: 12px;
  }
  .tools .more {
    color: var(--yap-muted);
  }
  .btns {
    display: flex;
    gap: 8px;
  }
  .btn {
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn.ink {
    border-color: var(--yap-ink);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .btn:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
</style>
