<script>
  // Chat's conversation list (OpenWhispr's ConversationList): New chat and
  // the list's ⋯ (Select all, Delete all chats…), the conversations grouped
  // Today / Yesterday / Previous 7 Days / Older. A conversation — also from
  // its hover ⋯ and the ContextMenu key / Shift+F10 — has Rename (in place:
  // Enter or leaving it saves, Esc cancels; chat/chats.rs `rename`), Copy as
  // Markdown and Delete… (asked first); a right-click opens it too.
  // Shift+click and Ctrl+click pick several (the "N selected" bar deletes
  // them); Delete, Esc and Ctrl+A from the keyboard.
  import { invoke } from '@tauri-apps/api/core';
  import { toast } from '../../ui/toast.svelte.js';
  import { openContextMenu, menuKey, menuOpenFor, devInspect } from '../../ui/contextMenu.svelte.js';
  import { showSaveError } from '../../notices/saveErrors.js';
  import { copyText, conversationMarkdown } from '../../core/itemMenus.js';

  /** `chat`: the view's state (chatState.svelte.js). */
  let { chat } = $props();

  // Rename in place, like Notes' folders: `{ id, value }`.
  let renaming = $state(null);

  // The list's own keys: Delete, Esc and Ctrl+A (Enter and the menu keys
  // stay with each row).
  function listKeydown(e) {
    if (renaming || e.target.closest?.('input, textarea')) return;
    if (e.key === 'Delete' && (chat.picked.length || e.target.dataset?.conv)) {
      e.preventDefault();
      chat.askDeleteMany(chat.picked.length ? chat.picked : [Number(e.target.dataset.conv)]);
    } else if (e.key === 'Escape' && chat.picked.length) {
      e.preventDefault();
      e.stopPropagation();
      chat.clearPicked();
    } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') {
      e.preventDefault();
      chat.picked = chat.ordered.map((c) => c.id);
    }
  }

  function listMenu(e) {
    openContextMenu(
      e,
      [
        { label: 'Select all', icon: 'check', onselect: () => (chat.picked = chat.ordered.map((c) => c.id)) },
        { separator: true },
        {
          label: 'Delete all chats…',
          icon: 'trash',
          danger: true,
          onselect: () => chat.askDeleteMany(chat.ordered.map((c) => c.id), { all: true }),
        },
      ],
      { label: 'Chats' }
    );
  }

  // ---- Right-click menus (ui/contextMenu.svelte.js) ----

  function conversationMenu(e, c) {
    const picked = chat.picked;
    // On a row of a selection: the selection's menu.
    if (picked.length > 1 && picked.includes(c.id)) {
      openContextMenu(
        e,
        [
          { label: `Delete ${picked.length} chats…`, icon: 'trash', danger: true, onselect: () => chat.askDeleteMany(chat.picked) },
          { separator: true },
          { label: 'Clear selection', onselect: chat.clearPicked },
        ],
        { label: `${picked.length} selected chats`, row: chat.convEls[c.id] }
      );
      return;
    }
    chat.clearPicked();
    // A right-click opens it too, as a click does (not while an answer comes).
    if (e.type === 'contextmenu' && !devInspect(e) && !chat.sending && chat.activeId !== c.id) chat.selectConversation(c.id);
    openContextMenu(
      e,
      [
        { label: 'Rename', icon: 'pencil', onselect: () => startRename(c) },
        { separator: true },
        { label: 'Copy as Markdown', icon: 'copy', onselect: () => copyConversation(c.id) },
        { separator: true },
        { label: 'Delete…', icon: 'trash', danger: true, onselect: () => chat.askDelete(c) },
      ],
      { label: `Conversation ${c.title || 'Untitled'}`, row: chat.convEls[c.id] }
    );
  }

  function startRename(c) {
    renaming = { id: c.id, value: c.title || '' };
  }
  async function finishRename(save = true) {
    const r = renaming;
    if (!r) return;
    renaming = null;
    const before = chat.conversations.find((c) => c.id === r.id)?.title || '';
    const title = r.value.trim();
    if (save && title && title !== before) {
      try {
        const stored = await invoke('chat_rename', { id: r.id, title });
        chat.conversations = chat.conversations.map((c) => (c.id === r.id ? { ...c, title: stored } : c));
      } catch (e) {
        if (showSaveError(e)) await chat.refreshList();
        else toast({ title: "Couldn't rename the conversation", description: String(e), variant: 'destructive' });
      }
    }
    setTimeout(() => chat.convEls[r.id]?.focus({ preventScroll: true }), 0);
  }
  function focusOnMount(el) {
    el.focus();
    el.select();
  }

  async function copyConversation(id) {
    try {
      const conv = await invoke('chat_get', { id });
      await copyText(conversationMarkdown(conv), 'Copied as Markdown');
    } catch (e) {
      toast({ title: "Couldn't copy", description: String(e), variant: 'destructive' });
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside class="convs" onkeydown={listKeydown}>
  {#if chat.picked.length}
    <div class="tools selbar" role="toolbar" aria-label="Selected chats">
      <span class="selcount" title="Shift+click selects a range, Ctrl+click adds or takes out one">{chat.picked.length} selected</span>
      <button class="selbtn danger" onclick={() => chat.askDeleteMany(chat.picked)}>Delete</button>
      <button class="selbtn" onclick={chat.clearPicked}>Cancel</button>
    </div>
  {:else}
    <div class="tools">
      <button class="tool" onclick={chat.newChat}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13.4 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7.4" /><path d="M18.4 2.6a2 2 0 0 1 2.8 2.8L13 13.6 9 14.6l1-4z" /></svg>
        New chat
      </button>
      {#if chat.conversations.length}
        <button class="listmore" title="More" aria-label="More for chats" aria-haspopup="menu" onclick={listMenu}>
          <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.7" /><circle cx="12" cy="12" r="1.7" /><circle cx="19" cy="12" r="1.7" /></svg>
        </button>
      {/if}
    </div>
  {/if}

  {#if chat.conversations.length === 0}
    <p class="convempty">Start your first conversation</p>
  {:else}
    <div class="convscroll">
      {#each chat.groups as g (g.label)}
        <div class="gcap">{g.label}</div>
        {#each g.items as c (c.id)}
          {#if renaming?.id === c.id}
            <!-- Rename in place (as Notes' folders): Enter or leaving it saves, Esc cancels. -->
            <div class="conv renaming">
              <input
                class="renameinput"
                aria-label={`Rename conversation ${c.title || 'Untitled'}`}
                maxlength="80"
                bind:value={renaming.value}
                use:focusOnMount
                onkeydown={(e) => {
                  if (e.key === 'Enter') finishRename(true);
                  if (e.key === 'Escape') {
                    e.stopPropagation();
                    finishRename(false);
                  }
                }}
                onblur={() => finishRename(true)}
              />
            </div>
          {:else}
            <div
              bind:this={chat.convEls[c.id]}
              class="conv"
              class:active={chat.activeId === c.id}
              class:picked={chat.picked.includes(c.id)}
              class:menuopen={menuOpenFor(chat.convEls[c.id])}
              role="button"
              tabindex="0"
              aria-haspopup="menu"
              data-conv={c.id}
              onmousedown={(e) => e.shiftKey && e.preventDefault()}
              onclick={(e) => chat.clickConversation(e, c)}
              oncontextmenu={(e) => conversationMenu(e, c)}
              onkeydown={(e) => {
                if (e.target !== e.currentTarget) return;
                if (e.key === 'Enter' || e.key === ' ') chat.selectConversation(c.id);
                else menuKey(e, (ev) => conversationMenu(ev, c));
              }}
            >
              {#if chat.picked.includes(c.id)}
                <span class="pickmark" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12.5 4.5 4.5L19 7.5" /></svg></span>
              {/if}
              <span class="ctitle">{c.title || 'Untitled'}{#if chat.picked.includes(c.id)}<span class="sr-only">, selected</span>{/if}</span>
              <button
                class="more"
                title="More"
                aria-label={`More for ${c.title || 'Untitled'}`}
                aria-haspopup="menu"
                data-more
                onclick={(e) => conversationMenu(e, c)}
              >
                <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5" cy="12" r="1.7" /><circle cx="12" cy="12" r="1.7" /><circle cx="19" cy="12" r="1.7" /></svg>
              </button>
            </div>
          {/if}
        {/each}
      {/each}
    </div>
  {/if}
</aside>

<style>
  .convs {
    flex: 0 0 224px;
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--yap-border-subtle);
    background: var(--yap-s0, var(--yap-s1));
  }
  .tools {
    padding: 12px 8px 4px;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .listmore {
    flex: 0 0 auto;
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .listmore:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .listmore svg {
    width: 14px;
    height: 14px;
  }
  .selbar {
    gap: 6px;
    padding: 12px 10px 4px;
  }
  .selcount {
    flex: 1 1 auto;
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .selbtn {
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .selbtn:hover {
    border-color: var(--yap-border-hover);
  }
  .selbtn.danger {
    color: var(--yap-danger);
  }
  .conv.picked,
  .conv.picked:hover,
  .conv.picked.active {
    background: var(--yap-primary-tint);
  }
  .pickmark {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
    border-radius: 4px;
    display: grid;
    place-items: center;
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .pickmark svg {
    width: 10px;
    height: 10px;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  .tool {
    display: flex;
    align-items: center;
    gap: 9px;
    flex: 1 1 auto;
    width: 100%;
    height: 30px;
    padding: 0 10px;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    text-align: left;
    cursor: pointer;
  }
  .tool:hover {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .tool svg {
    width: 13px;
    height: 13px;
    flex: 0 0 auto;
  }
  .convempty {
    margin: 10px 18px;
    font-size: 11.5px;
    color: var(--yap-muted-55);
  }
  .convscroll {
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
  .conv {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 7px 10px;
    border-radius: var(--yap-r);
    cursor: pointer;
  }
  .conv:hover {
    background: var(--yap-s2);
  }
  .conv.active {
    background: var(--yap-primary-wash);
  }
  .ctitle {
    flex: 1 1 auto;
    min-width: 0;
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .conv.active .ctitle {
    font-weight: 600;
  }
  .conv.menuopen {
    background: var(--yap-s2);
  }
  .conv.active.menuopen {
    background: var(--yap-primary-wash);
  }
  .conv:focus-visible {
    outline: 1.5px solid var(--yap-primary-line);
    outline-offset: -1px;
  }
  .conv.renaming {
    padding: 3px 4px;
    cursor: default;
  }
  .renameinput {
    width: 100%;
    min-width: 0;
    height: 26px;
    padding: 0 7px;
    box-sizing: border-box;
    border: 1px solid var(--yap-primary-line);
    border-radius: var(--yap-r-sm);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
    outline: none;
  }
  /* The ⋯ on a row (its menu without right-clicking), as in Notes: shown on
     hover, on focus and while its menu is open. */
  .more {
    display: inline-flex;
    width: 22px;
    height: 22px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
    opacity: 0;
    flex: 0 0 auto;
  }
  .more:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .more:focus-visible {
    opacity: 1;
    outline: 1.5px solid var(--yap-primary-line);
    outline-offset: -1px;
  }
  .more svg {
    width: 13px;
    height: 13px;
  }
  .conv:hover .more,
  .conv:focus-within .more,
  .conv.menuopen .more {
    opacity: 1;
  }
</style>
