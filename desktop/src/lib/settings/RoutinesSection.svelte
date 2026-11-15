<script>
  // Settings → Skills → Routines (desktop/src-tauri/src/chat/routines.rs):
  // Skills that run by themselves, on this PC while Yap runs — on a
  // schedule, after a meeting (any, in a folder, or with people from
  // outside), or a few minutes before a meeting on the calendar. Each
  // routine: its Skill, when it runs, where its answer goes (its own chat,
  // or a meeting's note), its switch, Run now, Edit and Delete. Under them,
  // the recent runs: when, what started it, its chat or note to open, and
  // why one failed, waits or was skipped. Saved in routines.json at once
  // (not Settings' auto-saved config). Wording: core/routines.js.
  import { tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Group from '../ui/Group.svelte';
  import Input from '../ui/Input.svelte';
  import Button from '../ui/Button.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Select from '../ui/Select.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { showSaveError } from '../notices/saveErrors.js';
  import { skills } from '../stores/skills.svelte.js';
  import { routines, initRoutines, saveRoutine, deleteRoutine, setRoutineOn, runNow, openResult } from '../stores/routines.svelte.js';
  import {
    DAYS,
    WEEKDAYS,
    TRIGGERS,
    LEADS,
    DESTS,
    GRACE_HOURS,
    times,
    triggerOf,
    blankRoutine,
    routineProblem,
    skillOptions,
    toSave,
    canGoWithNote,
    routineLine,
    runState,
    causeLine,
    runWhy,
    runResult,
    capLine,
    when,
  } from '../core/routines.js';

  /** A Skill to make a routine for, from a Skill's own row (SkillsPage); cleared once taken. */
  let { wanted = $bindable(null) } = $props();

  initRoutines();

  const TIMES = times(15);
  const WEEKDAY_OPTIONS = WEEKDAYS.map((label, value) => ({ value, label }));
  const LEAD_OPTIONS = LEADS.map((m) => ({ value: m, label: `${m} min before` }));
  const RUNS_SHOWN = 6;

  let folders = $state([]);
  invoke('notes_folders')
    .then((f) => (folders = f || []))
    .catch(() => {});
  const folderOptions = $derived([{ value: '', label: 'Any folder' }, ...folders.map((f) => ({ value: f, label: f }))]);

  const options = $derived(skillOptions(skills.list));
  const skillOf = (id) => skills.list.find((s) => s.id === id) || null;

  // The form: a routine being made (id '') or changed; null: none.
  let draft = $state(null);
  let formEl = $state(null);
  let saving = $state(false);
  let deleting = $state(null);
  let confirmOpen = $state(false);
  let allRuns = $state(false);

  const problem = $derived(routineProblem(draft, skills.list));
  const draftSkill = $derived(draft ? skillOf(draft.skill) : null);
  const withNote = $derived(draft ? canGoWithNote(draft.trigger, draftSkill) : false);
  const runsShown = $derived(allRuns ? routines.runs : routines.runs.slice(0, RUNS_SHOWN));
  const atMax = $derived(routines.routines.length >= routines.max);

  async function openForm(base = null, skill = null) {
    draft = base
      ? { id: base.id, skill: base.skill, words: base.words || '', trigger: { ...base.trigger }, on: base.on, to: base.to || 'chat' }
      : blankRoutine(skill);
    kept = {};
    kind = draft.trigger.kind;
    await tick();
    formEl?.scrollIntoView({ block: 'nearest' });
    formEl?.querySelector('select')?.focus();
  }
  function closeForm() {
    draft = null;
  }

  // A Skill's row asked for a routine of its own.
  $effect(() => {
    const w = wanted;
    if (!w) return;
    wanted = null;
    openForm(null, w);
  });

  // Each kind of trigger as it was last set in this form: switching When
  // away and back keeps the time, the folder, the minutes.
  let kept = {};
  function setKind(kind) {
    if (!draft || draft.trigger.kind === kind) return;
    kept[draft.trigger.kind] = { ...draft.trigger };
    draft.trigger = triggerOf(kind, kept[kind] ?? {});
  }
  // The When control's choice (set as the form opens; a change makes that trigger).
  let kind = $state('schedule');
  $effect(() => {
    const k = kind;
    if (draft && draft.trigger.kind !== k) setKind(k);
  });

  async function save() {
    if (!draft || problem || saving) return;
    saving = true;
    try {
      const saved = await saveRoutine(toSave(draft, skills.list));
      const skill = skillOf(saved.skill);
      draft = null;
      toast({ title: `Routine saved: /${skill?.command ?? ''}`, description: 'It runs on this PC while Yap is running.' });
    } catch (e) {
      if (showSaveError(e)) draft = null;
      else toast({ title: "Couldn't save the routine", description: String(e), variant: 'destructive' });
    } finally {
      saving = false;
    }
  }

  async function switchOn(r, on) {
    try {
      await setRoutineOn(r.id, on);
    } catch (e) {
      if (!showSaveError(e)) toast({ title: `Couldn't switch /${r.command} ${on ? 'on' : 'off'}`, description: String(e), variant: 'destructive' });
    }
  }

  async function runIt(r) {
    try {
      await runNow(r.id);
    } catch (e) {
      toast({ title: `Couldn't run /${r.command}`, description: String(e), variant: 'destructive' });
    }
  }

  function askDelete(r) {
    deleting = r;
    confirmOpen = true;
  }
  async function doDelete() {
    const r = deleting;
    confirmOpen = false;
    deleting = null;
    if (!r) return;
    try {
      await deleteRoutine(r.id);
    } catch (e) {
      toast({ title: "Couldn't delete the routine", description: String(e), variant: 'destructive' });
    }
  }

  function open(run) {
    const to = runResult(run);
    if (to) openResult(to).catch((e) => toast({ title: "Couldn't open it", description: String(e), variant: 'destructive' }));
  }

  function onFormKey(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      closeForm();
    }
  }
