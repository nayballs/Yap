<script>
  // **An agent's work, in the thread** (11 Nov 2026, after Claude Code's own
  // transcript): under the answer that sent it, in a Code chat, what the agent
  // says as it goes — plain words, no bubble — and between them one quiet line
  // for each stretch of steps ("Ran 3 commands, edited convert.js +4 −1 ›"),
  // which opens into a row for each step, each opening to its diff or its
  // command and what it printed (StepRows). Before, the thread said "On it"
  // and then nothing until the report: all of this was only in the Agent tab.
  //
  // - Its log comes from stores/agentLogs.svelte.js (read once, then live);
  //   the turn's events are `turnsOf`'s, grouped by core/agents.js `workOf`.
  // - While it works, the last line is the step it's on, with a spinner —
  //   in the thread, **the lead** instead (14 Nov 2026, the maintainer: the
  //   chat's own Claude "like the leader… clearly different from the agent
  //   animations"): its crew character in a captain's cap, conducting
  //   (CrewAvatar's `lead`), and how long it's worked ("3m 44s"; its tokens
  //   aren't here: the context ring under the message box has them — the
  //   maintainer, 14 Nov 2026). Writing, or
  //   between steps, the lead stays on a line of its own at the foot.
  // - **Its words as it writes them** (12 Nov 2026, as Claude Code's own
  //   transcript shows them): the message it's writing now is the last
  //   paragraph, growing (`agentLogs.saying`, live only); once it's whole
  //   it's the `said` line that was coming, or its final words (the
  //   chat's answer, below). Before, each message appeared at once when it
  //   was finished.
  // - A step whose tool call came back with a picture (a picture file read,
  //   a screenshot a tool took: agents/pictures.rs) ends its line with it,
  //   small (StepPictures), as Claude Code's transcript does; opened, its
  //   row has it. A click opens it (`onview`).
  // - Its final words aren't here once they're the chat's answer, right
  //   below (`answered`: the thread has it). Until then they stay where
  //   they were written: never shown twice, never gone for a moment.
  // - A turn's end is its own last event's (core/agents.js `turnEnd`): the
  //   store's record follows a moment later.
  // - Nothing to show (no log yet, a turn that never ran): nothing drawn.
  //
  // **Whose work shows where** (12 Nov 2026, the maintainer: "if you send out
  // an agent I don't think I see the work the agent does in the main chat
  // window, only if you yourself do it"): in the thread, only a Code chat
  // that works directly (its own session's work, under the message that
  // asked). An agent Chat sent out shows this same work in its row of the
  // Agent panel (`events`: that turn's, handed in; `wide`: the row's width),
  // with its last words too, since no report follows it there.
  //
  // **In order with the person's own messages** (15 Nov 2026, as Claude
  // Code's app lays a turn out): a message they sent while it worked shows
  // where Claude read it, and the work after it under it (core/agents.js
  // `turnParts`: the log's `read` events). The thread draws a turn as one
  // AgentWork a part — the first under the message that asked, each next
  // under the message read there (`notes`, `after`); only the last has the
  // lead, its words as written and its final words.
  //
  // **Its to-do list** (the thread's, `cards`; 16 Nov 2026, as the Claude
  // app has it): while the turn works, the live list isn't here but at the
  // thread's foot, the last thing above the message box (ChatThread,
  // core/agents.js `todoAtFoot`); once it ends, the list stays as the turn's
  // record at the end of its last part's work, folded. A part followed by a
  // message it read keeps the list as it stood at its end — "Plan from
  // before your last message" — and the live one goes on at the foot once
  // Claude writes it again after that message (AgentTodoCard).
  //
  // **The transcript view** (`view`, threadView.svelte.js: the chat's ⋯):
  // Normal, Thinking — its thinking too, between the stretches, quiet and
  // italic (the log's `thought`), under "Thought for 4s" (`thoughtMs`) — or
  // Verbose: that, and every stretch and row open. Opus 5.5's and Sonnet
  // 5.5's thinking comes with no words in a Claude Code run like Yap's (no
  // flag, setting or variable asks for them: yap-code.md): its line is the
  // time alone, the first saying plainly why.
  //
  // **A picture opens the viewer, paged** (`onpaged(list, at)`): ‹ 2 of 4 ›
  // across the whole turn's pictures, each captioned with its step, as
  // Claude Code's app pages a turn's. (The Agent panel's rows open the
  // panel's picture tab: `onview`.)
  //
  // **Waiting for its background work** (16 Nov 2026, agents/runner/
  // background.rs): its answer is in and a helper or a command it left
  // running hasn't reported back, so the lead stays, its line saying what
  // it waits for ("Waiting for background work (1 helper)"); what comes back
  // is the rest of its answer. Waited 45 minutes, it asks under the lead
  // whether to keep waiting (`agent_keep_waiting`) or stop it (`agent_stop`).
  //
  // Its own small chunk (ChatThread and AgentPanel import it lazily).
  import { invoke } from '@tauri-apps/api/core';
  import { workOf, workSummary, turnsOf, turnEnd, groupPictures, shownStepPicture, turnParts, turnPictures, todosIn, thoughtFor, THOUGHT_NOT_SENT } from '../../core/agents.js';
  import { isLive } from '../../core/agentNow.js';
  import { renderMarkdown } from '../../core/markdown.js';
  import { colourCode } from '../../ui/colourCode.js';
  import { codeCopy } from '../../ui/codeCopy.js';
  import { markdownVisuals } from '../../ui/markdownVisuals.js';
  import { getContext } from 'svelte';
  import { fileLinks, CODE_FILES } from '../../ui/fileLinks.js';
  import { agentLogs, watchAgentLog, watchSaying } from '../../stores/agentLogs.svelte.js';
  import StepPictures from './StepPictures.svelte';
  import CrewAvatar from './CrewAvatar.svelte';
  import { characterFor } from '../../core/crew.js';
  import { workClock } from '../../core/agents.js';
  import { turnSecs } from '../../core/agentPanel.js';
  import StepRows from './StepRows.svelte';
  import AgentTodoCard from './AgentTodoCard.svelte';
  import { showsThinking } from './threadView.svelte.js';

  /**
   * `agent`: the pill's (its record, as the store has it); `turn`: which of
   * its turns; `line`: the line it's on now; `onview(picture)`: open a step's
   * picture (a `ShownPicture`); `answered`: its final words are the
   * chat's answer in the thread now (null: go by the turn's end). In the
   * Agent panel's row: `events` (the turn's own, so its log isn't read
   * again), `status` (the turn's) and `wide`. In the thread: `notes` (the
   * ids of the messages it may have read as it worked), `after` (which part
   * this is: the message it follows, null for the first), `view` (the chat's
   * transcript view), `cards` (its to-do list here) and `onpaged(list, at)`.
   * A row's own view (AgentTaskTab): `events` as the row, `stretchesOpen`.
   */
  let {
    agent,
    turn = 1,
    line = '',
    onview = null,
    events = null,
    status = null,
    wide = false,
    answered = null,
    notes = [],
    after = null,
    view = 'normal',
    cards = false,
    onpaged = null,
    stretchesOpen = false,
  } = $props();
  // A Code chat's paths in what Claude said are links to the file (ChatView's context).
  const codeFiles = getContext(CODE_FILES) ?? null;

  $effect(() => {
    if (events == null && agent?.id) watchAgentLog(agent.id);
  });
  const log = $derived(events == null ? agentLogs.by[agent?.id] || [] : []);
  const note = $derived(events == null ? (turnsOf(agent, log).find((t) => t.n === turn) ?? null) : null);
  const end = $derived(turnEnd(events == null ? note?.events : null));
  const live = $derived(events == null ? isLive(note?.status) && !end.over : isLive(status));
  /** The turn's parts, at the messages it read (one in the Agent panel's row). */
  const parts = $derived(events != null ? [{ after: null, events }] : turnParts(note?.events || [], notes));
  const at = $derived(parts.findIndex((p) => p.after === after));
  const part = $derived(at >= 0 ? parts[at] : null);
  /** This is its last part: where it is now. */
  const latest = $derived(at === parts.length - 1);
  /** At work, here (an earlier part is done with). */
  const working = $derived(live && latest);
  /** Its final words, left out of its work: once the turn is over, or — in the thread — once they're the answer there. */
  const final = $derived(latest && (answered ?? !live) ? note?.text || end.text : '');
  $effect(() => {
    watchSaying();
  });
  /** The message it's writing now, so far (this turn's, while it works). */
  const saying = $derived.by(() => {
    const now = working && agent?.id ? agentLogs.saying[agent.id] : null;
    return now && now.turn === turn ? now.text : '';
  });
  // (Housekeeping alone — "Starting Claude Code" — is a line only while it's what the agent is on: not once it writes.)
  const onStep = $derived(working && !saying);
  const thinking = $derived(showsThinking(view));
  const items = $derived(!part ? [] : events != null ? workOf(events, '', onStep, { thinking }) : note?.startedTs || live ? workOf(part.events, final, onStep, { thinking }) : []);
  /** The first thought here whose words weren't sent: it says why, the rest their time alone. */
  const firstUnsent = $derived(items.findIndex((it) => it.kind === 'thought' && !it.text));
  /** **The lead**: in the thread it's the chat's own Claude at work (an agent's row in the Agent panel has its own walking crew). */
  const lead = $derived(events == null);
  let now = $state(Date.now() / 1000);
  $effect(() => {
    if (!lead || !working) return;
    now = Date.now() / 1000;
    const tick = setInterval(() => (now = Date.now() / 1000), 1000);
    return () => clearInterval(tick);
  });
  /** "1m 12s": how long it's worked this turn. */
  const clock = $derived(lead && working && note ? workClock(turnSecs(note, now)) : '');
  /** Its step line is up now (the last item a stretch of steps): the lead's on it; else on a line of its own. */
  const stepUp = $derived(onStep && items.length > 0 && items[items.length - 1].kind === 'work');
  const leadName = $derived(characterFor('claude-plan', agent?.model));
  /** The stretches the person opened or folded (by their first call: a stretch keeps it as more come, and whichever view shows it); Verbose opens them all. */
  let opened = $state({});
  const keyOf = (it, i) => it.steps?.[0]?.calls?.[0] ?? `at${i}`;
  // (`stretchesOpen`: every stretch starts open — a row's own view, AgentTaskTab — its rows folded.)
  const isOpen = (key) => opened[key] ?? (view === 'verbose' || stretchesOpen);
  const toggle = (key) => (opened = { ...opened, [key]: !isOpen(key) });

  // ---- Its to-do list (the thread's)
  /** The turn has parts: a message was read into it. */
  const split = $derived(parts.length > 1);
  /** The turn has a to-do list (the store keeps its latest: AgentTodoCard's). */
  const hasTodos = $derived((((agent?.turnList || []).find((t) => t.n === turn) ?? (agent?.turnList?.length ? null : agent))?.todos?.length ?? 0) > 0);
  /** The turn's list as its record, at the end of its last part, once it's over (while it works it's at the thread's
   *  foot: `todoAtFoot`); split, only once Claude wrote it again after the message it read. */
  const recordCard = $derived(cards && !!part && hasTodos && latest && !live && (!split || !!todosIn(part.events)?.length));
  /** The list as it stood at this part's end, before the message after it. */
  const beforeList = $derived(cards && part && !latest ? todosIn(part.events) : null);
  const beforeLabel = $derived(at === parts.length - 2 ? 'Plan from before your last message' : 'Plan from before your message');

  // ---- Its pictures, paged in the viewer
  const pictures = $derived(onpaged && events == null ? turnPictures(note?.events || []).map((p) => shownStepPicture(p, agent?.folder)) : []);
  function viewPicture(p) {
    const k = pictures.findIndex((x) => x.id === p?.id);
    if (onpaged && k >= 0) onpaged(pictures, k);
    else onview?.(shownStepPicture(p, agent?.folder));
  }
  const hasCard = $derived(recordCard || !!beforeList?.length);

  // ---- Waiting for its background work: asked whether to keep waiting
  /** Waited long enough that the person is asked (Rust's `waiting.ask`): here, at the lead. */
  const waitAsk = $derived(lead && working && agent?.waiting?.ask ? agent.waiting : null);
  const waitedMins = $derived(waitAsk ? Math.max(1, Math.round((now - (Number(waitAsk.since) || now)) / 60)) : 0);
  let answering = $state(false);
  async function keepWaiting(stop) {
    if (answering || !agent?.id) return;
    answering = true;
    try {
      await invoke(stop ? 'agent_stop' : 'agent_keep_waiting', { id: agent.id });
    } catch {
      /* it ended meanwhile: its record says so */
    } finally {
      answering = false;
    }
  }
