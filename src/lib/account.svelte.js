// Yap account state, shared by the Account settings page and the "signed in
// as" buttons in the Settings + main-window sidebars. Rust (auth.rs) owns the
// session — the token lives in Windows Credential Manager and never reaches
// the webview — so the UI only sees this status snapshot:
//   { signedIn, user: { id, email, name, image } | null, providers: [],
//     pending: 'google' | … | null, offline, signedInAt, serviceUrl }
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { toast } from './ui/toast.svelte.js';

export const account = $state({ status: null });

export const PROVIDER_LABELS = { google: 'Google', microsoft: 'Microsoft', github: 'GitHub' };

let started = false;

/** Load the status and follow changes. Idempotent; call from any component. */
export function initAccount() {
  if (started) return;
  started = true;
  invoke('auth_status')
    .then((s) => (account.status = s))
    .catch(() => {});
  listen('yap-auth-changed', (e) => {
    const before = account.status;
    account.status = e.payload;
    // A browser sign-in finishes in the background (deep link / loopback),
    // possibly with the Account page closed: say so.
    if (before?.pending && !before?.signedIn && e.payload?.signedIn) {
      toast({ title: 'Signed in', description: `Signed in as ${e.payload.user?.email ?? 'your account'}.`, variant: 'success' });
    }
  });
  listen('yap-auth-error', (e) => {
    toast({ title: "Sign-in didn't finish", description: String(e.payload || 'Something went wrong.'), variant: 'destructive' });
  });
}

export function displayName(user) {
  return user?.name?.trim() || user?.email || '';
}

export function initials(user) {
  const name = user?.name?.trim();
  if (name) {
    const parts = name.split(/\s+/).filter(Boolean);
    return ((parts[0]?.[0] ?? '') + (parts.length > 1 ? parts[parts.length - 1][0] : '')).toUpperCase();
  }
  return (user?.email?.[0] ?? '?').toUpperCase();
}
