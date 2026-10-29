<script>
  // Settings → Privacy & data → Your data: Download your data and Reset
  // app (src-tauri/src/privacy/export.rs, reset.rs).
  //
  // - Download your data: one .zip of everything Yap keeps about you —
  //   history, notes and meetings, chats, tasks, memory, the dictionary and
  //   snippets, settings without keys —, through Windows' Save dialog, never
  //   uploaded. The tick box adds the audio and screenshots and says their
  //   size. A big one shows its progress, with Cancel.
  // - Reset app: "Clear everything and keep my settings". Yap's confirm
  //   dialog names exactly what goes and what stays (`data_reset_plan`), and
  //   offers Download your data first. Done without a restart: every window
  //   is told (`yap-data-reset`).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Button from '../ui/Button.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import DownloadProgress from '../ui/DownloadProgress.svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { showSaveError } from '../notices/saveErrors.js';
  import { formatBytes } from '../core/downloads.js';

  // What the file would hold (export.rs `Summary`).
  let summary = $state(null);
  let withAudio = $state(false);
  // idle | saving | saved
  let stage = $state('idle');
  let progress = $state(null); // { percent, downloaded, total }
  let saved = $state(null); // { path, bytes }
  let panel = $state(null);

  async function loadSummary() {
    try {
      summary = await invoke('data_export_summary');
    } catch {
      summary = null;
    }
  }

  onMount(() => {
    loadSummary();
    const uns = [
      listen('yap-data-export', (e) => {
        const { done = 0, total = 0 } = e.payload || {};
        progress = { percent: total > 0 ? (done / total) * 100 : 0, downloaded: done, total };
      }),
      listen('yap-data-reset', loadSummary),
      listen('yap-history-changed', loadSummary),
    ];
    return () => uns.forEach((u) => u.then((f) => f()));
  });

  const audioText = $derived(
    !summary ? '' : summary.audioFiles ? formatBytes(summary.audioBytes) : 'none kept right now'
  );
  const fileName = $derived((saved?.path || '').split(/[/\\]/).pop());

  async function download() {
    if (stage === 'saving') return;
    stage = 'saving';
    progress = null;
    try {
      const r = await invoke('data_export_save', { audio: withAudio && !!summary?.audioFiles });
      if (r?.path) {
        saved = r;
        stage = 'saved';
        await tick();
        panel?.focus();
        return;
      }
      stage = 'idle';
      if (r?.cancelled) toast({ kind: 'done', title: 'Cancelled', description: 'Nothing was saved.' });
    } catch (err) {
      stage = 'idle';
      toast({ title: "Couldn't save your data", description: String(err), variant: 'destructive' });
    }
  }

  function cancel() {
    return invoke('data_export_cancel').catch(() => {});
  }

  function done() {
    stage = 'idle';
    saved = null;
  }

  // ---- Reset app ----
  let resetOpen = $state(false);
  let resetting = $state(false);
  let plan = $state(null); // reset.rs `Plan`
  let resetButton = $state(null);

  async function askReset() {
    try {
      plan = await invoke('data_reset_plan');
    } catch (err) {
      toast({ title: "Couldn't reset Yap", description: String(err), variant: 'destructive' });
      return;
    }
    if (plan?.blocked) {
      toast({ title: "Yap can't reset right now", description: plan.blocked, variant: 'destructive' });
      return;
    }
    resetOpen = true;
  }

  async function doReset() {
    if (resetting) return;
    resetting = true;
    try {
      await invoke('data_reset');
      resetOpen = false;
      toast({
        kind: 'done',
        title: 'Yap is reset',
        description: 'Your settings, dictionary and models are as they were. No restart needed.',
      });
    } catch (err) {
      // Cleared in memory; a store that couldn't be saved is said, with Try again.
      if (!showSaveError(err)) {
        toast({ title: "Couldn't reset Yap", description: String(err), variant: 'destructive' });
      } else {
        resetOpen = false;
      }
    } finally {
      resetting = false;
      loadSummary();
    }
  }

  function downloadFirst() {
    resetOpen = false;
    download();
  }
</script>

