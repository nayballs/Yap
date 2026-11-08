<script>
  // One of the person's connectors in Yours (Settings → Connectors): its mark
  // or initial, its name, its status in plain words ("Connected · 12 tools",
  // "Needs sign-in", "Signing in… Finish in your browser.", "Your sign-in
  // to … has ended. Sign in again.", "Couldn't start: …") with the one button
  // it asks for (Sign in, Cancel, Try again), its switch (on or off
  // everywhere) and ⋯ (Open, Sign in again / Sign out, Refresh tools,
  // Remove…; a right-click opens it too). A click on the row opens its page.
  //
  //   c: Rust's view; entry: its catalogue entry, if any; onopen; onremove.
  import Toggle from '../../ui/Toggle.svelte';
  import ConnectorLogo from './ConnectorLogo.svelte';
  import { openContextMenu, menuKey, menuOpenFor } from '../../ui/contextMenu.svelte.js';
  import { conn, statusAction, statusTone, signIn, cancelSignIn, retry, setEnabled } from './connectors.svelte.js';

  let { c, entry = null, onopen, onremove, onsignout } = $props();

  let dots = $state(null);
  let busy = $state(false);
  const action = $derived(statusAction(c));
  const tone = $derived(statusTone(c));
  const trouble = $derived(conn.trouble[c.id] && conn.trouble[c.id] !== c.status?.line ? conn.trouble[c.id] : '');
  const line = $derived(c.enabled ? (c.status?.line ?? '') : 'Off');

  async function run(f) {
    busy = true;
    try {
      await f();
    } finally {
      busy = false;
    }
  }

  function menu(e) {
    const oauth = !!c.signIn;
    openContextMenu(
      e,
      [
        { label: 'Open', icon: 'open', onselect: () => onopen?.() },
        oauth && c.signIn?.state === 'signed-in' ? { label: 'Sign in again', icon: 'person', onselect: () => run(() => signIn(c)) } : null,
        oauth && c.signIn?.state !== 'signed-in' && action !== 'cancel' ? { label: 'Sign in', icon: 'person', onselect: () => run(() => signIn(c)) } : null,
        oauth && c.auth === 'oauth' && c.signIn?.state !== 'needs-sign-in' ? { label: 'Sign out', icon: 'signout', onselect: () => onsignout?.() } : null,
        c.enabled ? { label: 'Refresh tools', icon: 'sync', onselect: () => run(() => retry(c)) } : null,
        { separator: true },
        { label: 'Remove…', icon: 'trash', danger: true, onselect: () => onremove?.(dots) },
      ],
      { label: `${c.name} options`, row: dots }
    );
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="row"
  class:menuopen={menuOpenFor(dots)}
  class:off={!c.enabled}
  data-connector={c.id}
  oncontextmenu={menu}
  onkeydown={(e) => menuKey(e, menu)}
>
  <button class="main" onclick={() => onopen?.()} aria-label={`${c.name}: ${line}`}>
    <ConnectorLogo logo={entry?.logo} color={entry?.color} name={c.name} size={34} />
    <span class="text">
      <span class="name">{c.name}</span>
      <span class="status" data-tone={tone}>
        <span class="dot" aria-hidden="true"></span>
        <span class="line">{line}</span>
      </span>
      {#if trouble}<span class="trouble" role="alert">{trouble}</span>{/if}
    </span>
  </button>
  {#if action === 'sign-in'}
    <button class="btn ink" disabled={busy} onclick={() => run(() => signIn(c))}>Sign in</button>
  {:else if action === 'cancel'}
    <button class="btn" onclick={() => cancelSignIn(c)}>Cancel</button>
  {:else if action === 'retry'}
    <button class="btn" disabled={busy} onclick={() => run(() => retry(c))}>{busy ? 'Trying…' : 'Try again'}</button>
  {/if}
  <Toggle checked={c.enabled} ariaLabel={`Use ${c.name}`} disabled={busy} onchange={(on) => run(() => setEnabled(c, on))} />
  <button
    class="dots"
    bind:this={dots}
    aria-label={`More options for ${c.name}`}
    aria-haspopup="menu"
    aria-expanded={menuOpenFor(dots)}
    data-more
    onclick={menu}
  >
    <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.8" /><circle cx="12" cy="12" r="1.8" /><circle cx="19" cy="12" r="1.8" /></svg>
  </button>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 8px 8px 4px;
    border-top: 1px solid var(--yap-rule);
  }
  .row:first-child {
    border-top: none;
  }
  .row.menuopen,
  .row:hover {
    background: var(--yap-raised-soft);
    border-radius: var(--yap-r);
  }
  .main {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 2px 6px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .main:focus-visible,
  .btn:focus-visible,
  .dots:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .row.off :global(.tile) {
    opacity: 0.55;
  }
  .text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name {
    font-size: 13.5px;
    font-weight: 620;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status {
    display: flex;
    align-items: baseline;
    gap: 6px;
    font-size: 12.5px;
    color: var(--yap-muted);
    min-width: 0;
  }
  .line {
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
  }
  .dot {
    flex: 0 0 auto;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--yap-fg-62);
    transform: translateY(-1px);
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
  [data-tone='bad'] .line {
    color: var(--yap-danger);
  }
  [data-tone='off'] .dot {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--yap-fg-62);
  }
  .trouble {
    font-size: 12px;
    color: var(--yap-danger);
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
  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .dots {
    flex: 0 0 auto;
    display: inline-flex;
    width: 30px;
    height: 30px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .dots:hover,
  .dots[aria-expanded='true'] {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .dots svg {
    width: 16px;
    height: 16px;
  }
</style>
