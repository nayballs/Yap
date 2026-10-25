// The Chat sidebar's search and its archive (ConversationList makes one).
//
// Search: every conversation, archived ones too, by the words typed in its
// title and messages (src-tauri/src/search/library.rs `chats_search`), then
// — with "Search by meaning" on — again by meaning; a result opens its
// conversation at the message, the words marked there.
//
// Archive: a conversation (or several picked) leaves the list for
// "Archived", where it can be opened, restored or deleted (asked first).
// Asking something in an archived conversation brings it back
// (chat/chats.rs `append_message`).
import { invoke } from '@tauri-apps/api/core';
import { tick } from 'svelte';
import { toast } from '../../ui/toast.svelte.js';
import { askConfirm } from '../../ui/contextMenu.svelte.js';
import { showSaveError } from '../../notices/saveErrors.js';
import { typedWords, markWords } from '../../core/findWords.js';

/** `chat()`: the view's state (chatState.svelte.js), as the list has it now. */
export function createChatLibrary(chat) {
  // ---- search ----
  let query = $state('');
  let found = $state(null); // { q, hits, semantic }
  let searching = $state(false);
  let meaningOn = $state(false);
  let seq = 0;
  let debounce = null;

  invoke('search_status')
    .then((s) => (meaningOn = !!(s?.enabled && s?.installed)))
    .catch(() => {});

  function run() {
    clearTimeout(debounce);
    const q = query.trim();
    if (!q) {
      seq++;
      found = null;
      searching = false;
      return;
    }
    debounce = setTimeout(async () => {
      const mine = ++seq;
      searching = true;
      try {
        const words = await invoke('chats_search', { query: q, meaning: false });
        if (mine !== seq) return;
        found = { q, ...words };
        if (meaningOn) {
          const both = await invoke('chats_search', { query: q, meaning: true });
          if (mine === seq) found = { q, ...both };
        }
      } catch {
        if (mine === seq) found = { q, hits: [], semantic: false };
      } finally {
        if (mine === seq) searching = false;
      }
    }, 120);
  }

  function clear() {
    query = '';
    run();
  }

  /** Open a result's conversation at its message, the words marked there. */
  async function open(h) {
    const words = typedWords(found?.q || query);
    await chat().selectConversation(h.id);
    if (chat().activeId !== h.id) return;
    await tick();
    await new Promise((r) => setTimeout(r, 0)); // after the thread's own scroll to the end
    const el = chat().threadEl?.querySelector(`[data-msg-id="chat:${h.id}:${h.line}"]`);
    if (!el) return;
    el.scrollIntoView({ block: 'center' });
    el.focus({ preventScroll: true });
    if (h.words && h.field !== 'title') markWords(el, words, { scroll: false, ms: 5000 });
  }

  // ---- archive ----
  let archived = $state([]);
  let showArchived = $state(false);

  async function refreshArchived() {
    try {
      archived = (await invoke('chats_archived')) || [];
    } catch {
      archived = [];
    }
  }
  refreshArchived();

  async function setArchived(ids, on) {
    if (!ids.length) return;
    try {
      await invoke('chat_archive', { ids, archived: on });
    } catch (e) {
      if (!showSaveError(e)) {
        toast({ title: on ? "Couldn't archive" : "Couldn't restore", description: String(e), variant: 'destructive' });
        return;
      }
    }
    if (on && ids.includes(chat().activeId)) chat().newChat();
    chat().clearPicked();
    await Promise.all([chat().refreshList(), refreshArchived()]);
    if (!archived.length) showArchived = false;
    const n = ids.length;
    toast({
      title: on ? (n === 1 ? 'Chat archived' : `${n} chats archived`) : n === 1 ? 'Chat restored' : `${n} chats restored`,
      variant: 'success',
      ...(on ? { action: { label: 'Undo', onClick: () => setArchived(ids, false) } } : {}),
    });
  }

  /** Delete an archived conversation, asked first. */
  function askDelete(c, returnFocus) {
    const count = c.messageCount || 0;
    askConfirm({
      title: `Delete “${c.title || 'Untitled'}”?`,
      body: count === 0 ? 'The conversation goes for good.' : `Its ${count === 1 ? 'message goes' : `${count} messages go`} for good.`,
      returnFocus,
      run: async () => {
        try {
          await invoke('chat_delete', { id: c.id });
        } catch (e) {
          if (!showSaveError(e)) {
            toast({ title: "Couldn't delete the conversation", description: String(e), variant: 'destructive' });
            return false;
          }
        }
        if (chat().activeId === c.id) chat().newChat();
        await refreshArchived();
        if (!archived.length) showArchived = false;
        return true;
      },
    });
  }

  return {
    get query() { return query; },
    set query(v) { query = v; },
    get found() { return found; },
    get searching() { return searching; },
    get archived() { return archived; },
    get showArchived() { return showArchived; },
    set showArchived(v) { showArchived = v; },
    run,
    clear,
    open,
    refreshArchived,
    archive: (ids) => setArchived(ids, true),
    restore: (ids) => setArchived(ids, false),
    askDelete,
  };
}