<Group title="Your data" id="settings-your-data">
  <Row
    label="Download your data"
    desc="Your history, notes, meetings, chats, memory, tasks, dictionary and settings, in one .zip you save. Nothing is uploaded"
    hint="Notes, meetings and chats come as Markdown, your history and dictionary as CSV, all of it as JSON too. API keys, tokens and your account's sign-in are never in it."
    setting="download-data"
  >
    {#if stage !== 'saving'}
      <Button variant="secondary" size="sm" onclick={download}>Download…</Button>
    {/if}
  </Row>
  {#if stage === 'saving'}
    <div class="progress">
      <DownloadProgress {progress} label="Saving your data" oncancel={cancel} />
    </div>
  {:else if stage === 'saved' && saved}
    <section class="panel" aria-labelledby="your-data-saved-h" tabindex="-1" bind:this={panel}>
      <h3 id="your-data-saved-h">Saved {fileName}</h3>
      <p class="lead">{formatBytes(saved.bytes)}, on this PC only. Nothing was uploaded.</p>
      <div class="actions">
        <Button variant="secondary" size="sm" onclick={() => invoke('data_export_reveal').catch(() => {})}>Show in folder</Button>
        <Button variant="ghost" size="sm" onclick={done}>Done</Button>
      </div>
    </section>
  {:else}
    <label class="tick" class:off={!summary?.audioFiles}>
      <input type="checkbox" bind:checked={withAudio} disabled={!summary?.audioFiles} />
      <span>Include audio and screenshots</span>
      {#if audioText}<span class="size">{audioText}</span>{/if}
    </label>
  {/if}
  <Row label="Reset app" desc="Clear everything and keep your settings, dictionary and models" setting="reset-app">
    <span bind:this={resetButton}>
      <Button variant="danger" size="sm" disabled={stage === 'saving'} onclick={askReset}>Reset app…</Button>
    </span>
  </Row>
</Group>

<ConfirmDialog
  bind:open={resetOpen}
  title="Clear everything and keep your settings?"
  body="These go from this PC for good:"
  confirmLabel="Reset app"
  busyLabel="Resetting…"
  busy={resetting}
  returnFocus={resetButton?.querySelector('button')}
  onconfirm={doReset}
>
  {#if plan}
    <ul class="goes" aria-label="What goes">
      {#each plan.goes as g (g.id)}
        <li>
          {g.label}{#if g.count}<span class="count">{` · ${g.count}`}</span>{/if}
        </li>
      {/each}
    </ul>
    <p class="stays-h">These stay:</p>
    <ul class="stays" aria-label="What stays">
      {#each plan.stays as s (s)}
        <li>{s}</li>
      {/each}
    </ul>
    <button type="button" class="first" onclick={downloadFirst} disabled={resetting}>Download your data first</button>
  {/if}
</ConfirmDialog>

<style>
  .progress {
    padding: 4px 0 12px;
  }
  .tick {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 2px 0 14px;
    font-size: 12.5px;
    color: var(--yap-fg-80);
    cursor: pointer;
    user-select: none;
  }
  .tick.off {
    color: var(--yap-muted);
    cursor: default;
  }
  .tick input {
    margin: 0;
    accent-color: var(--yap-ink);
  }
  .tick input:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .size {
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  .size::before {
    content: '· ';
  }
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
    overflow-wrap: anywhere;
  }
  .lead {
    margin: 0 0 10px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  /* Inside Yap's confirm dialog: its paper colours. */
  ul {
    margin: 10px 0 0;
    padding-left: 18px;
    font-size: 13.5px;
    line-height: 20px;
    color: var(--yap-paper-ink);
  }
  li + li {
    margin-top: 3px;
  }
  .count {
    color: var(--yap-paper-text-2);
    font-variant-numeric: tabular-nums;
  }
  .stays-h {
    margin: 14px 0 0;
    font-size: 14px;
    line-height: 21px;
    color: var(--yap-paper-text-2);
  }
  .stays {
    color: var(--yap-paper-text-2);
  }
  .first {
    margin-top: 14px;
    padding: 0;
    border: none;
    background: none;
    font: inherit;
    font-size: 13.5px;
    font-weight: 600;
    color: var(--yap-paper-ink);
    text-decoration: underline;
    text-underline-offset: 3px;
    cursor: pointer;
  }
  .first:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
    border-radius: 4px;
  }
  .first:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
