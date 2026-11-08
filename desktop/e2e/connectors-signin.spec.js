// Connectors (preview), signing in in the browser (wave 3,
// desktop/src-tauri/src/integrations/connectors/oauth.rs), through the
// commands (no Settings page yet: wave 5), against a fake MCP server with its
// own authorization server (support/fake-mcp-oauth.js). The browser is played
// by the test: the authorize address Yap would open (test mode keeps it,
// `e2e_opened_urls`), the fake's consent page approved, and the loopback's
// address it sends the browser to opened here.
// - discovery (the 401's challenge, protected resource metadata, the
//   authorization server's), PKCE with S256, `state`, the resource, the
//   scopes; "needs-sign-in" → "signing-in" → "signed-in" on
//   yap-connector-status; the tokens in the vault, never in Yap's files or
//   log; every request in the receipts as "Connector sign-in";
// - each client: dynamic registration (once, reused), Yap's client ID
//   metadata document, a hand-entered client and its secret;
// - refresh (a short-lived token, the refresh token rotating), a revoked
//   token, a revoked grant ("expired"), step-up (more scopes), sign-out
//   revoking;
// - refusals: another `iss`, metadata naming another issuer, a resource for
//   another server, no S256; a decline and a Cancel; a slow server, metadata
//   only where the challenge says, OpenID metadata for an issuer with a
//   path; a busy fixed port.
// Nothing leaves this PC; fictional data only.
import fs from 'node:fs';
import net from 'node:net';
import path from 'node:path';
import { test as base, expect } from './support/fixtures.js';
import { startFakeMcpOauth } from './support/fake-mcp-oauth.js';

const CALLBACK = 'http://127.0.0.1:51439/connectors/callback';

const test = base.extend({
  oauth: [
    async ({}, use) => {
      const fake = await startFakeMcpOauth();
      await use(fake);
      await fake.close();
    },
    { scope: 'worker' },
  ],
  yapOptions: [
    async ({ oauth }, use) => {
      await use({ name: 'connectors-signin', env: { YAP_CONNECTORS: '1', YAP_E2E_CONNECTOR_START_MS: '6000', YAP_E2E_CIMD_URL: oauth.cimdUrl() } });
    },
    { scope: 'worker' },
  ],
});

test.beforeEach(({ oauth }) => oauth.reset());
test.afterEach(async ({ yap }) => {
  for (const c of (await yap.invoke('connectors_list')).connectors) await yap.invoke('connector_remove', { id: c.id });
});

const opened = (yap) => yap.invoke('e2e_opened_urls');
const read = (yap, file) => {
  const p = path.join(yap.dataDir, file);
  return fs.existsSync(p) ? fs.readFileSync(p, 'utf8') : '';
};
/** Everything Yap logged: its stdout and its log files. */
function logged(yap) {
  let out = fs.existsSync(yap.logPath) ? fs.readFileSync(yap.logPath, 'utf8') : '';
  const dir = path.join(yap.dataDir, 'logs');
  if (fs.existsSync(dir)) for (const f of fs.readdirSync(dir)) out += fs.readFileSync(path.join(dir, f), 'utf8');
  return out;
}

/** Hear app event `event` in the main window from now on. */
async function listen(main, event) {
  await main.evaluate(async (ev) => {
    window.__heard = window.__heard || {};
    window.__heard[ev] = [];
    const handler = window.__TAURI_INTERNALS__.transformCallback((e) => window.__heard[ev].push(e.payload));
    await window.__TAURI_INTERNALS__.invoke('plugin:event|listen', { event: ev, target: { kind: 'Any' }, handler });
  }, event);
}
const heard = (main, event) => main.evaluate((ev) => window.__heard?.[ev] ?? [], event);

const add = (yap, oauth, name = 'Lights') => yap.invoke('connector_add', { spec: { name, url: oauth.url() } });

/**
 * Start a sign-in and wait for the browser to be opened on it (or for the
 * sign-in to end first): the authorize address, and its result to come.
 */
