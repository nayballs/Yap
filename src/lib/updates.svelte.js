// Update state shared by every surface — the update toast, Settings → About,
// the Settings attention badge and the status bar (the tray renders the same
// state Rust-side). Rust (updates.rs) owns the checks, the background download
// and the install; this mirrors its snapshot (`update_status` + the
// `yap-update` event):
//   { status: 'idle' | 'checking' | 'available' | 'downloading' | 'ready' |
//             'installing',
//     version, currentVersion, notes, date, progress, error, deferred,
//     installQueued, metered, portable, lastChecked, releaseUrl,
//     announce: { version, reminder } | null, updatedFrom: string | null }
// plus two page-side fields for a manual check's feedback: `checked`
// ('' | 'uptodate' | 'error', shown for a few seconds) and `checkError`.
//
// It also turns the snapshot into the in-app announcements: one sticky toast
// per update (Rust only sets `announce` while the main window is on screen —
// WebView2 reports `visibilityState: visible` even for a hidden window, so
// the page can't judge that itself; with the window hidden Rust posts a
// Windows notification instead), that toast's progress through
// "Restarting…", and "Updated to X" after an update restart.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { openExternalLink } from './externalLinks.js';
import { toast, updateToast, dismiss, isToastLive } from './ui/toast.svelte.js';

const RELEASES = 'https://github.com/nayballs/Yap/releases';

export const updates = $state({
  status: 'idle',
  version: '',
  currentVersion: '',
  notes: '',
  date: '',
  progress: 0,
  error: '',
  deferred: false,
  installQueued: false,
  metered: false,
  portable: false,
  lastChecked: null,
  releaseUrl: `${RELEASES}/latest`,
  announce: null,
  updatedFrom: null,
  checked: '',
  checkError: '',
});

let started = false;
// The update toast on screen (announcement → progress → restarting), if any.
let toastId = null;
// This window asked to install, so a failure belongs in that toast.
let acting = false;
let checkedTimer = null;
// "Updated to X" shows once per launch (a snapshot sent before Rust
// saw the ack still carries `updatedFrom`).
let updatedShown = false;

/** Load the snapshot and follow changes. Idempotent; the main window calls it. */
export function initUpdates() {
  if (started) return;
  started = true;
  invoke('update_status')
    .then(apply)
    .catch(() => {});
  listen('yap-update', (e) => apply(e.payload));
  listen('yap-update-blocked', (e) => blocked(String(e.payload || '')));
  // The tray's "Check for updates…" (Rust has already opened Settings → About).
  listen('check-for-updates', () => checkForUpdates());
}

/** The newest snapshot revision applied (`Status::rev` in updates.rs). */
let appliedRev = 0;

function apply(s) {
  if (!s) return;
  // Snapshots arrive as events and as command replies, and a reply can land
  // after a newer event (a quick download finishing before `update_check`
  // returns): never let an older one roll the page back.
  if (typeof s.rev === 'number') {
    if (s.rev < appliedRev) return;
    appliedRev = s.rev;
  }
  Object.assign(updates, s);
  const pending =
    ['downloading', 'installing'].includes(updates.status) || updates.deferred || updates.installQueued;
  if (acting && !pending) {
    // The install attempt ended without restarting: keep its toast showing
    // why (if it failed), otherwise stand down.
    acting = !!updates.error;
  }
  syncToast();
  announce();
}

/** The release page for a version (the nightly channel shares one page). */
export function releaseUrlFor(version) {
  if (!version) return `${RELEASES}/latest`;
  return version.includes('-nightly') ? `${RELEASES}/tag/nightly` : `${RELEASES}/tag/v${version}`;
}

function announce() {
  if (updates.updatedFrom && !updatedShown) {
    updatedShown = true;
    updates.updatedFrom = null;
    invoke('update_ack_updated').catch(() => {});
    const url = releaseUrlFor(updates.currentVersion);
    toast({
      title: `Updated to ${updates.currentVersion}`,
      description: 'Yap restarted on the new version.',
      variant: 'success',
      chip: 'Updated',
      duration: 6000,
      action: { label: "What's new", onClick: () => openExternalLink(url) },
    });
  }
  const a = updates.announce;
  if (a) {
    updates.announce = null;
    invoke('update_ack', { version: a.version, reminder: a.reminder }).catch(() => {});
    // A fresh announcement, not the aftermath of an earlier failed attempt.
    if (!isToastLive(toastId)) acting = false;
    showToast();
  }
}

