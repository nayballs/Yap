<script>
  // **A job's tab** beside the chat (Jobs: desktop/src-tauri/src/jobs.rs,
  // chatDoc.svelte.js `job`): what the person's own Claude Code is doing in
  // their folder, as it does it — fed by `job_get` (the job and its log, to
  // replay) and the app-wide `yap-job` event (live). Closing the tab never
  // stops the job.
  // - The folder and its status; the mode and model; how long it has run.
  // - What it's doing now (the latest line moving), then every step in
  //   plain words: reading a file, searching the web, editing a file,
  //   running a command.
  // - The files it changed, the commands it ran, what it wasn't allowed to
  //   do, its final words (markdown, core/markdown.js), and its cost, told
  //   apart by how it's paid ("Your Claude plan · ≈ $0.04 on the API" /
  //   "API key · $0.04").
  // - A problem with Claude Code itself in plain words, with one button
  //   (Sign in, when it isn't signed in).
  // - Stop (while it works), Continue (stopped, interrupted, failed), Undo
  //   job (a confirm, then what was put back; never when no backups were
  //   made: Claude Code's hooks didn't run, and the tab says so), Open in terminal (once it
  //   isn't running), and "Tell the job…": the person's own words, to it at
  //   once while it works, else its next turn (`job_message`).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick, untrack } from 'svelte';
  import ConfirmDialog from '../../ui/ConfirmDialog.svelte';
  import { toast } from '../../ui/toast.svelte.js';
  import { renderMarkdown } from '../../core/markdown.js';
  import { isLive, statusWord, statusTone, costLine, elapsed, stepsOf, stepKind, problemWords, modeLabel, modelLabel, canUndo as undoable, NO_BACKUPS } from '../../core/jobs.js';
  import { EFFORT_LABELS } from '../../core/effort.js';
  import { claudePlan, signInClaudePlan } from '../../stores/claudePlan.svelte.js';

  /** `doc`: the panel's state (its `job`, a JobRef); `narrow`: no room beside the chat (no expand). */
  let { doc, narrow = false } = $props();

  const ref = $derived(doc.job);
  let job = $state(null);
  let log = $state([]);
  let gone = $state(false);
  let now = $state(Date.now() / 1000);
  let busy = $state('');
  let message = $state('');
  let confirmUndo = $state(false);
  let undone = $state(null); // what Undo put back
  let stepsEl = $state(null);
  let undoBtn = $state(null);

  // The job as it is now: its saved record, else what the tab was opened with.
  const shown = $derived(job ?? (ref ? { ...ref, files: [], commands: [], denials: [] } : null));
  const live = $derived(isLive(shown?.status));
  const steps = $derived(stepsOf(log));
  const latest = $derived(live && steps.length ? steps[steps.length - 1] : null);
  // Its words while it works (the latest a `text` event brought).
  const working = $derived(live ? (log.findLast((e) => typeof e?.text === 'string' && e.text.trim())?.text ?? '') : '');
  const problem = $derived(problemWords(shown?.problem));
  const ran = $derived(shown ? elapsed(shown, now) : '');
  const canUndo = $derived(undoable(shown));

  // Events heard while the log is read: kept, unless the log has them already.
  let loading = false;
  let heard = [];

  async function load(id) {
    loading = true;
    heard = [];
    try {
      const r = await invoke('job_get', { id });
      if (doc.job?.id !== id) return;
      const saved = r.log || [];
      const tail = new Set(saved.slice(-50).map((e) => JSON.stringify(e)));
      job = r.job;
      log = [...saved, ...heard.filter((e) => !tail.has(JSON.stringify(e)))];
      gone = false;
      scrollSteps();
    } catch {
      if (doc.job?.id === id) gone = true;
    } finally {
      loading = false;
      heard = [];
    }
  }

  // Another job in the tab: read it.
  let loadedFor = null;
  $effect(() => {
    const id = ref?.id;
    untrack(() => {
      if (!id || id === loadedFor) return;
      loadedFor = id;
      job = null;
      log = [];
      undone = null;
      message = '';
      load(id);
    });
  });

  function scrollSteps() {
    tick().then(() => {
      if (stepsEl) stepsEl.scrollTop = stepsEl.scrollHeight;
    });
  }

  /** One `yap-job` event: into the log; the job as it is now (read again when its status turns). */
  function onJob(ev) {
    if (!ev?.id || ev.id !== doc.job?.id) return;
    if (loading) heard.push(ev);
    log = [...log, ev];
    if (job) {
      const was = job.status;
      const next = { ...job, status: ev.status ?? job.status };
      if (ev.file && !(job.files || []).includes(ev.file)) next.files = [...(job.files || []), ev.file];
      if (ev.command) next.commands = [...(job.commands || []), ev.command];
      if (typeof ev.cost === 'number') next.cost = ev.cost;
      if (ev.problem) next.problem = ev.problem;
      job = next;
      if (was !== next.status) refreshSoon();
    } else {
      refreshSoon();
    }
    if (ev.step) scrollSteps();
  }

  let refreshTimer = null;
  function refreshSoon() {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(async () => {
      const id = doc.job?.id;
      if (!id) return;
      try {
        const r = await invoke('job_get', { id });
        if (doc.job?.id === id) job = r.job;
      } catch {
        /* keep */
      }
    }, 120);
  }

  onMount(() => {
    const uns = [listen('yap-job', (e) => onJob(e.payload))];
    const timer = setInterval(() => (now = Date.now() / 1000), 1000);
    return () => {
      clearInterval(timer);
      clearTimeout(refreshTimer);
      uns.forEach((u) => u.then((f) => f()).catch(() => {}));
    };
  });

  async function act(what, cmd, args = {}) {
    if (!shown) return null;
    busy = what;
    try {
      const r = await invoke(cmd, { id: shown.id, ...args });
      return r ?? true;
    } catch (e) {
      toast({ title: `Couldn't ${what} the job`, description: String(e?.message ?? e), variant: 'destructive' });
      return null;
    } finally {
      busy = '';
    }
  }

  async function stop() {
    const r = await act('stop', 'job_stop');
    if (r && typeof r === 'object' && r.id) job = { ...job, ...r };
  }

  async function carryOn() {
    undone = null;
    const r = await act('continue', 'job_continue');
    if (r && typeof r === 'object' && r.id) job = { ...job, ...r };
  }

  async function openTerminal() {
    await act('open', 'job_open_terminal');
  }

  // Claude Code's own sign-in, in a terminal (the Claude tab's, stores/claudePlan.svelte.js).
  async function signIn() {
    await signInClaudePlan();
    if (claudePlan.failed) toast({ title: "Couldn't open Claude Code's sign-in", description: claudePlan.failed, variant: 'destructive' });
  }

  async function undo() {
    const r = await act('undo', 'job_undo');
    confirmUndo = false;
    if (!r || typeof r !== 'object') return;
    undone = r;
    refreshSoon();
    tick().then(() => undoBtn?.focus?.());
  }

  async function send() {
    const text = message.trim();
    if (!text || busy) return;
    const r = await act('tell', 'job_message', { text });
    if (!r) return;
    message = '';
    if (typeof r === 'object' && r.id) job = { ...job, ...r };
  }

  const shortPath = (p) => String(p || '').split(/[\\/]/).pop() || String(p || '');
  const undoneLine = (r) => {
    const back = (r.restored?.length ?? 0) + (r.deleted?.length ?? 0);
    if (!back) return 'Nothing needed putting back.';
    const parts = [];
    if (r.restored?.length) parts.push(`${r.restored.map(shortPath).join(', ')} put back`);
    if (r.deleted?.length) parts.push(`${r.deleted.map(shortPath).join(', ')} deleted (the job made ${r.deleted.length === 1 ? 'it' : 'them'})`);
    return `Undone: ${parts.join('; ')}.`;
  };
  const effortWord = (e) => EFFORT_LABELS[e] || e || '';
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="jobtab" aria-label="Job" data-testid="chat-job" data-status={shown?.status ?? ''}>
  <header class="jobhead">
    <svg class="jobicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
    <h2 class="jobtitle" title={shown?.path || shown?.folder}>{shown?.folder || 'Job'}</h2>
    {#if shown}
      <span class="status tone-{statusTone(shown.status)}" data-testid="chat-job-status"><span class="dot" aria-hidden="true"></span>{statusWord(shown.status)}</span>
    {/if}
    <div class="jobactions">
      {#if !narrow}
        <button
          type="button"
          class="jbtn"
          onclick={() => (doc.expanded = !doc.expanded)}
          aria-label={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
          title={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
        >
          {#if doc.expanded}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" /></svg>
          {/if}
        </button>
      {/if}
      <button type="button" class="jbtn" onclick={() => doc.close()} aria-label="Close the panel" title="Close (the job carries on)">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  </header>

  {#if gone}
    <p class="jgone">This job isn't in Yap any more.</p>
  {:else if shown}
    <div class="jobbody">
      <p class="meta" data-testid="chat-job-meta">
        <span>{modeLabel(shown.mode)}</span>
        {#if shown.model}<span>· {modelLabel(shown.model)}{shown.effort ? ` ${effortWord(shown.effort)}` : ''}</span>{/if}
        {#if ran}<span data-testid="chat-job-elapsed">· {live ? `${ran} so far` : `ran ${ran}`}</span>{/if}
      </p>
      {#if shown.path}<p class="path" title={shown.path}>{shown.path}</p>{/if}

      {#if problem}
        <div class="problem" role="alert" data-testid="chat-job-problem" data-kind={shown.problem.kind}>
          <p class="pline">{problem.line}</p>
          {#if problem.check}<p class="pcheck">{problem.check}</p>{/if}
          {#if problem.action === 'signIn'}
            <button type="button" class="pill primary" onclick={signIn}>Sign in</button>
          {/if}
        </div>
      {:else if shown.status === 'failed' && shown.error}
        <div class="problem" role="alert" data-testid="chat-job-problem">
          <p class="pline">{shown.error}</p>
        </div>
      {/if}

      {#if latest}
        <!-- What it's doing now. -->
        <p class="now" data-testid="chat-job-now" aria-live="polite"><span class="spin" aria-hidden="true"></span>{latest.step}</p>
      {/if}

      {#if steps.length}
        <details class="section" open>
          <summary>Steps <span class="count">{steps.length}</span></summary>
          <ol class="steps" bind:this={stepsEl} data-testid="chat-job-steps">
            {#each steps as s, i (i)}
              {@const kind = stepKind(s)}
              <li class="step k-{kind}" class:latest={latest && i === steps.length - 1}>
                <span class="sicon" aria-hidden="true">
                  {#if kind === 'edit'}
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z" /></svg>
                  {:else if kind === 'command'}
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="m4 17 6-6-6-6" /><path d="M12 19h8" /></svg>
                  {:else if kind === 'web'}
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></svg>
                  {:else if kind === 'read' || kind === 'search'}
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /></svg>
                  {:else if kind === 'ask'}
                    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 3l8 3v6c0 4.5-3.4 8.2-8 9-4.6-.8-8-4.5-8-9V6z" /></svg>
                  {:else}
                    <svg viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="12" r="2.4" /></svg>
                  {/if}
                </span>
                <span class="stext">{s.step}</span>
              </li>
            {/each}
          </ol>
        </details>
      {/if}

      {#if working}
        <p class="working">{working}</p>
      {/if}

      {#if shown.files?.length}
        <div class="section" data-testid="chat-job-files">
          <h3>Files changed <span class="count">{shown.files.length}</span></h3>
          <ul class="plain">
            {#each shown.files as f (f)}<li><code>{f}</code></li>{/each}
          </ul>
          {#if shown.noBackups}
            <p class="nobackups" data-testid="chat-job-no-backups">{NO_BACKUPS}</p>
          {/if}
        </div>
      {/if}
      {#if shown.commands?.length}
        <div class="section" data-testid="chat-job-commands">
          <h3>Commands run <span class="count">{shown.commands.length}</span></h3>
          <ul class="plain">
            {#each shown.commands as c, i (i)}<li><code class="cmd">{c}</code></li>{/each}
          </ul>
        </div>
      {/if}
      {#if shown.denials?.length}
        <div class="section" data-testid="chat-job-denials">
          <h3>Not allowed</h3>
          <ul class="plain">
            {#each shown.denials as d, i (i)}<li>{d.detail || d.tool}</li>{/each}
          </ul>
        </div>
      {/if}

      {#if !live && shown.text}
        <div class="section final" data-testid="chat-job-text">
          <h3>{shown.plan ? 'Its plan' : 'What it said'}</h3>
          <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
          <div class="md">{@html renderMarkdown(shown.text)}</div>
        </div>
      {/if}

      {#if undone}
        <p class="undone" aria-live="polite" data-testid="chat-job-undone">{undoneLine(undone)}</p>
      {:else if shown.undone}
        <p class="undone">Undone: its files are back as they were.</p>
      {/if}

      <p class="cost" data-testid="chat-job-cost" data-pay={shown.pay || 'plan'}>{costLine(shown)}</p>
    </div>

    <footer class="jobfoot">
      <div class="jbuttons">
        {#if live}
          <button type="button" class="pill" onclick={stop} disabled={!!busy}>Stop</button>
        {:else if shown.status === 'stopped' || shown.status === 'interrupted' || shown.status === 'failed'}
          <button type="button" class="pill primary" onclick={carryOn} disabled={!!busy}>Continue</button>
        {/if}
        {#if canUndo}
          <button type="button" class="pill" bind:this={undoBtn} onclick={() => (confirmUndo = true)} disabled={!!busy}>Undo job</button>
        {/if}
        {#if !live && shown.session && shown.startedTs}
          <button type="button" class="pill" onclick={openTerminal} disabled={!!busy} title="Its session in Windows Terminal, in its folder">Open in terminal</button>
        {/if}
      </div>
      <form
        class="tell"
        onsubmit={(e) => {
          e.preventDefault();
          send();
        }}
      >
        <input type="text" bind:value={message} placeholder="Tell the job…" aria-label="Tell the job" maxlength="4000" autocomplete="off" />
        <!-- Named apart from Chat's own Send (the message box's). -->
        <button type="submit" class="tsend" disabled={!message.trim() || !!busy} aria-label="Tell the job" title="Tell the job (Enter)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12h14M13 6l6 6-6 6" /></svg>
        </button>
      </form>
    </footer>
  {/if}
</section>

<ConfirmDialog
  bind:open={confirmUndo}
  title="Undo this job?"
  body="Every file it changed goes back to how it was before it, and the files it made are deleted. Yap kept their backups for this."
  confirmLabel="Undo job"
  busyLabel="Undoing…"
  busy={busy === 'undo'}
  returnFocus={undoBtn}
  onconfirm={undo}
/>

<style>
  .jobtab {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    color: var(--yap-paper-ink);
  }
  .jobhead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 8px 18px;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .jobicon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-primary);
  }
  .jobtitle {
    flex: 0 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-paper-text-2);
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: 0 0 auto;
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--yap-paper-s1);
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-paper-muted);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--yap-paper-muted);
  }
  .tone-live .dot {
    background: var(--yap-primary);
  }
  .tone-ask .dot {
    background: var(--yap-warning);
  }
  .tone-ask {
    color: var(--yap-warning);
  }
  .tone-done .dot {
    background: var(--yap-success);
  }
  .tone-failed .dot {
    background: var(--yap-danger);
  }
  .tone-failed {
    color: var(--yap-danger);
  }
  @media (prefers-reduced-motion: no-preference) {
    .tone-live .dot {
      animation: pulse 1.4s ease-in-out infinite;
    }
  }
  :global(:root[data-motion='reduced']) .tone-live .dot {
    animation: none;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
  .jobactions {
    margin-left: auto;
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .jbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
  }
  .jbtn:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .jbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .jbtn svg {
    width: 15px;
    height: 15px;
  }
  .jgone {
    margin: 18px;
    color: var(--yap-paper-muted);
    font-size: 13px;
  }
  .jobbody {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 12px 18px 16px;
    font-size: 13px;
    line-height: 1.5;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 0;
    color: var(--yap-paper-text-2);
    font-size: 12.5px;
  }
  .path {
    margin: 2px 0 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
  .problem {
    margin: 12px 0 0;
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--yap-danger) 35%, var(--yap-paper-rule));
    border-radius: 10px;
    background: color-mix(in srgb, var(--yap-danger) 6%, var(--yap-paper));
  }
  .pline,
  .pcheck {
    margin: 0;
  }
  .pline {
    font-weight: 600;
  }
  .pcheck {
    margin-top: 2px;
    font-size: 12.5px;
    color: var(--yap-paper-text-2);
  }
  .problem .pill {
    margin-top: 8px;
  }
  .now {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 12px 0 0;
    padding: 8px 10px;
    border-radius: 9px;
    background: var(--yap-paper-s1);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .spin {
    flex: 0 0 auto;
    width: 11px;
    height: 11px;
    border: 2px solid color-mix(in srgb, var(--yap-primary) 30%, transparent);
    border-top-color: var(--yap-primary);
    border-radius: 50%;
  }
  @media (prefers-reduced-motion: no-preference) {
    .spin {
      animation: spin 0.9s linear infinite;
    }
  }
  :global(:root[data-motion='reduced']) .spin {
    animation: none;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .section {
    margin: 14px 0 0;
  }
  .section > summary,
  .section > h3 {
    margin: 0 0 6px;
    font-size: 12px;
    font-weight: 650;
    color: var(--yap-paper-text-2);
    cursor: default;
  }
  .section > summary {
    cursor: pointer;
  }
  .count {
    margin-left: 4px;
    font-weight: 500;
    color: var(--yap-paper-muted);
  }
  .steps {
    max-height: 260px;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
  }
  .step {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 3px 0;
    color: var(--yap-paper-text-2);
    font-size: 12.5px;
  }
  .step.latest {
    color: var(--yap-paper-ink);
    font-weight: 600;
  }
  .sicon {
    flex: 0 0 auto;
    display: inline-flex;
    width: 14px;
    height: 14px;
    margin-top: 2px;
    color: var(--yap-paper-muted);
  }
  .k-edit .sicon {
    color: var(--yap-primary);
  }
  .k-ask .sicon {
    color: var(--yap-warning);
  }
  .sicon svg {
    width: 14px;
    height: 14px;
  }
  .stext {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .working {
    margin: 10px 0 0;
    color: var(--yap-paper-text-2);
    font-style: italic;
    font-size: 12.5px;
    overflow-wrap: anywhere;
  }
  .plain {
    margin: 0;
    padding: 0 0 0 2px;
    list-style: none;
  }
  .plain li {
    padding: 2px 0;
    font-size: 12.5px;
    overflow-wrap: anywhere;
  }
  code {
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 12px;
    padding: 1px 5px;
    border-radius: 5px;
    background: var(--yap-paper-s1);
  }
  code.cmd {
    white-space: pre-wrap;
  }
  .final .md {
    font-family: var(--yap-font-reading);
    font-size: calc(13px * var(--yap-reading-scale));
    line-height: 1.6;
    overflow-wrap: anywhere;
  }
  .final .md :global(p) {
    margin: 0 0 8px;
  }
  .nobackups {
    margin: 8px 0 0;
    padding: 7px 10px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--yap-warning) 10%, var(--yap-paper));
    color: var(--yap-paper-text-2);
    font-size: 12.5px;
  }
  .undone {
    margin: 12px 0 0;
    padding: 8px 10px;
    border-radius: 9px;
    background: color-mix(in srgb, var(--yap-success) 9%, var(--yap-paper));
    font-size: 12.5px;
  }
  .cost {
    margin: 14px 0 0;
    color: var(--yap-paper-muted);
    font-size: 12px;
  }
  .jobfoot {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 14px 12px;
    border-top: 1px solid var(--yap-paper-rule);
  }
  .jbuttons {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .jbuttons:empty {
    display: none;
  }
  .pill {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .pill:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .pill.primary {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .pill.primary:hover:not(:disabled) {
    background: var(--yap-primary-hover);
  }
  .pill:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .pill:focus-visible,
  .tell input:focus-visible,
  .tsend:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .tell {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .tell input {
    flex: 1 1 auto;
    min-width: 0;
    height: 30px;
    padding: 0 10px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 9px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
  }
  .tsend {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 9px;
    background: var(--yap-paper-s1);
    color: var(--yap-paper-ink);
    cursor: pointer;
  }
  .tsend:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .tsend svg {
    width: 15px;
    height: 15px;
  }
</style>
