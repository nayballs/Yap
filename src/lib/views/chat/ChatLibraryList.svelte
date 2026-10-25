<script>
  // The Chat sidebar's search results and its archive, in place of the
  // conversation list (ConversationList loads it the first time either is
  // shown; the state is chatLibrary.svelte.js):
  //
  // - Results: every conversation, archived ones too, holding the words
  //   typed (its title, its messages), and by meaning a few that mean them;
  //   each with the message it found, the words marked. A click opens the
  //   conversation at that message.
  // - Archived: each archived conversation opens on a click; Restore and
  //   Delete… on the row and in its menu (right-click, ⋯ keys), every delete
  //   asked first.
  //
  //   chat:     the view's state (chatState.svelte.js)
  //   lib:      the search and archive (chatLibrary.svelte.js)
  //   searchEl: the search box (↑ from the first result goes back to it)
  //   oncopy:   Copy as Markdown for a conversation id
  import { openContextMenu, menuKey, devInspect } from '../../ui/contextMenu.svelte.js';

  let { chat, lib, searchEl = null, oncopy = () => {} } = $props();

  const titleOf = (id) => chat.conversations.find((c) => c.id === id)?.title || lib.archived.find((c) => c.id === id)?.title || '';
  const isArchived = (id) => lib.archived.some((c) => c.id === id);
  let hitEls = $state([]);
  let archivedEls = $state({});

  /** The search box's keys: ↓ to the first result, Enter opens it. */
  export function keydown(e) {
    const hits = lib.found?.hits || [];
    if (e.key === 'ArrowDown' && hits.length) {
      e.preventDefault();
      hitEls[0]?.focus();
    } else if (e.key === 'Enter' && hits.length) {
      e.preventDefault();
      lib.open(hits[0]);
    }
  }
  function hitKeydown(e, i) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      hitEls[i + 1]?.focus();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      (i === 0 ? searchEl : hitEls[i - 1])?.focus();
    }
  }

  // An archived conversation's menu: Restore, Copy as Markdown, Delete….
  function archivedMenu(e, c) {
    if (e.type === 'contextmenu' && !devInspect(e) && !chat.sending && chat.activeId !== c.id) chat.selectConversation(c.id);
    openContextMenu(
      e,
      [
        { label: 'Restore', icon: 'restore', onselect: () => lib.restore([c.id]) },
        { label: 'Copy as Markdown', icon: 'copy', onselect: () => oncopy(c.id) },
        { separator: true },
        { label: 'Delete…', icon: 'trash', danger: true, onselect: () => lib.askDelete(c, archivedEls[c.id]) },
      ],
      { label: `Archived conversation ${c.title || 'Untitled'}`, row: archivedEls[c.id] }
    );
  }
</script>

