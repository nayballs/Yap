<script>
  // **What ▶ Run ran in this Code chat** (15 Nov 2026, after the Claude app,
  // whose ▶ runs a one-line block in its terminal beside the chat;
  // agents/run_block.rs): each run oldest first, like a terminal's
  // scrollback — `garden-planner $ npm test`, what it printed as it prints it
  // (colours taken out), and how it ended ("Exit 0 · 4.2 s", "Exit 1",
  // "Stopped"). While it runs: its clock and Stop. After: Run again (asked
  // first as ▶ is, but in Bypass), Copy the output, Add to message (the
  // line and its output into the message box, for Claude to see: nothing
  // run here goes into the conversation by itself).
  import { onMount, tick, untrack, getContext } from 'svelte';
  import { codeRuns, loadRuns, stopRun, runCommand } from './codeRuns.svelte.js';
  import { colourLines, loadColours } from '../../core/codeColours.js';
  import { copyText } from '../../core/itemMenus.js';
  import { fmtClock } from '../../core/agentPanel.js';
  import { CODE_FILES } from '../../ui/fileLinks.js';

  /** `doc`: the panel's state; `conversationId`: the Code chat; `workspace`: its folder's path. */
  let { doc, narrow = false, conversationId = null, workspace = null } = $props();
  const files = getContext(CODE_FILES);

  const runs = $derived(conversationId == null ? [] : codeRuns.by[String(conversationId)] || []);
  const place = $derived(String(workspace || '').split(/[\\/]/).filter(Boolean).pop() || 'workspace');
  let colours = $state.raw(null);
  let bodyEl = $state(null);
  let now = $state(Date.now());

  onMount(() => {
    loadColours()
      .then((h) => (colours = h))
      .catch(() => {});
    const t = setInterval(() => (now = Date.now()), 1000);
    return () => clearInterval(t);
  });
  $effect(() => {
    const conv = conversationId;
    untrack(() => loadRuns(conv));
  });

  // Follow the output while the view is at its end.
  let atEnd = true;
  const onScroll = () => {
    if (bodyEl) atEnd = bodyEl.scrollHeight - bodyEl.scrollTop - bodyEl.clientHeight < 40;
  };
  $effect(() => {
    void runs.map((r) => r.output?.length ?? 0).join();
    if (!atEnd) return;
    tick().then(() => bodyEl && (bodyEl.scrollTop = bodyEl.scrollHeight));
  });

  const langOf = (r) => (r.shell === 'powershell' ? 'powershell' : r.shell === 'cmd' ? 'cmd' : 'bash');
  const secs = (r) => Math.max(0, ((r.endedMs || now) - r.startedMs) / 1000);
  const took = (r) => {
    const s = secs(r);
    return s < 60 ? `${s.toFixed(s < 10 ? 1 : 0)} s` : fmtClock(s);
  };
  const ended = (r) =>
    r.state === 'running' ? fmtClock(secs(r)) : r.state === 'stopped' ? `Stopped · ${took(r)}` : `Exit ${r.code ?? '?'} · ${took(r)}`;
  /** Its output for the message box: the line, then (the newest part of) what it printed. */
  const forMessage = (r) => {
    const out = String(r.output || '').trimEnd();
    const tail = out.length > 6000 ? `…\n${out.slice(out.length - 6000)}` : out;
    return `I ran this in ${place}:\n\n\`\`\`\n$ ${r.command}\n${tail}\n\`\`\`\n${r.state === 'running' ? '(still running)' : r.state === 'stopped' ? '(stopped)' : `(exit ${r.code ?? '?'})`}`;
  };
</script>

