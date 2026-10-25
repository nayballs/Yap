<script>
  // Settings → Privacy & data → "Search my meetings by meaning" (search/):
  // the embedding model that reads every meeting and note on this PC, so a
  // search — the Meetings view's, and Ask across meetings in Chat — finds
  // what was meant, not just the words. Download it (its size, and the
  // on-device engine's when that isn't here yet, shown first), switch it on
  // and off, see the index (passages read, its size, where it runs, what
  // it's doing), rebuild it, delete the model and the index.
  //
  //   cfg: Settings' config copy (the switch is cfg.searchByMeaning; the
  //        auto-save saves it, and search.rs follows).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Button from '../ui/Button.svelte';
  import { askConfirm } from '../ui/contextMenu.svelte.js';
  import { toast } from '../ui/toast.svelte.js';
  import { openExternalLink } from '../core/externalLinks.js';
  import { cancelDownload, downloadLine, fromEvent, isCancelled } from '../core/downloads.js';

  let { cfg } = $props();

  // search_status: { enabled, installed, runtimeInstalled, installing, phase, error, passages,
  // indexed, notes, indexBytes, modelBytes, downloadBytes, runsOn,
  // modelName, license, licenseUrl }.
  let status = $state(null);
  // A download under way: { stage: 'runtime' | 'model', percent }.
  let progress = $state(null);
  let busy = $state(false);

  async function load() {
    try {
      status = await invoke('search_status');
    } catch {
      status = null;
    }
  }

  onMount(() => {
    load();
    const uns = [
      listen('yap-search-status', (e) => (status = e.payload)),
      listen('yap-search-download', (e) => (progress = { stage: 'model', ...fromEvent(e.payload) })),
      // The engine's download, when the on-device model hasn't brought it.
      listen('local-llm-download-progress', (e) => {
        if (e.payload?.model === 'search') progress = { stage: 'runtime', ...fromEvent(e.payload) };
      }),
    ];
    return () => uns.forEach((u) => u.then((f) => f()));
  });

  const size = (bytes) => {
    const mb = bytes / 1_048_576;
    if (mb >= 1) return `${mb >= 10 ? Math.round(mb).toLocaleString() : mb.toFixed(1)} MB`;
    return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  };
  const RUNS_ON = { gpu: 'Runs on your graphics card', partial: 'Runs partly on your graphics card', cpu: 'Runs on the processor' };

  const modelDesc = $derived.by(() => {
    if (!status) return '';
    if (progress) {
      // Its speed and time left once known (core/downloads.js).
      const line = downloadLine(progress);
      const what = progress.stage === 'runtime' ? 'the on-device AI engine' : status.modelName;
      return `Downloading ${what}… ${progress.percent ?? 0}%${line ? ` · ${line}` : ''}`;
    }
    if (status.installed) {
      return [status.modelName, size(status.modelBytes), RUNS_ON[status.runsOn]].filter(Boolean).join(' · ');
    }
    const engine = !status.runtimeInstalled;
    return `${status.modelName}: a one-time download of ${size(status.downloadBytes)}${engine ? ', the on-device AI engine included' : ''}`;
  });

  const indexDesc = $derived.by(() => {
    if (!status) return '';
    if (!status.installed) return 'Without the model, searches match your words';
    switch (status.phase) {
      case 'off':
        return 'Off: searches match your words';
      case 'waiting':
        return `Waiting for your dictation or meeting to finish · ${status.indexed.toLocaleString()} of ${status.passages.toLocaleString()} passages read`;
      case 'indexing':
        return `Reading your meetings and notes: ${status.indexed.toLocaleString()} of ${status.passages.toLocaleString()} passages`;
      case 'error':
        return `Couldn't read them: ${status.error || 'the search model didn’t start'}. Yap tries again later`;
      default:
        return status.passages
          ? `${status.passages.toLocaleString()} passages from ${status.notes.toLocaleString()} ${status.notes === 1 ? 'note' : 'notes'} · ${size(status.indexBytes)}`
          : 'Nothing to read yet';
    }
  });

  async function download() {
    busy = true;
    progress = { stage: 'model', percent: 0 };
    try {
      status = await invoke('search_install');
      // Rust switched it on; Settings' copy follows (and adopts the save).
      cfg.searchByMeaning = true;
    } catch (e) {
      // Cancelled: nothing to say (its half-downloaded file is gone).
      if (!isCancelled(e)) toast({ title: "Couldn't download the search model", description: String(e), variant: 'destructive' });
    } finally {
      busy = false;
      progress = null;
      load();
    }
  }

  async function rebuild() {
    try {
      status = await invoke('search_rebuild');
    } catch (e) {
      toast({ title: "Couldn't rebuild the index", description: String(e), variant: 'destructive' });
    }
  }

  function remove(e) {
    const btn = e?.currentTarget;
    askConfirm({
      title: 'Delete the search model and its index?',
      body: 'Searches go back to matching your words. Your meetings and notes stay. You can download it again later.',
      confirmLabel: 'Delete',
      busyLabel: 'Deleting…',
      returnFocus: btn,
      run: async () => {
        try {
          status = await invoke('search_delete');
          cfg.searchByMeaning = false;
        } catch (err) {
          toast({ title: "Couldn't delete the search model", description: String(err), variant: 'destructive' });
          return false;
        }
      },
    });
  }
</script>

<Group
  title="Search my meetings by meaning"
  id="settings-meeting-search"
  desc="A small AI model reads your meetings and notes on this PC, so a search finds what you meant, not just the words you remember. Ask across meetings answers from what it finds, and cites the moment."
>
  <Row setting="meeting-search">
    <Toggle
      bind:checked={cfg.searchByMeaning}
      label="Search my meetings by meaning"
      desc={status?.installed ? 'In the Meetings view’s search and when you ask across your meetings' : 'Download the search model first'}
      hint="Nothing leaves this PC to search: the model runs here, in the background, and waits while you dictate or record a meeting. Only an answer’s passages go to Chat’s model with your question."
      disabled={!status?.installed}
    />
  </Row>
  <Row label="Search model" desc={modelDesc} setting="meeting-search-model">
    {#if status && !status.installed && (busy || status.installing)}
      <!-- Stops it; its half-downloaded file goes (shell/download.rs). -->
      <Button variant="secondary" size="sm" onclick={() => cancelDownload('search')}>Cancel</Button>
    {:else if status && !status.installed}
      <Button size="sm" onclick={download}>Download {size(status.downloadBytes)}</Button>
    {:else if status?.installed}
      <Button variant="ghost" size="sm" onclick={() => openExternalLink(status.licenseUrl)}>{status.license}</Button>
    {/if}
  </Row>
  {#if status?.installed}
    <Row label="Search index" desc={indexDesc} setting="meeting-search-index">
      <Button variant="secondary" size="sm" onclick={rebuild}>Rebuild</Button>
      <Button variant="danger" size="sm" onclick={remove}>Delete…</Button>
    </Row>
  {/if}
</Group>
