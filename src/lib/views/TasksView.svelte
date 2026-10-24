<script>
  // Tasks (sidebar → Tasks): every task in the meetings' action plans (and
  // any note's summary with "- [ ]" lines), from src-tauri/src/notes/tasks.rs.
  // Grouped by meeting (newest first) or by person (You first); each with
  // who, when it's due, a link back to the meeting and to the moment it
  // was said ("Said at 12:40": the note opens in Notes at that line). A
  // tick here is the tick in the note: the summary's own "- [x]"
  // (`note_task_set`), so the notepad and the Notes view show it too, and
  // a tick there shows here (`yap-note-changed`). A task just ticked stays
  // in the list, struck through, until you leave, so a slip is one click
  // to undo; Show done lists the rest.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { clock, dateLine } from '../core/notepadText.js';
  import { setTask } from '../core/noteEdits.js';

  /** This view's `origin` for its ticks. */
  const LABEL = 'tasks';

  // `onopennote(id, line?)`: open note `id` in Notes (at transcript line `line`).
  let { onopennote = null } = $props();

  let notes = $state([]); // notes/tasks.rs `NoteTasks`
  let loaded = $state(false);
  let group = $state(readPref('yapTasksGroup') === 'person' ? 'person' : 'meeting');
  let showDone = $state(false);
  // Ticked here this visit: shown, struck through, until the view closes.
  let ticked = $state(new Set());

  function readPref(key) {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  }
  function setGroup(g) {
    group = g;
    try {
      localStorage.setItem('yapTasksGroup', g);
    } catch {
      /* private storage: this visit only */
    }
  }

  let asked = 0;
  async function refresh() {
    const n = ++asked;
    try {
      const list = (await invoke('tasks_list')) || [];
      if (n === asked) notes = list;
    } catch {
      /* keep what's shown */
    } finally {
      if (n === asked) loaded = true;
    }
  }

  const keyOf = (t) => `${t.noteId}:${t.text}`;
  const all = $derived(
    notes.flatMap((n) =>
      n.tasks.map((t) => ({ ...t, noteId: n.noteId, title: n.title, createdTs: n.createdTs, noteType: n.noteType }))
    )
  );
  const openCount = $derived(all.filter((t) => !t.done).length);
  const doneCount = $derived(all.length - openCount);
  const shown = $derived(all.filter((t) => showDone || !t.done || ticked.has(keyOf(t))));

  /** "You" first, then names A–Z, then Everyone, Unassigned and no owner. */
  function ownerRank(owner) {
    if (owner === 'You') return 0;
    if (owner === 'Everyone') return 2;
    if (owner === 'Unassigned') return 3;
    if (!owner) return 4;
    return 1;
  }
  const ownerLabel = (owner) => owner || 'No owner';

  const byMeeting = $derived.by(() => {
    const groups = [];
    for (const n of notes) {
      const tasks = shown
        .filter((t) => t.noteId === n.noteId)
        .sort((a, b) => (a.owner === 'You' ? 0 : 1) - (b.owner === 'You' ? 0 : 1) || a.line - b.line);
      if (tasks.length) groups.push({ key: `n${n.noteId}`, noteId: n.noteId, title: n.title, createdTs: n.createdTs, tasks });
    }
    return groups;
  });

  const byPerson = $derived.by(() => {
    const map = new Map();
    for (const t of shown) {
      const k = t.owner || '';
      if (!map.has(k)) map.set(k, []);
      map.get(k).push(t);
    }
    return [...map.entries()]
      .sort(([a], [b]) => ownerRank(a) - ownerRank(b) || a.localeCompare(b))
      .map(([owner, tasks]) => ({ key: `p${owner}`, owner, tasks }));
  });

  async function toggleTask(t, done) {
    const k = keyOf(t);
    const at = notes.findIndex((n) => n.noteId === t.noteId);
    const i = at < 0 ? -1 : notes[at].tasks.findIndex((x) => x.line === t.line && x.text === t.text);
    if (i < 0) return;
    notes[at].tasks[i].done = done;
    if (done) ticked = new Set([...ticked, k]);
    try {
      await setTask(t.noteId, { line: t.line, text: t.text, done }, LABEL);
    } catch (e) {
      notes[at].tasks[i].done = !done;
      toast({ title: "Couldn't tick the task", description: String(e), variant: 'destructive' });
      refresh();
    }
  }

  function open(t, line = null) {
    onopennote?.(t.noteId, line);
  }

  onMount(() => {
    refresh();
    const uns = [];
    let timer = null;
    // A tick, an edit or a new summary anywhere: the list as it is now.
    const later = () => {
      clearTimeout(timer);
      timer = setTimeout(refresh, 150);
    };
    for (const name of ['yap-note-changed', 'yap-notes-changed', 'yap-note-deleted']) {
      listen(name, later).then((u) => uns.push(u));
    }
    listen('yap-meeting-summary', (e) => e.payload?.state === 'done' && later()).then((u) => uns.push(u));
    return () => {
      clearTimeout(timer);
      uns.forEach((u) => u && u());
    };
  });
