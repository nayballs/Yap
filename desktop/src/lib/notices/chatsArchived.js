// "Archived 3 chats you hadn't used in 30 days" in the main window
// (desktop/src-tauri/src/chat/auto_archive.rs). Rust archives — with "Archive chats you
// haven't used in" set (Settings → Privacy & data), soon after Yap starts
// and once a day — and says it here only, quietly, once the person is in
// Yap's window (never a card on the Yap bar nor a Windows notification):
//
// `yap-chats-auto-archived` { ids, days }: an info toast with Undo
// (`chats_auto_archive_undo`: those chats back in the list). The toast
// up takes a later run's chats too (the same key). Shown: told back
// (`chats_auto_archive_said`). A window that opened after the event asks
// what's unsaid as it starts listening (`chats_auto_archive_unsaid`).
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';

let started = false;

/** "Archived 3 chats you hadn't used in 30 days." */
export function archivedLine(count, days) {
  const what = count === 1 ? 'a chat' : `${count} chats`;
  return `Archived ${what} you hadn't used in ${days} days`;
}

/** Say what a run archived (`{ ids, days }`), with Undo. */
export function sayArchived(a) {
  const ids = Array.isArray(a?.ids) ? a.ids : [];
  if (!ids.length || !a.days) return;
  invoke('chats_auto_archive_said', { ids }).catch(() => {});
  toast({
    kind: 'info',
    key: 'chats-auto-archived',
    title: archivedLine(ids.length, a.days),
    description: `${ids.length === 1 ? "It's" : "They're"} in Archived, at the foot of Chat's list. A new message brings one back.`,
    chip: 'Chat',
    duration: 12_000,
    action: {
      label: 'Undo',
      onClick: async () => {
        let back = 0;
        try {
          back = await invoke('chats_auto_archive_undo', { ids });
        } catch (err) {
          toast({ title: "Couldn't bring them back", description: String(err), variant: 'destructive' });
          return;
        }
        if (back) toast({ title: back === 1 ? 'Chat restored' : `${back} chats restored`, variant: 'success' });
      },
    },
  });
}

/** Show auto-archive's notice as a toast. Idempotent (the main window). */
export function initChatsArchived() {
  if (started) return;
  started = true;
  listen('yap-chats-auto-archived', (e) => sayArchived(e.payload))
    .then(() => invoke('chats_auto_archive_unsaid'))
    .then((a) => a && sayArchived(a))
    .catch(() => {});
}
