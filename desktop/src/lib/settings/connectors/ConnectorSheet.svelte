<script>
  // A Discover card's sheet (Settings → Connectors → Discover; the design's
  // §10.2–10.3): what it is and who makes it, **Tested with Yap** only when
  // someone has signed in and run its tools with Yap (never a tick claiming
  // anyone's review), how it signs in, where its data goes, the vendor's
  // page, and Connect:
  // - signs in in the browser → added from the catalogue (`catalog`: Rust
  //   uses the entry's own address), then the browser opens at once and the
  //   sheet says "Finish signing in in your browser" with Cancel; then
  //   "Connected to … · 12 tools";
  // - a token (GitHub, Zapier, Home Assistant; or "Use a token instead") →
  //   a masked field with where to make one, handed straight to connector_add;
  // - a program (Files) → + Add's program form, filled in, which shows the
  //   exact command before it runs.
  //
  //   entry: the catalogue's (with `added`); onopen(id); onprogram(entry); onclose.
  import { untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Dialog from './Dialog.svelte';
  import ConnectorLogo from './ConnectorLogo.svelte';
  import AddedResult from './AddedResult.svelte';
  import { putConnector, loadCatalog, words } from './connectors.svelte.js';
  import { howItSignsIn } from './catalog.js';
  import { openExternalLink } from '../../core/externalLinks.js';

  let { entry, returnFocus = null, onopen, onprogram, onclose } = $props();

  let token = $state('');
  const start = untrack(() => ({ url: entry.urlEditable ? (entry.url ?? '') : '', token: entry.signIn === 'token' }));
  let url = $state(start.url);
  let useToken = $state(start.token);
  let busy = $state(false);
  let error = $state('');
  let addedId = $state(null);
  let autoSignIn = $state(false);

  const wantsToken = $derived(entry?.signIn === 'token' || useToken);

  async function connect() {
    if (busy) return;
    if (entry.signIn === 'local') {
      onprogram?.(entry);
      return;
    }
    error = '';
    if (wantsToken && !token.trim()) {
      error = `Paste your ${entry.name} token first.`;
      return;
    }
    busy = true;
    try {
      const spec = { name: entry.name, catalog: entry.id, token: wantsToken ? token.trim() : null, url: entry.urlEditable ? url.trim() : null };
      const v = await invoke('connector_add', { spec });
      token = '';
      putConnector(v);
      addedId = v.id;
      autoSignIn = entry.signIn === 'oauth' && !wantsToken;
      loadCatalog();
    } catch (e) {
      error = words(e);
    } finally {
      busy = false;
    }
  }
</script>

<Dialog title={entry.name} {returnFocus} {busy} {onclose}>
  {#snippet head()}
    <ConnectorLogo logo={entry.logo} color={entry.color} name={entry.name} size={44} />
  {/snippet}
  <div class="by">
    <span>by {entry.publisher}</span>
    {#if entry.tested}<span class="tested" title={`Someone signed in and ran its tools with Yap on ${entry.tested}`}>Tested with Yap</span>{/if}
  </div>
  {#if addedId}
    <AddedResult id={addedId} {entry} {autoSignIn} />
  {:else}
    <p class="blurb">{entry.blurb}</p>
    {#if entry.notes}<p class="note">{entry.notes}</p>{/if}
    <dl class="facts">
      <dt>Signing in</dt>
      <dd>{howItSignsIn(entry)}</dd>
      <dt>Where your data goes</dt>
      <dd>
        {#if entry.signIn === 'local'}
          It runs on this PC. What it returns goes to the model you chat with.
        {:else}
          Yap keeps the {wantsToken ? 'token' : 'sign-in'} in Windows Credential Manager and talks to {entry.name} straight from this PC. What {entry.name} returns goes to the model you chat with.
        {/if}
      </dd>
      {#if entry.signIn === 'local'}
        <dt>It runs</dt>
        <dd><code>{[entry.command, ...(entry.args ?? [])].join(' ')}</code> and the folders you add.</dd>
      {/if}
    </dl>

    {#if entry.urlEditable}
      <label class="field">
        <span>Its address on your network</span>
        <input class="inp" bind:value={url} type="url" autocomplete="off" spellcheck="false" />
      </label>
    {/if}
    {#if wantsToken}
      <label class="field">
        <span>{entry.name} token</span>
        <input class="inp" bind:value={token} type="password" autocomplete="off" spellcheck="false" placeholder="Paste it here" data-autofocus />
        {#if entry.token?.help}<span class="hint">{entry.token.help}</span>{/if}
      </label>
      {#if entry.token?.url}
        <button type="button" class="linkbtn" onclick={() => openExternalLink(entry.token.url)}>Make a token on {new URL(entry.token.url).host} ↗</button>
      {/if}
    {:else if entry.signIn === 'oauth' && entry.token}
      <button type="button" class="linkbtn" onclick={() => (useToken = true)}>Use a token instead</button>
    {/if}
    {#if entry.docs}
      <button type="button" class="linkbtn" onclick={() => openExternalLink(entry.docs)}>About {entry.name}'s MCP server ↗</button>
    {/if}
    {#if error}<p class="err" role="alert">{error}</p>{/if}
  {/if}

  {#snippet foot()}
    {#if addedId}
      <button class="dbtn" onclick={() => onopen?.(addedId)}>Open its page</button>
      <button class="dbtn ink" onclick={() => onclose?.()}>Done</button>
    {:else if entry.added}
      <button class="dbtn" onclick={() => onclose?.()}>Close</button>
      <button class="dbtn ink" onclick={() => onopen?.(entry.added)}>Open its page</button>
    {:else}
      <button class="dbtn" onclick={() => onclose?.()} disabled={busy}>Cancel</button>
      <button class="dbtn ink" onclick={connect} disabled={busy}>
        {busy ? 'Connecting…' : entry.signIn === 'local' ? 'Continue' : wantsToken ? 'Connect' : 'Continue to sign in'}
      </button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .by {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: -10px;
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .tested {
    padding: 0 8px;
    border-radius: 99px;
    border: 1px solid var(--yap-border);
    color: var(--yap-fg-80);
    font-size: 11.5px;
    font-weight: 600;
    line-height: 19px;
  }
  .blurb {
    margin: 0;
    font-size: 14px;
    line-height: 1.5;
  }
  .note {
    margin: 0;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--yap-raised-soft);
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-fg-80);
  }
  .facts {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 6px 14px;
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
  }
  dt {
    font-weight: 620;
    color: var(--yap-fg-80);
  }
  dd {
    margin: 0;
    color: var(--yap-muted);
  }
  code {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 11.5px;
    color: var(--yap-fg-80);
  }
  .linkbtn {
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
  .linkbtn:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
</style>
