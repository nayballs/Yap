// "File every meeting with example.com in Clients? (3 past ones too)" in
// the main window (calendar/filing/companies.rs). Rust decides — a meeting
// with people from another company filed by hand in a folder of the
// person's own, once per company and folder, never after Never, at most one
// a day — and this shows its question while the window is in front (else
// it's a card on the Yap bar):
//
// `yap-company-folder-offer` { id, domain, folder, past, title, body }:
//   Yes / Just this one, and a small "Never for example.com" link. The
//   answer goes back with `company_folder_respond` ("yes" | "one" | "never"
//   | "expire"): the toast's ✕ is "Just this one", running out is no answer
//   at all. After Yes, Rust keeps the rule and moves the past meetings;
//   this says so, with Undo (`company_folder_undo`). Settings → Meetings
//   lists the rules, with Remove.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from '../ui/toast.svelte.js';

let started = false;

/** Show the company folder offers as toasts. Idempotent (the main window). */
export function initCompanyFolders() {
  if (started) return;
  started = true;
  listen('yap-company-folder-offer', (e) => askCompanyFolder(e.payload));
}

/** "Moved 3 past meetings to Clients too." */
export function filedLine(moved, folder) {
  if (!moved) return `New ones go to ${folder} as they're made.`;
  return `Moved ${moved} past ${moved === 1 ? 'meeting' : 'meetings'} to ${folder} too.`;
}

/** Ask about a company's folder (`{ id, domain, folder, past, title, body }`) as a toast. */
export function askCompanyFolder(o) {
  if (!o?.id || !o.title) return;
  const answer = (a) => invoke('company_folder_respond', { id: o.id, answer: a });
  const quietly = (a) => answer(a).catch(() => {});
  toast({
    kind: 'tip',
    title: o.title,
    description: o.body,
    chip: 'Folders',
    icon: 'calendar',
    duration: 30_000,
    action: {
      label: 'Yes',
      onClick: async () => {
        let done;
        try {
          done = await answer('yes');
        } catch (err) {
          toast({ title: "Couldn't file those meetings", description: String(err), variant: 'destructive' });
          return;
        }
        if (!done) return;
        toast({
          title: `Meetings with ${done.domain} go in ${done.folder}`,
          description: filedLine(done.moved, done.folder),
          variant: 'success',
          chip: 'Folders',
          duration: 10_000,
          action: {
            label: 'Undo',
            onClick: async () => {
              try {
                const back = await invoke('company_folder_undo', { undo: done.undo });
                toast({
                  title: 'Undone',
                  description: back ? `${back} ${back === 1 ? 'meeting is' : 'meetings are'} back where ${back === 1 ? 'it was' : 'they were'}.` : `New meetings with ${done.domain} go to Meetings again.`,
                  variant: 'success',
                });
              } catch (err) {
                toast({ title: "Couldn't undo that", description: String(err), variant: 'destructive' });
              }
            },
          },
        });
      },
    },
    secondary: { label: 'Just this one', onClick: () => quietly('one') },
    tertiary: { label: `Never for ${o.domain}`, onClick: () => quietly('never') },
    onClose: () => quietly('one'),
    onExpire: () => quietly('expire'),
  });
}
