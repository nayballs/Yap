// A fake MCP server that signs in, with its own authorization server, for
// connectors-signin.spec.js (desktop/src-tauri/src/integrations/connectors/oauth.rs).
// One HTTP server on 127.0.0.1 plays both, in the mode a test sets
// (`fake.set({...})`), and checks what a real one would:
//
// - discovery: a 401 with `WWW-Authenticate: Bearer resource_metadata=…,
//   scope="lights:read"`; protected resource metadata (RFC 9728) at both
//   well-known forms, or only at the address the header names
//   (`prm: 'header-only'`); authorization server metadata by RFC 8414 or
//   OpenID (`as: 'oidc'`), for an issuer with a path (`issuerPath`);
// - clients: Yap's client ID metadata document (`cimd`: fetched from the
//   stand-in address this fake serves, its `client_id` and redirects
//   checked), dynamic registration (`dcr`: a native public client),
//   pre-registered ones (`clients`: an id and a secret, sent as HTTP Basic
//   or in the form), or none of them (Yap must ask for a client id);
// - refusals: no `code_challenge_methods_supported` (`noS256`), an answer
//   with another `iss` (`wrongIss`), metadata naming another issuer
//   (`issuerMismatch`), a resource for another server (`resourceMismatch`),
//   a redirect off the client's list (exact, or any port on the loopback:
//   `redirect: 'exact' | 'any-port'`);
// - the browser: `approve(authUrl)` is the person clicking Allow (it
//   returns where the server sends the browser: Yap's loopback, which the
//   spec opens), `deny(authUrl)` clicking Deny;
// - tokens: `accessTtl` seconds (short, to see refreshes), refresh tokens
//   that rotate with replay detection (a spent one revokes its whole
//   family), `revokeAll()` (the server revoking everything), revocation
//   (RFC 7009) logged; `turn_off` needs `lights:write` (a `403
//   insufficient_scope`: step-up);
// - `slowMs`: every answer after that long.
//
// It logs every request (`log`) with header names and the tokens' ids for
// the spec to read. No network beyond 127.0.0.1; fictional data only.
import crypto from 'node:crypto';
import http from 'node:http';

const b64url = (buf) => Buffer.from(buf).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
const sha256 = (s) => b64url(crypto.createHash('sha256').update(s).digest());

const DEFAULTS = {
  prm: 'both',
  as: 'oauth',
  issuerPath: '',
  cimd: false,
  dcr: true,
  clients: [],
  noS256: false,
  wrongIss: false,
  issuerMismatch: false,
  resourceMismatch: false,
  redirect: 'exact',
  accessTtl: 3600,
  slowMs: 0,
};