{#snippet marked(parts)}{#each parts as p, k (k)}{#if p.hit}<mark class="found">{p.text}</mark>{:else}{p.text}{/if}{/each}{/snippet}

{#if lib.query.trim()}
  <!-- Every chat holding the words (archived too), and by meaning a few that mean them. -->
  <div class="scroll">
    <div class="gcap">
      {#if lib.found}{lib.found.hits.length === 1 ? '1 chat' : `${lib.found.hits.length} chats`}{#if lib.found.semantic}{' · by meaning too'}{/if}{:else}Searching…{/if}
    </div>
    {#if lib.found && !lib.found.hits.length}
      <p class="none">{lib.searching ? 'Searching…' : 'No chats match that'}</p>
    {:else if lib.found}
      <div role="group" aria-label="Chats that match" aria-busy={lib.searching}>
        {#each lib.found.hits as h, i (h.id)}
          <button bind:this={hitEls[i]} class="row hit" class:active={chat.activeId === h.id} onclick={() => lib.open(h)} onkeydown={(e) => hitKeydown(e, i)}>
            <span class="title">{#if h.title}{@render marked(h.titleParts)}{:else}{titleOf(h.id) || 'Untitled'}{/if}</span>
            {#if h.parts?.length && h.field !== 'title'}
              <span class="line">{#if h.speaker}<span class="who">{h.speaker}:</span>{' '}{/if}{@render marked(h.parts)}</span>
            {/if}
            {#if !h.words || isArchived(h.id)}
              <span class="tags">{[!h.words ? 'by meaning' : '', isArchived(h.id) ? 'Archived' : ''].filter(Boolean).join(' · ')}</span>
            {/if}
          </button>
        {/each}
      </div>
    {/if}
  </div>
{:else}
  <!-- The archive: open one, restore it, or delete it (asked first). -->
  <div class="scroll" role="region" aria-label="Archived chats">
    <button class="back" onclick={() => (lib.showArchived = false)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 18l-6-6 6-6" /></svg>
      Archived chats
    </button>
    {#each lib.archived as c (c.id)}
      <div
        bind:this={archivedEls[c.id]}
        class="row"
        class:active={chat.activeId === c.id}
        role="button"
        tabindex="0"
        aria-haspopup="menu"
        onclick={() => chat.selectConversation(c.id)}
        oncontextmenu={(e) => archivedMenu(e, c)}
        onkeydown={(e) => {
          if (e.target !== e.currentTarget) return;
          if (e.key === 'Enter' || e.key === ' ') chat.selectConversation(c.id);
          else menuKey(e, (ev) => archivedMenu(ev, c));
        }}
      >
        <span class="title">{c.title || 'Untitled'}</span>
        <button class="rowbtn" title="Restore" aria-label={`Restore ${c.title || 'Untitled'}`} onclick={(e) => (e.stopPropagation(), lib.restore([c.id]))}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 12a9 9 0 1 0 3-6.7L3 8" /><path d="M3 3v5h5" /></svg>
        </button>
        <button class="rowbtn danger" title="Delete" aria-label={`Delete ${c.title || 'Untitled'}`} onclick={(e) => (e.stopPropagation(), lib.askDelete(c, archivedEls[c.id]))}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m3 0v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" /></svg>
        </button>
      </div>
    {/each}
  </div>
{/if}

<style>
  .scroll {
    flex: 1 1 auto;
    overflow-y: auto;
    min-height: 0;
    padding: 0 8px 12px;
  }
  .gcap {
    padding: 12px 10px 4px;
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.09em;
    color: var(--yap-muted-55);
  }
  .none {
    margin: 10px;
    font-size: 11.5px;
    color: var(--yap-muted-55);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 7px 10px;
    border-radius: var(--yap-r);
    cursor: pointer;
  }
  .row:hover {
    background: var(--yap-s2);
  }
  .row.active {
    background: var(--yap-primary-wash);
  }
  .row:focus-visible {
    outline: 1.5px solid var(--yap-primary-line);
    outline-offset: -1px;
  }
  .row.hit {
    flex-direction: column;
    align-items: stretch;
    gap: 2px;
    width: 100%;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
  }
  .row.hit:hover {
    background: var(--yap-s2);
  }
  .row.hit.active {
    background: var(--yap-primary-wash);
  }
  .title {
    flex: 1 1 auto;
    min-width: 0;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .row.active .title {
    font-weight: 600;
  }
  .line {
    display: -webkit-box;
    overflow: hidden;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    font-size: 11px;
    line-height: 1.45;
    color: var(--yap-muted);
    overflow-wrap: anywhere;
  }
  .who {
    font-weight: 600;
  }
  .tags {
    font-size: 10px;
    color: var(--yap-link);
  }
  .back {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    height: 30px;
    margin: 6px 0 2px;
    padding: 0 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    text-align: left;
    cursor: pointer;
  }
  .back:hover {
    color: var(--yap-fg);
  }
  .back svg,
  .rowbtn svg {
    width: 13px;
    height: 13px;
    flex: 0 0 auto;
  }
  .rowbtn {
    display: inline-flex;
    width: 22px;
    height: 22px;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .rowbtn:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .rowbtn.danger:hover {
    color: var(--yap-danger);
  }
  .rowbtn:focus-visible {
    outline: 1.5px solid var(--yap-primary-line);
  }
</style>