async function startSignIn(yap, id) {
  const before = (await opened(yap)).length;
  let settled = null;
  const pending = yap.invoke('connector_sign_in', { id }).then(
    (ok) => (settled = { ok }),
    (e) => (settled = { err: String(e?.message ?? e).replace(/^page\.evaluate: /, '') })
  );
  const deadline = Date.now() + 20_000;
  while (Date.now() < deadline) {
    const urls = await opened(yap);
    if (urls.length > before) return { url: urls.at(-1), done: () => pending.then(() => settled) };
    if (settled) return { url: null, done: async () => settled };
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error('the browser was never opened');
}

/** A whole sign-in, the person allowing it: the address, the page, the result. */
async function signIn(yap, oauth, id) {
  const s = await startSignIn(yap, id);
  expect(s.url, `the browser opened (${JSON.stringify(s.url ? null : await s.done())})`).toBeTruthy();
  const back = await oauth.approve(s.url);
  const page = await (await fetch(back)).text();
  const r = await s.done();
  return { url: s.url, q: new URL(s.url).searchParams, back, page, r };
}

const call = (yap, id, tool, args = {}) => yap.invoke('connector_call_tool', { id, tool, args });

test('dynamic registration: signed in through the browser, the tokens kept only in the vault, calls carry them, sign-out revokes', async ({ yap, main, oauth }) => {
  await listen(main, 'yap-connector-status');
  const added = await add(yap, oauth);
  expect(added.status).toMatchObject({ state: 'needsSignIn', signIn: 'needs-sign-in', line: 'Needs sign-in. Sign in in your browser, or use a token if it offers one.' });
  expect(added.signIn).toMatchObject({ state: 'needs-sign-in', redirectUri: CALLBACK, client: null });

  const { url, q, page, r } = await signIn(yap, oauth, added.id);
  expect(url.startsWith(`${oauth.base}/authorize?`)).toBe(true);
  expect(q.get('client_id')).toMatch(/^dcr-\d+$/);
  expect(q.get('response_type')).toBe('code');
  expect(q.get('code_challenge_method')).toBe('S256');
  expect(q.get('code_challenge')).toMatch(/^[A-Za-z0-9_-]{43}$/);
  expect(q.get('state')).toMatch(/^[A-Za-z0-9]{32}$/);
  expect(q.get('resource')).toBe(oauth.url());
  expect(q.get('scope')).toBe('lights:read offline_access');
  expect(q.get('redirect_uri')).toBe(CALLBACK);
  expect(page).toContain("You're signed in to Lights.");
  expect(r.ok.outcome).toBe('signed-in');
  expect(r.ok.connector.status).toMatchObject({ state: 'connected', signIn: 'signed-in', line: 'Connected · 3 tools' });
  expect(r.ok.connector.signIn).toMatchObject({ state: 'signed-in', client: 'dcr', host: '127.0.0.1', scopes: ['lights:read', 'offline_access'] });
  expect(r.ok.connector.auth).toBe('oauth');

  // Registered once: a native public client on the loopback.
  expect(oauth.registrations.size).toBe(1);
  const reg = [...oauth.registrations.values()][0];
  expect(reg).toMatchObject({ client_name: 'Yap', application_type: 'native', token_endpoint_auth_method: 'none', grant_types: ['authorization_code', 'refresh_token'] });
  expect(reg.redirect_uris).toEqual([CALLBACK, 'http://127.0.0.1/connectors/callback']);
  // The code swapped by a public client, with the resource.
  expect(oauth.log.find((l) => l.grant === 'authorization_code')).toMatchObject({ resource: oauth.url(), auth: 'none' });
  // The states, told as they went.
  const states = (await heard(main, 'yap-connector-status')).filter((s) => s.id === added.id).map((s) => s.signIn);
  expect(states).toContain('signing-in');
  expect(states.at(-1)).toBe('signed-in');

  // A call carries the token, to its own server only.
  const who = await call(yap, added.id, 'whoami');
  const token = who.data.text;
  expect(token).toMatch(/^at-\d+$/);
  expect(oauth.log.filter((l) => l.token).every((l) => l.host === new URL(oauth.base).host)).toBe(true);
  // Never in Yap's own files or its log (a test run's vault is its own file).
  for (const f of ['connectors.json', 'connector_tools.json', 'config.json']) expect(read(yap, f)).not.toContain(token);
  expect(read(yap, 'connectors.json')).not.toMatch(/rt-\d|code-/);
  expect(read(yap, 'connector-secrets.e2e.json')).toContain(token);
  const log = logged(yap);
  expect(log).not.toContain(token);
  expect(log).not.toMatch(/rt-\d|code-[0-9a-f]{12}|code_verifier/);
  expect(log).not.toContain(q.get('state'));
  // Every sign-in request in the receipts, by purpose.
  const groups = (await yap.invoke('connections_summary')).groups;
  expect(groups.map((g) => g.label)).toContain('Connector sign-in · A connector');

  // Signed out: the refresh token revoked, the tokens gone, "needs-sign-in".
  const out = await yap.invoke('connector_sign_out', { id: added.id });
  expect(out.confirmed).toBe(true);
  expect(out.connector.status.signIn).toBe('needs-sign-in');
  const revoked = oauth.log.find((l) => l.revoked)?.revoked;
  expect(revoked).toMatchObject({ hint: 'refresh_token', client: q.get('client_id') });
  expect(revoked.token).toMatch(/^rt-\d+$/);
  expect(read(yap, 'connector-secrets.e2e.json')).not.toContain(token);
  expect((await call(yap, added.id, 'whoami').catch((e) => ({ refused: String(e) }))).refused).toBeTruthy();

  // Signed in again: the registration reused.
  const again = await signIn(yap, oauth, added.id);
  expect(again.q.get('client_id')).toBe(q.get('client_id'));
  expect(oauth.registrations.size).toBe(1);
});

test("Yap's client ID metadata document is the client id: nothing registered", async ({ yap, oauth }) => {
  oauth.set({ cimd: true });
  const added = await add(yap, oauth);
  const { q, r } = await signIn(yap, oauth, added.id);
  expect(q.get('client_id')).toBe(oauth.cimdUrl());
  expect(r.ok.connector.signIn.client).toBe('cimd');
  expect(oauth.registrations.size).toBe(0);
  expect(oauth.cimdFetches.length).toBeGreaterThan(0);
  expect(oauth.log.find((l) => l.grant === 'authorization_code')).toMatchObject({ client: oauth.cimdUrl(), auth: 'none' });
  expect((await call(yap, added.id, 'list_lights')).data.text).toContain('Kitchen: on');
});

test('a hand-entered client: asked for when the server has no other way, then used with its secret', async ({ yap, oauth }) => {
  oauth.set({ dcr: false, cimd: false, clients: [{ id: 'own-client-FAKE', secret: 'own-secret-FAKE-91c2' }] });
  const added = await add(yap, oauth);
  const asked = await startSignIn(yap, added.id);
  expect(asked.url, 'no browser without a client').toBeNull();
  const why = (await asked.done()).err;
  expect(why).toContain('needs an app registration');
  expect(why).toContain(CALLBACK);

  const v = await yap.invoke('connector_set_client', { id: added.id, clientId: 'own-client-FAKE', secret: 'own-secret-FAKE-91c2' });
  expect(v.signIn).toMatchObject({ ownClientId: 'own-client-FAKE', hasOwnSecret: true });
  expect(read(yap, 'connectors.json')).toContain('own-client-FAKE');
  expect(read(yap, 'connectors.json')).not.toContain('own-secret-FAKE');

  const { q, r } = await signIn(yap, oauth, added.id);
  expect(q.get('client_id')).toBe('own-client-FAKE');
  expect(r.ok.connector.signIn.client).toBe('own');
  // The secret sent as the server lists first (HTTP Basic), never in the address.
  expect(oauth.log.find((l) => l.grant === 'authorization_code')).toMatchObject({ client: 'own-client-FAKE', auth: 'basic' });
  expect(q.toString()).not.toContain('own-secret');
  expect(logged(yap)).not.toContain('own-secret-FAKE');
});

test('a short-lived token is refreshed by itself, its refresh token rotating and never used twice', async ({ yap, oauth }) => {
  oauth.set({ accessTtl: 4 });
  const added = await add(yap, oauth);
  await signIn(yap, oauth, added.id);
  const first = (await call(yap, added.id, 'whoami')).data.text;
  // Half its life on, the next call refreshes first.
  await new Promise((r) => setTimeout(r, 2_600));
  const second = (await call(yap, added.id, 'whoami')).data.text;
  expect(second).not.toBe(first);
  await new Promise((r) => setTimeout(r, 2_600));
  const third = (await call(yap, added.id, 'whoami')).data.text;
  expect(third).not.toBe(second);
  const refreshes = oauth.log.filter((l) => l.grant === 'refresh_token');
  expect(refreshes.length).toBeGreaterThanOrEqual(2);
  expect(new Set(refreshes.map((l) => l.refreshToken)).size, 'each refresh token used once').toBe(refreshes.length);
  expect(refreshes.every((l) => !l.replay && l.resource === oauth.url())).toBe(true);
});

test('a revoked token is refreshed; a revoked grant ends the sign-in until it is signed in again', async ({ yap, oauth }) => {
  const added = await add(yap, oauth);
  await signIn(yap, oauth, added.id);
  const first = (await call(yap, added.id, 'whoami')).data.text;
  // The server turns that token down: one refresh, the call sent again.
  oauth.expire(first);
  const second = (await call(yap, added.id, 'whoami')).data.text;
  expect(second).not.toBe(first);
  expect(oauth.log.some((l) => l.token === first && l.refused)).toBe(true);
  // The server revokes everything: the refresh is turned down.
  oauth.revokeAll();
  const failed = await call(yap, added.id, 'whoami');
  expect(failed.success).toBe(false);
  expect(failed.error).toBe('Your sign-in to 127.0.0.1 has ended. Sign in again.');
  const listed = (await yap.invoke('connectors_list')).connectors.find((c) => c.id === added.id);
  expect(listed.status).toMatchObject({ signIn: 'expired', line: 'Your sign-in to 127.0.0.1 has ended. Sign in again.' });
  expect(listed.signIn.state).toBe('expired');
  const { r } = await signIn(yap, oauth, added.id);
  expect(r.ok.connector.status.signIn).toBe('signed-in');
  expect((await call(yap, added.id, 'list_lights')).success).toBe(true);
});

test('a server wanting more access asks to sign in again, and the next sign-in asks for more', async ({ yap, oauth }) => {
  const added = await add(yap, oauth);
  await signIn(yap, oauth, added.id);
  const denied = await call(yap, added.id, 'turn_off', { area: 'hall' });
  expect(denied.success).toBe(false);
  expect(denied.error).toBe('127.0.0.1 needs more access for this. Sign in again.');
  expect(oauth.log.some((l) => l.stepUp)).toBe(true);
  const listed = (await yap.invoke('connectors_list')).connectors.find((c) => c.id === added.id);
  expect(listed.status.signIn).toBe('needs-sign-in');
  const { q } = await signIn(yap, oauth, added.id);
  expect(q.get('scope')).toBe('lights:read offline_access lights:write');
  expect((await call(yap, added.id, 'turn_off', { area: 'hall' })).data.text).toBe('Turned off the hall lights.');
});

test('refused: an answer from another issuer, metadata naming another, a resource for another server, no PKCE', async ({ yap, oauth }) => {
  const added = await add(yap, oauth);
  // The answer names another issuer: its code is never used, nothing from it shown.
  oauth.set({ wrongIss: true });
  const mixed = await signIn(yap, oauth, added.id);
  expect(mixed.page).toContain('Not signed in');
  expect(mixed.page).toContain("didn't come from the sign-in server Yap asked");
  expect(mixed.r.err).toContain("didn't come from the sign-in server Yap asked");
  expect(oauth.log.some((l) => l.grant === 'authorization_code')).toBe(false);

  // Refused before the browser opens.
  const refusedBefore = async (changes, words) => {
    oauth.reset();
    oauth.set(changes);
    const s = await startSignIn(yap, added.id);
    expect(s.url, JSON.stringify(changes)).toBeNull();
    expect((await s.done()).err).toContain(words);
  };
  await refusedBefore({ issuerMismatch: true }, 'answered as a different sign-in server');
  await refusedBefore({ resourceMismatch: true }, "sign-in is for another server (mcp.example.com), so Yap stopped there");
  await refusedBefore({ noS256: true }, "doesn't meet the MCP security rules");
  expect(oauth.registrations.size, 'nothing registered with a server Yap refused').toBe(0);
  const listed = (await yap.invoke('connectors_list')).connectors.find((c) => c.id === added.id);
  expect(listed.status.signIn).toBe('needs-sign-in');
});

test('a decline and a Cancel', async ({ yap, oauth }) => {
  const added = await add(yap, oauth);
  // Declined on the consent page: Yap's words, never the server's text.
  const s = await startSignIn(yap, added.id);
  const page = await (await fetch(oauth.deny(s.url))).text();
  expect(page).toContain("You didn't allow Yap to use 127.0.0.1.");
  expect(page).not.toContain('evil.example.com');
  expect((await s.done()).err).toBe("You didn't allow Yap to use 127.0.0.1.");

  // Cancelled while the browser is open: the loopback stops listening.
  const waiting = await startSignIn(yap, added.id);
  const back = await oauth.approve(waiting.url);
  expect((await yap.invoke('connectors_list')).connectors[0].status).toMatchObject({ signIn: 'signing-in', line: 'Signing in… Finish in your browser.' });
  expect(await yap.invoke('connector_sign_in_cancel', { id: added.id })).toBe(true);
  expect((await waiting.done()).ok.outcome).toBe('cancelled');
  await expect.poll(() => fetch(back).then(() => 'answered', () => 'closed'), { message: 'the loopback closed' }).toBe('closed');
  expect((await yap.invoke('connectors_list')).connectors[0].status.signIn).toBe('needs-sign-in');
});

test('discovery: metadata only where the challenge says, OpenID for an issuer with a path, a slow server', async ({ yap, oauth }) => {
  oauth.set({ prm: 'header-only', as: 'oidc', issuerPath: '/tenant1', slowMs: 400 });
  const added = await add(yap, oauth);
  const { r } = await signIn(yap, oauth, added.id);
  expect(r.ok.connector.status).toMatchObject({ state: 'connected', signIn: 'signed-in' });
  const gets = oauth.log.filter((l) => l.method === 'GET').map((l) => l.path);
  expect(gets).toContain('/meta/prm.json');
  // RFC 8414 with the path inserted first, then OpenID with it inserted.
  expect(gets.indexOf('/.well-known/oauth-authorization-server/tenant1')).toBeLessThan(gets.indexOf('/.well-known/openid-configuration/tenant1'));
  expect(gets).not.toContain('/.well-known/oauth-protected-resource/mcp');
});

test('the fixed port busy: a server allowing any loopback port takes the one Yap listens on', async ({ yap, oauth }) => {
  const blocker = net.createServer();
  const held = await new Promise((r) => blocker.once('error', () => r(false)).listen(51439, '127.0.0.1', () => r(true)));
  test.skip(!held, 'something else holds 51439');
  try {
    oauth.set({ redirect: 'any-port' });
    const added = await add(yap, oauth);
    const { q, r } = await signIn(yap, oauth, added.id);
    const port = Number(q.get('redirect_uri').match(/^http:\/\/127\.0\.0\.1:(\d+)\/connectors\/callback$/)[1]);
    expect(port).not.toBe(51439);
    expect([...oauth.registrations.values()][0].redirect_uris).toContain(q.get('redirect_uri'));
    expect(r.ok.outcome).toBe('signed-in');
  } finally {
    await new Promise((r) => blocker.close(r));
  }
});
