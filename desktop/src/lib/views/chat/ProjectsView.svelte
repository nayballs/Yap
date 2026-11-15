<script>
  // **Projects** (chat/projects.rs, 15 Nov 2026; the Claude app's projects):
  // in the thread's place when Projects is open in the conversation list.
  // - The list (`chat.page === 'projects'`): every project, last changed
  //   first, and New project (a name and what it's for).
  // - One project (`chat.page.project`): its name and what it's for (Edit
  //   details), New chat in it, its chats; beside them its Instructions
  //   (saved as typed, a moment after) and its Files (Add files…, ×). Its
  //   ⋯: Edit details, Delete project… (its chats stay, out of it).
  import { invoke } from '@tauri-apps/api/core';
  import { tick } from 'svelte';
  import ConfirmDialog from '../../ui/ConfirmDialog.svelte';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { kindLabel, sizeLabel, whenLabel } from './projectLines.js';

  let { chat } = $props();

  let list = $state([]);
  let current = $state(null); // { project, chats: [ids] }
  let making = $state(false);
  let draftName = $state('');
  let draftAbout = $state('');
  let editing = $state(false);
  let instructions = $state('');
  let instructionsSaved = $state(true);
  let saveTimer = null;
  let adding = $state(false);
  let deleteOpen = $state(false);
  let deleting = $state(false);
  let headEl = $state(null);

  const projectId = $derived(typeof chat.page === 'object' && chat.page ? chat.page.project : null);
  const chatsHere = $derived(current ? chat.conversations.filter((c) => current.chats.includes(c.id)) : []);

  async function loadList() {
    try {
      list = (await invoke('projects_list')) || [];
    } catch (e) {
      toast({ title: "Couldn't read your projects", description: String(e), variant: 'destructive' });
    }
  }
  async function loadProject(id) {
    try {
      current = await invoke('project_get', { id });
      instructions = current.project.instructions || '';
      instructionsSaved = true;
    } catch (e) {
      toast({ title: "Couldn't open the project", description: String(e), variant: 'destructive' });
      chat.openProjects();
    }
  }
  $effect(() => {
    const id = projectId;
    editing = false;
    if (id) loadProject(id);
    else {
      current = null;
      loadList();
    }
    tick().then(() => headEl?.focus({ preventScroll: true }));
  });

  async function create() {
    const name = draftName.trim();
    if (!name) return;
    try {
      const p = await invoke('project_create', { name, description: draftAbout });
      making = false;
      draftName = '';
      draftAbout = '';
      chat.projectsChanged();
      chat.openProject(p.id);
    } catch (e) {
      toast({ title: "Couldn't make the project", description: String(e), variant: 'destructive' });
    }
  }

  async function saveDetails() {
    if (!current) return;
    try {
      const p = await invoke('project_update', { id: current.project.id, name: draftName, description: draftAbout });
      current = { ...current, project: p };
      editing = false;
      chat.projectsChanged();
    } catch (e) {
      toast({ title: "Couldn't save the project", description: String(e), variant: 'destructive' });
    }
  }
  function startEdit() {
    draftName = current.project.name;
    draftAbout = current.project.description;
    editing = true;
  }

  // Instructions are saved a moment after the typing stops (and on leaving the box).
  function typed() {
    instructionsSaved = false;
    clearTimeout(saveTimer);
    saveTimer = setTimeout(saveInstructions, 700);
  }
  async function saveInstructions() {
    clearTimeout(saveTimer);
    if (!current || instructionsSaved) return;
    try {
      const p = await invoke('project_update', { id: current.project.id, instructions });
      current = { ...current, project: p };
      instructionsSaved = true;
    } catch (e) {
      toast({ title: "Couldn't save the instructions", description: String(e), variant: 'destructive' });
    }
  }

  async function addFiles() {
    if (!current || adding) return;
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const picked = await open({ multiple: true, title: `Add files to ${current.project.name}` });
      const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
      if (!paths.length) return;
      adding = true;
      const out = await invoke('project_files_add', { id: current.project.id, paths });
      for (const s of out?.skipped || []) toast({ title: `Couldn't add ${s.name}`, description: s.why, variant: 'destructive' });
      await loadProject(current.project.id);
    } catch (e) {
      toast({ title: "Couldn't add the files", description: String(e), variant: 'destructive' });
    } finally {
      adding = false;
    }
  }
  async function removeFile(f) {
    try {
      await invoke('project_file_remove', { id: current.project.id, file: f.id });
      await loadProject(current.project.id);
    } catch (e) {
      toast({ title: "Couldn't remove the file", description: String(e), variant: 'destructive' });
    }
  }

  async function doDelete() {
    if (!current) return;
    deleting = true;
    try {
      await invoke('project_delete', { id: current.project.id });
      deleteOpen = false;
      chat.projectsChanged();
      chat.openProjects();
    } catch (e) {
      toast({ title: "Couldn't delete the project", description: String(e), variant: 'destructive' });
    } finally {
      deleting = false;
    }
  }

  function moreMenu(e) {
    openContextMenu(
      e,
      [
        { label: 'Edit details', icon: 'pencil', onselect: startEdit },
        { separator: true },
        { label: 'Delete project…', icon: 'trash', danger: true, onselect: () => (deleteOpen = true) },
      ],
      { label: `Project ${current?.project?.name || ''}` }
    );
  }