</script>

{#if items.length || saying || hasCard}
  <div class="agentwork" class:wide data-testid="chat-agent-work" data-agent={agent?.id} data-turn={turn} data-part={at} data-live={working ? 'true' : 'false'} data-view={view}>
    {#each items as it, i (i)}
      {#if it.kind === 'said'}
        <div class="wsaid" data-testid="chat-agent-work-said" use:colourCode use:codeCopy use:markdownVisuals use:fileLinks={codeFiles}>{@html renderMarkdown(it.text)}</div>
      {:else if it.kind === 'notice'}
        <!-- A quiet line of Yap's own: background work from before that Claude Code says stopped. -->
        <div class="wnotice" data-testid="chat-agent-work-notice">{it.text}</div>
      {:else if it.kind === 'thought'}
        <!-- Its thinking (the Thinking and Verbose views): quieter than its words, with how long it took. Words
             Claude Code didn't send (Opus 5.5, Sonnet 5.5): its time alone, and the first such line says why. -->
        <div class="wthought" class:unsent={!it.text} data-testid="chat-agent-work-thought" data-sent={it.text ? 'true' : 'false'} title={it.text ? null : THOUGHT_NOT_SENT}>
          <span class="wthink">{thoughtFor(it.ms)}</span>
          {#if it.text}
            <div class="wthoughttext">{@html renderMarkdown(it.text)}</div>
          {:else if i === firstUnsent}
            <span class="wnotsent" data-testid="chat-agent-work-thought-not-sent">{THOUGHT_NOT_SENT}</span>
          {/if}
        </div>
      {:else}
        {@const busy = onStep && i === items.length - 1}
        {@const summary = busy && line ? [{ text: line }] : workSummary(it, busy)}
        {@const key = keyOf(it, i)}
        {@const open = isOpen(key)}
        <div class="wgroup" data-testid="chat-agent-work-steps" data-open={open ? 'true' : 'false'} data-busy={busy ? 'true' : 'false'}>
          <div class="wrow">
            <button type="button" class="wline" aria-expanded={open} onclick={() => toggle(key)} title={open ? 'Hide the steps' : 'Show the steps'}>
              {#if busy && lead}<CrewAvatar character={leadName} prop="lead" pose="working" size={0.5} label="Claude is working" testid="chat-lead" />{:else if busy}<span class="spin" aria-hidden="true"></span>{/if}
              <span class="wtext">
                {#each summary as p, k (k)}{k ? ', ' : ''}{p.text}{#if p.added}<span class="plus">+{p.added}</span>{/if}{#if p.removed}<span class="minus">−{p.removed}</span>{/if}{/each}
              </span>
              {#if busy && clock}<span class="wclock" data-testid="chat-lead-clock">{clock}</span>{/if}
              <svg class="chev" class:down={open} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
            </button>
            <!-- The pictures its steps came back with, at the end of the line (opened: each by its step). -->
            {#if !open}<StepPictures pictures={groupPictures(it)} onview={viewPicture} />{/if}
          </div>
          {#if open}<StepRows steps={it.steps} {busy} onview={viewPicture} allOpen={view === 'verbose'} />{/if}
        </div>
      {/if}
    {/each}
    {#if saying}
      <!-- (Its code is coloured once the message is whole: not at every piece.) -->
      <div class="wsaid" data-testid="chat-agent-work-saying" aria-live="off">{@html renderMarkdown(saying)}</div>
    {/if}
    {#if lead && working && !stepUp}
      <!-- The lead, writing or between steps: on a line of its own at the foot, as Claude Code's working line. -->
      <div class="wlead" data-testid="chat-lead-line">
        <CrewAvatar character={leadName} prop="lead" pose="working" size={0.5} label="Claude is working" testid="chat-lead" />
        <span class="wtext">{saying ? 'Writing' : line || 'Thinking'}</span>
        {#if clock}<span class="wclock" data-testid="chat-lead-clock">{clock}</span>{/if}
      </div>
    {/if}
    {#if waitAsk}
      <!-- Waited long for its background work: keep waiting, or stop it (never stopped unasked). -->
      <div class="wwait" role="group" aria-label="Background work" data-testid="chat-agent-wait-ask">
        <span class="wwaittext">Still waiting for background work{waitAsk.what ? ` (${waitAsk.what})` : ''} after {waitedMins === 1 ? '1 minute' : `${waitedMins} minutes`}.</span>
        <span class="wwaitbtns">
          <button type="button" class="wbtn primary" disabled={answering} onclick={() => keepWaiting(false)} data-testid="chat-agent-wait-keep">Keep waiting</button>
          <button type="button" class="wbtn" disabled={answering} onclick={() => keepWaiting(true)} data-testid="chat-agent-wait-stop">Stop it</button>
        </span>
      </div>
    {/if}
    {#if recordCard}
      <!-- The turn's list, its record now it's over (folded), at the end of its work. -->
      <div class="wcard"><AgentTodoCard {agent} {turn} {line} /></div>
    {/if}
    {#if beforeList?.length}
      <!-- The plan as it stood when the person wrote: it stays here, above their message. -->
      <div class="wcard"><AgentTodoCard {agent} {turn} list={beforeList} before={beforeLabel} /></div>
    {/if}
  </div>
{/if}

<style>
  .agentwork {
    align-self: flex-start;
    display: flex;
    flex-direction: column;
    gap: 6px;
    box-sizing: border-box;
    width: min(620px, 86%);
    /* (In line with a bubble's words: its border and padding.) */
    padding: 2px 2px 2px 13px;
    color: var(--yap-fg);
    font-size: calc(12.5px * var(--yap-reading-scale, 1));
    line-height: 1.5;
  }
  /* In the Agent panel's row: the row's own width, no bubble to line up with. */
  .agentwork.wide {
    align-self: stretch;
    width: 100%;
    padding: 0;
  }
  /* What it said: plain words, the thread's own text, no bubble. */
  .wsaid {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .wsaid :global(p) {
    margin: 0 0 6px;
  }
  .wsaid :global(p:last-child) {
    margin-bottom: 0;
  }
  .wsaid :global(ul),
  .wsaid :global(ol) {
    margin: 0 0 6px;
    padding-left: 20px;
  }
  .wsaid :global(code) {
    padding: 1px 4px;
    border-radius: 4px;
    background: var(--yap-s2);
    font-size: 0.92em;
  }
  .wsaid :global(pre) {
    margin: 0 0 6px;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--yap-s2);
    overflow-x: auto;
  }
  /* Its to-do list: in line with the thread's edge, not its words'. */
  .wcard {
    margin-left: -13px;
  }
  /* Its thinking (Thinking, Verbose): quiet, italic, a hairline to its left. */
  .wthought {
    min-width: 0;
    padding: 1px 0 1px 9px;
    border-left: 2px solid var(--yap-border-subtle);
    color: var(--yap-muted);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .wthink {
    display: block;
    margin-bottom: 1px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.01em;
  }
  .wthoughttext {
    font-style: italic;
  }
  /* A thought whose words weren't sent: its time on one quiet line, the first one saying why. */
  .wthought.unsent .wthink {
    display: inline;
    margin: 0;
  }
  .wnotsent {
    display: block;
    color: var(--yap-muted-70);
    font-size: 11px;
    font-style: italic;
  }
  .wthoughttext :global(p) {
    margin: 0 0 4px;
  }
  .wthoughttext :global(p:last-child) {
    margin-bottom: 0;
  }
  .wthoughttext :global(ul),
  .wthoughttext :global(ol) {
    margin: 0 0 4px;
    padding-left: 18px;
  }
  /* A stretch of steps: one quiet line, its pictures at its end. */
  .wgroup {
    min-width: 0;
  }
  .wrow {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .wline {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    max-width: 100%;
    margin: 0;
    padding: 1px 4px 1px 0;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .wline:hover {
    color: var(--yap-fg-80);
  }
  .wline:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .wtext {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .plus,
  .minus {
    margin-left: 5px;
  }
  .plus {
    color: var(--yap-success-text);
    font-variant-numeric: tabular-nums;
  }
  .minus {
    color: var(--yap-danger);
    font-variant-numeric: tabular-nums;
  }
  .chev {
    flex: 0 0 auto;
    width: 12px;
    height: 12px;
    opacity: 0.8;
    transition: transform 120ms ease;
  }
  .chev.down {
    transform: rotate(90deg);
  }
  /* How long it's worked: quieter than its words. */
  .wclock {
    flex: 0 0 auto;
    color: var(--yap-muted-70);
    font-variant-numeric: tabular-nums;
  }
  /* The lead on a line of its own (writing, or between steps). */
  .wlead {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    padding: 2px 0;
    color: var(--yap-muted);
    font-size: 12px;
  }
  /* A quiet line of Yap's own (background work from before, stopped). */
  .wnotice {
    color: var(--yap-muted);
    font-size: 12px;
    font-style: italic;
  }
  /* Waited long for its background work: keep waiting, or stop it (PageConfirm's card, quieter). */
  .wwait {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 10px;
    max-width: min(520px, 100%);
    margin: 2px 0;
    padding: 8px 12px;
    border: 1px solid var(--yap-primary-line);
    border-radius: 10px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font-size: 13px;
  }
  .wwaittext {
    min-width: 0;
  }
  .wwaitbtns {
    display: flex;
    gap: 8px;
  }
  .wbtn {
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
  .wbtn.primary {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .wbtn:hover {
    background: var(--yap-paper-s2);
  }
  .wbtn.primary:hover {
    background: var(--yap-primary-hover);
  }
  .wbtn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .wbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .spin {
    flex: 0 0 auto;
    width: 10px;
    height: 10px;
    border: 1.5px solid color-mix(in srgb, var(--yap-code) 30%, transparent);
    border-top-color: var(--yap-code);
    border-radius: 50%;
    animation: work-spin 0.9s linear infinite;
  }
  @keyframes work-spin {
    to {
      transform: rotate(360deg);
    }
  }
  :global(:root[data-motion='reduced']) .spin {
    animation: none;
  }
  :global(:root[data-motion='reduced']) .chev {
    transition: none;
  }
</style>
