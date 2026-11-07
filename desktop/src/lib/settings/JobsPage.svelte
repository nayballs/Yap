<script>
  // **Settings → Assistant → Jobs** (desktop/src-tauri/src/jobs.rs): Chat and
  // the Voice Assistant hand a task to the person's own Claude Code, in a
  // folder they chose, and it reports back.
  // - The **Jobs** switch (`jobsEnabled`); Claude Code on this PC, found and
  //   signed in, or Sign in (ClaudeCodeStatus).
  // - **Pay with**: Your Claude plan / API key (`jobsPay`), saying which is
  //   in use, and for the key whether Language Models' Claude tab has one.
  // - **Defaults**: model and effort (`jobsModel`, `jobsEffort`), the Mode a
  //   new chat starts in (`jobsMode`; Bypass permissions asks once).
  // - **Folders** (`job_folders_*`): name, path and a cap ("Never above:"),
  //   Add folder… (the folder picker), Remove; their own Claude settings
  //   and MCP servers run too; the Bypass warning where Bypass can run.
  // - **Ask before starting a job** (`jobsAsk`), **Read job reports aloud**
  //   (`jobsSpeak`), and the limits: backups 7 days, one job a folder, 3
  //   at once; Undo needs Claude Code's hooks (a managed policy can turn
  //   them off: then no backups, and the job's tab says so).
  // - **History** (`job_list`): each job's status, folder, when and cost
  //   (plan and key told apart), Open (Chat on it) and Undo.
  //
  //   cfg:  Settings' config $state (auto-save does the saving).
  //   goto(target): another Settings page; onshowview(view): leaves Settings.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Select from '../ui/Select.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Button from '../ui/Button.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import { toast } from '../ui/toast.svelte.js';
  import ClaudeCodeStatus from './ClaudeCodeStatus.svelte';
  import { sharedKey } from '../core/providers.js';
  import { EFFORT_LABELS } from '../core/effort.js';
  import { jobs, initJobs, refreshJobs } from '../stores/jobs.svelte.js';
  import { statusWord, statusTone, costLine, MODE_LABELS, modelLabel, canUndo } from '../core/jobs.js';

  let { cfg, goto = null, onshowview = null } = $props();

  const BYPASS = 'bypassPermissions';
  const WARNING = 'Claude Code can run anything on your PC without asking. Use it only in folders you trust.';

  const MODELS = ['claude-opus-5-5', 'claude-sonnet-5-5', 'claude-haiku-5-5'];
  const modelOptions = $derived(
    [...MODELS, ...(cfg.jobsModel && !MODELS.includes(cfg.jobsModel) ? [cfg.jobsModel] : [])].map((m) => ({ value: m, label: modelLabel(m) }))
  );
  const EFFORTS = ['low', 'medium', 'high', 'xhigh', 'max'].map((e) => ({ value: e, label: EFFORT_LABELS[e] }));
  const MODES = ['auto', 'manual', 'acceptEdits', 'plan', BYPASS].map((m) => ({ value: m, label: MODE_LABELS[m] + (m === 'auto' ? ' (Recommended)' : '') }));
  const CAPS = [
    { value: '', label: 'No limit' },
    { value: 'auto', label: 'Auto' },
    { value: 'acceptEdits', label: 'Accept edits' },
    { value: 'manual', label: 'Manual' },
    { value: 'plan', label: 'Plan' },
  ];
  const ASK = [
    { value: 'new-or-web', label: 'Only when the chat read the web or the folder is new (Recommended)' },
    { value: 'always', label: 'Always' },
    { value: 'never', label: 'Never' },
  ];
  const SPEAK = [
    { value: 'voice', label: 'When I asked by voice' },
    { value: 'always', label: 'Always' },
    { value: 'never', label: 'Never' },
  ];
  const PAY = [
    { value: 'plan', label: 'Your Claude plan' },
    { value: 'key', label: 'API key' },
  ];

  let folders = $state([]);
  let bypassConfirmed = $state(false);
  let confirmBypass = $state(false);
  let modeBefore = 'auto';
  let undoing = $state(null);
  let undoOpen = $state(false);
  let undoBusy = $state(false);
  let showAll = $state(false);

  const hasKey = $derived(!!sharedKey(cfg, 'anthropic'));
  const payLine = $derived(
    cfg.jobsPay === 'key'
      ? hasKey
        ? "Jobs are charged to the API key on Language Models' Claude tab."
        : "There's no Claude API key in Language Models yet: add one on its Claude tab, or jobs can't start."
      : "Jobs use your Claude plan's limits, through Claude Code's own sign-in. Each job shows what it would have cost on the API."
  );
  const history = $derived(showAll ? jobs.list : jobs.list.slice(0, 20));

  async function loadFolders() {
    try {
      folders = (await invoke('job_folders_list')) || [];
    } catch {
      folders = [];
    }
  }

  async function loadMode() {
    try {
      bypassConfirmed = !!(await invoke('job_mode_get', { conversation: null }))?.bypassConfirmed;
    } catch {
      /* keep */
    }
  }

  onMount(() => {
    initJobs();
    refreshJobs();
    loadFolders();
    loadMode();
    modeBefore = cfg.jobsMode || 'auto';
  });

  // The default Mode: Bypass asks once (as the chip does).
  function onMode(v) {
    if (v === BYPASS && !bypassConfirmed) {
      cfg.jobsMode = modeBefore;
      confirmBypass = true;
      return;
    }
    modeBefore = v;
  }

  async function okBypass() {
    try {
      await invoke('job_bypass_confirmed');
    } catch {
      return;
    }
    bypassConfirmed = true;
    confirmBypass = false;
    cfg.jobsMode = BYPASS;
    modeBefore = BYPASS;
  }

  // Add folder…: Windows' folder picker, opened in Rust (`job_folders_pick`),
  // the folder picked added there.
  async function addFolder() {
    try {
      const f = await invoke('job_folders_pick');
      if (!f) return;
      await loadFolders();
      toast({ title: `“${f.name}” added`, description: 'Jobs can work in it now.', variant: 'success' });
    } catch (e) {
      toast({ title: "Couldn't add that folder", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }

  async function removeFolder(f) {
    try {
      await invoke('job_folders_remove', { name: f.name });
      toast({ title: `“${f.name}” removed`, description: 'Jobs will ask before working there again. Its jobs stay in History.' });
    } catch (e) {
      toast({ title: "Couldn't remove it", description: String(e?.message ?? e), variant: 'destructive' });
    }
    loadFolders();
  }

  async function setCap(f, cap) {
    try {
      const next = await invoke('job_folders_set_cap', { name: f.name, cap: cap || null });
      folders = folders.map((x) => (x.name === f.name ? next : x));
    } catch (e) {
      toast({ title: "Couldn't change it", description: String(e?.message ?? e), variant: 'destructive' });
      loadFolders();
    }
  }

  function openJob(j) {
    jobs.openWanted = { id: j.id, conversation: j.conversation ?? null };
    onshowview?.('chat');
  }

  async function undo() {
    const j = undoing;
    if (!j) return;
    undoBusy = true;
    try {
      const r = await invoke('job_undo', { id: j.id });
      const back = (r?.restored?.length ?? 0) + (r?.deleted?.length ?? 0);
      toast({ title: back ? `Undone: ${back === 1 ? '1 file' : `${back} files`} put back` : 'Nothing needed putting back', variant: 'success' });
      refreshJobs();
    } catch (e) {
      toast({ title: "Couldn't undo the job", description: String(e?.message ?? e), variant: 'destructive' });
    } finally {
      undoBusy = false;
      undoOpen = false;
    }
  }

  const when = (secs) =>
    secs ? new Date(secs * 1000).toLocaleString(undefined, { day: 'numeric', month: 'short', hour: 'numeric', minute: '2-digit' }) : '';
</script>

<div class="page-h">
  <h1>Jobs</h1>
  <p>Hand a task to your own Claude Code from Chat or the Voice Assistant. It works in a folder you chose, and tells you what it did.</p>
</div>

<Group title="Jobs" id="settings-jobs">
  <Row setting="jobs-enabled">
    <Toggle bind:checked={cfg.jobsEnabled} label="Jobs" desc="Let Chat and the Voice Assistant hand tasks to your own Claude Code, in folders you choose." />
  </Row>
  <ClaudeCodeStatus />
</Group>

<Group title="Paying for jobs" id="settings-jobs-pay">
  <Row label="Pay with" desc={payLine} setting="jobs-pay">
    <Segmented bind:value={cfg.jobsPay} options={PAY} label="Pay with" />
  </Row>
  {#if cfg.jobsPay === 'key' && !hasKey}
    <Row>
      <p class="set-note">
        <button type="button" class="set-link" onclick={() => goto?.('cleanup#chat')}>Add an API key on Language Models' Claude tab</button>
      </p>
    </Row>
  {/if}
</Group>

<Group title="Defaults" desc="What a job runs on unless you ask for something else." id="settings-jobs-defaults">
  <Row label="Model" desc="The Claude model a job runs on" setting="jobs-model">
    <Select bind:value={cfg.jobsModel} options={modelOptions} ariaLabel="Model" />
  </Row>
  <Row label="Effort" desc="Higher effort thinks longer: more thorough, slower, and it uses more of your plan" setting="jobs-effort">
    <Select bind:value={cfg.jobsEffort} options={EFFORTS} ariaLabel="Effort" />
  </Row>
  <Row label="Mode" desc="The Mode a new chat starts in. Change it for one chat beside Chat's message box." setting="jobs-mode">
    <Select bind:value={cfg.jobsMode} options={MODES} ariaLabel="Mode" onchange={onMode} wide />
  </Row>
</Group>

<Group title="Folders" desc="Where jobs may work. A job asks first in a folder that isn't here yet." id="settings-jobs-folders">
  {#each folders as f (f.path)}
    <Row label={f.name} setting="jobs-folder" stack>
      {#snippet desc()}
        <span class="fpath" title={f.path}>{f.path}</span>
        {#if bypassConfirmed && (!f.cap || f.cap === BYPASS)}
          <span class="fwarn" data-testid="jobs-bypass-warning">Bypass permissions can run here: {WARNING}</span>
        {/if}
      {/snippet}
      <div class="frow" data-testid="jobs-folder" data-name={f.name}>
        <span class="caplabel">Never above:</span>
        <Select value={f.cap || ''} options={CAPS} ariaLabel={`Never above, in ${f.name}`} onchange={(v) => setCap(f, v)} />
        <Button variant="ghost" size="sm" onclick={() => removeFolder(f)}>Remove</Button>
      </div>
    </Row>
  {:else}
    <Row>
      <p class="set-note">No folders yet. Add one, or name a folder's full path when you ask Chat for a job.</p>
    </Row>
  {/each}
  <Row label="Add folder…" desc="Pick a folder jobs may work in" setting="jobs-add-folder">
    <Button variant="secondary" size="sm" onclick={addFolder}>Add folder…</Button>
  </Row>
  <Row>
    <p class="set-note small">This folder's own Claude settings and MCP servers also run.</p>
  </Row>
</Group>

<Group title="Asking and reports" id="settings-jobs-asking">
  <Row label="Ask before starting a job" desc="A card in Chat, or a question out loud, before it starts" setting="jobs-ask" stack>
    <Select bind:value={cfg.jobsAsk} options={ASK} ariaLabel="Ask before starting a job" wide />
  </Row>
  <Row label="Read job reports aloud" desc="When a job ends, its report in the chat's own voice" setting="jobs-speak">
    <Select bind:value={cfg.jobsSpeak} options={SPEAK} ariaLabel="Read job reports aloud" />
  </Row>
  <Row>
    <p class="set-note" data-setting="jobs-limits">Backups of changed files are kept 7 days. One job at a time per folder, at most 3 at once.</p>
  </Row>
  <Row>
    <p class="set-note small" data-setting="jobs-backups-hooks">Undo needs Claude Code's hooks, which back up each file before a job changes it. If a policy set by your organisation turns hooks off, no backups are made, and the job's tab says so.</p>
  </Row>
</Group>

<Group title="History" id="settings-jobs-history">
  {#each history as j (j.id)}
    <div class="hrow" data-testid="jobs-history-row" data-status={j.status} data-setting="jobs-history">
      <span class="hdot tone-{statusTone(j.status)}" aria-hidden="true"></span>
      <div class="htext">
        <p class="htop"><span class="hfolder">{j.folder}</span><span class="hstatus">{statusWord(j.status)}</span><span class="hwhen">{when(j.createdTs)}</span></p>
        <p class="hcost" data-testid="jobs-history-cost">{costLine(j)}{j.undone ? ' · Undone' : ''}</p>
      </div>
      <div class="hbtns">
        <Button variant="ghost" size="sm" onclick={() => openJob(j)}>Open</Button>
        {#if canUndo(j)}
          <Button
            variant="ghost"
            size="sm"
            onclick={() => {
              undoing = j;
              undoOpen = true;
            }}>Undo</Button
          >
        {/if}
      </div>
    </div>
  {:else}
    <Row>
      <p class="set-note" data-setting="jobs-history">No jobs yet. Ask Chat to start one, in a folder above.</p>
    </Row>
  {/each}
  {#if jobs.list.length > 20 && !showAll}
    <Row>
      <button type="button" class="set-link" onclick={() => (showAll = true)}>Show all {jobs.list.length}</button>
    </Row>
  {/if}
</Group>

<p class="set-note foot" data-testid="jobs-connections-note">Jobs connect to Anthropic through Claude Code itself, so they aren't in Network connections.</p>

<ConfirmDialog bind:open={confirmBypass} title="Use Bypass permissions?" body={WARNING} confirmLabel="Use Bypass permissions" onconfirm={okBypass} />
<ConfirmDialog
  bind:open={undoOpen}
  title="Undo this job?"
  body={undoing ? `Every file the job in ${undoing.folder} changed goes back to how it was before it, and the files it made are deleted.` : ''}
  confirmLabel="Undo job"
  busyLabel="Undoing…"
  busy={undoBusy}
  onconfirm={undo}
/>

<style>
  .fpath {
    display: block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .fwarn {
    display: block;
    margin-top: 2px;
    color: var(--yap-danger);
    font-size: 12px;
  }
  .frow {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .caplabel {
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .small {
    font-size: 12px;
  }
  .hrow {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 0;
  }
  .hdot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--yap-muted);
  }
  .hdot.tone-live {
    background: var(--yap-primary);
  }
  .hdot.tone-ask {
    background: var(--yap-warning);
  }
  .hdot.tone-done {
    background: var(--yap-success);
  }
  .hdot.tone-failed {
    background: var(--yap-danger);
  }
  .htext {
    flex: 1 1 auto;
    min-width: 0;
  }
  .htop,
  .hcost {
    margin: 0;
  }
  .htop {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 8px;
    font-size: 13px;
  }
  .hfolder {
    font-weight: 600;
    color: var(--yap-fg);
  }
  .hstatus,
  .hwhen {
    font-size: 12px;
    color: var(--yap-muted);
  }
  .hcost {
    font-size: 12px;
    color: var(--yap-muted);
  }
  .hbtns {
    flex: 0 0 auto;
    display: flex;
    gap: 4px;
  }
  .foot {
    margin-top: -12px;
  }
</style>
