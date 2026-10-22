<script>
  // Settings → Memory (src-tauri/src/memory.rs, 2026-10-22): what Yap
  // remembers about the person, laid out like Claude's Settings → Memory in
  // Yap's own look.
  // - The switches (cfg, Settings' auto-save): "Search and reference past
  //   chats", "Learn from my chats", "Learn from meetings", "Include
  //   sensitive topics in memory".
  // - The sections: You (Profile, Preferences, then About you's name, region
  //   and note — set in About you, each row goes there; memory keeps no
  //   second copy of them, ops::drop_about_you), People, Projects,
  //   Topics — each row a
  //   name, one line and "Updated 5 Oct"; a row opens to edit it (name, who
  //   or what it is, facts one per line, "Spell this name in dictation" for
  //   people and projects), delete it (Undo on the toast), or add one.
  // - "Tell Yap what to change": the Chat model proposes changes, shown as a
  //   diff to apply (Undo on the toast) or discard.
  // - Import (a prompt to copy into ChatGPT / Claude / Gemini, its answer
  //   pasted back, the changes reviewed one by one) and Export (a .zip of
  //   Markdown, one file per section, through the save dialog).
  // - "Clear all memory" (asked first), and the privacy line.
  //   cfg: Settings' config $state.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import Group from './ui/Group.svelte';
  import Row from './ui/Row.svelte';
  import Toggle from './ui/Toggle.svelte';
  import Button from './ui/Button.svelte';
  import Input from './ui/Input.svelte';
  import Textarea from './ui/Textarea.svelte';
  import { toast } from './ui/toast.svelte.js';
  import { askConfirm } from './ui/contextMenu.svelte.js';
  import { memory, loadMemory, changedToast } from './memory.svelte.js';

  let { cfg } = $props();

  onMount(loadMemory);

  const goto = (where) => window.dispatchEvent(new CustomEvent('yap-settings-goto', { detail: where }));

  // ---- the sections ---------------------------------------------------------------------------
  const YOU = [
    { section: 'profile', title: 'Profile', line: 'Who you are: where you live, your work, what you do' },
    { section: 'preferences', title: 'Preferences', line: 'How you want Yap’s AI to answer you' },
  ];
  const LISTS = [
    { section: 'people', title: 'People', one: 'person', add: 'Add a person', aboutLabel: 'Who they are to you', aboutHint: 'Sister, colleague at Acme…' },
    { section: 'projects', title: 'Projects', one: 'project', add: 'Add a project', aboutLabel: 'What it is', aboutHint: 'My dictation app…' },
    { section: 'topics', title: 'Topics', one: 'topic', add: 'Add a topic', aboutLabel: 'What it is to you', aboutHint: 'A game I play…' },
  ];
  const bySection = (s) =>
    memory.items.filter((i) => i.section === s).sort((a, b) => (b.updatedTs ?? 0) - (a.updatedTs ?? 0));
  const youItem = (s) => memory.items.find((i) => i.section === s) ?? null;

  // About you (AboutYouSettings.svelte, about_you.rs) is part of "You": its
  // name, region and note, read as Settings has them now (before the
  // auto-save, like About you itself), each a row that goes there to change.
  let about = $state(null);
  $effect(() => {
    const overrides = {
      regionCountry: cfg.regionCountry ?? '',
      aboutName: cfg.aboutName ?? '',
      aboutMe: cfg.aboutMe ?? '',
    };
    let stale = false;
    invoke('about_you_status', { overrides })
      .then((s) => {
        if (!stale) about = s;
      })
      .catch(() => {});
    return () => {
      stale = true;
    };
  });
  const aboutRows = $derived.by(() => {
    const name = (cfg.aboutName ?? '').trim();
    const note = (cfg.aboutMe ?? '').trim();
    const region = about?.region;
    const country = region?.countryName;
    const regionLine = !about
      ? '…'
      : country
        ? [region.automatic ? `${country} (from Windows)` : country, about.summary].filter(Boolean).join(' · ')
        : 'Not set: answers come without your prices and units';
    return [
      { key: 'name', title: 'Name', set: !!name, line: name || 'Not set: what Yap should call you' },
      { key: 'region', title: 'Region', set: !!country, line: regionLine },
      { key: 'note', title: 'Note', set: !!note, line: note ? `“${note}”` : 'Not set: anything the AI should know about you' },
    ];
  });

  /** "Updated 5 Oct" (with the year when it isn't this one). */
  function updated(ts) {
    if (!ts) return '';
    const d = new Date(ts * 1000);
    const sameYear = d.getFullYear() === new Date().getFullYear();
    return `Updated ${d.toLocaleDateString(undefined, { day: 'numeric', month: 'short', ...(sameYear ? {} : { year: 'numeric' }) })}`;
  }

  /** Where an item came from, in words. */
  function origin(i) {
    const t = i.source?.title ? ` (“${i.source.title}”)` : '';
    switch (i.source?.kind) {
      case 'chat':
        return `Learned from a chat${t}`;
      case 'meeting':
        return `Learned from a meeting${t}`;
      case 'voice':
        return 'You asked Yap to remember it';
      case 'tell':
        return 'You told Yap';
      case 'import':
        return 'Imported';
      default:
        return 'Added by you';
    }
  }

  // ---- editing --------------------------------------------------------------------------------
  /** What's open: an item id, or "new:<section>". */
  let open = $state(null);
  let draft = $state({ title: '', summary: '', facts: '', dictation: true });
  let saving = $state(false);

  function edit(item, section) {
    if (open === (item?.id ?? `new:${section}`)) {
      open = null;
      return;
    }
    open = item?.id ?? `new:${section}`;
    draft = {
      title: item?.title ?? '',
      summary: item?.summary ?? '',
      facts: (item?.details ?? []).join('\n'),
      dictation: item?.dictation ?? true,
    };
  }

  async function save(item, section) {
    saving = true;
    try {
      await invoke('memory_save_item', {
        item: {
          id: item?.id ?? null,
          section,
          title: draft.title,
          summary: draft.summary,
          details: draft.facts.split('\n').map((l) => l.trim()).filter(Boolean),
          dictation: draft.dictation,
        },
      });
      open = null;
      await loadMemory();
    } catch (err) {
      toast({ title: "Couldn't save that", description: String(err), variant: 'destructive' });
    } finally {
      saving = false;
    }
  }

  async function remove(item) {
    try {
      const batch = await invoke('memory_delete_item', { id: item.id });
      open = null;
      await loadMemory();
      changedToast(`Deleted “${item.title}”`, batch);
    } catch (err) {
      toast({ title: "Couldn't delete that", description: String(err), variant: 'destructive' });
    }
  }

  async function setDictation(item, on) {
    try {
      await invoke('memory_set_dictation', { id: item.id, on });
      await loadMemory();
    } catch (err) {
      toast({ title: "Couldn't change that", description: String(err), variant: 'destructive' });
    }
  }

  // ---- "Tell Yap what to change" and imports -------------------------------------------------------
  let tellText = $state('');
  let tellBusy = $state(false);
  let tellProposal = $state(null);
  let importOpen = $state(false);
  let importText = $state('');
  let importBusy = $state(false);
  let importProposal = $state(null);
  let importIncluded = $state([]);
  let importPrompt = $state('');
  let applying = $state(false);

  const SECTION_NAME = { profile: 'Profile', preferences: 'Preferences', people: 'People', projects: 'Projects', topics: 'Topics' };

  async function tell() {
    if (!tellText.trim()) return;
    tellBusy = true;
    tellProposal = null;
    try {
      tellProposal = await invoke('memory_tell', { text: tellText });
    } catch (err) {
      toast({ title: "Yap couldn't work that out", description: String(err), variant: 'destructive' });
    } finally {
      tellBusy = false;
    }
  }

  async function apply(proposal, include, done) {
    applying = true;
    try {
      const r = await invoke('memory_apply', { proposal: proposal.id, include });
      done();
      await loadMemory();
      const n = r?.done?.length ?? 0;
      changedToast(n === 1 ? 'Memory updated' : `Memory updated: ${n} changes`, r?.batch, (r?.done ?? []).map((d) => d.say).filter(Boolean).join(' · '));
    } catch (err) {
      toast({ title: "Couldn't change memory", description: String(err), variant: 'destructive' });
    } finally {
      applying = false;
    }
  }

  async function openImport() {
    importOpen = !importOpen;
    if (importOpen && !importPrompt) {
      try {
        importPrompt = await invoke('memory_import_prompt');
      } catch {
        importPrompt = '';
      }
    }
  }

  async function copyPrompt() {
    try {
      await navigator.clipboard.writeText(importPrompt);
      toast({ title: 'Prompt copied', description: 'Paste it into ChatGPT, Claude or Gemini, then paste its answer below.', variant: 'success', duration: 4_000 });
    } catch (err) {
      toast({ title: "Couldn't copy the prompt", description: String(err), variant: 'destructive' });
    }
  }

  async function runImport() {
    if (!importText.trim()) return;
    importBusy = true;
    importProposal = null;
    try {
      importProposal = await invoke('memory_import', { text: importText });
      importIncluded = importProposal.changes.map((c) => c.change);
    } catch (err) {
      toast({ title: "Yap couldn't read that", description: String(err), variant: 'destructive' });
    } finally {
      importBusy = false;
    }
  }

  function toggleIncluded(i) {
    importIncluded = importIncluded.includes(i) ? importIncluded.filter((x) => x !== i) : [...importIncluded, i];
  }

  // ---- export, clear ------------------------------------------------------------------------------
  async function exportMemory() {
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const day = new Date().toISOString().slice(0, 10);
      const path = await save({ defaultPath: `Yap memory ${day}.zip`, filters: [{ name: 'Zip archive', extensions: ['zip'] }] });
      if (!path) return;
      await invoke('memory_export', { path });
      toast({ title: 'Memory exported', description: 'A Markdown file for each section, in one .zip.', variant: 'success', duration: 4_000 });
    } catch (err) {
      toast({ title: "Couldn't export memory", description: String(err), variant: 'destructive' });
    }
  }

  function clearAll(e) {
    askConfirm({
      title: 'Clear all memory?',
      body: 'Yap forgets everything here: you, the people, projects and topics. Your chats and notes stay.',
      confirmLabel: 'Clear all',
      busyLabel: 'Clearing…',
      returnFocus: e?.currentTarget ?? null,
      run: async () => {
        const batch = await invoke('memory_clear');
        await loadMemory();
        changedToast('Memory cleared', batch);
      },
    });
  }

  const total = $derived(memory.items.length);
