<script>
  // **Style** (chat/styles.rs, 15 Nov 2026; the Claude app's styles): how
  // this chat's answers are written — Normal, Concise, Explanatory, Formal, or
  // the person's own. Under the message box beside the model; its menu picks
  // one for the chat shown (a new chat: the one it starts with, kept as the
  // last picked) and opens "Create & edit styles…" (StylesDialog.svelte, its
  // own chunk). Chat's side only: a Code chat's answers are Claude Code's.
  import { invoke } from '@tauri-apps/api/core';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';

  /** `conversationId`: the chat shown (null: a new chat). */
  let { conversationId = null } = $props();

  let builtins = $state([]);
  let mine = $state([]);
  let current = $state('normal');
  let chipEl = $state(null);
  let open = $state(false);
  let Dialog = $state(null);
  let dialogOpen = $state(false);

  const all = $derived([...builtins, ...mine]);
  const name = $derived(all.find((s) => s.id === current)?.name || 'Normal');

  async function loadList() {
    try {
      const out = await invoke('chat_styles');
      builtins = out?.builtins || [];
      mine = out?.styles || [];
    } catch {
      // (Unread: the chip says Normal.)
    }
  }
  async function loadCurrent(id) {
    try {
      const s = await invoke('chat_style_of', { conversation: id });
      if (id === conversationId) current = s || 'normal';
    } catch {
      current = 'normal';
    }
  }
  // The chat shown: its style (a new one: the last picked).
  $effect(() => {
    const id = conversationId;
    loadCurrent(id);
  });
  loadList();

  async function pick(id) {
    try {
      current = await invoke('chat_style_set', { conversation: conversationId, style: id });
    } catch (e) {
      toast({ title: "Couldn't change the style", description: String(e), variant: 'destructive' });
      loadList();
      loadCurrent(conversationId);
    }
  }

  async function openDialog() {
    if (!Dialog) Dialog = (await import('./StylesDialog.svelte')).default;
    dialogOpen = true;
  }

  async function onChip(e) {
    await loadList();
    open = true;
    const items = [
      ...builtins.map((s) => ({ label: s.name, checked: s.id === current, onselect: () => pick(s.id) })),
      ...(mine.length ? [{ separator: true }] : []),
      ...mine.map((s) => ({ label: s.name, checked: s.id === current, onselect: () => pick(s.id) })),
      { separator: true },
      { label: 'Create & edit styles…', icon: 'pencil', onselect: openDialog },
    ];
    openContextMenu(e, items, { label: 'Style', row: chipEl });
    // (The menu takes the focus; the chip's aria-expanded follows it loosely.)
    setTimeout(() => (open = false), 0);
  }

  /** The dialog closed: the list again, and the chat's style if it went. */
  async function closed() {
    dialogOpen = false;
    await loadList();
    await loadCurrent(conversationId);
    chipEl?.focus({ preventScroll: true });
  }
</script>

<button
  type="button"
  class="stylechip"
  class:set={current !== 'normal'}
  bind:this={chipEl}
  aria-haspopup="menu"
  aria-expanded={open}
  aria-label={`Style: ${name}`}
  title="How answers are written in this chat"
  data-testid="chat-style-chip"
  data-more
  onclick={onChip}
>
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16z" /><path d="M13.5 6.5l4 4" /></svg>
  <span class="sname">{name}</span>
</button>

{#if Dialog && dialogOpen}
  <Dialog onclose={closed} onpick={(id) => pick(id)} />
{/if}

<style>
  .stylechip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    height: 26px;
    padding: 0 8px 0 6px;
    border: 1px solid transparent;
    border-radius: var(--yap-r-full);
    background: none;
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    white-space: nowrap;
    cursor: pointer;
    transition: background var(--yap-dur);
  }
  .stylechip svg {
    flex: none;
    width: 13px;
    height: 13px;
  }
  .stylechip:hover,
  .stylechip[aria-expanded='true'] {
    background: var(--yap-s3);
  }
  .stylechip.set .sname {
    color: var(--yap-fg-80);
    font-weight: 600;
  }
  .stylechip:focus-visible {
    outline: 2px solid var(--yap-primary);
    outline-offset: 1px;
  }
  .sname {
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
