<script>
  // **A Code chat's summary** (15 Nov 2026, after the Claude app's right
  // column in its Code tab): one panel tab, opened from the panel's row (its
  // list button) or the title bar's, with what's true of the chat now:
  //
  // - **Environment**: its workspace (name and path), the git branch and
  //   whether it's a linked worktree (agents/files.rs `code_chat_environment`:
  //   read from `.git` itself), and the workspace's Remote Control switch
  //   (remoteControl.js: the same as the "Code · garden-planner" card's);
  // - **Changes**: the branch's pull requests (agents/prs.rs
  //   `code_chat_pull_requests`: the person's own `gh`), newest first — title,
  //   `owner/repo #N`, state, checks; a click opens it in their browser, a
  //   merged one's Archive archives this chat; a quiet line when there's no
  //   `gh`, no sign-in or no GitHub remote, no section without git;
  // - **Context**: broken down by category from Claude Code's own `/context`
  //   for the chat's session (agents/context.rs `code_chat_context`,
  //   contextBreakdown.js: "16% used · 156k / 1M", a bar by category, the
  //   rows, the deferred ones, the lists behind them); else how full, from
  //   what Yap already knows — its own Claude's latest turn (agents/usage.rs),
  //   else the plan's count for the chat, else the context ring's
  //   (`chat_context`);
  // - **Connectors**: the chat's own, on or off (Connectors preview);
  // - **Outputs**: the files its work changed and the files it shared
  //   (outputs.js, OutputsList.svelte: one list, any kind adds its rows),
  //   each with a right-click menu (outputsOpen.js `outputMenuItems`);
  // - **Background tasks**: what's working for it now — its helpers and the
  //   agents Chat sent (core/agentPanel.js `panelRows`, the Agent panel's
  //   own rows) and its ▶ Runs (codeRuns.svelte.js) — each with its clock and
  //   Stop, then "N finished".
  //
  // Context and Changes are read when it opens and again when a turn ends.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, untrack, getContext } from 'svelte';
  import { agents as agentsStore } from '../../stores/agents.svelte.js';
  import { agentLogs, watchAgentLog } from '../../stores/agentLogs.svelte.js';
  import { planUsage } from '../../stores/claudePlanUsage.svelte.js';
  import { panelRows, rowActions, ctxPercent, fmtClock } from '../../core/agentPanel.js';
  import { percentOf } from '../../core/aiUsage.js';
  import { codeRuns, loadRuns, stopRun } from './codeRuns.svelte.js';
  import { outputItems } from './outputs.js';
  import { openOutput as open } from './outputsOpen.js';
  import { made } from './made.svelte.js';
  import { listen } from '@tauri-apps/api/event';
  import { CODE_FILES } from '../../ui/fileLinks.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { openExternalLink } from '../../core/externalLinks.js';
  import ConfirmDialog from '../../ui/ConfirmDialog.svelte';
  import OutputsList from './OutputsList.svelte';
  import { outputMenuItems, outputPath, outputRefs } from './outputsOpen.js';
  import { contextView, fmtTokens, PR_STATE, PR_CHECKS, prsNote } from './contextBreakdown.js';
  import { REMOTE_ASK, remoteStatus, remoteOn, toggleRemote, remoteOnEverywhere } from './remoteControl.js';

  /** `doc`: the panel's state; `conversationId`: the Code chat (null: a new one); `workspace`: its folder's path; `messages`: its thread. */
  let { doc, conversationId = null, workspace = null, messages = [] } = $props();
  const files = getContext(CODE_FILES);

  let env = $state(null);
  let connectors = $state(null);
  let ringUse = $state(null);
  let now = $state(Date.now() / 1000);
  /** Claude Code's `/context` for the chat's session (agents/context.rs `Breakdown`), or null. */
  let breakdown = $state(null);
  /** The branch's pull requests (agents/prs.rs `Prs`), or null until read. */
  let prs = $state(null);
  /** The workspace's Remote Control (`link_remote_status`), or null. */
  let remote = $state(null);
  let askingRemote = $state(false);
  /** The chat's agents as they stood (a turn's end reads Context and Changes again). */
  let lastTurns = null;

  onMount(() => {
    const t = setInterval(() => (now = Date.now() / 1000), 1000);
    let un = null;
    let gone = false;
    listen('yap-link-changed', () => loadRemote()).then((u) => (gone ? u() : (un = u)));
    return () => {
      gone = true;
      clearInterval(t);
      un?.();
    };
  });

  async function loadRemote() {
    const name = env?.name;
    remote = name ? await remoteStatus(name) : null;
  }

  /** The context's breakdown (`/context`: run only once a turn has ended, never while one works). */
  async function loadBreakdown() {
    const conv = conversationId;
    if (conv == null) {
      breakdown = null;
      return;
    }
    let b = null;
    try {
      b = await invoke('code_chat_context', { conversation: conv });
    } catch {
      b = null;
    }
    if (conv === conversationId) breakdown = b ?? null;
  }

  /** The branch's pull requests; `fresh` after a turn's end (else kept 30 s). */
  async function loadPrs(fresh) {
    const conv = conversationId;
    const ws = workspace;
    let got = null;
    try {
      got = await invoke('code_chat_pull_requests', { conversation: conv ?? null, workspace: ws ?? null, fresh: !!fresh });
    } catch {
      got = null;
    }
    if (conv === conversationId && ws === workspace) prs = got;
  }

  async function load() {
    const conv = conversationId;
    lastTurns = null;
    breakdown = null;
    prs = null;
    try {
      env = await invoke('code_chat_environment', { conversation: conv ?? null, workspace: workspace ?? null });
    } catch {
      env = null;
    }
    loadRemote();
    loadPrs(false);
    loadBreakdown();
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

  // **A turn's end**: Context and Changes read again (Claude may have pushed a branch or opened a pull request).
  const AT_WORK = ['queued', 'running', 'asking'];
  const turnsKey = $derived(chatAgents.map((a) => `${a.id}:${a.status}:${a.turns ?? 0}:${a.endedTs ?? 0}`).join('|'));
  $effect(() => {
    const k = turnsKey;
    untrack(() => {
      if (lastTurns == null || k === lastTurns) {
        lastTurns = k;
        return;
      }
      lastTurns = k;
      if (chatAgents.some((a) => AT_WORK.includes(a.status))) return;
      loadBreakdown();
      loadPrs(true);
    });
  });
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

  const ctxView = $derived(breakdown ? contextView(breakdown) : null);

  // (And the pages it keeps, the documents it wrote: chat/made/pages.rs, chat/documents.rs.)
  const outputs = $derived(outputItems({ agents: chatAgents, messages, made: made.of(conversationId) }));
  const openOutput = (it) => open(it, { doc, conversationId });

  // Which outputs are files in the workspace (`code_files_check`: Rust says, from the conversation's own
  // workspace): their menu's Show in folder, Copy path and Open in editor.
  let refs = $state({});
  const outputsKey = $derived(outputs.map(outputPath).filter(Boolean).join('|'));
  $effect(() => {
    void outputsKey;
    const conv = conversationId;
    untrack(async () => {
      const got = await outputRefs(conv, outputs);
      if (conv === conversationId) refs = got;
    });
  });
  function outputMenu(e, it) {
    const p = outputPath(it);
    openContextMenu(e, outputMenuItems(it, { doc, conversationId, ref: p ? refs[p] || null : null }), { label: it.name || 'Output' });
  }

  async function toggleRc() {
    if (!remote || !env?.name) return;
    try {
      // (Settings → Phone's switch off: asked once, with what it allows.)
      if ((await toggleRemote(env.name, remote)) === 'ask') {
        askingRemote = true;
        return;
      }
    } catch (e) {
      toast({ title: "Couldn't change Remote Control", description: String(e?.message ?? e), variant: 'destructive' });
    }
    loadRemote();
  }
  async function rcEverywhere() {
    try {
      await remoteOnEverywhere(env.name);
    } catch (e) {
      toast({ title: "Couldn't turn Remote Control on", description: String(e?.message ?? e), variant: 'destructive' });
    }
    askingRemote = false;
    setTimeout(loadRemote, 400);
  }

  const prsLine = $derived(prsNote(prs));

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
        {#if remote}
          <!-- Remote Control for this workspace: the "Code · …" card's own switch (remoteControl.js). -->
          <div class="srow" data-testid="chat-summary-remote">
            <svg class="si" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="7" y="2.5" width="10" height="19" rx="2.2" /><path d="M11 18.5h2" /></svg>
            <span class="sv">Remote Control <span class="sdim">· {remoteOn(remote) ? 'your paired phones can use it' : remote.on ? 'off for this workspace' : 'off'}</span></span>
            <button type="button" class="switch" role="switch" aria-checked={remoteOn(remote)} aria-label={`Remote Control for ${env.name}`} data-testid="chat-summary-remote-switch" onclick={toggleRc}><span class="knob" aria-hidden="true"></span></button>
          </div>
        {/if}
      {:else}
        <p class="snote">…</p>
      {/if}
    </section>

    {#if prs && prsLine !== null}
      <section class="sec" data-testid="chat-summary-changes">
        <h3 class="sh">Changes</h3>
        {#each prs.list as p (p.url || p.number)}
          <div class="prrow" data-testid="chat-summary-pr" data-state={p.state} data-checks={p.checks}>
            <button type="button" class="propen" onclick={() => openExternalLink(p.url)} title={`Open ${p.repo} #${p.number} in your browser`}>
              <svg class="pricon" data-state={p.state} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                {#if p.state === 'merged'}
                  <circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="13" r="2" /><path d="M6 7v10M6 7c0 4 4 6 10 6" />
                {:else if p.state === 'closed'}
                  <circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="19" r="2" /><path d="M6 7v10M16 4l4 4M20 4l-4 4M18 11v6" />
                {:else}
                  <circle cx="6" cy="5" r="2" /><circle cx="6" cy="19" r="2" /><circle cx="18" cy="19" r="2" /><path d="M6 7v10M18 17V9a3 3 0 0 0-3-3h-4M13 3.5 10.5 6 13 8.5" />
                {/if}
              </svg>
              <span class="prtext">
                <span class="prtitle">{p.title || `Pull request #${p.number}`}</span>
                <span class="prmeta">
                  <span class="mono">{p.repo} #{p.number}</span>
                  <span class="prstate" data-state={p.state}>{PR_STATE[p.state] || p.state}</span>
                  {#if PR_CHECKS[p.checks]}
                    <span class="prchecks" data-checks={p.checks}>
                      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                        {#if p.checks === 'passing'}<path d="m5 12 4 4L19 6" />{:else if p.checks === 'failing'}<path d="M6 6l12 12M18 6 6 18" />{:else}<circle cx="12" cy="12" r="7" stroke-dasharray="4 3" />{/if}
                      </svg>
                      {PR_CHECKS[p.checks]}
                    </span>
                  {/if}
                </span>
              </span>
            </button>
            {#if p.state === 'merged' && conversationId != null && files?.archive}
              <button type="button" class="prarchive" data-testid="chat-summary-pr-archive" onclick={() => files.archive(conversationId)} title="Archive this chat: its pull request is merged">Archive</button>
            {/if}
          </div>
        {/each}
        {#if prsLine}
          <p class="snote" data-testid="chat-summary-changes-note">{prsLine}</p>
        {/if}
      </section>
    {/if}

    <section class="sec" data-testid="chat-summary-context">
      <h3 class="sh">Context</h3>
      {#if ctxView}
        <!-- Claude Code's own /context for this chat's session (agents/context.rs), drawn as the Claude app draws it. -->
        <p class="ctxhead" data-testid="chat-summary-context-head">{ctxView.label}</p>
        <div class="ctxbar multi" role="img" aria-label={`${ctxView.pct}% of the context used`}>
          {#each ctxView.segments as g (g.key)}
            <span class="seg" data-colour={g.colour} style:width={`${g.width}%`} style:background={`var(--yap-ctx-${g.colour})`}></span>
          {/each}
        </div>
        <ul class="cats">
          {#each ctxView.rows as r (r.key)}
            <li class="cat" data-testid="chat-summary-context-cat" data-name={r.name}>
              <span class="cdot" class:hollow={r.kind === 'free'} style:background={r.kind === 'free' ? undefined : `var(--yap-ctx-${r.colour})`} aria-hidden="true"></span>
              <span class="cn">{r.name}</span>
              <span class="ct">{fmtTokens(r.tokens)}</span>
              <span class="cp">{r.share}</span>
            </li>
          {/each}
        </ul>
        {#if ctxView.deferred.length}
          <ul class="cats deferred">
            {#each ctxView.deferred as r (r.key)}
              <li class="cat" data-testid="chat-summary-context-deferred" data-name={r.name} title="Loaded only when it's looked for: not in the context until then">
                <span class="cdot hollow" aria-hidden="true"></span>
                <span class="cn">{r.name}</span>
                <span class="ct">{fmtTokens(r.tokens)}</span>
                <span class="cp"></span>
              </li>
            {/each}
          </ul>
        {/if}
        {#each ctxView.lists as l (l.key)}
          <details class="clist" data-testid="chat-summary-context-list" data-list={l.key}>
            <summary>
              <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
              <span class="cn">{l.title}</span>
              <span class="ccount">{l.count}</span>
            </summary>
            <ul>
              {#each l.rows as row (row.key)}
                <li class="crow">
                  <span class="c1" title={row.title || row.cells[0]}>{row.cells[0]}</span>
                  <span class="c2">{row.cells[1]}</span>
                  <span class="c3">{row.cells[2]}</span>
                </li>
              {/each}
            </ul>
          </details>
        {/each}
      {:else if context}
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
        <OutputsList items={outputs} onopen={openOutput} onmenu={outputMenu} />
      {:else}
        <p class="snote">Nothing made, changed or shared yet.</p>
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

<ConfirmDialog bind:open={askingRemote} title={REMOTE_ASK.title} body={REMOTE_ASK.body} confirmLabel={REMOTE_ASK.confirmLabel} danger={false} onconfirm={rcEverywhere} />

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

  /* Remote Control's switch: the "Code · …" card's. */
  .switch {
    flex: none;
    position: relative;
    width: 34px;
    height: 20px;
    padding: 0;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    cursor: pointer;
  }
  .switch[aria-checked='true'] {
    border-color: var(--yap-code);
    background: var(--yap-code);
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: var(--yap-r-full);
    background: var(--yap-paper);
    transition: transform 0.12s ease;
  }
  .switch[aria-checked='true'] .knob {
    transform: translateX(14px);
  }
  .switch:focus-visible,
  .propen:focus-visible,
  .prarchive:focus-visible,
  .clist summary:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  @media (prefers-reduced-motion: reduce) {
    .knob {
      transition: none;
    }
  }

  /* Changes: the branch's pull requests. */
  .prrow {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .propen {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    flex: 1 1 auto;
    min-width: 0;
    padding: 5px 6px;
    border: none;
    border-radius: 7px;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .propen:hover {
    background: var(--yap-paper-s2);
  }
  .pricon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    margin-top: 1px;
    color: var(--yap-success);
  }
  .pricon[data-state='draft'] {
    color: var(--yap-paper-muted);
  }
  .pricon[data-state='merged'] {
    color: var(--yap-pr-merged);
  }
  .pricon[data-state='closed'] {
    color: var(--yap-danger);
  }
  .prtext {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .prtitle {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
  }
  .prmeta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px 8px;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
  .prmeta .mono {
    font-size: 11px;
  }
  .prstate[data-state='open'] {
    color: var(--yap-success-text);
  }
  .prstate[data-state='merged'] {
    color: var(--yap-pr-merged);
  }
  .prstate[data-state='closed'] {
    color: var(--yap-danger);
  }
  .prchecks {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }
  .prchecks svg {
    width: 11px;
    height: 11px;
  }
  .prchecks[data-checks='passing'] {
    color: var(--yap-success-text);
  }
  .prchecks[data-checks='failing'] {
    color: var(--yap-danger);
  }
  .prchecks[data-checks='running'] {
    color: var(--yap-warning);
  }
  .prarchive {
    flex: 0 0 auto;
    padding: 3px 9px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: none;
    color: var(--yap-paper-text-2);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }
  .prarchive:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }

  /* Context, broken down: the header, a bar by category, the rows, the lists. */
  .ctxhead {
    margin: 0 0 7px;
    color: var(--yap-paper-text-2);
    font-variant-numeric: tabular-nums;
  }
  .ctxbar.multi {
    display: flex;
    height: 7px;
    margin-bottom: 8px;
  }
  .ctxbar.multi .seg {
    flex: 0 0 auto;
    height: 100%;
    border-radius: 0;
  }
  .ctxbar.multi .seg + .seg {
    box-shadow: inset 1px 0 0 var(--yap-paper);
  }
  .cats {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .cats.deferred {
    margin-top: 4px;
    padding-top: 4px;
    border-top: 1px dashed var(--yap-paper-rule);
  }
  .cat,
  .clist summary,
  .crow {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 22px;
  }
  .cdot {
    flex: 0 0 auto;
    width: 8px;
    height: 8px;
    border-radius: 2px;
  }
  .cdot.hollow {
    box-sizing: border-box;
    border: 1px solid var(--yap-paper-muted);
    background: none;
  }
  .cn {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ct,
  .cp,
  .ccount,
  .c3 {
    flex: 0 0 auto;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .ct {
    min-width: 40px;
  }
  .cp {
    min-width: 42px;
  }
  .deferred .cn {
    color: var(--yap-paper-muted);
  }
  .clist {
    margin-top: 2px;
  }
  .clist summary {
    list-style: none;
    padding: 0 2px;
    border-radius: 6px;
    cursor: pointer;
  }
  .clist summary::-webkit-details-marker {
    display: none;
  }
  .clist summary:hover {
    background: var(--yap-paper-s2);
  }
  .chev {
    flex: 0 0 auto;
    width: 12px;
    height: 12px;
    color: var(--yap-paper-muted);
    transition: transform 0.12s ease;
  }
  .clist[open] .chev {
    transform: rotate(90deg);
  }
  .clist ul {
    margin: 2px 0 6px 20px;
    padding: 0;
    list-style: none;
  }
  .crow {
    min-height: 20px;
    font-size: 11.5px;
  }
  .c1 {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11px;
  }
  .c2 {
    flex: 0 1 auto;
    min-width: 0;
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-paper-muted);
  }
  @media (prefers-reduced-motion: reduce) {
    .chev {
      transition: none;
    }
  }
</style>
