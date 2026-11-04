<script>
  // Settings → Skills (chat/skills.rs; Wispr Flow's Skills): saved
  // instructions run by typing /name — in a meeting's Ask box on that
  // meeting, or in Chat and Ask across meetings on the meetings. Three
  // groups, each Skill with its "Show in the / menu" switch:
  // - Your Skills: made here (name, /command, a line on what it's for, the
  //   instructions, where it runs), changed in place, deleted behind Yap's
  //   confirm dialog; in the menus unless switched off.
  // - Yap's Skills: the core six, in the menus from the start.
  // - More Skills: the other nine, off until switched on.
  // Open one of Yap's to read its instructions; Save a copy makes it yours
  // to change. A Skill switched off still runs when its whole command is
  // typed. Saved in skills.json at once (not Settings' auto-saved config);
  // every window's "/" menu follows (`yap-skills-changed`).
  import { tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Group from '../ui/Group.svelte';
  import Input from '../ui/Input.svelte';
  import Textarea from '../ui/Textarea.svelte';
  import Button from '../ui/Button.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import ConfirmDialog from '../ui/ConfirmDialog.svelte';
  import { toast } from '../ui/toast.svelte.js';
  import { showSaveError } from '../notices/saveErrors.js';
  import { skills, initSkills, refreshSkills, setShown } from '../stores/skills.svelte.js';
  import { commandFromName, commandProblem, writesDocument, skillGroups, whereItRuns } from '../core/skills.js';

  initSkills();

  const INSTRUCTIONS_MAX = 4000;
  const RUNS_ON = [
    { value: 'meeting', label: 'One meeting' },
    { value: 'meetings', label: 'Across meetings' },
    { value: 'both', label: 'Both' },
  ];

  const groups = $derived(skillGroups(skills.list));

  // One of Yap's opened to read its instructions.
  let reading = $state(null);
  // The form: { id: an own Skill's id | null (new), basedOn, name, command,
  // description, instructions, runsOn, commandTouched }; null: none.
  let draft = $state(null);
  let formEl = $state(null);
  let saving = $state(false);
  let deleting = $state(null);
  let confirmOpen = $state(false);

  const problem = $derived.by(() => {
    if (!draft) return '';
    if (!draft.name.trim()) return 'Give it a name.';
    const c = commandProblem(draft.command, skills.list, draft.id);
    if (c) return c;
    if (!draft.instructions.trim()) return 'Say what the Skill should do.';
    return '';
  });

  async function openForm(base = null, copy = false) {
    reading = null;
    draft = {
      id: copy ? null : (base?.id ?? null),
      basedOn: copy ? base.command : (base?.basedOn ?? ''),
      name: copy ? `${base.name} (copy)` : (base?.name ?? ''),
      command: copy ? `my-${base.command}`.slice(0, 32) : (base?.command ?? ''),
      description: base?.description ?? '',
      instructions: base?.instructions ?? '',
      runsOn: base?.runsOn ?? 'both',
      commandTouched: !!base,
    };
    await tick();
    formEl?.scrollIntoView({ block: 'nearest' });
    formEl?.querySelector('input')?.focus();
  }
  function closeForm() {
    draft = null;
  }
  // A new Skill's command follows its name until the person types one.
  function nameChanged() {
    if (draft && !draft.commandTouched) draft.command = commandFromName(draft.name);
  }

  async function save() {
    if (!draft || problem || saving) return;
    saving = true;
    try {
      const saved = await invoke('skill_save', {
        skill: {
          id: draft.id ?? '',
          name: draft.name,
          command: draft.command,
          description: draft.description,
          instructions: draft.instructions,
          runsOn: draft.runsOn,
          basedOn: draft.basedOn,
        },
      });
      draft = null;
      await refreshSkills();
      toast({ title: `Saved /${saved.command}`, description: saved.runsOn === 'meetings' ? 'Run it in Chat or Ask across meetings.' : "Run it in a meeting's Ask box." });
    } catch (e) {
      // Made, but not on disk yet (a full disk): Yap keeps it and says so
      // with Try again; the form closes as for a save. Left open, Save again
      // found the Skill already there ("You already have a Skill with /…").
      if (showSaveError(e)) {
        draft = null;
        await refreshSkills();
      } else {
        toast({ title: "Couldn't save the Skill", description: String(e), variant: 'destructive' });
      }
    } finally {
      saving = false;
    }
  }

  // "Show in the / menu": at once, saved behind it.
  async function show(s, on) {
    try {
      await setShown(s.id, on);
    } catch (e) {
      if (!showSaveError(e)) toast({ title: `Couldn't change /${s.command}`, description: String(e), variant: 'destructive' });
    }
  }

  function askDelete(s) {
    deleting = s;
    confirmOpen = true;
  }
  async function doDelete() {
    const s = deleting;
    confirmOpen = false;
    deleting = null;
    if (!s) return;
    try {
      await invoke('skill_delete', { id: s.id });
      await refreshSkills();
    } catch (e) {
      toast({ title: "Couldn't delete the Skill", description: String(e), variant: 'destructive' });
    }
  }

  function onFormKey(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      closeForm();
    }
  }
