<script>
  // + Add (Settings → Connectors; the design's §10.6), one dialog, four ways:
  //
  // - **link** — "Add a custom connector": a name and the server's address,
  //   a token or request headers if it takes them (masked; handed straight
  //   to connector_add, then the fields forget them). Added, it shows what
  //   it found: the server's own name and its tools, or "… asks you to sign
  //   in" with Sign in (the browser, then Cancel while it waits), or why it
  //   didn't connect.
  // - **program** — "Add a program on this PC": the command, its arguments
  //   and variables; Review shows connector_preview's exact command line and
  //   warnings *before* anything runs, and Approve and run sends its
  //   fingerprint back (CommandConsent.svelte).
  // - **import** — "Import from Claude Desktop": its claude_desktop_config.json
  //   servers (read only; never Yap's own), each whole with a tick box, then
  //   one Approve for the ticked ones (connectors_import_scan → _import:
  //   Rust reads the file again, so its secrets never come to the page).
  // - **paste** — "Paste a setup": an mcpServers block, a server's JSON, a
  //   link or an npx / uvx line, read the same way.
  //
  //   mode; preset: Discover's Files ({name, command, args, catalog});
  //   onopen(id): open its page; onclose.
  import { invoke } from '@tauri-apps/api/core';
  import { untrack } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import Dialog from './Dialog.svelte';
  import CommandConsent from './CommandConsent.svelte';
  import AddedResult from './AddedResult.svelte';
  import { putConnector, loadConnectors, words } from './connectors.svelte.js';
  import { splitArgs, joinArgs, nameFromUrl } from './catalog.js';

  let { mode = 'link', preset = null, returnFocus = null, onopen, onclose } = $props();

  const TITLES = {
    link: ['Add a custom connector', 'A server at a link, over MCP. Yap reaches it straight from this PC.'],
    program: ['Add a program on this PC', 'A local MCP server, which Yap starts and talks to as Claude Desktop does.'],
    import: ['Import from Claude Desktop', "Its MCP servers, read from its settings file. Yap doesn't change the file."],
    paste: ['Paste a setup', 'An mcpServers block, a server’s JSON, a link, or an npx or uvx line.'],
  };
  const titles = $derived(TITLES[mode] ?? TITLES.link);
  const title = $derived(titles[0]);
  const lead = $derived(titles[1]);

  let step = $state('form'); // form → review (a program) → done
  let busy = $state(false);
  let error = $state('');

  // A link.
  // The form starts from the preset (Discover's Files), as it was when it opened.
  const start = untrack(() => ({ name: preset?.name ?? '', command: preset?.command ?? '', args: preset?.args ? joinArgs(preset.args) : '' }));
  let name = $state(start.name);
  let url = $state('');
  let token = $state('');
  let headers = $state([]);
  let sse = $state(false);
  // A program.
  let command = $state(start.command);
  let argsText = $state(start.args);
  let env = $state([]);
  let preview = $state(null);
  let spec = $state.raw(null);
  // What was added.
  let addedId = $state(null);
  // Import / paste.
  let text = $state('');
  let scan = $state(null);
  const picked = new SvelteSet();
  let imported = $state(null);

  const isSecret = (n) => /(^|[_-])(TOKEN|KEY|SECRET|PASSWORD|PASSWD|PAT|CREDENTIAL|AUTH)([_-]|$)|(TOKEN|KEY|SECRET|PASSWORD|PASSWD|PAT|CREDENTIAL|AUTH)$/i.test(n);

  function addHeader() {
    headers = [...headers, { name: '', value: '' }];
  }
  function addVar() {
    env = [...env, { name: '', value: '', secret: null }];
  }

  async function addLink() {
    if (busy) return;
    error = '';
    const n = name.trim() || nameFromUrl(url);
    const s = {
      name: n,
      url: url.trim(),
      token: token.trim() || null,
      headers: headers.filter((h) => h.name.trim()).map((h) => ({ name: h.name.trim(), value: h.value })),
      sse,
    };
    busy = true;
    try {
      // The address checked first (https on the internet, no metadata address).
      await invoke('connector_preview', { spec: s });
      const v = await invoke('connector_add', { spec: s });
      // Sent: the page forgets the secrets.
      token = '';
      headers = headers.map((h) => ({ ...h, value: '' }));
      putConnector(v);
      addedId = v.id;
      step = 'done';
    } catch (e) {
      error = words(e);
    } finally {
      busy = false;
    }
  }

  function programSpec() {
    return {
      name: name.trim(),
      command: command.trim(),
      args: splitArgs(argsText),
      env: env.filter((v) => v.name.trim()).map((v) => ({ name: v.name.trim(), value: v.value, secret: v.secret ?? isSecret(v.name.trim()) })),
      catalog: preset?.catalog ?? null,
    };
  }

  async function review() {
    if (busy) return;
    error = '';
    if (!name.trim()) {
      error = 'Give the connector a name.';
      return;
    }
    spec = programSpec();
    busy = true;
    try {
      preview = await invoke('connector_preview', { spec });
      step = 'review';
    } catch (e) {
      error = words(e);
    } finally {
      busy = false;
    }
  }

  async function approve() {
    if (busy || !preview?.program) return;
    error = '';
    busy = true;
    try {
      const v = await invoke('connector_add', { spec: { ...spec, approved: preview.program.fingerprint } });
      env = env.map((x) => ((x.secret ?? isSecret(x.name)) ? { ...x, value: '' } : x));
      spec = null;
      putConnector(v);
      addedId = v.id;
      step = 'done';
    } catch (e) {
      error = words(e);
    } finally {
      busy = false;
    }
  }

  const source = () => (mode === 'paste' ? { from: 'paste', text } : { from: 'claude-desktop' });
  const canPick = (s) => !s.added && !s.problem && !!s.preview;

  async function read() {
    if (busy) return;
    error = '';
    busy = true;
    try {
      scan = await invoke('connectors_import_scan', { source: source() });
      picked.clear();
      for (const s of scan.servers) if (canPick(s)) picked.add(s.key);
    } catch (e) {
      scan = null;
      error = words(e);
    } finally {
      busy = false;
    }
  }

  // Claude Desktop's file is read as the dialog opens.
  $effect(() => {
    if (mode === 'import' && !scan && !error && !busy) read();
  });

  const pickedServers = $derived((scan?.servers ?? []).filter((s) => picked.has(s.key)));
  const pickedPrograms = $derived(pickedServers.filter((s) => s.kind === 'local').length);

  async function importPicked() {
    if (busy || !pickedServers.length) return;
    error = '';
    busy = true;
    try {
      const picks = pickedServers.map((s) => ({ key: s.key, approved: s.preview?.program?.fingerprint ?? null }));
      imported = await invoke('connectors_import', { source: source(), picks });
      text = '';
      for (const v of imported.added) putConnector(v);
      loadConnectors();
      step = 'done';
    } catch (e) {
      error = words(e);
    } finally {
      busy = false;
    }
  }

  function done() {
    onclose?.();
  }
