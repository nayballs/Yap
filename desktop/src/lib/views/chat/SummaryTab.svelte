<script>
  // **A Code chat's summary** (15 Nov 2026, after the Claude app's right
  // column in its Code tab): one panel tab, opened from the panel's row (its
  // list button) or the title bar's, with what's true of the chat now:
  //
  // - **Environment**: its workspace (name and path), the git branch and
  //   whether it's a linked worktree (agents/files.rs `code_chat_environment`:
  //   read from `.git` itself);
  // - **Context**: how full the chat's context is, from what Yap already
  //   knows — its own Claude's latest turn (a Code chat that works
  //   directly: agents/usage.rs), else the plan's count for the chat, else
  //   the context ring's (`chat_context`);
  // - **Connectors**: the chat's own, on or off (Connectors preview);
  // - **Outputs**: the files its work changed and the files it shared
  //   (outputs.js, OutputsList.svelte: one list, any kind adds its rows);
  // - **Background tasks**: what's working for it now — its helpers and the
  //   agents Chat sent (core/agentPanel.js `panelRows`, the Agent panel's
  //   own rows) and its ▶ Runs (codeRuns.svelte.js) — each with its clock and
  //   Stop, then "N finished".
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, untrack, getContext } from 'svelte';
  import { agents as agentsStore } from '../../stores/agents.svelte.js';
  import { agentLogs, watchAgentLog } from '../../stores/agentLogs.svelte.js';
  import { planUsage } from '../../stores/claudePlanUsage.svelte.js';
  import { panelRows, rowActions, ctxPercent, fmtClock } from '../../core/agentPanel.js';
  import { percentOf } from '../../core/aiUsage.js';
  import { codeRuns, loadRuns, stopRun } from './codeRuns.svelte.js';
  import { outputItems } from './outputs.js';
  import { CODE_FILES } from '../../ui/fileLinks.js';
  import { toast } from '../../ui/toast.svelte.js';
  import OutputsList from './OutputsList.svelte';

  /** `doc`: the panel's state; `conversationId`: the Code chat (null: a new one); `workspace`: its folder's path; `messages`: its thread. */
  let { doc, conversationId = null, workspace = null, messages = [] } = $props();
  const files = getContext(CODE_FILES);

  let env = $state(null);
  let connectors = $state(null);
  let ringUse = $state(null);
  let now = $state(Date.now() / 1000);

  onMount(() => {
    const t = setInterval(() => (now = Date.now() / 1000), 1000);
    return () => clearInterval(t);
  });

  async function load() {
    const conv = conversationId;
    try {
      env = await invoke('code_chat_environment', { conversation: conv ?? null, workspace: workspace ?? null });
    } catch {
      env = null;
    }
    try {
      connectors = await invoke('connector_chat_list', { conversationId: conv ?? null });
    } catch {
      connectors = null;
    }
    if (conv != null) {
      try {
        ringUse = await invoke('chat_context', { conversationId: conv });
      } catch {
        ringUse = null;
      }
      loadRuns(conv);
    } else {
      ringUse = null;
    }
  }
  $effect(() => {
    void conversationId;
    void workspace;
    untrack(load);
  });

  // The chat's agents (its own Claude's turns, and agents Chat sent), their logs followed.
  const chatAgents = $derived(conversationId == null ? [] : agentsStore.list.filter((a) => a.conversation === conversationId));
  $effect(() => {
    for (const a of chatAgents) untrack(() => watchAgentLog(a.id));
  });
  const rows = $derived(panelRows(chatAgents, { logs: agentLogs.by, steps: agentsStore.steps, now, approvals: agentsStore.approvals }));
  const runs = $derived(conversationId == null ? [] : codeRuns.by[String(conversationId)] || []);
  const working = $derived(rows.filter((r) => r.state === 'running' || r.state === 'needs'));
  const runsWorking = $derived(runs.filter((r) => r.state === 'running'));
  const finished = $derived(rows.filter((r) => r.state === 'finished').length + runs.filter((r) => r.state !== 'running').length);

  /** How long a row has worked: a turn's own clock, a helper's since it was handed its part. */
  const rowSecs = (r) => (r.kind === 'helper' ? (r.helper?.ts ? Math.max(0, now - r.helper.ts) : 0) : r.secs);
  const runSecs = (r) => Math.max(0, now - r.startedMs / 1000);

  // How full the context is (see above): `{ pct, about }` or null.
  const context = $derived.by(() => {
    for (const a of chatAgents) {
      const t = [...(a.turnList || [])].reverse().find((x) => x.direct && x.usage);
      const p = t ? ctxPercent(t.usage) : null;
      if (p != null) return { pct: p, about: false };
    }
    const fill = conversationId == null ? null : planUsage.contexts?.[String(conversationId)];
    if (fill && Number(fill.window) > 0) return { pct: Math.min(100, Math.round((Number(fill.used) / Number(fill.window)) * 100)), about: !fill.known };
    if (ringUse && ringUse.context > 0 && ringUse.messages > 0) return { pct: Math.min(100, percentOf(ringUse.used, ringUse.context)), about: false };
    return null;
  });

  const outputs = $derived(outputItems({ agents: chatAgents, messages }));

  async function openOutput(it) {
    if (it.kind === 'changed') {
      doc.showFile({ rel: it.rel, line: null });
      return;
    }
    if (it.kind === 'shared') {
      if (it.picture) {
        doc.showPicture({ id: it.item.id, w: it.item.w, h: it.item.h, caption: it.item.name });
        return;
      }
      // A file in the workspace: the file view; any other as its card opens it.
      try {
        const [ref] = await invoke('code_files_check', { conversation: conversationId, paths: [it.item.path] });
        if (ref) {
          doc.showFile(ref);
          return;
        }
        await invoke('chat_file_open', { conversation: conversationId, path: it.item.path });
      } catch (e) {
        toast({ title: "Couldn't open it", description: String(e?.message ?? e), variant: 'destructive' });
      }
    }
  }

  function openRow(r) {
    const a = r.agent;
    if (!a) return;
    doc.showAgent({ id: a.id, folder: a.folder, status: a.status, mode: a.mode, conversation: a.conversation, turn: r.n });
  }

  async function stopRow(r) {
    try {
      await invoke('agent_stop', { id: r.agentId });
    } catch (e) {
      toast({ title: "Couldn't stop it", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }
  /** A row Stop can end now: its turn at work (a helper's: the turn that sent it). */
  const canStop = (r) => (r.kind === 'helper' ? !!r.agent?.active : rowActions(r).stop);

  const connectorList = $derived(connectors?.shown ? connectors.connectors || [] : []);
</script>

<section class="summarytab" aria-label="Summary" data-testid="chat-summary">
  <header class="sumhead">
    <svg class="sumicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 6h13M8 12h13M8 18h13M3.5 6h.01M3.5 12h.01M3.5 18h.01" /></svg>
    <h2 class="sumtitle">Summary</h2>
    <div class="sumactions">
      <button type="button" class="sbtn" onclick={() => doc.close()} aria-label="Close the panel" title="Close">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  </header>
  <div class="sumbody">
    <section class="sec" data-testid="chat-summary-environment">
      <h3 class="sh">Environment</h3>
      {#if env}
        <div class="srow">
          <svg class="si" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
          <span class="sv" title={env.path}><b>{env.name}</b> <span class="sdim">· {env.path}</span></span>
        </div>
        {#if !env.exists}
          <p class="snote">This workspace's folder isn't on this PC any more.</p>
        {:else if env.repo}
          <div class="srow" data-testid="chat-summary-branch">
            <svg class="si" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="6" cy="6" r="2" /><circle cx="6" cy="18" r="2" /><circle cx="18" cy="8" r="2" /><path d="M6 8v8M18 10c0 4-6 3-11 6" /></svg>
            <span class="sv mono">{env.branch || 'No branch'}{#if env.detached}<span class="sdim"> (detached)</span>{/if}</span>
            {#if env.worktree}<span class="stag" data-testid="chat-summary-worktree">Worktree</span>{/if}
          </div>
        {:else}
          <p class="snote">Not a git repository.</p>
        {/if}
        <div class="srow">
          <svg class="si" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="12" rx="2" /><path d="M8 20h8M12 16v4" /></svg>
          <span class="sv">Runs on this PC</span>
        </div>
      {:else}
        <p class="snote">…</p>
      {/if}
    </section>

    <section class="sec" data-testid="chat-summary-context">
      <h3 class="sh">Context</h3>
      {#if context}
        <div class="ctx" title={`${context.about ? 'About ' : ''}${context.pct}% of the context used`}>
          <span class="ctxbar" aria-hidden="true"><span style:width={`${context.pct}%`} class:high={context.pct >= 80}></span></span>
          <span class="ctxpct">{context.about ? '≈ ' : ''}{context.pct}%</span>
        </div>
      {:else}
        <p class="snote">Nothing used yet.</p>
      {/if}
    </section>

    <section class="sec" data-testid="chat-summary-connectors">
      <h3 class="sh">Connectors</h3>
      {#if connectorList.length}
        {#each connectorList as c (c.id)}
          <div class="srow">
            <span class="cmark" aria-hidden="true" style:background={c.color || undefined}>{(c.name || '?').slice(0, 1).toUpperCase()}</span>
            <span class="sv">{c.name}</span>
            <span class="sdim right">{c.on ? 'On' : 'Off in this chat'}</span>
          </div>
        {/each}
      {:else}
        <p class="snote">None in this chat.</p>
      {/if}
      {#if files?.openSettings}
        <button type="button" class="slink" onclick={() => files.openSettings('connectors')}>Manage connectors…</button>
      {/if}
    </section>

    <section class="sec" data-testid="chat-summary-outputs">
      <h3 class="sh">Outputs</h3>
      {#if outputs.length}
        <OutputsList items={outputs} onopen={openOutput} />
      {:else}
        <p class="snote">Nothing changed or shared yet.</p>
      {/if}
    </section>

    <section class="sec" data-testid="chat-summary-tasks">
      <h3 class="sh">Background tasks</h3>
      {#each working as r (r.key)}
        {@const secs = rowSecs(r)}
        <div class="trow" data-testid="chat-summary-task" data-kind={r.kind}>
          <button type="button" class="topen" onclick={() => openRow(r)} title={r.line || r.title}>
            {#if r.kind === 'helper'}
              <svg class="si" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v4M12 17v4M3 12h4M17 12h4" /><circle cx="12" cy="12" r="4" /></svg>
            {:else}
              <svg class="si" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
            {/if}
            <span class="tv">{r.title}</span>
            {#if r.state === 'needs'}<span class="tneeds">Needs you</span>{/if}
            {#if secs > 0}<span class="tclock">{fmtClock(secs)}</span>{/if}
          </button>
          {#if canStop(r)}
            <button type="button" class="tstop" onclick={() => stopRow(r)} aria-label={`Stop ${r.title}`} title={r.kind === 'helper' ? "Stop (Claude's turn, with its helpers)" : 'Stop the agent'} data-testid="chat-summary-task-stop">
              <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="2" /></svg>
            </button>
          {/if}
        </div>
      {/each}
      {#each runsWorking as r (r.id)}
        <div class="trow" data-testid="chat-summary-task" data-kind="run">
          <button type="button" class="topen" onclick={() => doc.showRuns()} title={r.command}>
            <svg class="si" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="m7 9 3 3-3 3M12 15h5" /></svg>
            <span class="tv mono">{r.command}</span>
            <span class="tclock">{fmtClock(runSecs(r))}</span>
          </button>
          <button type="button" class="tstop" onclick={() => stopRun(r.id)} aria-label={`Stop ${r.command}`} title="Stop it" data-testid="chat-summary-task-stop">
            <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="2" /></svg>
          </button>
        </div>
      {/each}
      {#if !working.length && !runsWorking.length}
        <p class="snote">Nothing running.</p>
      {/if}
      {#if finished > 0}
        <button type="button" class="tfinished" data-testid="chat-summary-finished" onclick={() => (runs.length && !chatAgents.length ? doc.showRuns() : chatAgents[0] && openRow({ agent: chatAgents[0], n: null }))}>
          <svg class="si" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12 4 4L19 6" /></svg>
          {finished} finished
        </button>
      {/if}
    </section>
  </div>
</section>

<style>
  .summarytab {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    color: var(--yap-paper-ink);
  }
  .sumhead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 8px 18px;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .sumicon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-code);
  }
  .sumtitle {
    margin: 0;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-paper-text-2);
  }
  .sumactions {
    margin-left: auto;
  }
  .sbtn,
  .tstop {
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
  .sbtn:hover,
  .tstop:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .sbtn svg {
    width: 15px;
    height: 15px;
  }
  .tstop svg {
    width: 11px;
    height: 11px;
  }
  .sbtn:focus-visible,
  .tstop:focus-visible,
  .topen:focus-visible,
  .slink:focus-visible,
  .tfinished:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .sumbody {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 12px 16px;
    font-size: 12.5px;
  }
  .sec {
    padding: 10px 6px 12px;
  }
  .sec + .sec {
    border-top: 1px solid var(--yap-paper-rule);
  }
  .sh {
    margin: 0 0 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-paper-text-2);
  }
  .srow {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
  }
  .si {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-paper-muted);
  }
  .sv {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mono {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
  }
  .sdim {
    color: var(--yap-paper-muted);
  }
  .right {
    flex: 0 0 auto;
    font-size: 11.5px;
  }
  .stag {
    flex: 0 0 auto;
    padding: 1px 7px;
    border-radius: 999px;
    background: var(--yap-paper-s1);
    color: var(--yap-paper-muted);
    font-size: 11px;
  }
  .snote {
    margin: 2px 0;
    color: var(--yap-paper-muted);
  }
  .ctx {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .ctxbar {
    flex: 1 1 auto;
    height: 5px;
    border-radius: 999px;
    background: var(--yap-paper-s2);
    overflow: hidden;
  }
  .ctxbar span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--yap-code);
  }
  .ctxbar span.high {
    background: var(--yap-warning);
  }
  .ctxpct {
    flex: 0 0 auto;
    font-variant-numeric: tabular-nums;
    color: var(--yap-paper-text-2);
  }
  .cmark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    width: 16px;
    height: 16px;
    border-radius: 4px;
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
    font-size: 10px;
    font-weight: 700;
  }
  .slink,
  .tfinished {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
    padding: 2px 0;
    border: none;
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .slink:hover,
  .tfinished:hover {
    color: var(--yap-paper-ink);
  }
  .trow {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .topen {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1 1 auto;
    min-width: 0;
    min-height: 28px;
    padding: 2px 6px;
    border: none;
    border-radius: 7px;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }
  .topen:hover {
    background: var(--yap-paper-s2);
  }
  .tv {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tneeds {
    flex: 0 0 auto;
    color: var(--yap-status-needs);
    font-size: 11.5px;
  }
  .tclock {
    flex: 0 0 auto;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }
</style>
