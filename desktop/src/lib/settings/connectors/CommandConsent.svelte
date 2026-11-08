<script>
  // A program's consent panel (Settings → Connectors → + Add, Import, Paste,
  // Discover's Files): what will run, before anything does — "This runs a
  // program on your PC with your permissions", the exact command line as
  // Windows will run it (connector_preview's, resolved: npx → its full
  // path), its warnings (npx -y fetching the latest version every start, a
  // shell's -c, downloads, deletes, folders outside yours), and its
  // variables by name (a secret one's value goes to Windows Credential
  // Manager, never shown). The caller's Approve and run sends the preview's
  // fingerprint back: Rust adds only the command line seen here.
  //
  //   preview: connector_preview's `program` + `warnings`; env: [{name, secret, value?}]
  let { program, warnings = [], env = [], compact = false } = $props();

  // The first warning is the one every program gets: it's the heading.
  const rest = $derived(warnings.filter((w) => !/^Yap will run this program with your permissions/.test(w)));
</script>

<div class="consent" class:compact role="group" aria-label="What this program runs">
  {#if !compact}
    <p class="lead"><b>This runs a program on your PC with your permissions.</b> It can read and change anything you can, so only add programs you trust. Yap doesn't sandbox it.</p>
  {/if}
  <div class="cmdlabel">It runs exactly this</div>
  <pre class="cmd" data-testid="command-line">{program?.commandLine}</pre>
  {#if rest.length}
    <ul class="warn">
      {#each rest as w (w)}<li>{w}</li>{/each}
    </ul>
  {/if}
  {#if env.length}
    <div class="env">
      <span class="cmdlabel">With</span>
      {#each env as v (v.name)}
        <span class="var">
          <code>{v.name}</code>{#if v.secret}<span class="lock" title="Kept in Windows Credential Manager">🔒 kept in Credential Manager</span>{:else if v.value != null}<code class="val">= {v.value}</code>{/if}
        </span>
      {/each}
    </div>
  {/if}
</div>

<style>
  .consent {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border: 1px solid var(--yap-border);
    border-radius: 10px;
    background: var(--yap-s1);
  }
  .consent.compact {
    padding: 10px 12px;
    gap: 6px;
  }
  .lead {
    margin: 0;
    font-size: 13px;
    line-height: 1.5;
    color: var(--yap-fg-80);
  }
  .lead b {
    color: var(--yap-fg);
  }
  .cmdlabel {
    font-size: 11.5px;
    font-weight: 650;
    color: var(--yap-muted);
  }
  .cmd {
    margin: 0;
    padding: 8px 10px;
    border-radius: 7px;
    background: var(--yap-s3);
    color: var(--yap-fg);
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 12px;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .warn {
    margin: 0;
    padding-left: 18px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-warning);
  }
  .env {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px 10px;
    font-size: 12px;
  }
  .var {
    display: inline-flex;
    align-items: baseline;
    gap: 5px;
  }
  code {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    font-size: 11.5px;
  }
  .val {
    color: var(--yap-muted);
  }
  .lock {
    font-size: 11.5px;
    color: var(--yap-muted);
  }
</style>