</script>

{#snippet taskRow(t, showOwner, showMeeting)}
  {@const id = `task-${t.noteId}-${t.line}`}
  <li class="task" class:done={t.done}>
    <input {id} type="checkbox" checked={t.done} onchange={(e) => toggleTask(t, e.currentTarget.checked)} />
    <div class="tbody">
      <label for={id} class="ttext">{t.task}</label>
      <div class="tmeta">
        {#if showOwner}
          <span class="chip owner" class:you={t.owner === 'You'}>{ownerLabel(t.owner)}</span>
        {/if}
        {#if t.due}<span class="chip due">Due {t.due}</span>{/if}
        {#if showMeeting}
          <button class="link" onclick={() => open(t)} title="Open this meeting in Notes">{t.title}</button>
        {/if}
        {#if t.at}
          <button
            class="link moment"
            onclick={() => open(t, t.at.index)}
            title="Open the meeting at the moment it was said"
            aria-label={`Said at ${clock(t.at.secs)} in ${t.title}: open the meeting there`}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="8.5" /><path d="M12 7.5V12l3 2" /></svg>
            Said at {clock(t.at.secs)}
          </button>
        {/if}
      </div>
    </div>
  </li>
{/snippet}

<div class="tasksview">
  <div class="scroll">
    <div class="wrap">
      <header class="head">
        <div>
          <h1 tabindex="-1">Tasks</h1>
          <p class="sub">From your meetings' action plans. Tick one here or in the meeting: it's the same task.</p>
        </div>
      </header>

      {#if loaded && all.length}
        <div class="bar">
          <div class="seg" role="group" aria-label="Group tasks">
            <button aria-pressed={group === 'meeting'} onclick={() => setGroup('meeting')}>By meeting</button>
            <button aria-pressed={group === 'person'} onclick={() => setGroup('person')}>By person</button>
          </div>
          <span class="count" aria-live="polite">{openCount} open</span>
          <span class="grow"></span>
          <label class="showdone">
            <input type="checkbox" bind:checked={showDone} />
            Show done{doneCount ? ` (${doneCount})` : ''}
          </label>
        </div>
      {/if}

      {#if !loaded}
        <p class="quiet">Loading…</p>
      {:else if !all.length}
        <div class="empty">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="3.5" y="3.5" width="17" height="17" rx="3" /><path d="M8 12.5l3 3 5-6" /></svg>
          <p class="e1">No tasks yet</p>
          <p class="e2">When a meeting ends, its action plan lists who does what. Those tasks show here, to tick off as they're done.</p>
        </div>
      {:else if !shown.length}
        <div class="empty">
          <p class="e1">Nothing open</p>
          <p class="e2">Every task is ticked off. Show done lists them.</p>
        </div>
      {:else if group === 'meeting'}
        {#each byMeeting as g (g.key)}
          <section class="sec" aria-labelledby={`h-${g.key}`}>
            <div class="seccap">
              <h2 id={`h-${g.key}`}><button class="meetlink" onclick={() => onopennote?.(g.noteId, null)}>{g.title}</button></h2>
              <span class="secdate">{dateLine(g.createdTs)}</span>
            </div>
            <ul class="list">
              {#each g.tasks as t (`${t.noteId}:${t.line}`)}
                {@render taskRow(t, true, false)}
              {/each}
            </ul>
          </section>
        {/each}
      {:else}
        {#each byPerson as g (g.key)}
          <section class="sec" aria-labelledby={`h-${g.key}`}>
            <div class="seccap">
              <h2 id={`h-${g.key}`} class="person">{ownerLabel(g.owner)}</h2>
              <span class="secdate">{g.tasks.filter((t) => !t.done).length} open</span>
            </div>
            <ul class="list">
              {#each g.tasks as t (`${t.noteId}:${t.line}`)}
                {@render taskRow(t, false, true)}
              {/each}
            </ul>
          </section>
        {/each}
      {/if}
    </div>
  </div>
</div>

<style>
  .tasksview {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .scroll {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
  }
  .wrap {
    max-width: 780px;
    margin: 0 auto;
    padding: 30px 36px 24px;
  }
  .head {
    margin-bottom: 16px;
  }
  .head h1 {
    margin: 0;
    font-family: var(--yap-font-display);
    font-size: 30px;
    font-weight: 550;
    letter-spacing: -0.01em;
  }
  .head h1:focus {
    outline: none;
  }
  .sub {
    margin: 4px 0 0;
    font-size: 13px;
    color: var(--yap-muted);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 12px;
    margin: 0 0 20px;
  }
  .grow {
    flex: 1;
  }
  .seg {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-raised-soft);
  }
  .seg button {
    height: 26px;
    padding: 0 12px;
    border: none;
    border-radius: var(--yap-r-full);
    background: none;
    color: var(--yap-fg-62);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .seg button[aria-pressed='true'] {
    background: var(--yap-s2);
    color: var(--yap-fg);
    box-shadow: var(--yap-shadow-sm);
  }
  .count {
    font-size: 12.5px;
    color: var(--yap-muted-70);
  }
  .showdone {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    color: var(--yap-fg-80);
    cursor: pointer;
  }
  .showdone input {
    accent-color: var(--yap-primary);
  }
  .sec {
    margin-bottom: 24px;
  }
  .seccap {
    display: flex;
    align-items: baseline;
    gap: 10px;
    margin: 0 0 6px;
    padding-left: 2px;
  }
  .seccap h2 {
    margin: 0;
    font-size: 14px;
    font-weight: 650;
    color: var(--yap-fg);
  }
  .seccap h2.person {
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.09em;
    text-transform: uppercase;
    color: var(--yap-muted-70);
  }
  .meetlink {
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .meetlink:hover {
    text-decoration: underline;
  }
  .secdate {
    font-size: 12px;
    color: var(--yap-muted-55);
  }
  .list {
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .task {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 10px 14px;
  }
  .task + .task {
    border-top: 1px solid var(--yap-border-subtle);
  }
  .task input {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    margin: 2px 0 0;
    accent-color: var(--yap-primary);
    cursor: pointer;
  }
  .tbody {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .ttext {
    font-family: var(--yap-font-reading);
    font-size: calc(13.5px * var(--yap-reading-scale));
    line-height: 1.45;
    color: var(--yap-fg);
    cursor: pointer;
    overflow-wrap: anywhere;
  }
  .task.done .ttext {
    color: var(--yap-muted);
    text-decoration: line-through;
  }
  .tmeta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 10px;
    font-size: 11.5px;
    color: var(--yap-muted-70);
  }
  .chip {
    padding: 1px 7px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised-soft);
    color: var(--yap-fg-80);
    font-weight: 600;
  }
  .chip.you {
    background: var(--yap-primary-tint);
    color: var(--yap-primary-hover);
  }
  .chip.due {
    background: none;
    padding: 0;
    color: var(--yap-fg-62);
  }
  .link {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  .link svg {
    width: 12px;
    height: 12px;
  }
  .seg button:focus-visible,
  .link:focus-visible,
  .meetlink:focus-visible,
  .task input:focus-visible,
  .showdone input:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
    border-radius: 4px;
  }
  .quiet {
    font-size: 13px;
    color: var(--yap-muted);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 48px 24px;
    border: 1px dashed var(--yap-border);
    border-radius: var(--yap-r-lg);
    text-align: center;
  }
  .empty svg {
    width: 28px;
    height: 28px;
    color: var(--yap-muted-55);
  }
  .e1 {
    margin: 0;
    font-size: 14px;
    font-weight: 650;
  }
  .e2 {
    max-width: 380px;
    margin: 0;
    font-size: 12.5px;
    color: var(--yap-muted);
  }
</style>
