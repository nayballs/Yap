<script>
  // A meeting's Tasks tab, in the meeting notepad (look "paper") and the Notes
  // view (look "app"), after Wispr Flow's (src-tauri/src/notes/tasks.rs): the
  // summary's tasks, then the ones added here, with how many are done, a
  // tick for each — where it lives: the summary's "- [x]", or the added
  // task's own — **Complete all** (with Undo), and **Add a task** (Enter
  // adds; "Priya: book the room (due: Friday)" reads its owner and date). An
  // added task can be removed; a summary's goes with the summary. Follows
  // `yap-tasks-changed`, and a summary written or ticked anywhere.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { showSaveError } from '../notices/saveErrors.js';
  import { taskRef, tasksDoneLine } from '../core/notepadText.js';

  /** `noteId`; `look`: "paper" | "app". */
  let { noteId, look = 'app' } = $props();
  // Its ticks' origin: not this window's own label, so this window's copy
  // of the note (its Summary tab) follows them too, as every other does.
  const origin = 'tasks';

  let data = $state(null); // notes/tasks.rs `NoteTasks`
  let loaded = $state(false);
  let text = $state('');
  let adding = $state(false);
  let inputEl = $state(null);

  let asked = 0;
  async function load() {
    const n = ++asked;
    const id = noteId;
    try {
      const d = await invoke('note_tasks_get', { noteId: id });
      if (n === asked && id === noteId) data = d;
    } catch {
      if (n === asked) data = null;
    } finally {
      if (n === asked) loaded = true;
    }
  }
  $effect(() => {
    void noteId;
    loaded = false;
    load();
  });

  const tasks = $derived(data?.tasks || []);
  const progress = $derived(tasksDoneLine(tasks));
  const open = $derived(tasks.filter((t) => !t.done));

  function failed(title, e) {
    if (showSaveError(e)) return;
    toast({ title, description: String(e), variant: 'destructive' });
  }

  async function tick(t, done) {
    t.done = done;
    try {
      await invoke('tasks_set', { items: [taskRef(noteId, t)], done, origin });
    } catch (e) {
      failed("Couldn't tick the task", e);
      load();
    }
  }

  async function completeAll() {
    const items = open.map((t) => taskRef(noteId, t));
    if (!items.length) return;
    for (const t of open) t.done = true;
    try {
      const n = await invoke('tasks_set', { items, done: true, origin });
      toast({
        title: n === 1 ? '1 task done' : `${n} tasks done`,
        key: `tasks-done-${noteId}`,
        duration: 8_000,
        action: {
          label: 'Undo',
          onClick: async () => {
            try {
              await invoke('tasks_set', { items, done: false, origin });
            } catch (e) {
              failed("Couldn't undo that", e);
            }
            load();
          },
        },
      });
    } catch (e) {
      failed("Couldn't complete them", e);
      load();
    }
  }

  async function add(e) {
    e?.preventDefault();
    const words = text.trim();
    if (!words || adding) return;
    adding = true;
    try {
      await invoke('task_add', { text: words, noteId });
      text = '';
      await load();
    } catch (err) {
      failed("Couldn't add the task", err);
    } finally {
      adding = false;
      inputEl?.focus();
    }
  }

  async function remove(t) {
    try {
      await invoke('task_remove', { id: t.own });
      await load();
    } catch (e) {
      failed("Couldn't remove the task", e);
    }
  }

  onMount(() => {
    const uns = [];
    let timer = null;
    const later = () => {
      clearTimeout(timer);
      timer = setTimeout(load, 120);
    };
    listen('yap-tasks-changed', (e) => {
      const ids = e.payload?.notes || [];
      if (ids.includes(noteId)) later();
    }).then((u) => uns.push(u));
    // A summary written, edited or ticked anywhere (its tasks are in it).
    listen('yap-note-changed', (e) => {
      if (e.payload?.id === noteId && typeof e.payload?.enhancedContent === 'string') later();
    }).then((u) => uns.push(u));
    listen('yap-meeting-summary', (e) => e.payload?.noteId === noteId && e.payload?.state === 'done' && later()).then((u) =>
      uns.push(u)
    );
    return () => {
      clearTimeout(timer);
      uns.forEach((u) => u && u());
    };
  });
</script>