function toastContent() {
  const v = updates.version;
  const base = {
    variant: 'default',
    chip: 'Update',
    icon: 'update',
    progress: null,
    busy: false,
    action: null,
    secondary: null,
    expand: null,
  };
  const later = { label: 'Later', onClick: () => {} };
  const notes = updates.notes ? { label: "What's new", markdown: updates.notes } : null;

  if (acting && updates.error && ['ready', 'available'].includes(updates.status)) {
    return {
      ...base,
      variant: 'destructive',
      chip: 'Error',
      icon: '',
      title: "Couldn't update Yap",
      description: updates.error,
      action: { label: 'Try again', onClick: installUpdate, keepOpen: true },
      secondary: later,
    };
  }
  // "Download and restart" was clicked: the download is about to start.
  const status = updates.status === 'available' && updates.installQueued ? 'downloading' : updates.status;
  switch (status) {
    case 'installing':
      return {
        ...base,
        busy: true,
        title: 'Restarting Yap…',
        description: `Installing ${v}. Yap will be right back.`,
      };
    case 'downloading':
      return {
        ...base,
        progress: updates.progress,
        title: `Downloading Yap ${v}…`,
        description: updates.installQueued
          ? `${updates.progress}% · Yap restarts as soon as it's done.`
          : `${updates.progress}% · You can keep working.`,
        secondary: { label: 'Hide', onClick: () => {} },
      };
    case 'ready':
      if (updates.deferred) {
        return {
          ...base,
          busy: true,
          title: 'Restarting after this dictation',
          description: `Yap installs ${v} as soon as you finish dictating.`,
        };
      }
      return {
        ...base,
        title: `Yap ${v} is ready`,
        description: 'Restart to finish updating. It only takes a few seconds.',
        action: { label: 'Restart to update', onClick: installUpdate, keepOpen: true },
        secondary: later,
        expand: notes,
      };
    case 'available':
      if (updates.portable) {
        return {
          ...base,
          title: `Yap ${v} is available`,
          description: 'Portable Yap updates by hand. Get the new version from GitHub.',
          action: { label: 'Get it on GitHub', onClick: openRelease },
          secondary: later,
          expand: notes,
        };
      }
      return {
        ...base,
        title: `Yap ${v} is available`,
        description: updates.metered
          ? "You're on a metered connection, so Yap hasn't downloaded it yet."
          : 'Download it and restart to update.',
        action: { label: 'Download and restart', onClick: installUpdate, keepOpen: true },
        secondary: later,
        expand: notes,
      };
    default:
      return null;
  }
}

/** Show (or refresh) the update toast for the current state. */
function showToast() {
  const content = toastContent();
  if (!content) return;
  if (toastId && isToastLive(toastId)) updateToast(toastId, content);
  else toastId = toast({ ...content, duration: 0 });
}

/** Keep a toast that's already up in step with the state (never opens one). */
function syncToast() {
  if (!toastId || !isToastLive(toastId)) return;
  const content = toastContent();
  if (content) updateToast(toastId, content);
  else dismiss(toastId);
}

function blocked(message) {
  if (!message) return;
  toast({
    title: "Yap can't restart right now",
    description: message,
    chip: 'Update',
    icon: 'update',
    duration: 7000,
  });
}

/** Open the GitHub release page for the pending (or latest) version. */
export function openRelease() {
  openExternalLink(updates.releaseUrl || releaseUrlFor(updates.version));
}

/** "Restart to update" / "Download and restart" (portable → GitHub). */
export async function installUpdate() {
  if (updates.portable) {
    openRelease();
    return;
  }
  acting = true;
  try {
    apply(await invoke('update_install'));
  } catch (e) {
    acting = false;
    blocked(String(e));
  }
}

function flashChecked(kind) {
  clearTimeout(checkedTimer);
  updates.checked = kind;
  checkedTimer = setTimeout(() => (updates.checked = ''), kind === 'error' ? 6000 : 4000);
}

/** A manual check (About, status bar, tray). Works with automatic checks off. */
export async function checkForUpdates() {
  if (updates.status === 'checking') return;
  clearTimeout(checkedTimer);
  updates.checked = '';
  updates.checkError = '';
  try {
    const r = await invoke('update_check');
    apply(r.status);
    // Found: the person is looking at the result where they asked (Settings →
    // About, the status bar), and Rust counts it as this update's announcement
    // (`run_check`), so no toast repeats it.
    if (r.outcome === 'uptodate') {
      flashChecked('uptodate');
    } else if (r.outcome === 'error') {
      updates.checkError = r.error || "Couldn't check for updates right now.";
      flashChecked('error');
    }
  } catch (e) {
    updates.checkError = String(e);
    flashChecked('error');
  }
}

/** "5 minutes ago"-style label for a unix-seconds timestamp. */
export function formatAgo(secs) {
  if (!secs) return '';
  const diff = Math.max(0, Date.now() / 1000 - secs);
  if (diff < 60) return 'just now';
  if (diff < 3600) {
    const m = Math.round(diff / 60);
    return `${m} minute${m === 1 ? '' : 's'} ago`;
  }
  if (diff < 86400) {
    const h = Math.round(diff / 3600);
    return `${h} hour${h === 1 ? '' : 's'} ago`;
  }
  const d = Math.round(diff / 86400);
  return d === 1 ? 'yesterday' : `${d} days ago`;
}