</script>

<Group
  title="Routines"
  desc="Skills that run by themselves on this PC while Yap is running: on a schedule, after a meeting, or before one on your calendar."
  id="settings-routines"
  bare
>
  <div class="list" data-setting="routines">
    {#each routines.routines as r (r.id)}
      {#if draft?.id === r.id}
        {@render form()}
      {:else}
        <div class="routine" class:off={!r.on} data-routine={r.id}>
          <div class="lhs">
            <span class="title">
              <span class="cmd">{r.skillGone ? 'Deleted Skill' : `/${r.command}`}</span>
              <span class="name">{r.name}</span>
              {#if r.busy}<span class="busy">Running</span>{/if}
            </span>
            <span class="line">{routineLine(r)}</span>
          </div>
          <div class="rhs">
            <Button variant="ghost" size="sm" disabled={r.busy || r.skillGone} onclick={() => runIt(r)} ariaLabel={`Run /${r.command} now`}>Run now</Button>
            <Button variant="ghost" size="sm" onclick={() => openForm(r)} ariaLabel={`Edit the /${r.command} routine`}>Edit</Button>
            <button class="tool del" title="Delete" aria-label={`Delete the /${r.command} routine`} onclick={() => askDelete(r)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 7h14M10 7V5h4v2M7 7l1 12h8l1-12" /></svg>
            </button>
            <span class="sw" title={r.on ? 'On' : 'Off'}>
              <Toggle checked={r.on} ariaLabel={`Run /${r.command} by itself`} onchange={(on) => switchOn(r, on)} />
            </span>
          </div>
        </div>
      {/if}
    {:else}
      {#if !draft}<p class="empty">No routines yet. Make one to have a Skill run by itself, like /standup every weekday at 09:00.</p>{/if}
    {/each}
    {#if draft && !draft.id}
      {@render form()}
    {:else if !draft}
      <div class="addrow">
        <button class="add" data-setting="routines-add" disabled={atMax} title={atMax ? `${routines.max} routines at most` : undefined} onclick={() => openForm()}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
          New routine
        </button>
      </div>
    {/if}
  </div>
</Group>

{#if routines.runs.length}
  <Group title="Recent runs" desc={capLine(routines.today, routines.cap)} id="settings-routine-runs" bare>
    <div class="list" data-setting="routines-runs" role="list" aria-label="Recent runs">
      {#each runsShown as run (run.id)}
        {@const st = runState(run)}
        {@const result = runResult(run)}
        <div class="run" role="listitem" data-run={run.id} data-state={run.state}>
          <div class="lhs">
            <span class="title">
              <span class="cmd">/{run.command || '…'}</span>
              <span class="pill {st.tone}">{st.label}</span>
              <span class="when">{when(run.started || run.ended || run.queued)}</span>
            </span>
            <span class="line">{causeLine(run)}{#if runWhy(run)} · <span class="why">{runWhy(run)}</span>{/if}</span>
          </div>
          <div class="rhs">
            {#if result}
              <Button variant="ghost" size="sm" onclick={() => open(run)} ariaLabel={`Open what /${run.command} wrote`}>{result.chat ? 'Open chat' : 'Open note'}</Button>
            {/if}
          </div>
        </div>
      {/each}
      {#if routines.runs.length > RUNS_SHOWN}
        <div class="addrow">
          <Button variant="ghost" size="sm" onclick={() => (allRuns = !allRuns)}>{allRuns ? 'Show fewer' : `Show all ${routines.runs.length}`}</Button>
        </div>
      {/if}
    </div>
  </Group>
{/if}

{#snippet form()}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="form" bind:this={formEl} role="group" aria-label={draft.id ? 'Edit routine' : 'New routine'} onkeydown={onFormKey}>
    <div class="field">
      <span>Skill</span>
      <Select bind:value={draft.skill} options={[{ value: '', label: 'Pick a Skill…' }, ...options]} ariaLabel="Routine Skill" fill />
    </div>
    <div class="field">
      <span>When</span>
      <Segmented bind:value={kind} options={TRIGGERS} label="When it runs" />
    </div>
    {#if draft.trigger.kind === 'schedule'}
      <div class="row">
        <Segmented bind:value={draft.trigger.days} options={DAYS} label="Days" />
        {#if draft.trigger.days === 'weekly'}
          <Select bind:value={draft.trigger.weekday} options={WEEKDAY_OPTIONS} ariaLabel="Day of the week" />
        {/if}
        <Select bind:value={draft.trigger.time} options={TIMES} ariaLabel="Time of day" />
      </div>
    {:else if draft.trigger.kind === 'meetingEnd'}
      <div class="row">
        <Select bind:value={draft.trigger.folder} options={folderOptions} ariaLabel="Meetings in folder" />
        <div class="check">
          <Toggle checked={draft.trigger.outside} ariaLabel="Only meetings with people from outside" onchange={(on) => (draft.trigger.outside = on)} />
          <span aria-hidden="true">Only with people from outside your company</span>
        </div>
      </div>
      <p class="hint">Once the meeting's summary is written. "Outside" goes by your calendar's invites: anyone not on your own email domain.</p>
    {:else}
      <div class="row">
        <Select bind:value={draft.trigger.minutes} options={LEAD_OPTIONS} ariaLabel="Minutes before" />
      </div>
      <p class="hint">Before each meeting on your calendar with someone invited or a call link. The meeting's title goes with the Skill unless you add words below.</p>
    {/if}
    <label class="field">
      <span>Words to run it with <em>(optional)</em></span>
      <Input bind:value={draft.words} placeholder="Like a project's name for /project-update" maxlength={200} ariaLabel="Routine words" />
    </label>
    {#if withNote}
      <div class="field">
        <span>Where it goes</span>
        <Segmented bind:value={draft.to} options={DESTS} label="Where it goes" />
      </div>
    {/if}
    <p class="hint">
      It runs on your Chat model, never while you dictate or a meeting records, and never looks at your screen or opens apps. Missed while
      your PC was off? It runs once when Yap starts, up to {GRACE_HOURS} hours late.
    </p>
    {#if problem}<p class="problem" role="status">{problem}</p>{/if}
    <div class="actions">
      <Button variant="secondary" size="sm" onclick={closeForm}>Cancel</Button>
      <Button size="sm" disabled={!!problem || saving} onclick={save}>{draft.id ? 'Save changes' : 'Save routine'}</Button>
    </div>
  </div>
{/snippet}

<ConfirmDialog
  bind:open={confirmOpen}
  title={deleting ? `Delete the /${deleting.command} routine?` : ''}
  body="What it already wrote stays in its chat. The Skill itself stays."
  confirmLabel="Delete"
  onconfirm={doDelete}
/>

<style>
  .list {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--yap-rule);
  }
  .routine,
  .run {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 20px;
    min-height: 58px;
    box-sizing: border-box;
    padding: 11px 0;
    border-bottom: 1px solid var(--yap-rule);
  }
  .run {
    min-height: 50px;
    padding: 9px 0;
  }
  .lhs {
    display: flex;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .title {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
    font-size: 13.5px;
    line-height: 1.4;
  }
  .cmd {
    flex: 0 0 auto;
    font-weight: 680;
    color: var(--yap-fg);
  }
  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
    color: var(--yap-fg-62);
  }
  .routine.off .cmd {
    color: var(--yap-fg-80);
  }
  .line {
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--yap-muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .why {
    color: var(--yap-fg-62);
  }
  .when {
    font-size: 12px;
    color: var(--yap-muted);
    white-space: nowrap;
  }
  .busy,
  .pill {
    flex: 0 0 auto;
    align-self: center;
    padding: 1px 7px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 620;
    line-height: 1.5;
    background: var(--yap-s3);
    color: var(--yap-fg-80);
  }
  .pill.ok {
    background: color-mix(in srgb, var(--yap-success, #2f9e5b) 16%, transparent);
    color: var(--yap-success, #2f9e5b);
  }
  .pill.bad {
    background: color-mix(in srgb, var(--yap-danger) 14%, transparent);
    color: var(--yap-danger);
  }
  .pill.quiet {
    color: var(--yap-muted);
  }
  .rhs {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .sw {
    display: inline-flex;
    margin-left: 6px;
  }
  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg-62);
    cursor: pointer;
  }
  .tool svg {
    width: 15px;
    height: 15px;
  }
  .tool:hover {
    background: var(--yap-s3);
    color: var(--yap-danger);
  }
  .tool:focus-visible,
  .add:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .empty {
    margin: 0;
    padding: 14px 0 2px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .addrow {
    padding: 12px 0 2px;
  }
  .add {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 30px;
    padding: 0 12px 0 9px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .add:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .add:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .add svg {
    width: 14px;
    height: 14px;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 12px 0;
    padding: 14px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .field > span {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg-80);
  }
  .field em {
    font-style: normal;
    font-weight: 500;
    color: var(--yap-muted);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
  }
  .check {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--yap-fg-80);
  }
  .hint {
    margin: 0;
    font-size: 12px;
    line-height: 1.45;
    color: var(--yap-muted);
  }
  .problem {
    margin: 0;
    font-size: 12px;
    color: var(--yap-danger);
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  @media (max-width: 560px) {
    .title {
      flex-wrap: wrap;
      row-gap: 0;
    }
    .routine {
      grid-template-columns: 1fr;
      row-gap: 6px;
    }
  }
</style>
