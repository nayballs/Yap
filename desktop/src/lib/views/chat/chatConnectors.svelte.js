// Each chat's own connectors (Connectors wave 6, the composer's + menu →
// Connectors ▸; desktop/src-tauri/src/integrations/connectors/chats.rs): a
// switch per enabled connector allowed in Chat, on unless this chat switched
// it off, then Manage connectors…. Shown only with the connectors preview
// on and a Chat model that gets connectors (Rust's `shown`).
//
// An open conversation's switches are saved as they're flipped
// (`connector_chat_set`, connector_chats.json by its id). A new chat has
// every connector on: its switches stay here until its first question,
// which takes them (`chat_send`'s `connectorsOff`), and a new chat starts
// with all on again.
import { invoke } from '@tauri-apps/api/core';

/** `activeId()`: the conversation the view shows (null: a new chat). */
export function createChatConnectors(activeId) {
  let shown = $state(false);
  let list = $state([]);
  // A new chat's switches, before its first question (ids switched off).
  let newOff = $state([]);
  let reading = 0;

  /** Read the list for the chat shown (the menu opening, another chat, a change in Settings). */
  async function refresh() {
    const conv = activeId();
    const n = ++reading;
    try {
      const v = await invoke('connector_chat_list', { conversationId: conv ?? null });
      if (n !== reading) return;
      shown = !!v?.shown;
      const rows = Array.isArray(v?.connectors) ? v.connectors : [];
      list = conv == null ? rows.map((c) => ({ ...c, on: !newOff.includes(c.id) })) : rows;
    } catch {
      if (n !== reading) return;
      shown = false;
      list = [];
    }
  }

  /** Switch connector `id` on or off in the chat shown. `false`: it didn't take (the reason). */
  async function toggle(id, on) {
    const conv = activeId();
    list = list.map((c) => (c.id === id ? { ...c, on } : c));
    if (conv == null) {
      newOff = on ? newOff.filter((x) => x !== id) : [...new Set([...newOff, id])];
      return { ok: true };
    }
    try {
      const v = await invoke('connector_chat_set', { conversationId: conv, id, on });
      if (activeId() === conv) {
        shown = !!v?.shown;
        list = Array.isArray(v?.connectors) ? v.connectors : list;
      }
      return { ok: true };
    } catch (e) {
      list = list.map((c) => (c.id === id ? { ...c, on: !on } : c));
      return { ok: false, why: String(e?.message ?? e) };
    }
  }

  return {
    get shown() {
      return shown;
    },
    get list() {
      return list;
    },
    /** How many are on in this chat (the row's line). */
    get onCount() {
      return list.filter((c) => c.on).length;
    },
    refresh,
    toggle,
    /** The new chat's switched-off ids, for its first question (null when there are none). */
    newChatOff() {
      return newOff.length ? [...newOff] : null;
    },
    /** A new chat: every connector on again. */
    resetNew() {
      newOff = [];
    },
  };
}