</script>

{#if !projectId}
  <!-- Every project, and New project. -->
  <section class="projects" aria-labelledby="projects-title" data-testid="chat-projects">
    <div class="phead">
      <h2 id="projects-title" tabindex="-1" bind:this={headEl}>Projects</h2>
      {#if !making}<button type="button" class="btn primary" onclick={() => ((making = true), tick().then(() => document.getElementById('project-name')?.focus()))}>New project</button>{/if}
    </div>
    <p class="sub">A project keeps instructions and files that every chat in it gets.</p>
    {#if making}
      <form class="make" aria-label="New project" onsubmit={(e) => (e.preventDefault(), create())}>
        <label class="field"><span>Name</span><input id="project-name" type="text" maxlength="60" bind:value={draftName} placeholder="Garden 2027" /></label>
        <label class="field"><span>What it's for <em>(optional)</em></span><input type="text" maxlength="300" bind:value={draftAbout} placeholder="Planning the allotment's year" /></label>
        <div class="actions">
          <button type="button" class="btn quiet" onclick={() => (making = false)}>Cancel</button>
          <button type="submit" class="btn primary" disabled={!draftName.trim()}>Create project</button>
        </div>
      </form>
    {/if}
    {#if list.length}
      <ul class="cards" aria-label="Your projects">
        {#each list as p (p.id)}
          <li>
            <button type="button" class="card" onclick={() => chat.openProject(p.id)}>
              <span class="cname">{p.name}</span>
              {#if p.description}<span class="cabout">{p.description}</span>{/if}
              <span class="cmeta">{p.chats === 1 ? '1 chat' : `${p.chats} chats`} · {p.files === 1 ? '1 file' : `${p.files} files`} · {whenLabel(p.updatedMs)}</span>
            </button>
          </li>
        {/each}
      </ul>
    {:else if !making}
      <p class="none">No projects yet.</p>
    {/if}
  </section>
{:else if current}
  {@const p = current.project}
  <section class="project" aria-labelledby="project-title" data-testid="chat-project">
    <button type="button" class="back" onclick={() => chat.openProjects()}>‹ Projects</button>
    {#if editing}
      <form class="make" aria-label="Project details" onsubmit={(e) => (e.preventDefault(), saveDetails())}>
        <label class="field"><span>Name</span><input type="text" maxlength="60" bind:value={draftName} /></label>
        <label class="field"><span>What it's for</span><input type="text" maxlength="300" bind:value={draftAbout} /></label>
        <div class="actions">
          <button type="button" class="btn quiet" onclick={() => (editing = false)}>Cancel</button>
          <button type="submit" class="btn primary" disabled={!draftName.trim()}>Save</button>
        </div>
      </form>
    {:else}
      <div class="phead">
        <h2 id="project-title" tabindex="-1" bind:this={headEl}>{p.name}</h2>
        <button type="button" class="more" aria-label={`More for ${p.name}`} aria-haspopup="menu" data-more onclick={moreMenu}>⋯</button>
      </div>
      {#if p.description}<p class="sub">{p.description}</p>{/if}
    {/if}
    <div class="cols">
      <div class="main">
        <button type="button" class="btn primary newchat" onclick={() => chat.newChat({ project: { id: p.id, name: p.name } })}>New chat in {p.name}</button>
        <h3>Chats</h3>
        {#if chatsHere.length}
          <ul class="chats" aria-label={`Chats in ${p.name}`}>
            {#each chatsHere as c (c.id)}
              <li><button type="button" class="chatrow" onclick={() => chat.selectConversation(c.id)}><span>{c.title || 'Untitled'}</span><span class="cmeta">{whenLabel(c.updatedTs * 1000)}</span></button></li>
            {/each}
          </ul>
        {:else}
          <p class="none">No chats in it yet.</p>
        {/if}
      </div>
      <aside class="side" aria-label="Project knowledge">
        <h3><label for="project-instructions">Instructions</label></h3>
        <textarea
          id="project-instructions"
          rows="6"
          maxlength="8000"
          bind:value={instructions}
          oninput={typed}
          onblur={saveInstructions}
          placeholder="How every answer in this project should go: who it's for, what to assume, what to always do."
        ></textarea>
        <p class="saved" aria-live="polite">{instructionsSaved ? (instructions.trim() ? 'Saved' : '') : 'Saving…'}</p>
        <div class="fhead">
          <h3>Files</h3>
          <button type="button" class="btn" disabled={adding} onclick={addFiles}>{adding ? 'Adding…' : 'Add files…'}</button>
        </div>
        {#if p.files.length}
          <ul class="files" aria-label={`Files in ${p.name}`}>
            {#each p.files as f (f.id)}
              <li class="file">
                <span class="fname">{f.name}</span>
                <span class="cmeta">{kindLabel(f)} · {sizeLabel(f)}</span>
                <button type="button" class="x" aria-label={`Remove ${f.name}`} onclick={() => removeFile(f)}>×</button>
              </li>
            {/each}
          </ul>
        {:else}
          <p class="none">Text, Markdown, code, PDFs and Word documents: every chat here reads them.</p>
        {/if}
      </aside>
    </div>
  </section>
{/if}

<ConfirmDialog
  bind:open={deleteOpen}
  title={current ? `Delete \u201c${current.project.name}\u201d?` : ''}
  body="Its instructions and files go. Its chats stay, out of the project."
  busy={deleting}
  onconfirm={doDelete}
/>

<style>
  .projects,
  .project {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    width: 100%;
    max-width: var(--yap-thread-width);
    margin: 0 auto;
    padding: 8px 4px 24px;
    box-sizing: border-box;
  }
  .phead {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  h2 {
    flex: 1 1 auto;
    margin: 0;
    font-size: 20px;
    font-weight: 650;
    line-height: 28px;
    outline: none;
  }
  h3 {
    margin: 18px 0 8px;
    font-size: 12.5px;
    font-weight: 650;
    color: var(--yap-fg-80);
  }
  .sub,
  .none {
    margin: 6px 0 0;
    font-size: 13px;
    line-height: 19px;
    color: var(--yap-muted);
  }
  .back {
    margin: 0 0 6px;
    padding: 2px 0;
    border: none;
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .back:hover {
    color: var(--yap-fg);
  }
  .cards {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 10px;
    margin: 16px 0 0;
    padding: 0;
    list-style: none;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 100%;
    height: 100%;
    padding: 12px 14px;
    box-sizing: border-box;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .card:hover {
    border-color: var(--yap-border-hover);
    background: var(--yap-s2);
  }
  .cname {
    font-size: 14px;
    font-weight: 650;
  }
  .cabout {
    font-size: 12.5px;
    color: var(--yap-fg-80);
  }
  .cmeta {
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .make {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 14px 0 0;
    padding: 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s1);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-fg-80);
  }
  .field em {
    font-style: normal;
    font-weight: 400;
    color: var(--yap-muted);
  }
  input[type='text'],
  textarea {
    box-sizing: border-box;
    width: 100%;
    padding: 7px 9px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    font-weight: 400;
    resize: vertical;
  }
  input[type='text']:focus,
  textarea:focus {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  /* Its chats, and beside them (below, when narrow) its knowledge. */
  .cols {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 28px;
    margin-top: 14px;
  }
  .main {
    flex: 1 1 320px;
  }
  .side {
    flex: 1 1 240px;
    max-width: 100%;
  }
  .newchat {
    align-self: flex-start;
  }
  .main {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .side {
    min-width: 0;
  }
  .chats,
  .files {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .chatrow {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 8px 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .chatrow span:first-child {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chatrow:hover {
    background: var(--yap-s2);
  }
  .side h3:first-child {
    margin-top: 0;
  }
  .saved {
    min-height: 16px;
    margin: 4px 0 0;
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .fhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .file {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    column-gap: 6px;
    padding: 6px 0;
    border-bottom: 1px solid var(--yap-border-subtle);
  }
  .fname {
    overflow: hidden;
    font-size: 13px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .file .cmeta {
    grid-column: 1;
  }
  .x {
    grid-row: 1 / span 2;
    grid-column: 2;
    align-self: center;
    width: 24px;
    height: 24px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    font-size: 16px;
    cursor: pointer;
  }
  .x:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .more {
    width: 30px;
    height: 30px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    font-size: 16px;
    cursor: pointer;
  }
  .more:hover {
    background: var(--yap-s2);
  }
  .btn {
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-full);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
  }
  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .btn.primary {
    border-color: var(--yap-primary);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .btn.quiet {
    border-color: transparent;
    background: none;
  }
  .btn:focus-visible,
  .card:focus-visible,
  .chatrow:focus-visible,
  .x:focus-visible,
  .more:focus-visible,
  .back:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
  }
</style>