</script>

<div class="page-h">
  <h1>Skills</h1>
  <p>
    Saved instructions you run by typing / and their name: in a meeting's Ask box on that meeting, or in Chat and Ask
    across meetings on all of them. Every answer cites the moments it comes from.
  </p>
</div>

<!-- A Skill's "Show in the / menu" switch. -->
{#snippet menuSwitch(s)}
  <span class="sw" title="Show in the / menu">
    <Toggle checked={s.shown} ariaLabel={`Show /${s.command} in the / menu`} onchange={(on) => show(s, on)} />
  </span>
{/snippet}

<!-- One of Yap's: what it does, Open for its instructions, its switch. -->
{#snippet yaps(s)}
  <div class="skill" class:open={reading === s.id} class:off={!s.shown} data-skill={s.command}>
    <div class="lhs">
      <span class="title"><span class="cmd">/{s.command}</span><span class="name">{s.name}</span></span>
      <span class="line">{s.description}</span>
    </div>
    <div class="rhs">
      <Button variant="ghost" size="sm" onclick={() => (reading = reading === s.id ? null : s.id)}>{reading === s.id ? 'Close' : 'Open'}</Button>
      {@render menuSwitch(s)}
    </div>
    {#if reading === s.id}
      <div class="detail">
        <p class="where">{whereItRuns(s.runsOn)}{writesDocument(s) ? ' In Chat, it writes its answer as a document beside the chat.' : ''}</p>
        <p class="instr" aria-label={`Instructions of /${s.command}`}>{s.instructions}</p>
        <div class="dfoot">
          <span class="dnote">Make it your own and change what it does.</span>
          <Button variant="secondary" size="sm" onclick={() => openForm(s, true)}>Save a copy</Button>
        </div>
      </div>
    {/if}
  </div>
{/snippet}

<Group title="Your Skills" desc="Your own instructions, in the / menu wherever they run." id="settings-skills-yours" bare>
  <div class="list" data-setting="skills-yours">
    {#each groups.mine as s (s.id)}
      {#if draft?.id === s.id}
        {@render form()}
      {:else}
        <div class="skill" class:off={!s.shown} data-skill={s.command}>
          <div class="lhs">
            <span class="title"><span class="cmd">/{s.command}</span><span class="name">{s.name}</span></span>
            <span class="line clip">{s.description || s.instructions}</span>
          </div>
          <div class="rhs">
            <Button variant="ghost" size="sm" onclick={() => openForm(s)}>Edit</Button>
            <button class="tool del" title="Delete" aria-label={`Delete /${s.command}`} onclick={() => askDelete(s)}>
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 7h14M10 7V5h4v2M7 7l1 12h8l1-12" /></svg>
            </button>
            {@render menuSwitch(s)}
          </div>
        </div>
      {/if}
    {:else}
      {#if !draft}<p class="empty">Skills you make show up here: your own instructions, run with a /command.</p>{/if}
    {/each}
    {#if draft && draft.id == null}
      {@render form()}
    {:else if !draft}
      <div class="addrow">
        <button class="add" data-setting="skills-add" onclick={() => openForm()}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
          New Skill
        </button>
      </div>
    {/if}
  </div>
</Group>

<Group title="Yap's Skills" desc="In the / menu from the start. Open one to read its instructions, or save a copy to change it." id="settings-skills-builtin" bare>
  <div class="list" data-setting="skills-builtin">
    {#each groups.yaps as s (s.id)}{@render yaps(s)}{/each}
  </div>
</Group>

<Group title="More Skills" desc="Switch one on to add it to the / menu. Off, it still runs when you type its whole name, like /tickets." id="settings-skills-more" bare>
  <div class="list" data-setting="skills-more">
    {#each groups.more as s (s.id)}{@render yaps(s)}{/each}
  </div>
</Group>

{#snippet form()}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="form" bind:this={formEl} role="group" aria-label={draft.id ? 'Edit Skill' : 'New Skill'} onkeydown={onFormKey}>
    {#if draft.basedOn}
      <p class="hint">Based on Yap's /{draft.basedOn}.</p>
    {/if}
    <label class="field">
      <span>Name</span>
      <Input bind:value={draft.name} placeholder="Customer follow-up" maxlength={60} ariaLabel="Skill name" oninput={nameChanged} />
    </label>
    <label class="field">
      <span>Command</span>
      <div class="cmdfield">
        <span class="slash" aria-hidden="true">/</span>
        <Input bind:value={draft.command} placeholder="customer-follow-up" maxlength={32} ariaLabel="Skill command" oninput={() => (draft.commandTouched = true)} />
      </div>
    </label>
    <label class="field">
      <span>What it's for <em>(optional)</em></span>
      <Input bind:value={draft.description} placeholder="One line on what it does, shown in the / menu" maxlength={160} ariaLabel="Skill description" />
    </label>
    <label class="field">
      <span>Instructions</span>
      <Textarea
        bind:value={draft.instructions}
        rows={5}
        maxlength={INSTRUCTIONS_MAX}
        placeholder="Say what it should do, the rules to follow and how the answer should look."
        ariaLabel="Skill instructions"
      />
    </label>
    <div class="field">
      <span>Runs on</span>
      <Segmented bind:value={draft.runsOn} options={RUNS_ON} label="Runs on" />
      <p class="hint">{whereItRuns(draft.runsOn)}</p>
    </div>
    {#if problem}<p class="problem" role="status">{problem}</p>{/if}
    <div class="actions">
      <Button variant="secondary" size="sm" onclick={closeForm}>Cancel</Button>
      <Button size="sm" disabled={!!problem || saving} onclick={save}>{draft.id ? 'Save changes' : 'Save Skill'}</Button>
    </div>
  </div>
{/snippet}

<ConfirmDialog
  bind:open={confirmOpen}
  title={deleting ? `Delete /${deleting.command}?` : ''}
  body="Answers it already gave stay in your chats."
  confirmLabel="Delete"
  onconfirm={doDelete}
/>

<style>
  .list {
    display: flex;
    flex-direction: column;
    border-top: 1px solid var(--yap-rule);
  }
  .skill {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 20px;
    min-height: 58px;
    box-sizing: border-box;
    padding: 11px 0;
    border-bottom: 1px solid var(--yap-rule);
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
  .line {
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--yap-muted);
  }
  .line.clip {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* Off the menu: quieter, never greyed out (it still runs typed in full). */
  .skill.off .cmd {
    color: var(--yap-fg-80);
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
  .detail {
    grid-column: 1 / -1;
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 10px;
    padding: 12px 14px;
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
  }
  .where {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-fg-80);
  }
  .instr {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--yap-fg-80);
    white-space: pre-wrap;
  }
  .dfoot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-top: 2px;
  }
  .dnote {
    font-size: 12px;
    color: var(--yap-muted);
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
  .add:hover {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
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
  .cmdfield {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .slash {
    font-weight: 680;
    color: var(--yap-muted);
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
    .dfoot {
      flex-direction: column;
      align-items: flex-start;
    }
  }
</style>