<section class="runstab" aria-label="Runs" data-testid="chat-runs">
  <header class="runshead">
    <svg class="runsicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="m7 9 3 3-3 3M12 15h5" /></svg>
    <h2 class="runstitle">Runs <span class="where" title={workspace}>in {place}</span></h2>
    <div class="runsactions">
      {#if !narrow}
        <button
          type="button"
          class="rbtn"
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
      <button type="button" class="rbtn" onclick={() => doc.close()} aria-label="Close the panel" title="Close (what's running carries on)">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  </header>
  <div class="runsbody" bind:this={bodyEl} onscroll={onScroll}>
    {#each runs as r (r.id)}
      {@const typed = colourLines(colours, r.command, langOf(r))}
      <article class="run" data-testid="chat-run" data-state={r.state} data-id={r.id}>
        <div class="rline">
          <code class="rcmd"
            ><span class="prompt" aria-hidden="true">{place} {r.shell === 'powershell' ? 'PS>' : r.shell === 'cmd' ? '>' : '$'}</span>
            {#if typed?.[0]}{#each typed[0] as t, k (k)}<span class={t.kind ? `syn-${t.kind}` : undefined}>{t.text}</span>{/each}{:else}{r.command}{/if}</code
          >
          <span class="rstate" data-testid="chat-run-state">
            {#if r.state === 'running'}<span class="spin" aria-hidden="true"></span>{/if}{ended(r)}
          </span>
        </div>
        {#if r.note}<p class="rnote">{r.note}</p>{/if}
        {#if r.cut}<p class="rnote">Its earlier output was let go: only the newest is kept.</p>{/if}
        {#if r.output}
          <pre class="rout" data-testid="chat-run-output">{r.output.replace(/\n$/, '')}</pre>
        {:else if r.state !== 'running'}
          <p class="rnote">It printed nothing.</p>
        {/if}
        <div class="racts">
          {#if r.state === 'running'}
            <button type="button" class="rpill" onclick={() => stopRun(r.id)} data-testid="chat-run-stop">
              <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><rect x="6" y="6" width="12" height="12" rx="2" /></svg>Stop
            </button>
          {:else}
            <button type="button" class="rpill" onclick={() => runCommand(conversationId, { command: r.command, lang: langOf(r) }, () => doc.showRuns())} data-testid="chat-run-again">Run again</button>
          {/if}
          {#if r.output}
            <button type="button" class="rpill" onclick={() => copyText(r.output, 'Output copied')}>Copy output</button>
          {/if}
          {#if files?.addToMessage}
            <button type="button" class="rpill" onclick={() => files.addToMessage(forMessage(r))} data-testid="chat-run-add">Add to message</button>
          {/if}
        </div>
      </article>
    {:else}
      <p class="empty" data-testid="chat-runs-empty">Nothing run yet. ▶ on a one-line shell block in an answer runs it here, in {place}.</p>
    {/each}
  </div>
</section>

<style>
  .runstab {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    color: var(--yap-paper-ink);
  }
  .runshead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 8px 18px;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .runsicon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-code);
  }
  .runstitle {
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
  .where {
    font-weight: 400;
    color: var(--yap-paper-muted);
  }
  .runsactions {
    display: flex;
    gap: 2px;
    margin-left: auto;
  }
  .rbtn {
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
  .rbtn:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .rbtn:focus-visible,
  .rpill:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .rbtn svg {
    width: 15px;
    height: 15px;
  }
  .runsbody {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 10px 14px 16px;
  }
  .run + .run {
    margin-top: 14px;
    padding-top: 12px;
    border-top: 1px solid var(--yap-paper-rule);
  }
  .rline {
    display: flex;
    align-items: baseline;
    gap: 10px;
  }
  .rcmd {
    flex: 1 1 auto;
    min-width: 0;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 12px;
  }
  .prompt {
    color: var(--yap-code);
    font-weight: 600;
    margin-right: 6px;
  }
  .rstate {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex: 0 0 auto;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }
  .run[data-state='failed'] .rstate {
    color: var(--yap-danger);
  }
  .run[data-state='done'] .rstate {
    color: var(--yap-success-text);
  }
  .spin {
    width: 10px;
    height: 10px;
    border: 2px solid var(--yap-paper-rule);
    border-top-color: var(--yap-code);
    border-radius: 50%;
    animation: spin 0.9s linear infinite;
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  .rnote {
    margin: 4px 0 0;
    color: var(--yap-paper-muted);
    font-size: 12px;
  }
  .rout {
    margin: 6px 0 0;
    padding: 8px 10px;
    max-height: 420px;
    overflow: auto;
    border-radius: 8px;
    background: var(--yap-paper-s1);
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11.5px;
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--yap-paper-ink);
  }
  .racts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 8px;
  }
  .rpill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: none;
    color: var(--yap-paper-text-2);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .rpill:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .rpill svg {
    width: 11px;
    height: 11px;
  }
  .empty {
    margin: 8px 4px;
    color: var(--yap-paper-muted);
    font-size: 13px;
  }
</style>
