<script>
  // Integrations — Yap's local-first take on OpenWhispr's IntegrationsView.
  // OpenWhispr's cards are Google Calendar OAuth + paid cloud API keys + a
  // hosted MCP server + their npm CLI. Yap has no cloud, so the surface here
  // is the piece that actually works offline: the LOCAL API BRIDGE
  // (src-tauri/bridge.rs, their cliBridge.js ported to Rust) — a
  // token-authenticated loopback HTTP server that terminals, scripts, and
  // coding agents can drive while Yap runs.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import { toast } from './ui/toast.svelte.js';

  let enabled = $state(true);
  let status = $state({ running: false, port: null, bridgeFile: '' });
  let loaded = $state(false);

  async function refresh() {
    try {
      status = await invoke('bridge_status');
    } catch {
      /* best-effort */
    }
  }

  onMount(async () => {
    try {
      const cfg = await invoke('get_config');
      enabled = cfg?.bridgeEnabled !== false;
    } catch {
      /* keep default */
    }
    await refresh();
    loaded = true;
  });

  // Patch-save onto a FRESH config (same pattern as DictionaryView) so this
  // toggle can never clobber settings changed elsewhere.
  async function setEnabled(on) {
    enabled = on;
    try {
      const fresh = await invoke('get_config');
      await invoke('save_config', { cfg: { ...fresh, bridgeEnabled: on } });
      await refresh();
      toast({
        title: on ? 'Local API on' : 'Local API off',
        description: on && status.port ? `Listening on 127.0.0.1:${status.port}` : undefined,
        variant: 'success',
      });
    } catch (e) {
      toast({ title: "Couldn't update the bridge", description: String(e), variant: 'destructive' });
      await refresh();
      enabled = status.running;
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

  const curlExample = `TOKEN=$(jq -r .token ~/.yap/cli-bridge.json)
PORT=$(jq -r .port ~/.yap/cli-bridge.json)
curl -s -H "Authorization: Bearer $TOKEN" "http://127.0.0.1:$PORT/v1/notes/list"`;

  const ROUTES = [
    ['GET', '/v1/health', 'liveness ping'],
    ['GET', '/v1/notes/list?folder=&note_type=&limit=', 'note summaries, newest first'],
    ['GET', '/v1/notes/search?q=&limit=', 'keyword search over the notes library'],
    ['GET', '/v1/notes/{id}', 'one full note (raw + enhanced + transcript)'],
    ['POST', '/v1/notes/create', '{title, content, folder}'],
    ['PATCH', '/v1/notes/{id}', '{title?, content?, folder?, participants?}'],
    ['DELETE', '/v1/notes/{id}', 'delete a note'],
    ['GET', '/v1/folders/list', 'folder names'],
    ['POST', '/v1/folders/create', '{name}'],
    ['GET', '/v1/transcriptions/list?limit=', 'dictation history'],
    ['GET', '/v1/transcriptions/{ts}', 'one history entry (id = its unix ts)'],
    ['DELETE', '/v1/transcriptions/{ts}', 'delete a history entry'],
  ];

  // A paste-into-your-agent cheat sheet (the local answer to OpenWhispr's
  // agent-skills/openwhispr-cli SKILL.md + hosted MCP card).
  const agentGuide = () => `# Yap local API

Yap (the local voice-dictation app) exposes a loopback HTTP API while it runs.

## Connect
Read \`~/.yap/cli-bridge.json\` → \`{version, port, token}\`. The file exists only
while Yap is running (delete-on-exit). Send every request to
\`http://127.0.0.1:<port>\` with the header \`Authorization: Bearer <token>\`.

## Endpoints (all JSON; lists use {data, has_more, next_cursor}; errors use {error:{code,message}})
${ROUTES.map(([m, p, d]) => `- ${m} ${p} — ${d}`).join('\n')}

## Notes
- Note ids are integers; transcription ids are the entry's unix-seconds \`ts\`.
- POST/PATCH bodies are JSON. Create returns 201 + the note; deletes return 204.
- Everything is local — no cloud, no account. If the bridge file is missing or
  the port refuses connections, Yap isn't running (or Integrations → Local API
  is switched off).

Example:
\`\`\`bash
${curlExample}
\`\`\`
`;
</script>

<div class="wrap">
  <div class="inner">
    <div class="page-h">
      <h1>Integrations</h1>
      <p>
        Connect Yap to terminals, scripts, and coding agents. Everything below runs on this
        machine — loopback only, token-protected, no cloud, no account.
      </p>
    </div>

    <!-- Local API bridge -->
    <div class="card">
      <div class="card-top">
        <div class="card-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M4 17l6-6-6-6" /><path d="M12 19h8" /></svg>
        </div>
        <div class="card-head">
          <div class="titlerow">
            <h2>Local API</h2>
            {#if loaded}
              {#if status.running}
                <span class="badge on">Running · 127.0.0.1:{status.port}</span>
              {:else}
                <span class="badge off">Off</span>
              {/if}
            {/if}
          </div>
          <p>
            A tiny HTTP server other tools on this PC can call to read and write your notes,
            folders, and dictation history while Yap runs.
          </p>
        </div>
        <label class="switch">
          <input
            type="checkbox"
            checked={enabled}
            onchange={(e) => setEnabled(e.currentTarget.checked)}
          />
          <span class="slider"></span>
        </label>
      </div>

      {#if status.running}
        <div class="kv">
          <span class="k">Discovery file</span>
          <code class="v">{status.bridgeFile}</code>
          <button class="mini" onclick={() => copy(status.bridgeFile, 'Path')}>Copy</button>
        </div>
        <div class="kv col">
          <span class="k">Try it (Git Bash / WSL)</span>
          <pre class="code">{curlExample}</pre>
          <button class="mini" onclick={() => copy(curlExample, 'Example')}>Copy</button>
        </div>
      {/if}
    </div>

    <!-- Coding agents -->
    <div class="card">
      <div class="card-top">
        <div class="card-icon" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="14" rx="2" /><path d="M8 20h8" /><path d="m8.5 9 2 2-2 2" /><path d="M12.5 13h3" /></svg>
        </div>
        <div class="card-head">
          <div class="titlerow"><h2>Coding agents</h2></div>
          <p>
            Give Claude Code, Cursor, or any agent access to your Yap notes: copy the API guide
            below and paste it into the agent (or save it as a skill / rules file). The agent
            reads the discovery file, then talks to the local API directly.
          </p>
        </div>
      </div>
      <div class="agent-actions">
        <button class="primary" onclick={() => copy(agentGuide(), 'API guide')}>
          Copy API guide for agents
        </button>
      </div>
    </div>

    <!-- Endpoint reference -->
    <div class="card">
      <div class="card-top">
        <div class="card-head">
          <div class="titlerow"><h2>Endpoints</h2></div>
        </div>
      </div>
      <div class="routes">
        {#each ROUTES as [method, path, desc] (path + method)}
          <div class="route">
            <span class="method {method.toLowerCase()}">{method}</span>
            <code class="path">{path}</code>
            <span class="desc">{desc}</span>
          </div>
        {/each}
      </div>
    </div>
  </div>
</div>

<style>
  .wrap {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
  }
  .inner {
    max-width: 780px;
    margin: 0 auto;
    padding: 26px 30px 40px;
  }
  .page-h {
    margin: 0 0 22px;
  }
  .page-h h1 {
    margin: 0 0 4px;
    font-size: 19px;
    letter-spacing: -0.01em;
  }
  .page-h p {
    margin: 0;
    font-size: 12px;
    color: var(--yap-muted);
    line-height: 1.55;
  }

  .card {
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    padding: 16px;
    margin-bottom: 14px;
  }
  .card-top {
    display: flex;
    align-items: flex-start;
    gap: 12px;
  }
  .card-icon {
    width: 34px;
    height: 34px;
    flex: 0 0 auto;
    border-radius: 8px;
    background: var(--yap-s1);
    border: 1px solid var(--yap-border);
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--yap-muted);
  }
  .card-icon svg {
    width: 17px;
    height: 17px;
  }
  .card-head {
    flex: 1 1 auto;
    min-width: 0;
  }
  .titlerow {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .card-head h2 {
    margin: 0;
    font-size: 13.5px;
    font-weight: 600;
  }
  .card-head p {
    margin: 3px 0 0;
    font-size: 12px;
    color: var(--yap-muted);
    line-height: 1.55;
  }
  .badge {
    font-size: 10px;
    padding: 2px 7px;
    border-radius: 999px;
    border: 1px solid var(--yap-border);
    white-space: nowrap;
  }
  .badge.on {
    color: var(--yap-success);
    border-color: color-mix(in srgb, var(--yap-success) 35%, transparent);
    background: color-mix(in srgb, var(--yap-success) 10%, transparent);
  }
  .badge.off {
    color: var(--yap-muted-55);
  }

  /* toggle (matches the pill-style switches used elsewhere) */
  .switch {
    position: relative;
    flex: 0 0 auto;
    width: 34px;
    height: 20px;
  }
  .switch input {
    opacity: 0;
    width: 0;
    height: 0;
  }
  .slider {
    position: absolute;
    inset: 0;
    border-radius: 999px;
    background: var(--yap-s1);
    border: 1px solid var(--yap-border);
    transition: background var(--yap-dur) ease;
    cursor: pointer;
  }
  .slider::before {
    content: '';
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--yap-muted);
    transition:
      transform var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .switch input:checked + .slider {
    background: var(--yap-primary);
    border-color: var(--yap-primary);
  }
  .switch input:checked + .slider::before {
    transform: translateX(14px);
    background: #fff;
  }

  .kv {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--yap-border-subtle);
  }
  .kv.col {
    flex-direction: column;
    align-items: flex-start;
  }
  .k {
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    color: var(--yap-muted-55);
    flex: 0 0 auto;
  }
  .v {
    font-size: 11.5px;
    color: var(--yap-fg);
    background: var(--yap-s1);
    border: 1px solid var(--yap-border);
    border-radius: 5px;
    padding: 3px 7px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    flex: 1 1 auto;
  }
  .code {
    margin: 0;
    width: 100%;
    box-sizing: border-box;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--yap-fg);
    background: var(--yap-s1);
    border: 1px solid var(--yap-border);
    border-radius: 5px;
    padding: 8px 10px;
    overflow-x: auto;
    white-space: pre;
  }
  .mini {
    flex: 0 0 auto;
    background: none;
    border: 1px solid var(--yap-border);
    color: var(--yap-muted);
    border-radius: 5px;
    padding: 3px 9px;
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }
  .mini:hover {
    color: var(--yap-fg);
    border-color: var(--yap-border-hover);
  }

  .agent-actions {
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--yap-border-subtle);
  }
  .primary {
    background: var(--yap-ink, var(--yap-primary));
    border: none;
    color: var(--yap-ink-fg, #fff);
    border-radius: var(--yap-r);
    padding: 7px 14px;
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
  }
  .primary:hover {
    filter: brightness(1.08);
  }

  .routes {
    margin-top: 12px;
    padding-top: 6px;
    border-top: 1px solid var(--yap-border-subtle);
  }
  .route {
    display: grid;
    grid-template-columns: 52px minmax(0, 1fr);
    grid-template-areas:
      'method path'
      '. desc';
    column-gap: 8px;
    row-gap: 1px;
    padding: 6px 0;
    border-bottom: 1px solid var(--yap-border-subtle);
    align-items: baseline;
  }
  .route:last-child {
    border-bottom: none;
  }
  .method {
    grid-area: method;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.4px;
    text-align: center;
    border-radius: 4px;
    padding: 1.5px 0;
  }
  .method.get {
    color: #1a66b8;
    background: color-mix(in srgb, #1a66b8 10%, transparent);
  }
  .method.post {
    color: var(--yap-success);
    background: color-mix(in srgb, var(--yap-success) 10%, transparent);
  }
  .method.patch {
    color: var(--yap-warning);
    background: color-mix(in srgb, var(--yap-warning) 12%, transparent);
  }
  .method.delete {
    color: var(--yap-danger);
    background: color-mix(in srgb, var(--yap-danger) 10%, transparent);
  }
  .path {
    grid-area: path;
    font-size: 11.5px;
    color: var(--yap-fg);
    overflow-wrap: anywhere;
  }
  .desc {
    grid-area: desc;
    font-size: 11px;
    color: var(--yap-muted-55);
  }
</style>