</script>

{#snippet varRows()}
  {#each env as v, i (i)}
    <div class="pair">
      <input class="inp" placeholder="NAME" aria-label="Variable name" bind:value={v.name} spellcheck="false" autocomplete="off" />
      <input
        class="inp"
        type={(v.secret ?? isSecret(v.name)) ? 'password' : 'text'}
        placeholder="value"
        aria-label={`Value of ${v.name || 'the variable'}`}
        bind:value={v.value}
        spellcheck="false"
        autocomplete="off"
      />
      <button type="button" class="lockbtn" aria-pressed={v.secret ?? isSecret(v.name)} title="A secret: kept in Windows Credential Manager" aria-label={`${v.name || 'This variable'} is a secret`} onclick={() => (v.secret = !(v.secret ?? isSecret(v.name)))}>🔒</button>
      <button type="button" class="rm" aria-label="Remove this variable" onclick={() => (env = env.filter((_, j) => j !== i))}>×</button>
    </div>
  {/each}
{/snippet}

{#snippet foundList()}
  {#if scan}
    {#if scan.readFrom?.length}<p class="fine">Read from {scan.readFrom.join(' and ')}.</p>{/if}
    <ul class="found" aria-label="Servers found">
      {#each scan.servers as s (s.key)}
        <li class:off={!canPick(s)}>
          <label>
            <input
              type="checkbox"
              checked={picked.has(s.key)}
              disabled={!canPick(s) || busy}
              aria-label={`Add ${s.name}`}
              onchange={(e) => (e.currentTarget.checked ? picked.add(s.key) : picked.delete(s.key))}
            />
            <span class="ftext">
              <span class="fname">{s.name} <span class="chip">{s.kind === 'local' ? 'Program' : 'Link'}</span>{#if s.added}<span class="chip">Added already</span>{/if}</span>
              {#if s.kind === 'local' && s.preview?.program}
                <CommandConsent program={s.preview.program} warnings={s.preview.warnings} env={s.env} compact />
              {:else}
                <code class="addr">{s.address}</code>
                {#if s.token || s.headers.length}<span class="fine">Sends {[s.token ? 'a token' : '', ...s.headers].filter(Boolean).join(', ')}: kept in Windows Credential Manager.</span>{/if}
              {/if}
              {#if s.problem}<span class="err">{s.problem}</span>{/if}
            </span>
          </label>
        </li>
      {/each}
    </ul>
    {#if scan.skipped}<p class="fine">{scan.skipped === 1 ? 'One entry' : `${scan.skipped} entries`} left out: Yap's own, or not a server Yap can add.</p>{/if}
    {#if pickedPrograms}
      <p class="lead-warn"><b>{pickedPrograms === 1 ? 'The program' : `The ${pickedPrograms} programs`} you tick will run on your PC with your permissions.</b> Only add programs you trust.</p>
    {/if}
  {/if}
{/snippet}

<Dialog {title} {lead} {busy} {returnFocus} onclose={done} wide={mode !== 'link'}>
  {#if step === 'done' && addedId}
    <AddedResult id={addedId} entry={null} />
  {:else if step === 'done' && imported}
    {#if imported.added.length}
      <div class="results">
        {#each imported.added as v (v.id)}<AddedResult id={v.id} />{/each}
      </div>
    {/if}
    {#each imported.failed as f (f.name)}
      <p class="err">{f.name}: {f.why}</p>
    {/each}
  {:else if mode === 'link'}
    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        addLink();
      }}
    >
      <label class="field"><span>Name</span><input class="inp" bind:value={name} placeholder="Linear" autocomplete="off" maxlength="60" /></label>
      <label class="field">
        <span>Server address</span>
        <input class="inp" bind:value={url} type="url" placeholder="https://mcp.example.com/mcp" autocomplete="off" spellcheck="false" data-autofocus />
      </label>
      <label class="field">
        <span>Token <span class="opt">(if it takes one)</span></span>
        <input class="inp" bind:value={token} type="password" placeholder="An API key or access token" autocomplete="off" spellcheck="false" />
        <span class="hint">Sent as <code>Authorization: Bearer …</code>, only to this server. Leave it empty for a server that signs in in your browser.</span>
      </label>
      {#each headers as h, i (i)}
        <div class="pair">
          <input class="inp" placeholder="Header name" aria-label="Header name" bind:value={h.name} spellcheck="false" autocomplete="off" />
          <input class="inp" type="password" placeholder="value" aria-label={`Value of ${h.name || 'the header'}`} bind:value={h.value} spellcheck="false" autocomplete="off" />
          <button type="button" class="rm" aria-label="Remove this header" onclick={() => (headers = headers.filter((_, j) => j !== i))}>×</button>
        </div>
      {/each}
      <div class="more">
        <button type="button" class="linkbtn" onclick={addHeader}>+ Add a request header</button>
        <label class="check"><input type="checkbox" bind:checked={sse} /> An older HTTP+SSE server</label>
      </div>
      <p class="fine">Yap keeps a token or a header's value in Windows Credential Manager. What the server returns goes to the model you chat with.</p>
      {#if error}<p class="err" role="alert">{error}</p>{/if}
      <button type="submit" hidden aria-hidden="true" tabindex="-1"></button>
    </form>
  {:else if mode === 'program' && step === 'review' && preview?.program}
    <CommandConsent program={preview.program} warnings={preview.warnings} env={spec?.env ?? []} />
    <p class="fine">Nothing has run yet. Approve and run starts it and lists its tools.</p>
    {#if error}<p class="err" role="alert">{error}</p>{/if}
  {:else if mode === 'program'}
    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        review();
      }}
    >
      <label class="field"><span>Name</span><input class="inp" bind:value={name} placeholder="Files" autocomplete="off" maxlength="60" data-autofocus /></label>
      <label class="field">
        <span>Command</span>
        <input class="inp mono" bind:value={command} placeholder="npx" autocomplete="off" spellcheck="false" />
        <span class="hint">A program on your PATH (npx, uvx, docker…) or its full path.</span>
      </label>
      <label class="field">
        <span>Arguments</span>
        <input class="inp mono" bind:value={argsText} placeholder="-y @modelcontextprotocol/server-filesystem C:\Users\you\Documents" autocomplete="off" spellcheck="false" />
        <span class="hint">{preset?.argsHint ?? 'Separated by spaces; "quote" one that has spaces in it.'}</span>
      </label>
      <div class="field">
        <span>Environment variables</span>
        {@render varRows()}
        <button type="button" class="linkbtn" onclick={addVar}>+ Add a variable</button>
        <span class="hint">A token's or key's value (🔒) goes to Windows Credential Manager, not Yap's files.</span>
      </div>
      {#if error}<p class="err" role="alert">{error}</p>{/if}
      <button type="submit" hidden aria-hidden="true" tabindex="-1"></button>
    </form>
  {:else if mode === 'paste'}
    {#if !scan}
      <label class="field">
        <span>Your setup</span>
        <textarea class="inp" bind:value={text} spellcheck="false" autocomplete="off" data-autofocus placeholder={'{\n  "mcpServers": {\n    "files": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\\\Users\\\\you\\\\Documents"] }\n  }\n}'}></textarea>
        <span class="hint">Tokens in it go to Windows Credential Manager. Nothing runs until you approve it.</span>
      </label>
    {/if}
    {@render foundList()}
    {#if error}<p class="err" role="alert">{error}</p>{/if}
  {:else}
    {#if busy && !scan}<p class="fine">Reading Claude Desktop's settings…</p>{/if}
    {@render foundList()}
    {#if error}<p class="err" role="alert">{error}</p>{/if}
  {/if}

  {#snippet foot()}
    {#if step === 'done'}
      {#if addedId}<button class="dbtn" onclick={() => onopen?.(addedId)}>Open its page</button>{/if}
      <button class="dbtn ink" onclick={done} data-autofocus>Done</button>
    {:else if mode === 'link'}
      <button class="dbtn" onclick={done} disabled={busy}>Cancel</button>
      <button class="dbtn ink" onclick={addLink} disabled={busy || !url.trim()}>{busy ? 'Adding…' : 'Add'}</button>
    {:else if mode === 'program' && step === 'review'}
      <button class="dbtn" onclick={() => (step = 'form')} disabled={busy}>Back</button>
      <button class="dbtn ink" onclick={approve} disabled={busy}>{busy ? 'Starting…' : 'Approve and run'}</button>
    {:else if mode === 'program'}
      <button class="dbtn" onclick={done} disabled={busy}>Cancel</button>
      <button class="dbtn ink" onclick={review} disabled={busy || !command.trim()}>{busy ? 'Checking…' : 'Review'}</button>
    {:else if !scan && mode === 'paste'}
      <button class="dbtn" onclick={done} disabled={busy}>Cancel</button>
      <button class="dbtn ink" onclick={read} disabled={busy || !text.trim()}>{busy ? 'Reading…' : 'Read it'}</button>
    {:else}
      {#if mode === 'paste'}<button class="dbtn" onclick={() => (scan = null)} disabled={busy}>Back</button>{:else}<button class="dbtn" onclick={done} disabled={busy}>Cancel</button>{/if}
      <button class="dbtn ink" onclick={importPicked} disabled={busy || !pickedServers.length}>
        {busy ? 'Adding…' : pickedPrograms ? `Approve and add ${pickedServers.length}` : `Add ${pickedServers.length || ''}`.trim()}
      </button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .opt {
    font-weight: 500;
    color: var(--yap-muted);
  }
  .mono {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 12.5px;
  }
  .pair {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .pair .inp {
    flex: 1 1 0;
  }
  .rm,
  .lockbtn {
    flex: 0 0 auto;
    width: 30px;
    height: 30px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 15px;
    cursor: pointer;
  }
  .lockbtn {
    font-size: 12px;
    opacity: 0.45;
  }
  .lockbtn[aria-pressed='true'] {
    opacity: 1;
    background: var(--yap-raised-soft);
  }
  .rm:focus-visible,
  .lockbtn:focus-visible,
  .linkbtn:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .more {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }
  .linkbtn {
    align-self: flex-start;
    padding: 2px 0;
    border: none;
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    color: var(--yap-fg-80);
  }
  code {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 11.5px;
  }
  .found {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .found li {
    border: 1px solid var(--yap-border-subtle);
    border-radius: 10px;
    background: var(--yap-s1);
  }
  .found li.off {
    opacity: 0.7;
  }
  .found label {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 10px 12px;
    cursor: pointer;
  }
  .found input[type='checkbox'] {
    margin-top: 3px;
    accent-color: var(--yap-ink);
  }
  .ftext {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .fname {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 13.5px;
    font-weight: 620;
  }
  .chip {
    padding: 0 7px;
    border-radius: 99px;
    background: var(--yap-raised);
    color: var(--yap-fg-80);
    font-size: 11px;
    font-weight: 600;
    line-height: 18px;
  }
  .addr {
    overflow-wrap: anywhere;
    color: var(--yap-fg-80);
  }
  .lead-warn {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-fg-80);
  }
  .results {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }
</style>
