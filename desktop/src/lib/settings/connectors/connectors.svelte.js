// Settings → Connectors' state that outlives the page (connectors (preview):
// desktop/src-tauri/src/integrations/connectors.rs): whether the preview is on
// (`connectors_list`'s `enabled`: the page and its search entries show only
// then), the person's connectors as Rust views them, Discover's catalogue,
// and the statuses live — `yap-connectors-changed` reads the list again,
// `yap-connector-status` patches one connector's status in place (a sign-in
// in progress, a crash). Settings is always mounted, so this follows the
// events for the window's life once the preview is seen on.
//
// No secret is ever here: a token or a header's value is typed into a field
// and handed straight to its command (connector_add, connector_set_client),
// and Rust's views name which secrets there are, never their values.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export const conn = $state({
  /** connectors_list has answered once. */
  checked: false,
  /** The preview is on in this Yap. */
  enabled: false,
  /** Rust's views, in the order they were added. */
  list: [],
  /** Discover's list (connector_catalog), read the first time it shows. */
  catalog: null,
  catalogError: '',
  /** The Yours | Discover tab, and the connector whose page is open. */
  tab: 'yours',
  open: null,
});

let following = false;
let reading = null;
let again = false;

/** Read the list again (one read at a time; a change meanwhile reads once more). */
export async function loadConnectors() {
  if (reading) {
    again = true;
    return reading;
  }
  reading = (async () => {
    do {
      again = false;
      try {
        const v = await invoke('connectors_list');
        conn.enabled = !!v?.enabled;
        conn.list = Array.isArray(v?.connectors) ? v.connectors : [];
      } catch {
        conn.enabled = false;
        conn.list = [];
      }
      conn.checked = true;
    } while (again);
  })();
  try {
    await reading;
  } finally {
    reading = null;
  }
  if (conn.enabled) follow();
  return conn.list;
}

/** Discover's catalogue (bundled with Yap: nothing is fetched). */
export async function loadCatalog() {
  try {
    conn.catalog = await invoke('connector_catalog');
    conn.catalogError = '';
  } catch (e) {
    conn.catalogError = String(e);
  }
  return conn.catalog;
}

/** One connector's view put in place (a command answered with it). */
export function putConnector(view) {
  if (!view?.id) return;
  const i = conn.list.findIndex((c) => c.id === view.id);
  if (i >= 0) conn.list[i] = view;
  else conn.list = [...conn.list, view];
}

/** A status event: that connector's status line, state and sign-in now. */
export function patchStatus(s) {
  const c = conn.list.find((x) => x.id === s?.id);
  if (!c) return;
  c.status = s;
  if (c.signIn && s.signIn) c.signIn = { ...c.signIn, state: s.signIn };
}

function follow() {
  if (following) return;
  following = true;
  listen('yap-connectors-changed', () => {
    loadConnectors();
    if (conn.catalog) loadCatalog();
  }).catch(() => (following = false));
  listen('yap-connector-status', (e) => patchStatus(e.payload)).catch(() => {});
}

/** The connector `id` as the list has it now. */
export function connectorById(id) {
  return conn.list.find((c) => c.id === id) ?? null;
}

/** What a status asks of the person: its button, if any. */
export function statusAction(c) {
  const s = c?.status;
  if (!c?.enabled || !s) return null;
  const sign = s.signIn ?? c.signIn?.state;
  if (sign === 'signing-in') return 'cancel';
  if (sign === 'needs-sign-in' || sign === 'expired') return 'sign-in';
  if (s.state === 'needsSignIn') return 'sign-in';
  if (s.state === 'failed' || s.state === 'stopped') return 'retry';
  return null;
}

/** The dot's colour: ok, wait, needs (the person), bad, off. */
export function statusTone(c) {
  const s = c?.status;
  if (!c?.enabled || s?.state === 'off') return 'off';
  const sign = s?.signIn ?? c?.signIn?.state;
  if (sign === 'signing-in' || s?.state === 'starting') return 'wait';
  if (sign === 'needs-sign-in' || sign === 'expired' || s?.state === 'needsSignIn') return 'needs';
  if (s?.state === 'failed' || s?.state === 'stopped') return 'bad';
  if (s?.state === 'connected') return 'ok';
  return 'idle';
}

/** "just now", "5 min ago", "3 h ago", "12 Nov". */
export function ago(ts, now = Date.now() / 1000) {
  if (!ts) return '';
  const s = Math.max(0, now - ts);
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.floor(s / 60)} min ago`;
  if (s < 86_400) return `${Math.floor(s / 3600)} h ago`;
  return new Date(ts * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
}

/** An invoke's error in words (Tauri gives a string; a test run's evaluate an Error). */
export function words(e) {
  return String(e?.message ?? e ?? '').replace(/^Error: /, '');
}

// ---- what the rows and a connector's page do -----------------------------------------------
// Each answers what happened ({ok, why?}); a sign-in's or Try again's
// trouble is kept per connector (`conn.trouble[id]`) for its row and page to
// say, until the next attempt.

/** Words to show under a connector after an attempt failed, by id. */
conn.trouble = {};

/**
 * Sign connector `c` in, in the browser: the status says "Signing in…" meanwhile
 * (with Cancel). Answers once it's over.
 */
export async function signIn(c) {
  delete conn.trouble[c.id];
  try {
    const r = await invoke('connector_sign_in', { id: c.id });
    if (r?.connector) putConnector(r.connector);
    return { ok: r?.outcome === 'signed-in', outcome: r?.outcome };
  } catch (e) {
    conn.trouble[c.id] = words(e);
    loadConnectors();
    return { ok: false, why: words(e) };
  }
}

export async function cancelSignIn(c) {
  try {
    await invoke('connector_sign_in_cancel', { id: c.id });
  } catch {
    /* it ended already */
  }
}

/** Sign out: `confirmed` false when the server couldn't confirm Yap's access ended. */
export async function signOut(c) {
  try {
    const r = await invoke('connector_sign_out', { id: c.id });
    if (r?.connector) putConnector(r.connector);
    return { ok: true, confirmed: !!r?.confirmed };
  } catch (e) {
    return { ok: false, why: words(e) };
  }
}

/** Try again: a fresh connection, its tools listed again. */
export async function retry(c) {
  delete conn.trouble[c.id];
  try {
    const r = await invoke('connector_refresh_tools', { id: c.id });
    if (r?.connector) putConnector(r.connector);
    return { ok: true, diff: r?.diff };
  } catch (e) {
    conn.trouble[c.id] = words(e);
    loadConnectors();
    return { ok: false, why: words(e) };
  }
}

export async function setEnabled(c, enabled) {
  try {
    putConnector(await invoke('connector_set_enabled', { id: c.id, enabled }));
    return { ok: true };
  } catch (e) {
    return { ok: false, why: words(e) };
  }
}

export async function setTool(c, tool, setting) {
  try {
    putConnector(await invoke('connector_set_tool', { id: c.id, tool, setting }));
    return { ok: true };
  } catch (e) {
    return { ok: false, why: words(e) };
  }
}

/** Remove connector `c` (its sign-in revoked, its secrets gone). */
export async function removeConnector(c) {
  try {
    await invoke('connector_remove', { id: c.id });
    conn.list = conn.list.filter((x) => x.id !== c.id);
    if (conn.open === c.id) conn.open = null;
    delete conn.trouble[c.id];
    return { ok: true };
  } catch (e) {
    return { ok: false, why: words(e) };
  }
}
