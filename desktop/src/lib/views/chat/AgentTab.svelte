<script>
  // **An agent's tab** beside the chat (Yap Code: desktop/src-tauri/src/agents.rs,
  // chatDoc.svelte.js `agent`): what the person's own Claude Code is doing in
  // their folder, as it does it — fed by `agent_get` (the agent and its log, to
  // replay) and the app-wide `yap-agent` event (live). One tab an agent; closing
  // it never stops the agent. Two views:
  // - **The panel** (AgentPanel.svelte, 2026-11-08, after savvy-progress's
  //   agents panel): its totals, and its turns as rows in groups (Needs you,
  //   Running, Finished, Queued), each opening in place. What the tab opens
  //   on — a pill's or a report card's turn opened, in view.
  // - **The timeline** (a row's Open; Back to the panel), as it read
  //   before:
  // - The folder and its status; the mode and model; how long it has run.
  // - **Its turns** (2026-11-07: one agent per Code chat, each follow-up a turn
  //   of its own on the same Claude Code session): a section each, its
  //   request as its header with its status, cost and time, oldest first —
  //   the newest open (and the one a pill or a report card asked for), the
  //   earlier folded. A request waiting behind the turn working: "Queued".
  //   Each turn's section (AgentTurn.svelte): the line it's on, its to-do
  //   list, its timeline, the files it changed with their diffs and **Undo
  //   this turn**, the commands it ran, what it wasn't allowed to do, its
  //   final words.
  // - Its cost in all, told apart by how it's paid ("Your Claude plan · ≈
  //   $0.04 on the API" / "API key · $0.04").
  // - A problem with Claude Code itself in plain words, with one button
  //   (Sign in, when it isn't signed in).
  // - Stop (while it works), Continue (stopped, interrupted, failed), Undo
  //   agent — **Undo all** once it has turns, back to before its first (a
  //   confirm, then what was put back; never when no backups were made:
  //   Claude Code's hooks didn't run, and the tab says so), Open in terminal
  //   (once it isn't running), and "Tell the agent…": the person's own words,
  //   its next turn (queued while it works: `agent_message`).
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, tick, untrack } from 'svelte';
  import ConfirmDialog from '../../ui/ConfirmDialog.svelte';
  import { toast } from '../../ui/toast.svelte.js';
  import {
    isLive,
    statusWord,
    statusTone,
    costLine,
    money,
    elapsed,
    workedFor,
    turnsOf,
    turnStatusWord,
    problemWords,
    modeLabel,
    modelLabel,
    canUndo as undoable,
    canUndoTurn,
  } from '../../core/agents.js';
  import { EFFORT_LABELS } from '../../core/effort.js';
  import { claudePlan, signInClaudePlan } from '../../stores/claudePlan.svelte.js';
  import { agents as agentsStore } from '../../stores/agents.svelte.js';
  import AgentTurn from './AgentTurn.svelte';
  import AgentPanel from './AgentPanel.svelte';

  /** `doc`: the panel's state (its `agent`, a AgentRef, `turn` the one asked for); `narrow`: no room beside the chat (no expand). */
  let { doc, narrow = false } = $props();

  const ref = $derived(doc.agent);
  let agent = $state(null);
  let log = $state([]);
  let gone = $state(false);
  let now = $state(Date.now() / 1000);
  let busy = $state('');
  let message = $state('');
  let confirmUndo = $state(false);
  let undone = $state(null); // what Undo all put back
  let undoBtn = $state(null);
  // Each turn as the person left it: open or folded (n → bool), what its
  // Undo put back or why it couldn't (n → words).
  let folded = $state({});
  let turnUndone = $state({});
  let turnRefused = $state({});
  let bodyEl = $state(null);
  // Which view: 'panel' (its rows) or 'timeline' (a row's Open); the row a
  // pill or a report card asked for (`{key, seq}`).
  let view = $state('panel');
  let want = $state(null);
  // The Code chat's other agents (a fresh one beside its first): their logs,
  // read once each and followed (id → events).
  let otherLogs = $state({});

  // The agent as it is now: its saved record, else what the tab was opened with.
  const shown = $derived(agent ?? (ref ? { ...ref, files: [], commands: [], denials: [] } : null));
  const live = $derived(isLive(shown?.status));
  const turns = $derived(turnsOf(shown, log));
  const many = $derived(turns.length > 1);
  /**
   * A request waiting its turn: queued behind the turn working, or never
   * sent (the agent stopped first: Continue sends it). Shown as a line, not
   * a section of steps.
   */
  const waiting = (t) =>
    !t.startedTs &&
    t.n !== turns[0]?.n &&
    (t.status === 'queued' ? turns.some((o) => o.n !== t.n && (o.status === 'running' || o.status === 'asking')) : t.status === 'stopped' || t.status === 'interrupted');
  // The turn working now (or the newest): open unless folded.
  const current = $derived([...turns].reverse().find((t) => !waiting(t))?.n ?? turns.at(-1)?.n);
  const problem = $derived(problemWords(shown?.problem));
  // How long it has worked: its turns added up (not the time between them).
  const ran = $derived(shown ? workedFor(shown, now) : '');
  const canUndo = $derived(undoable(shown));

  const isOpen = (t) => (folded[t.n] ?? t.n !== current) === false;
  /** The Code chat's agents for the panel: this one first, then the chat's others. */
  const others = $derived(shown?.conversation == null ? [] : agentsStore.list.filter((j) => j.conversation === shown.conversation && j.id !== shown.id));
  const panelAgents = $derived(shown ? [shown, ...others] : []);
  const panelLogs = $derived({ ...otherLogs, ...(shown ? { [shown.id]: log } : {}) });
  const turnLive = (t) => live && t.n === current;

  // Events heard while the log is read: kept, unless the log has them already.
  let loading = false;
  let heard = [];

  async function load(id) {
    loading = true;
    heard = [];
    try {
      const r = await invoke('agent_get', { id });
      if (doc.agent?.id !== id) return;
      const saved = r.log || [];
      const tail = new Set(saved.slice(-50).map((e) => JSON.stringify(e)));
      agent = r.agent;
      log = [...saved, ...heard.filter((e) => !tail.has(JSON.stringify(e)))];
      gone = false;
    } catch {
      if (doc.agent?.id === id) gone = true;
    } finally {
      loading = false;
      heard = [];
    }
  }

  // Another agent in the tab: read it.
  let loadedFor = null;
  $effect(() => {
    const id = ref?.id;
    untrack(() => {
      if (!id || id === loadedFor) return;
      loadedFor = id;
      agent = null;
      log = [];
      undone = null;
      message = '';
      folded = {};
      turnUndone = {};
      turnRefused = {};
      otherLogs = {};
      load(id);
    });
  });

  // The chat's other agents: each one's log read once (live events add to it).
  $effect(() => {
    const ids = others.map((j) => j.id);
    untrack(() => {
      for (const id of ids) {
        if (otherLogs[id]) continue;
        otherLogs = { ...otherLogs, [id]: [] };
        invoke('agent_get', { id })
          .then((r) => (otherLogs = { ...otherLogs, [id]: [...(r?.log || []), ...(otherLogs[id] || [])] }))
          .catch(() => {});
      }
    });
  });

  // A new turn begins (a follow-up, however it was sent): it opens, and the
  // earlier ones fold again.
  let lastCurrent = null;
  $effect(() => {
    const c = current;
    const loaded = !!agent;
    untrack(() => {
      if (!loaded) {
        lastCurrent = null;
        return;
      }
      if (lastCurrent != null && c > lastCurrent) folded = {};
      lastCurrent = c;
    });
  });

  // Shown again (a pill, a report card, a turn's own start, the bar's Open):
  // the panel, with that turn's row open and in view — or, from a row's
  // Open on another agent of the chat (`view: 'timeline'`), its timeline.
  $effect(() => {
    const turn = ref?.turn;
    const seq = ref?.seq;
    const id = ref?.id;
    const at = ref?.view;
    untrack(() => {
      if (at === 'timeline') {
        showTimeline(turn);
        return;
      }
      view = 'panel';
      if (turn != null && id) want = { key: `${id}:${turn}`, seq };
    });
  });

  /** **A row's Open**: the timeline at its turn (another agent of the chat: its own tab, there). */
  function openRow(r) {
    if (r.agentId !== shown?.id) {
      const j = agentsStore.list.find((a) => a.id === r.agentId);
      doc.showAgent({ ...(j ? { id: j.id, folder: j.folder, status: j.status, mode: j.mode, conversation: j.conversation } : { id: r.agentId }), turn: r.n, view: 'timeline' });
      return;
    }
    showTimeline(r.n);
  }

  /** The timeline, turn `n` open and in view (the newest, with none). */
  async function showTimeline(n) {
    view = 'timeline';
    if (n != null) folded = { [n]: false };
    await tick();
    const el = n != null ? bodyEl?.querySelector(`[data-testid="chat-agent-turn"][data-turn="${n}"]`) : null;
    el?.scrollIntoView?.({ block: 'start' });
  }

  /** One `yap-agent` event: into the log; the agent as it is now (read again when its status turns, or a turn is new to it). */
  function onAgent(ev) {
    if (!ev?.id) return;
    // What a turn has used so far (agents/usage.rs): on its note, never a line of the log.
    const usageOnly = !!ev.usage && !ev.step && !ev.now && !ev.text && !ev.todos && !ev.said && typeof ev.cost !== 'number';
    if (ev.id !== doc.agent?.id) {
      // Another agent of the chat (the panel's rows): its log, as it goes.
      if (otherLogs[ev.id] && !usageOnly) otherLogs = { ...otherLogs, [ev.id]: [...otherLogs[ev.id], ev] };
      return;
    }
    if (!usageOnly) {
      if (loading) heard.push(ev);
      log = [...log, ev];
    }
    if (agent) {
      const was = agent.status;
      const next = { ...agent, status: ev.status ?? agent.status };
      if (ev.file && !(agent.files || []).includes(ev.file)) next.files = [...(agent.files || []), ev.file];
      if (ev.command) next.commands = [...(agent.commands || []), ev.command];
      if (typeof ev.cost === 'number') next.cost = ev.cost;
      if (ev.problem) next.problem = ev.problem;
      // This turn's files as they come (its note says them once read again).
      if (ev.file && Number.isInteger(ev.turn) && next.turnList?.length) {
        next.turnList = next.turnList.map((t) => (t.n === ev.turn && !(t.files || []).includes(ev.file) ? { ...t, files: [...(t.files || []), ev.file] } : t));
      }
      // Its turn's to-do list and what it has used, as they come.
      if ((ev.todos || ev.usage) && Number.isInteger(ev.turn) && next.turnList?.length) {
        next.turnList = next.turnList.map((t) => (t.n === ev.turn ? { ...t, ...(ev.todos ? { todos: ev.todos } : {}), ...(ev.usage ? { usage: ev.usage } : {}) } : t));
      }
      agent = next;
      const known = !Number.isInteger(ev.turn) || (agent.turnList || []).some((t) => t.n === ev.turn);
      if (was !== next.status || !known || ev.queued) refreshSoon();
    } else {
      refreshSoon();
    }
  }

  let refreshTimer = null;
  function refreshSoon() {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(async () => {
      const id = doc.agent?.id;
      if (!id) return;
      try {
        const r = await invoke('agent_get', { id });
        if (doc.agent?.id === id) agent = r.agent;
      } catch {
        /* keep */
      }
    }, 120);
  }

  onMount(() => {
    const uns = [listen('yap-agent', (e) => onAgent(e.payload))];
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
      toast({ title: `Couldn't ${what} the agent`, description: String(e?.message ?? e), variant: 'destructive' });
      return null;
    } finally {
      busy = '';
    }
  }

  async function stop() {
    const r = await act('stop', 'agent_stop');
    if (r && typeof r === 'object' && r.id) agent = { ...agent, ...r };
  }

  async function carryOn() {
    undone = null;
    const r = await act('continue', 'agent_continue');
    if (r && typeof r === 'object' && r.id) agent = { ...agent, ...r };
  }

  async function openTerminal() {
    await act('open', 'agent_open_terminal');
  }

  // Claude Code's own sign-in, in a terminal (the Claude tab's, stores/claudePlan.svelte.js).
  async function signIn() {
    await signInClaudePlan();
    if (claudePlan.failed) toast({ title: "Couldn't open Claude Code's sign-in", description: claudePlan.failed, variant: 'destructive' });
  }

  async function undo() {
    const r = await act('undo', 'agent_undo');
    confirmUndo = false;
    if (!r || typeof r !== 'object') return;
    undone = r;
    turnRefused = {};
    refreshSoon();
    tick().then(() => undoBtn?.focus?.());
  }

  /** **Undo this turn**: its files back as they were before it — refused while a later turn changed one of them. */
  async function undoTurn(t) {
    if (!shown || busy) return;
    busy = 'undo';
    turnRefused = { ...turnRefused, [t.n]: '' };
    try {
      const r = await invoke('agent_undo_turn', { id: shown.id, turn: t.n });
      turnUndone = { ...turnUndone, [t.n]: undoneLine(r) };
      refreshSoon();
    } catch (e) {
      turnRefused = { ...turnRefused, [t.n]: String(e?.message ?? e) };
    } finally {
      busy = '';
    }
  }

  async function send() {
    const text = message.trim();
    if (!text || busy) return;
    const r = await act('tell', 'agent_message', { text });
    if (!r) return;
    message = '';
    if (typeof r === 'object' && r.id) agent = { ...agent, ...r };
  }

  const shortPath = (p) => String(p || '').split(/[\\/]/).pop() || String(p || '');
  const undoneLine = (r) => {
    const back = (r.restored?.length ?? 0) + (r.deleted?.length ?? 0);
    if (!back) return 'Nothing needed putting back.';
    const parts = [];
    if (r.restored?.length) parts.push(`${r.restored.map(shortPath).join(', ')} put back`);
    if (r.deleted?.length) parts.push(`${r.deleted.map(shortPath).join(', ')} deleted (the agent made ${r.deleted.length === 1 ? 'it' : 'them'})`);
    return `Undone: ${parts.join('; ')}.`;
  };
  const effortWord = (e) => EFFORT_LABELS[e] || e || '';
  /** A turn's header line after its title: its status, how long, its cost. */
  const turnFacts = (t) => {
    const parts = [turnStatusWord(t)];
    const took = t.startedTs ? elapsed(t, now) : '';
    if (took) parts.push(isLive(t.status) ? `${took} so far` : took);
    if (Number(t.cost) > 0) parts.push(shown?.pay === 'key' ? money(t.cost) : `≈ ${money(t.cost)}`);
    return parts.join(' · ');
  };
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="agenttab" aria-label="Agent" data-testid="chat-agent" data-status={shown?.status ?? ''} data-turns={turns.length}>
  <header class="agenthead">
    <svg class="agenticon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" /></svg>
    <h2 class="agenttitle" title={shown?.path || shown?.folder}>{shown?.folder || 'Agent'}</h2>
    {#if shown}
      <span class="status tone-{statusTone(shown.status)}" data-testid="chat-agent-status"><span class="dot" aria-hidden="true"></span>{statusWord(shown.status)}</span>
    {/if}
    <div class="agentactions">
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
      <button type="button" class="jbtn" onclick={() => doc.close()} aria-label="Close the panel" title="Close (the agent carries on)">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  </header>

  {#if gone}
    <p class="jgone">This agent isn't in Yap any more.</p>
  {:else if shown}
    <div class="agentbody" bind:this={bodyEl} data-view={view}>
      {#if view === 'panel'}
        {@render notices()}
        <AgentPanel agent={shown} agents={panelAgents} logs={panelLogs} steps={agentsStore.steps} {now} {want} onopen={openRow} />
        {#if undone}
          <p class="undone" aria-live="polite" data-testid={many ? 'chat-agent-undone-all' : 'chat-agent-undone'}>{undoneLine(undone)}</p>
        {/if}
      {:else}
        <button type="button" class="back" data-testid="chat-agent-back" onclick={() => (view = 'panel')} aria-label="Back to the agent's turns">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m15 18-6-6 6-6" /></svg>
          Back
        </button>
        <p class="meta" data-testid="chat-agent-meta">
          <span>{modeLabel(shown.mode)}</span>
          {#if shown.model}<span>· {modelLabel(shown.model)}{shown.effort ? ` ${effortWord(shown.effort)}` : ''}</span>{/if}
          {#if ran}<span data-testid="chat-agent-elapsed">· {live ? `${ran} so far` : `ran ${ran}`}</span>{/if}
          {#if many}<span data-testid="chat-agent-turn-count">· {turns.length} turns</span>{/if}
        </p>
        {#if shown.path}<p class="path" title={shown.path}>{shown.path}</p>{/if}
        <div class="tnotices">{@render notices()}</div>

        <!-- Its turns: each request and what came of it, the newest open. -->
        {#each turns as t (t.n)}
          {@const open = isOpen(t)}
          {@const queued = waiting(t)}
          <section class="turn" class:open class:queued data-testid="chat-agent-turn" data-turn={t.n} data-status={t.status ?? ''} data-open={open}>
            {#if queued}
              <div class="thead" data-testid="chat-agent-turn-head">
                <span class="tdot tone-idle" aria-hidden="true"></span>
                <span class="ttitle" title={t.request || t.title}>{t.title || 'Your next request'}</span>
                <span class="tfacts">{turnStatusWord(t)}</span>
              </div>
              <p class="tqueued">{t.status === 'queued' ? 'It goes to the agent once it finishes this step.' : 'Not sent: the agent stopped first. Continue sends it.'}</p>
            {:else}
              <button
                type="button"
                class="thead"
                aria-expanded={open}
                data-testid="chat-agent-turn-head"
                title={t.request || t.title}
                onclick={() => (folded = { ...folded, [t.n]: open })}
              >
                <svg class="tchev" class:down={open} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
                <span class="tdot tone-{statusTone(t.status)}" aria-hidden="true"></span>
                <span class="ttitle">{t.title || shown.folder}</span>
                <span class="tfacts" data-testid="chat-agent-turn-facts">{turnFacts(t)}</span>
              </button>
              {#if open}
                <div class="tbody">
                  <AgentTurn
                    agent={shown}
                    turn={t}
                    live={turnLive(t)}
                    single={!many}
                    undoable={many && canUndoTurn(shown, t)}
                    undone={turnUndone[t.n] || (!many && undone ? undoneLine(undone) : '')}
                    refused={turnRefused[t.n] || ''}
                    busy={!!busy}
                    onundo={() => undoTurn(t)}
                  />
                </div>
              {/if}
            {/if}
          </section>
        {/each}

        {#if many && undone}
          <p class="undone" aria-live="polite" data-testid="chat-agent-undone-all">{undoneLine(undone)}</p>
        {/if}

        <p class="cost" data-testid="chat-agent-cost" data-pay={shown.pay || 'plan'}>{costLine(shown)}{many && Number(shown.cost) > 0 ? ' in all' : ''}</p>
      {/if}
    </div>

    <footer class="agentfoot">
      <div class="jbuttons">
        {#if live}
          <button type="button" class="pill" onclick={stop} disabled={!!busy}>Stop</button>
        {:else if shown.status === 'stopped' || shown.status === 'interrupted' || shown.status === 'failed'}
          <button type="button" class="pill primary" onclick={carryOn} disabled={!!busy}>Continue</button>
        {/if}
        {#if canUndo}
          <button type="button" class="pill" bind:this={undoBtn} onclick={() => (confirmUndo = true)} disabled={!!busy}>{many ? 'Undo all' : "Undo the agent's changes"}</button>
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
        <input type="text" bind:value={message} placeholder={live ? 'Tell the agent (it goes next)…' : 'Tell the agent…'} aria-label="Tell the agent" maxlength="4000" autocomplete="off" />
        <!-- Named apart from Chat's own Send (the message box's). -->
        <button type="submit" class="tsend" disabled={!message.trim() || !!busy} aria-label="Tell the agent" title="Tell the agent (Enter)">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 12h14M13 6l6 6-6 6" /></svg>
        </button>
      </form>
    </footer>
  {/if}
</section>

{#snippet notices()}
  <!-- Why it's a new agent, and a problem with Claude Code: in both views. -->
  {#if shown.whyNew}<p class="whynew" data-testid="chat-agent-why-new">{shown.whyNew}</p>{/if}
  {#if problem}
    <div class="problem" role="alert" data-testid="chat-agent-problem" data-kind={shown.problem.kind}>
      <p class="pline">{problem.line}</p>
      {#if problem.check}<p class="pcheck">{problem.check}</p>{/if}
      {#if problem.action === 'signIn'}
        <button type="button" class="pill primary" onclick={signIn}>Sign in</button>
      {/if}
    </div>
  {:else if shown.status === 'failed' && shown.error}
    <div class="problem" role="alert" data-testid="chat-agent-problem">
      <p class="pline">{shown.error}</p>
    </div>
  {/if}
{/snippet}

<ConfirmDialog
  bind:open={confirmUndo}
  title={many ? 'Undo every turn of this agent?' : "Undo the agent's changes?"}
  body={many
    ? 'Every file it changed, in every turn, goes back to how it was before its first turn, and the files it made are deleted. Yap kept their backups for this.'
    : 'Every file it changed goes back to how it was before it, and the files it made are deleted. Yap kept their backups for this.'}
  confirmLabel={many ? 'Undo all' : "Undo the agent's changes"}
  busyLabel="Undoing…"
  busy={busy === 'undo'}
  returnFocus={undoBtn}
  onconfirm={undo}
/>

<style>
  .agenttab {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    color: var(--yap-paper-ink);
  }
  .agenthead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 8px 18px;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .agenticon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-primary);
  }
  .agenttitle {
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
  .agentactions {
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
  .agentbody {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 12px 18px 16px;
    font-size: 13px;
    line-height: 1.5;
  }
  .tnotices > :global(:first-child) {
    margin-top: 10px;
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    margin: -4px 0 8px -6px;
    padding: 2px 8px 2px 4px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-text-2);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .back:hover {
    background: var(--yap-paper-s1);
    color: var(--yap-paper-ink);
  }
  .back:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .back svg {
    width: 13px;
    height: 13px;
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
    margin: 0 0 12px;
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
  .whynew {
    margin: 0 0 10px;
    padding: 7px 10px;
    border-radius: 9px;
    background: var(--yap-paper-s1);
    color: var(--yap-paper-text-2);
    font-size: 12.5px;
  }
  /* A turn: its request as a header, its steps under it when open. */
  .turn {
    margin: 14px 0 0;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 10px;
    background: var(--yap-paper);
  }
  .turn.open {
    border-color: color-mix(in srgb, var(--yap-primary) 22%, var(--yap-paper-rule));
  }
  .turn.queued {
    border-style: dashed;
    background: var(--yap-paper-s1);
  }
  .thead {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    box-sizing: border-box;
    min-height: 34px;
    padding: 6px 10px;
    border: none;
    border-radius: 10px;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
  }
  button.thead {
    cursor: pointer;
  }
  button.thead:hover {
    background: var(--yap-paper-s1);
  }
  button.thead:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .tchev {
    flex: 0 0 auto;
    width: 13px;
    height: 13px;
    color: var(--yap-paper-muted);
    transition: transform 0.15s ease;
  }
  .tchev.down {
    transform: rotate(90deg);
  }
  :global(:root[data-motion='reduced']) .tchev {
    transition: none;
  }
  .tdot {
    flex: 0 0 auto;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--yap-paper-muted);
  }
  .tdot.tone-live {
    background: var(--yap-primary);
  }
  .tdot.tone-ask {
    background: var(--yap-warning);
  }
  .tdot.tone-done {
    background: var(--yap-success);
  }
  .tdot.tone-failed {
    background: var(--yap-danger);
  }
  .ttitle {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 650;
  }
  .tfacts {
    flex: 0 0 auto;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
  .tqueued {
    margin: -2px 10px 8px 24px;
    color: var(--yap-paper-text-2);
    font-size: 12px;
  }
  .tbody {
    padding: 0 12px 12px;
    border-top: 1px solid color-mix(in srgb, var(--yap-paper-rule) 70%, transparent);
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
  .agentfoot {
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
