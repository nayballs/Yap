<script>
  // Settings → About → Report a problem (src-tauri/src/privacy/diagnostics.rs): a
  // diagnostic file the person looks through before it's saved — the file
  // list and a preview of what's in it — then saves where they choose
  // (Windows' Save dialog). Nothing is uploaded: once it's saved, Yap offers
  // GitHub's new-issue page with a template (its version and Windows' only),
  // and the person attaches the file themselves if they want to.
  import { invoke } from '@tauri-apps/api/core';
  import { tick } from 'svelte';
  import Row from '../ui/Row.svelte';
  import Button from '../ui/Button.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { openExternalLink } from '../core/externalLinks.js';

  // idle | making | preview | saving | saved
  let stage = $state('idle');
  let preview = $state(null); // diagnostics::Preview
  let tab = $state('settings');
  let savedPath = $state('');
  let panel = $state(null);

  const TABS = [
    { value: 'settings', label: 'Settings' },
    { value: 'system', label: 'System' },
    { value: 'connections', label: 'Connections' },
    { value: 'errors', label: 'Errors' },
  ];
  const ABOUT = {
    'README.txt': 'What this file is',
    'system.txt': 'Windows, processor, memory, graphics card, Yap version',
    'settings.json': 'Switches, providers and models: no keys, names or your words',
    'connections.txt': 'The last 7 days of connections, by purpose',
    'errors.txt': "The latest warnings and errors from Yap's log",
  };
  const about = (name) => ABOUT[name] || (name.startsWith('logs/') ? 'A day of the log: info and above, scrubbed' : '');

  function size(bytes) {
    const kb = bytes / 1024;
    return kb >= 1024 ? `${(kb / 1024).toFixed(1)} MB` : `${Math.max(1, Math.round(kb))} KB`;
  }
  const fileName = $derived((savedPath || '').split(/[/\\]/).pop());

  async function make() {
    stage = 'making';
    try {
      preview = await invoke('diagnostics_preview');
      tab = 'settings';
      stage = 'preview';
      await tick();
      panel?.focus();
    } catch (err) {
      stage = 'idle';
      toast({ title: "Couldn't make the diagnostic file", description: String(err), variant: 'destructive' });
    }
  }

  async function save() {
    stage = 'saving';
    try {
      const path = await invoke('diagnostics_save');
      if (!path) {
        stage = 'preview'; // cancelled
        return;
      }
      savedPath = path;
      stage = 'saved';
      await tick();
      panel?.focus();
    } catch (err) {
      stage = 'preview';
      toast({ title: "Couldn't save the file", description: String(err), variant: 'destructive' });
    }
  }

  async function openIssue() {
    try {
      openExternalLink(await invoke('diagnostics_issue_url'));
    } catch {
      openExternalLink('https://github.com/nayballs/Yap/issues/new');
    }
  }

  function done() {
    stage = 'idle';
    preview = null;
    savedPath = '';
  }

  const shown = $derived(preview ? preview[tab] : '');
</script>

<Row
  label="Diagnostic file"
  desc="For a bug report: you see what's in it before it's saved, and nothing is uploaded"
  setting="report-problem"
>
  {#if stage === 'idle' || stage === 'making'}
    <Button variant="secondary" size="sm" disabled={stage === 'making'} onclick={make}>
      {stage === 'making' ? 'Making the file…' : 'Report a problem…'}
    </Button>
  {/if}
</Row>

{#if preview && (stage === 'preview' || stage === 'saving')}
  <section class="panel" aria-labelledby="report-h" tabindex="-1" bind:this={panel}>
    <h3 id="report-h">What's in the file</h3>
    <p class="lead">
      Look it over before you save it. It stays on this PC until you share it yourself.
    </p>
    <ul class="files" aria-label="Files in the diagnostic file">
      {#each preview.files as f (f.name)}
        <li>
          <span class="fname">{f.name}</span>
          <span class="fabout">{about(f.name)}</span>
          <span class="fsize">{size(f.bytes)}</span>
        </li>
      {/each}
    </ul>
    <div class="tabs">
      <Segmented bind:value={tab} options={TABS} label="Preview" />
    </div>
    <!-- A scrolling preview: focusable so the keyboard can scroll it. -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <pre class="peek" aria-label={`Preview: ${TABS.find((t) => t.value === tab)?.label}`} tabindex="0">{shown}</pre>
    <div class="actions">
      <Button size="sm" disabled={stage === 'saving'} onclick={save}>{stage === 'saving' ? 'Saving…' : 'Save…'}</Button>
      <Button variant="ghost" size="sm" disabled={stage === 'saving'} onclick={done}>Cancel</Button>
      <span class="total">{preview.name} · {size(preview.total)} before it's zipped</span>
    </div>
  </section>
{:else if stage === 'saved'}
  <section class="panel" aria-labelledby="report-saved-h" tabindex="-1" bind:this={panel}>
    <h3 id="report-saved-h">Saved {fileName}</h3>
    <p class="lead">
      Nothing was uploaded. To tell us what went wrong, open an issue on GitHub and attach the file if you're happy to
      share it.
    </p>
    <div class="actions">
      <Button size="sm" onclick={openIssue}>Open a GitHub issue</Button>
      <Button variant="secondary" size="sm" onclick={() => invoke('diagnostics_reveal').catch(() => {})}>Show in folder</Button>
      <Button variant="ghost" size="sm" onclick={done}>Done</Button>
    </div>
  </section>
{/if}

<style>
  .panel {
    margin: 4px 0 14px;
    padding: 14px 16px;
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
  }
  .panel:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  h3 {
    margin: 0 0 4px;
    font-size: 13.5px;
    font-weight: 640;
    color: var(--yap-fg);
  }
  .lead {
    margin: 0 0 10px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .files {
    list-style: none;
    margin: 0 0 12px;
    padding: 0;
  }
  .files li {
    display: grid;
    grid-template-columns: minmax(0, 190px) minmax(0, 1fr) auto;
    column-gap: 12px;
    padding: 5px 0;
    border-top: 1px solid var(--yap-rule);
    font-size: 12px;
  }
  .files li:first-child {
    border-top: none;
  }
  .fname {
    font-family: ui-monospace, Consolas, monospace;
    color: var(--yap-fg);
    overflow-wrap: anywhere;
  }
  .fabout {
    color: var(--yap-muted);
  }
  .fsize {
    color: var(--yap-fg-62);
    font-variant-numeric: tabular-nums;
  }
  .tabs {
    margin-bottom: 8px;
  }
  .peek {
    margin: 0 0 12px;
    max-height: 240px;
    overflow: auto;
    padding: 10px 12px;
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--yap-fg-80);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .peek:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .total {
    margin-left: auto;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
</style>