<section class="mtasks {look}" aria-label="Tasks">
  <div class="mhead">
    <div class="mcount" aria-live="polite">
      <span class="mlabel">{progress.label}</span>
      {#if progress.total}
        <span class="mbar" role="progressbar" aria-label="Tasks done" aria-valuemin="0" aria-valuemax={progress.total} aria-valuenow={progress.done}>
          <span class="mfill" style="width: {(progress.done * 100) / progress.total}%"></span>
        </span>
      {/if}
    </div>
    {#if open.length}
      <button class="mall" onclick={completeAll}>Complete all</button>
    {/if}
  </div>

  {#if loaded && !tasks.length}
    <p class="mempty">
      No tasks yet. The summary's action plan lists who does what; add your own here too.
    </p>
  {/if}

  {#if tasks.length}
    <ul class="mlist">
      {#each tasks as t (t.own != null ? `o${t.own}` : `l${t.line}:${t.text}`)}
        {@const id = `mtask-${noteId}-${t.own != null ? `o${t.own}` : t.line}`}
        <li class="mtask" class:done={t.done}>
          <input {id} type="checkbox" checked={t.done} onchange={(e) => tick(t, e.currentTarget.checked)} />
          <div class="mbody">
            <label for={id} class="mtext">{t.task}</label>
            <span class="mmeta">
              {#if t.owner}<span class="mowner" class:you={t.owner === 'You'}>{t.owner}</span>{/if}
              {#if t.due}<span class="mdue">Due {t.due}</span>{/if}
              {#if t.own != null}<span class="madded">Added by you</span>{/if}
            </span>
          </div>
          {#if t.own != null}
            <button class="mremove" aria-label={`Remove “${t.task}”`} title="Remove this task" onclick={() => remove(t)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}

  <form class="madd" onsubmit={add}>
    <input
      bind:this={inputEl}
      bind:value={text}
      class="maddin"
      placeholder="Add a task…"
      aria-label="Add a task"
      maxlength="500"
      disabled={adding}
    />
    <button class="maddbtn" type="submit" disabled={!text.trim() || adding}>Add</button>
  </form>
</section>

<style>
  /* Two looks: the notepad's paper, the main window's. Colours from tokens only. */
  .mtasks.paper {
    --t-ink: var(--yap-paper-ink);
    --t-muted: var(--yap-paper-date);
    --t-faint: var(--yap-paper-faint);
    --t-line: var(--yap-paper-s2);
    --t-card: var(--yap-paper-s1);
    --t-chip: var(--yap-paper-s2);
    --t-accent: var(--yap-live);
    --t-you: var(--yap-paper-s2-strong);
    --t-ring: var(--yap-paper-ring);
    padding: 16px 20px 160px 36px;
    font-size: 15px;
  }
  .mtasks.app {
    --t-ink: var(--yap-fg);
    --t-muted: var(--yap-muted);
    --t-faint: var(--yap-muted-55);
    --t-line: var(--yap-border-subtle);
    --t-card: var(--yap-s2);
    --t-chip: var(--yap-raised-soft);
    --t-accent: var(--yap-primary);
    --t-you: var(--yap-primary-tint);
    --t-ring: var(--yap-primary-line);
    padding: 10px 0 14px;
    font-size: 13.5px;
  }
  .mtasks {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-height: 0;
    overflow-y: auto;
    color: var(--t-ink);
  }
  .mhead {
    display: flex;
    align-items: center;
    gap: 12px;
  }
  .mcount {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .mlabel {
    font-weight: 600;
    white-space: nowrap;
  }
  .mbar {
    flex: 1;
    max-width: 180px;
    height: 6px;
    border-radius: 3px;
    background: var(--t-chip);
    overflow: hidden;
  }
  .mfill {
    display: block;
    height: 100%;
    border-radius: 3px;
    background: var(--t-accent);
    transition: width 0.25s ease;
  }
  .mall,
  .maddbtn {
    height: 30px;
    padding: 0 12px;
    border: 1px solid var(--t-line);
    border-radius: 8px;
    background: var(--t-card);
    color: var(--t-ink);
    font: inherit;
    font-size: 0.87em;
    font-weight: 600;
    cursor: pointer;
  }
  .mall:hover,
  .maddbtn:hover:not(:disabled) {
    background: var(--t-chip);
  }
  .maddbtn:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .mempty {
    margin: 0;
    font-size: 0.9em;
    color: var(--t-muted);
  }
  .mlist {
    margin: 0;
    padding: 0;
    list-style: none;
    border: 1px solid var(--t-line);
    border-radius: 10px;
    background: var(--t-card);
  }
  .mtask {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 9px 12px;
  }
  .mtask + .mtask {
    border-top: 1px solid var(--t-line);
  }
  .mtask input {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    margin: 3px 0 0;
    accent-color: var(--t-accent);
    cursor: pointer;
  }
  .mbody {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }
  .mtext {
    font-family: var(--yap-font-reading);
    line-height: 1.45;
    overflow-wrap: anywhere;
    cursor: pointer;
  }
  .mtask.done .mtext {
    color: var(--t-muted);
    text-decoration: line-through;
  }
  .mmeta {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 8px;
    font-size: 0.8em;
    color: var(--t-muted);
  }
  .mowner {
    padding: 0 7px;
    border-radius: 999px;
    background: var(--t-chip);
    font-weight: 600;
  }
  .mowner.you {
    background: var(--t-you);
    color: var(--t-ink);
  }
  .madded {
    color: var(--t-faint);
  }
  .mremove {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--t-muted);
    opacity: 0;
    cursor: pointer;
  }
  .mremove svg {
    width: 12px;
    height: 12px;
  }
  .mtask:hover .mremove,
  .mremove:focus-visible {
    opacity: 1;
  }
  .mremove:hover {
    background: var(--t-chip);
    color: var(--t-ink);
  }
  .madd {
    display: flex;
    gap: 8px;
  }
  .maddin {
    flex: 1;
    min-width: 0;
    height: 32px;
    box-sizing: border-box;
    padding: 0 10px;
    border: 1px solid var(--t-line);
    border-radius: 8px;
    background: transparent;
    color: var(--t-ink);
    font: inherit;
    font-size: 0.93em;
  }
  .maddin::placeholder {
    color: var(--t-faint);
  }
  .maddin:focus,
  .mall:focus-visible,
  .maddbtn:focus-visible,
  .mtask input:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--t-ring);
  }
</style>