export async function startFakeMcpOauth() {
  let mode = { ...DEFAULTS };
  let base = '';
  const log = [];
  const codes = new Map(); // code → { challenge, redirectUri, clientId, scope, resource }
  const access = new Map(); // token → { scope, clientId, family }
  const refresh = new Map(); // token → { scope, clientId, family }
  const spent = new Map(); // a used refresh token → its family
  const registrations = new Map(); // client_id → its metadata
  const cimdFetches = [];
  let n = 0;

  const issuer = () => `${base}${mode.issuerPath}`;
  const resource = () => `${base}/mcp`;
  const cimdUrl = () => `${base}/oauth/desktop-client.json`;

  const json = (res, status, body, headers = {}) => {
    res.writeHead(status, { 'content-type': 'application/json', ...headers });
    res.end(JSON.stringify(body));
  };
  const form = (raw) => Object.fromEntries(new URLSearchParams(raw));

  /** Yap's client ID metadata document (the stand-in for where it's hosted). */
  function cimdDocument() {
    return {
      client_id: cimdUrl(),
      client_name: 'Yap',
      redirect_uris: ['http://127.0.0.1:51439/connectors/callback', 'http://127.0.0.1/connectors/callback'],
      grant_types: ['authorization_code', 'refresh_token'],
      response_types: ['code'],
      token_endpoint_auth_method: 'none',
      application_type: 'native',
    };
  }

  /** `uri` is one of `listed` (exactly; or, in any-port mode, on the loopback whatever the port). */
  function redirectAllowed(uri, listed) {
    if (listed.includes(uri)) return true;
    if (mode.redirect !== 'any-port') return false;
    const u = new URL(uri);
    return listed.some((l) => {
      const v = new URL(l);
      return v.hostname === '127.0.0.1' && u.hostname === '127.0.0.1' && v.protocol === u.protocol && v.pathname === u.pathname;
    });
  }

  /** The client `id` as this server knows it: its redirects and secret. */
  async function client(id) {
    if (mode.cimd && id === cimdUrl()) {
      // The authorization server fetches the document the id names.
      const doc = await (await fetch(id)).json();
      cimdFetches.push(id);
      if (doc.client_id !== id) throw new Error('the document names another client');
      return { redirects: doc.redirect_uris, secret: null, kind: 'cimd' };
    }
    if (registrations.has(id)) return { redirects: registrations.get(id).redirect_uris, secret: null, kind: 'dcr' };
    const pre = mode.clients.find((c) => c.id === id);
    if (pre) return { redirects: pre.redirects ?? ['http://127.0.0.1:51439/connectors/callback'], secret: pre.secret ?? null, kind: 'pre' };
    return null;
  }

  function issue(scope, clientId, family = `f${++n}`) {
    n += 1;
    const at = `at-${n}`;
    const rt = `rt-${n}`;
    access.set(at, { scope, clientId, family, expires: Date.now() + mode.accessTtl * 1000 });
    refresh.set(rt, { scope, clientId, family });
    return { access_token: at, token_type: 'Bearer', expires_in: mode.accessTtl, refresh_token: rt, scope };
  }

  function revokeFamily(family) {
    for (const [t, v] of access) if (v.family === family) access.delete(t);
    for (const [t, v] of refresh) if (v.family === family) refresh.delete(t);
  }

  /** The client a token request authenticates as (Basic, the form, or a public id). */
  async function authenticate(req, f) {
    const basic = req.headers.authorization?.startsWith('Basic ') ? Buffer.from(req.headers.authorization.slice(6), 'base64').toString() : null;
    const [id, secret] = basic ? basic.split(':').map(decodeURIComponent) : [f.client_id, f.client_secret];
    const known = id ? await client(id) : null;
    if (!known) return { error: 'invalid_client' };
    if (known.secret && known.secret !== secret) return { error: 'invalid_client' };
    return { id, how: basic ? 'basic' : f.client_secret ? 'post' : 'none' };
  }

  const tools = [
    { name: 'list_lights', description: 'List the lights.', inputSchema: { type: 'object', properties: {} }, annotations: { readOnlyHint: true } },
    { name: 'turn_off', description: 'Turn a light off.', inputSchema: { type: 'object', properties: { area: { type: 'string' } } }, annotations: { destructiveHint: false } },
    { name: 'whoami', description: 'Which token called.', inputSchema: { type: 'object', properties: {} }, annotations: { readOnlyHint: true } },
  ];

  function mcp(msg, token) {
    const { id, method, params } = msg;
    if (id === undefined || id === null) return undefined;
    const ok = (result) => ({ jsonrpc: '2.0', id, result });
    const text = (t) => ok({ content: [{ type: 'text', text: t }] });
    switch (method) {
      case 'initialize':
        return ok({ protocolVersion: params?.protocolVersion ?? '2025-06-18', capabilities: { tools: {} }, serverInfo: { name: 'fake-mcp-oauth', version: '1.0.0' } });
      case 'tools/list':
        return ok({ tools });
      case 'ping':
        return ok({});
      case 'tools/call':
        if (params?.name === 'list_lights') return text('Kitchen: on\nHall: off');
        if (params?.name === 'turn_off') return text(`Turned off the ${params?.arguments?.area ?? ''} lights.`);
        if (params?.name === 'whoami') return text(token);
        return { jsonrpc: '2.0', id, error: { code: -32602, message: 'Unknown tool' } };
      default:
        return { jsonrpc: '2.0', id, error: { code: -32601, message: 'Method not found' } };
    }
  }

  function challenge() {
    const prm = mode.prm === 'header-only' ? `${base}/meta/prm.json` : `${base}/.well-known/oauth-protected-resource/mcp`;
    return mode.prm === 'well-known' ? 'Bearer scope="lights:read"' : `Bearer resource_metadata="${prm}", scope="lights:read"`;
  }

  function prmDocument() {
    return {
      resource: mode.resourceMismatch ? 'https://mcp.example.com/mcp' : resource(),
      authorization_servers: [issuer()],
      scopes_supported: ['lights:read', 'lights:write'],
      bearer_methods_supported: ['header'],
    };
  }

  function asDocument() {
    const doc = {
      issuer: mode.issuerMismatch ? `${base}/someone-else` : issuer(),
      authorization_endpoint: `${base}/authorize`,
      token_endpoint: `${base}/token`,
      revocation_endpoint: `${base}/revoke`,
      response_types_supported: ['code'],
      grant_types_supported: ['authorization_code', 'refresh_token'],
      scopes_supported: ['lights:read', 'lights:write', 'offline_access'],
      token_endpoint_auth_methods_supported: ['none', 'client_secret_basic', 'client_secret_post'],
      authorization_response_iss_parameter_supported: true,
    };
    if (!mode.noS256) doc.code_challenge_methods_supported = ['S256'];
    if (mode.dcr) doc.registration_endpoint = `${base}/register`;
    if (mode.cimd) doc.client_id_metadata_document_supported = true;
    return doc;
  }

  /** Where each metadata document is served, by the mode. */
  function metadataAt(path) {
    const p = mode.issuerPath;
    if (path === '/meta/prm.json' && mode.prm === 'header-only') return prmDocument();
    if (mode.prm !== 'header-only' && (path === '/.well-known/oauth-protected-resource/mcp' || path === '/.well-known/oauth-protected-resource')) return prmDocument();
    if (mode.as === 'oauth' && path === `/.well-known/oauth-authorization-server${p}`) return asDocument();
    if (mode.as === 'oidc' && (path === `/.well-known/openid-configuration${p}` || (p && path === `${p}/.well-known/openid-configuration`))) return asDocument();
    return null;
  }

  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (d) => (raw += d));
    req.on('end', async () => {
      if (mode.slowMs) await new Promise((r) => setTimeout(r, mode.slowMs));
      const url = new URL(req.url, base);
      const entry = { method: req.method, path: url.pathname, host: req.headers.host, headers: Object.keys(req.headers).sort() };
      log.push(entry);
      try {
        if (req.method === 'GET' && url.pathname === '/oauth/desktop-client.json') return json(res, 200, cimdDocument());
        if (req.method === 'GET') {
          const doc = metadataAt(url.pathname);
          return doc ? json(res, 200, doc) : json(res, 404, { error: 'not_found' });
        }
        if (req.method === 'POST' && url.pathname === '/mcp') {
          const token = req.headers.authorization?.startsWith('Bearer ') ? req.headers.authorization.slice(7) : null;
          const live = token && access.get(token);
          entry.token = token;
          if (!live || live.expires < Date.now()) {
            entry.refused = true;
            res.writeHead(401, { 'WWW-Authenticate': challenge() });
            return res.end();
          }
          let msg = null;
          try {
            msg = JSON.parse(raw || 'null');
          } catch {}
          entry.rpc = msg?.method;
          entry.tool = msg?.params?.name;
          if (msg?.method === 'tools/call' && msg.params?.name === 'turn_off' && !live.scope.split(' ').includes('lights:write')) {
            entry.stepUp = true;
            res.writeHead(403, { 'WWW-Authenticate': 'Bearer error="insufficient_scope", scope="lights:read lights:write"' });
            return res.end();
          }
          const a = msg ? mcp(msg, token) : undefined;
          if (!a) {
            res.writeHead(202);
            return res.end();
          }
          return json(res, 200, a);
        }
        if (req.method === 'POST' && url.pathname === '/register') {
          const body = JSON.parse(raw || '{}');
          entry.registration = body;
          if (body.application_type !== 'native' || !Array.isArray(body.redirect_uris) || !body.redirect_uris.length) return json(res, 400, { error: 'invalid_client_metadata' });
          const id = `dcr-${++n}`;
          registrations.set(id, body);
          return json(res, 201, { ...body, client_id: id, client_id_issued_at: Math.floor(Date.now() / 1000) });
        }
        if (req.method === 'POST' && url.pathname === '/token') {
          const f = form(raw);
          entry.grant = f.grant_type;
          entry.resource = f.resource;
          const who = await authenticate(req, f);
          entry.client = who.id;
          entry.auth = who.how;
          if (who.error) return json(res, 401, { error: who.error });
          if (f.resource !== resource()) return json(res, 400, { error: 'invalid_target' });
          if (f.grant_type === 'authorization_code') {
            const c = codes.get(f.code);
            codes.delete(f.code);
            entry.code = f.code;
            if (!c || c.clientId !== who.id || c.redirectUri !== f.redirect_uri || sha256(f.code_verifier ?? '') !== c.challenge) return json(res, 400, { error: 'invalid_grant' });
            return json(res, 200, issue(c.scope, who.id));
          }
          if (f.grant_type === 'refresh_token') {
            entry.refreshToken = f.refresh_token;
            if (spent.has(f.refresh_token)) {
              // A rotated token used again: the whole grant ends.
              entry.replay = true;
              revokeFamily(spent.get(f.refresh_token));
              return json(res, 400, { error: 'invalid_grant' });
            }
            const r = refresh.get(f.refresh_token);
            if (!r || r.clientId !== who.id) return json(res, 400, { error: 'invalid_grant' });
            refresh.delete(f.refresh_token);
            spent.set(f.refresh_token, r.family);
            return json(res, 200, issue(r.scope, who.id, r.family));
          }
          return json(res, 400, { error: 'unsupported_grant_type' });
        }
        if (req.method === 'POST' && url.pathname === '/revoke') {
          const f = form(raw);
          const who = await authenticate(req, f);
          entry.revoked = { token: f.token, hint: f.token_type_hint, client: who.id };
          const family = access.get(f.token)?.family ?? refresh.get(f.token)?.family;
          if (family) revokeFamily(family);
          res.writeHead(200);
          return res.end();
        }
        json(res, 404, { error: 'not_found' });
      } catch (e) {
        json(res, 500, { error: 'server_error', error_description: String(e) });
      }
    });
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  base = `http://127.0.0.1:${server.address().port}`;

  /** The person clicks Allow: checked as the server would; where the browser goes next. */
  async function approve(authUrl) {
    const q = new URL(authUrl).searchParams;
    const known = await client(q.get('client_id'));
    if (!known) throw new Error(`unknown client ${q.get('client_id')}`);
    if (!redirectAllowed(q.get('redirect_uri'), known.redirects)) throw new Error(`redirect not registered: ${q.get('redirect_uri')}`);
    if (q.get('response_type') !== 'code' || q.get('code_challenge_method') !== 'S256' || !q.get('code_challenge') || !q.get('state')) throw new Error('not a PKCE code request');
    if (q.get('resource') !== resource()) throw new Error(`resource ${q.get('resource')}`);
    const code = `code-${crypto.randomBytes(6).toString('hex')}`;
    codes.set(code, { challenge: q.get('code_challenge'), redirectUri: q.get('redirect_uri'), clientId: q.get('client_id'), scope: q.get('scope') ?? '', resource: q.get('resource') });
    const back = new URL(q.get('redirect_uri'));
    back.searchParams.set('code', code);
    back.searchParams.set('state', q.get('state'));
    back.searchParams.set('iss', mode.wrongIss ? 'http://127.0.0.1:1/elsewhere' : issuer());
    return back.toString();
  }

  /** The person clicks Deny. */
  function deny(authUrl) {
    const q = new URL(authUrl).searchParams;
    const back = new URL(q.get('redirect_uri'));
    back.searchParams.set('error', 'access_denied');
    back.searchParams.set('error_description', 'Visit evil.example.com to fix this');
    back.searchParams.set('state', q.get('state'));
    back.searchParams.set('iss', issuer());
    return back.toString();
  }

  return {
    get base() {
      return base;
    },
    url: () => resource(),
    cimdUrl,
    log,
    registrations,
    cimdFetches,
    approve,
    deny,
    /** Change the mode (the rest as it is). */
    set(changes) {
      mode = { ...mode, ...changes };
    },
    /** Back to the defaults, everything forgotten. */
    reset() {
      mode = { ...DEFAULTS };
      log.length = 0;
      codes.clear();
      access.clear();
      refresh.clear();
      spent.clear();
      registrations.clear();
      cimdFetches.length = 0;
    },
    /** The server revokes every token it gave out. */
    revokeAll() {
      access.clear();
      refresh.clear();
    },
    /** The server turns one access token down. */
    expire(token) {
      access.delete(token);
    },
    tokens: () => ({ access: [...access.keys()], refresh: [...refresh.keys()] }),
    close: () => new Promise((r) => server.close(r)),
  };
}