</script>

{#snippet diff(changes, pick = null)}
  <ul class="diff" aria-label="Proposed changes">
    {#each changes as c (c.change + ':' + c.id)}
      <li class="change {c.kind}">
        {#if pick}
          <input
            type="checkbox"
            checked={pick.includes(c.change)}
            onchange={() => toggleIncluded(c.change)}
            aria-label={`Keep ${c.title}`}
          />
        {/if}
        <span class="mark" aria-hidden="true">{c.kind === 'added' ? '+' : c.kind === 'removed' ? '−' : '~'}</span>
        <span class="what">
          <span class="where">{SECTION_NAME[c.section] ?? c.section}</span>
          {#if !['profile', 'preferences'].includes(c.section)}<strong>{c.title}</strong>{/if}
          {#if c.kind === 'removed'}
            <span class="gone">forgotten</span>
          {:else}
            {#if c.summary && c.summary !== c.summaryBefore}
              {#if c.summaryBefore}<del>{c.summaryBefore}</del>{/if}
              <ins>{c.summary}</ins>
            {/if}
            {#each c.factsRemoved as f}<del>{f}</del>{/each}
            {#each c.factsAdded as f}<ins>{f}</ins>{/each}
          {/if}
        </span>
      </li>
    {/each}
  </ul>
{/snippet}

{#snippet editor(item, section, list)}
  <div class="editor" role="group" aria-label={item ? `Edit ${item.title}` : list ? list.add : `Add to ${section}`}>
    {#if list}
      <label class="field">
        <span>Name</span>
        <Input bind:value={draft.title} ariaLabel="Name" maxlength={60} placeholder={list.one === 'person' ? 'Rochelle' : ''} />
      </label>
      <label class="field">
        <span>{list.aboutLabel}</span>
        <Input bind:value={draft.summary} ariaLabel={list.aboutLabel} maxlength={100} placeholder={list.aboutHint} />
      </label>
    {/if}
    <label class="field">
      <span>{list ? 'What Yap knows' : 'One per line'}</span>
      <Textarea bind:value={draft.facts} rows={list ? 3 : 5} ariaLabel="Facts, one per line" placeholder={list ? 'Lives in York\nBirthday 3 May' : section === 'preferences' ? 'Short answers\nBritish spelling' : 'Works night shifts as a nurse\nLives in Leeds'} />
    </label>
    {#if item && list && (section === 'people' || section === 'projects')}
      <div class="spell">
        <Toggle
          checked={item.dictation}
          label="Spell this name in dictation"
          desc={item.dictationHint === 'fixed'
            ? 'Yap writes it this way when you dictate, and fixes near-misses of it.'
            : 'Yap tells Whisper models how to spell it. Its near-misses aren’t fixed: it’s short or an everyday word.'}
          onchange={(on) => setDictation(item, on)}
        />
      </div>
    {/if}
    {#if item}<p class="from">{origin(item)} · {updated(item.updatedTs)}</p>{/if}
    <div class="actions">
      {#if item}
        <Button variant="ghost" size="sm" onclick={() => remove(item)}>Delete</Button>
      {/if}
      <span class="grow"></span>
      <Button variant="secondary" size="sm" onclick={() => (open = null)}>Cancel</Button>
      <Button size="sm" disabled={saving} onclick={() => save(item, section)}>{saving ? 'Saving…' : 'Save'}</Button>
    </div>
  </div>
{/snippet}

<div class="memory">
  {#if memory.loaded && !memory.ai?.ready}
    <div class="noai" role="note">
      <div>
        <strong>Yap needs an AI model for memory</strong>
        <p>Learning from your chats, “Tell Yap what to change” and imports use your Chat model. Memory you add yourself works without one.</p>
      </div>
      <span class="go"><Button variant="secondary" size="sm" onclick={() => goto('cleanup#chat')}>Language Models</Button></span>
    </div>
  {/if}

  <Group title="Memory">
    <Row>
      <Toggle
        bind:checked={cfg.memorySearchChats}
        label="Search and reference past chats"
        desc="Yap’s AI can look through your earlier chats when you mention one."
      />
    </Row>
    <Row>
      <Toggle
        bind:checked={cfg.memoryLearnChats}
        label="Learn from my chats"
        desc="When a chat goes quiet, Yap keeps lasting facts about you from it: never passwords, numbers or addresses."
      />
    </Row>
    {#if memory.loaded && memory.ai?.ready && memory.ai?.learns === false && (cfg.memoryLearnChats || cfg.memoryLearnMeetings)}
      <Row>
        <p class="small-model" role="note">
          Your Chat model is too small to pick out what to remember reliably, so Yap doesn’t learn by itself with it.
          It does with Qwen3.5 4B on this PC or a cloud model. “Tell Yap what to change”, imports and
          “Remember that…” in Chat still work.
          <button type="button" class="link" onclick={() => goto('cleanup#chat')}>Language Models</button>
        </p>
      </Row>
    {/if}
    <Row>
      <Toggle
        bind:checked={cfg.memoryLearnMeetings}
        label="Learn from meetings"
        desc="Who your attendees are and the projects you work on, from finished meetings’ action plans. Off by default: it’s other people’s words."
      />
    </Row>
    <Row>
      <Toggle
        bind:checked={cfg.memorySensitive}
        label="Include sensitive topics in memory"
        desc="Health, religion, politics, sexuality and the like. Off: Yap leaves them out of what it learns by itself."
      />
    </Row>
  </Group>

  <Group title="You">
    {#each YOU as y (y.section)}
      {@const item = youItem(y.section)}
      <div class="item">
        <button
          type="button"
          class="row"
          aria-expanded={open === (item?.id ?? `new:${y.section}`)}
          onclick={() => edit(item, y.section)}
        >
          <span class="name">{y.title}</span>
          <span class="line">{item?.details?.length ? item.details.join(' · ') : y.line}</span>
          <span class="when">{item ? updated(item.updatedTs) : 'Nothing yet'}</span>
        </button>
        {#if open === (item?.id ?? `new:${y.section}`)}
          {@render editor(item, y.section, null)}
        {/if}
      </div>
    {/each}
    {#each aboutRows as a (a.key)}
      <div class="item">
        <button
          type="button"
          class="row"
          data-about={a.key}
          aria-label={`${a.title}: ${a.line}. Change it in About you`}
          onclick={() => goto('general#about-you')}
        >
          <span class="name">{a.title}</span>
          <span class="line" class:unset={!a.set}>{a.line}</span>
          <span class="when link">In About you</span>
        </button>
      </div>
    {/each}
  </Group>

  {#each LISTS as list (list.section)}
    {@const rows = bySection(list.section)}
    <Group title={list.title}>
      {#each rows as item (item.id)}
        <div class="item">
          <button type="button" class="row" aria-expanded={open === item.id} onclick={() => edit(item, list.section)}>
            <span class="name">{item.title}</span>
            <span class="line">{item.summary || item.details?.[0] || ''}</span>
            <span class="when">{updated(item.updatedTs)}</span>
          </button>
          {#if open === item.id}
            {@render editor(item, list.section, list)}
          {/if}
        </div>
      {:else}
        <p class="empty">Nothing yet.</p>
      {/each}
      <div class="item">
        <button
          type="button"
          class="row add"
          aria-expanded={open === `new:${list.section}`}
          onclick={() => edit(null, list.section)}
        >
          <span class="plus" aria-hidden="true">+</span>
          <span class="name">{list.add}</span>
        </button>
        {#if open === `new:${list.section}`}
          {@render editor(null, list.section, list)}
        {/if}
      </div>
    </Group>
  {/each}

  <Group title="Tell Yap what to change">
    <Row>
      <div class="tell">
        <Textarea
          bind:value={tellText}
          rows={2}
          maxlength={1500}
          ariaLabel="Tell Yap what to change"
          placeholder="My dog's name is Beans · Forget everything about my old job"
        />
        <div class="actions">
          <span class="hint">Yap’s AI turns it into changes you can check first.</span>
          <span class="grow"></span>
          <Button size="sm" disabled={tellBusy || !tellText.trim() || !memory.ai?.ready} onclick={tell}>
            {tellBusy ? 'Working it out…' : 'Suggest changes'}
          </Button>
        </div>
        {#if tellProposal}
          {#if tellProposal.changes.length}
            {@render diff(tellProposal.changes)}
            <div class="actions">
              <span class="grow"></span>
              <Button variant="secondary" size="sm" onclick={() => (tellProposal = null)}>Discard</Button>
              <Button
                size="sm"
                disabled={applying}
                onclick={() => apply(tellProposal, null, () => {
                  tellProposal = null;
                  tellText = '';
                })}>Apply</Button
              >
            </div>
          {:else}
            <p class="none" role="status">Nothing in memory changes for that.</p>
          {/if}
        {/if}
      </div>
    </Row>
  </Group>

  <Group title="Bring it in, take it out">
    <Row label="Import memory from other AI providers" desc="Bring in what ChatGPT, Claude or Gemini remembers about you.">
      <Button variant="secondary" size="sm" onclick={openImport}>{importOpen ? 'Close' : 'Import'}</Button>
    </Row>
    {#if importOpen}
      <Row>
        <div class="import">
          <p class="step"><span>1</span> Copy this prompt and send it to the other assistant.</p>
          <div class="prompt">
            <p>{importPrompt}</p>
            <Button variant="secondary" size="sm" onclick={copyPrompt}>Copy prompt</Button>
          </div>
          <p class="step"><span>2</span> Paste its answer here. You check what Yap keeps before it’s saved.</p>
          <Textarea bind:value={importText} rows={5} ariaLabel="The other assistant's answer" placeholder="Paste the answer…" />
          <div class="actions">
            <span class="grow"></span>
            <Button size="sm" disabled={importBusy || !importText.trim() || !memory.ai?.ready} onclick={runImport}>
              {importBusy ? 'Reading it…' : 'Turn into memory'}
            </Button>
          </div>
          {#if importProposal}
            {#if importProposal.changes.length}
              {@render diff(importProposal.changes, importIncluded)}
              <div class="actions">
                <span class="grow"></span>
                <Button variant="secondary" size="sm" onclick={() => (importProposal = null)}>Discard</Button>
                <Button
                  size="sm"
                  disabled={applying || !importIncluded.length}
                  onclick={() => apply(importProposal, importIncluded, () => {
                    importProposal = null;
                    importText = '';
                    importOpen = false;
                  })}>Save {importIncluded.length}</Button
                >
              </div>
            {:else}
              <p class="none" role="status">Yap found nothing to keep in that.</p>
            {/if}
          {/if}
        </div>
      </Row>
    {/if}
    <Row label="Export memory" desc="A .zip with a Markdown file for each section.">
      <Button variant="secondary" size="sm" onclick={exportMemory}>Export</Button>
    </Row>
    <Row label="Clear all memory" desc={total ? `${total} ${total === 1 ? 'thing' : 'things'} Yap remembers` : 'Memory is empty'}>
      <Button variant="danger" size="sm" disabled={!total} onclick={clearAll}>Clear all</Button>
    </Row>
  </Group>

  <p class="privacy">
    Memory is kept on this PC. It goes with your questions to the AI model you use (a cloud provider sees it; a model on this PC keeps it here) and is never sent to Yap.
  </p>
</div>

<style>
  .memory {
    display: flex;
    flex-direction: column;
  }
  .noai {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 14px 16px;
    margin-bottom: 22px;
    border: 1px solid var(--yap-primary-line);
    background: var(--yap-primary-wash);
    border-radius: var(--yap-r-lg);
  }
  .small-model {
    margin: 0;
    font-size: 12px;
    line-height: 1.55;
    color: var(--yap-fg-62);
  }
  .link {
    background: none;
    border: 0;
    padding: 0;
    font: inherit;
    font-weight: 600;
    color: var(--yap-primary);
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  .noai .go {
    flex: 0 0 auto;
    white-space: nowrap;
  }
  .noai strong {
    font-size: 13px;
    color: var(--yap-fg);
  }
  .noai p {
    margin: 3px 0 0;
    font-size: 12px;
    color: var(--yap-fg-62);
    line-height: 1.5;
  }
  .item {
    display: block;
  }
  .row {
    width: 100%;
    display: grid;
    /* Fixed outer columns, so every row's line starts at the same place. */
    grid-template-columns: 150px minmax(0, 1fr) 112px;
    align-items: baseline;
    gap: 14px;
    padding: 12px 16px;
    background: transparent;
    border: 0;
    font: inherit;
    text-align: left;
    cursor: pointer;
    color: var(--yap-fg);
    transition: background var(--yap-dur) ease;
  }
  .row:hover,
  .row[aria-expanded='true'] {
    background: var(--yap-s3);
  }
  .row:focus-visible {
    outline: 2px solid var(--yap-primary);
    outline-offset: -2px;
  }
  .name {
    font-size: 13px;
    font-weight: 650;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .line {
    font-size: 12.5px;
    color: var(--yap-fg-62);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .when {
    font-size: 11.5px;
    color: var(--yap-muted-70);
    white-space: nowrap;
    text-align: right;
  }
  .when.link {
    color: var(--yap-primary);
    font-weight: 600;
  }
  .line.unset {
    color: var(--yap-muted-70);
  }
  .row.add {
    grid-template-columns: auto 1fr;
    gap: 8px;
  }
  .row.add .name {
    font-weight: 600;
    color: var(--yap-fg-80);
  }
  .plus {
    width: 18px;
    height: 18px;
    display: inline-grid;
    place-items: center;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised);
    color: var(--yap-fg-80);
    font-weight: 700;
    font-size: 13px;
    line-height: 1;
  }
  .empty {
    margin: 0;
    padding: 12px 16px;
    font-size: 12.5px;
    color: var(--yap-muted-70);
  }
  .editor {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 4px 16px 14px;
    background: var(--yap-s3);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }
  .field > span {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--yap-fg-62);
  }
  .spell :global(.toggle-row) {
    padding: 0;
  }
  .from {
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-muted-70);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .grow {
    flex: 1;
  }
  .tell,
  .import {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .hint {
    font-size: 11.5px;
    color: var(--yap-muted-70);
  }
  .diff {
    list-style: none;
    margin: 0;
    padding: 6px 0;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
  }
  .change {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 6px 12px;
    font-size: 12.5px;
    line-height: 1.5;
  }
  .change input {
    accent-color: var(--yap-primary);
    margin: 0;
    align-self: center;
  }
  .mark {
    width: 12px;
    font-weight: 700;
    text-align: center;
    color: var(--yap-muted);
  }
  .change.added .mark {
    color: var(--yap-success);
  }
  .change.removed .mark {
    color: var(--yap-danger);
  }
  .change.updated .mark {
    color: var(--yap-primary);
  }
  .what {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 8px;
    min-width: 0;
  }
  .where {
    font-size: 10.5px;
    font-weight: 650;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--yap-muted-70);
  }
  ins,
  del {
    text-decoration: none;
    border-radius: var(--yap-r-sm);
    padding: 0 5px;
  }
  ins {
    background: color-mix(in srgb, var(--yap-success) 12%, transparent);
    color: var(--yap-fg);
  }
  del {
    background: color-mix(in srgb, var(--yap-danger) 10%, transparent);
    color: var(--yap-fg-62);
    text-decoration: line-through;
    text-decoration-color: color-mix(in srgb, var(--yap-danger) 55%, transparent);
  }
  .gone {
    color: var(--yap-danger);
    font-size: 12px;
  }
  .none {
    margin: 0;
    font-size: 12.5px;
    color: var(--yap-fg-62);
  }
  .step {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--yap-fg-80);
  }
  .step span {
    width: 18px;
    height: 18px;
    display: inline-grid;
    place-items: center;
    border-radius: var(--yap-r-full);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    font-size: 11px;
    font-weight: 700;
  }
  .prompt {
    display: flex;
    gap: 12px;
    align-items: flex-start;
    padding: 10px 12px;
    border: 1px dashed var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
  }
  .prompt p {
    margin: 0;
    flex: 1;
    font-size: 12px;
    line-height: 1.55;
    color: var(--yap-fg-80);
  }
  .privacy {
    margin: 4px 2px 0;
    font-size: 11.5px;
    line-height: 1.55;
    color: var(--yap-muted-70);
  }
</style>
