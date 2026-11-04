// A local stand-in for OpenAI's Sign in with ChatGPT endpoints and the
// Responses API on a ChatGPT plan (src-tauri/src/ai/chatgpt.rs and
// ai/llm/responses.rs, pointed here with the debug-only YAP_CHATGPT_ISSUER /
// YAP_CHATGPT_API variables). It checks what OpenAI would:
// - sign-in: the discovery document's endpoints on the issuer; a new
//   registration (`dynamic_agent_client`, the app's name, a host id) gets
//   an issued client id back; the PKCE verifier against the challenge, the
//   redirect address, the resource; an RS256 ID token signed with a key it
//   publishes (its nonce, the issued client as audience); refresh tokens
//   that rotate (a reused one is refused); revocation; with
//   `refuseNewRegistrationCodes(true)`, a new registration's code refused
//   with `invalid_grant`, as OpenAI did on the first real sign-in (2 Nov
//   2026), while a sign-in with the issued id goes through;
// - inference: a bearer token it issued; `store: false` and `stream: true`
//   on every request, none of the fields the flow refuses, Yap's tools in
//   one `yap` namespace; answers as server-sent events ending in
//   `response.completed`. A question with "notes" in it gets a namespaced
//   `search_notes` call first (with an encrypted reasoning item), then an
//   answer once its result comes back with that reasoning sent back whole;
//   one with "limit" gets the plan's usage limit.
//
// It plays the browser's part too: `approve(authUrl)` is the person
// clicking through the consent page; it returns the address OpenAI would
// send the browser back to (Yap's loopback listener), which the spec opens.
import crypto from 'node:crypto';
import http from 'node:http';

const b64url = (buf) => Buffer.from(buf).toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

const REFUSED = ['temperature', 'top_p', 'max_output_tokens', 'previous_response_id', 'metadata', 'user', 'truncation', 'background'];

export async function startFakeChatGpt({ email = 'sam@example.com', name = 'Sam Example', plan = true, models = null } = {}) {
  const { privateKey, publicKey } = crypto.generateKeyPairSync('rsa', { modulusLength: 2048 });
  const jwk = { ...publicKey.export({ format: 'jwk' }), kid: 'e2e-key', use: 'sig', alg: 'RS256' };
  const issued = 'oaiapp_e2e_yap';
  const sub = 'user-e2e-1';
  const codes = new Map(); // code → { challenge, redirectUri, nonce, fresh }
  let refuseNew = false;
  const access = new Set();
  const refresh = new Set();
  const spent = new Set();
  const registrations = []; // the authorize requests' query parameters
  const revoked = [];
  const responses = []; // every /v1/responses body
  const problems = []; // anything a real OpenAI would refuse
  let base = '';

  const json = (res, status, body) => {
    res.writeHead(status, { 'content-type': 'application/json' });
    res.end(JSON.stringify(body));
  };

  function idToken(nonce) {
    const now = Math.floor(Date.now() / 1000);
    const enc = (v) => b64url(JSON.stringify(v));
    const signed = `${enc({ alg: 'RS256', kid: 'e2e-key', typ: 'JWT' })}.${enc({ iss: base, aud: issued, sub, email, name, nonce, iat: now, exp: now + 3600 })}`;
    return `${signed}.${b64url(crypto.sign('RSA-SHA256', Buffer.from(signed), privateKey))}`;
  }

  function tokens(res, nonce, withId = true) {
    const a = `e2e-access-${crypto.randomBytes(6).toString('hex')}`;
    const r = `e2e-refresh-${crypto.randomBytes(6).toString('hex')}`;
    access.add(a);
    refresh.add(r);
    const scope = `openid profile email offline_access resource.invoke${plan ? ' chatgpt.tokens.use.direct' : ''}`;
    json(res, 200, { access_token: a, token_type: 'Bearer', expires_in: 3600, refresh_token: r, scope, ...(withId ? { id_token: idToken(nonce) } : {}) });
  }

  function token(res, p) {
    if (p.resource !== 'https://api.openai.com/v1') problems.push(`token: resource ${p.resource}`);
    if (p.client_id !== issued) return json(res, 401, { error: 'invalid_client' });
    if (p.grant_type === 'authorization_code') {
      const grant = codes.get(p.code);
      codes.delete(p.code);
      if (grant?.fresh && refuseNew) return json(res, 400, { error: 'invalid_grant' });
      const challenge = p.code_verifier ? b64url(crypto.createHash('sha256').update(p.code_verifier).digest()) : '';
      if (!grant || grant.challenge !== challenge || grant.redirectUri !== p.redirect_uri) return json(res, 400, { error: 'invalid_grant' });
      return tokens(res, grant.nonce);
    }
    if (p.grant_type === 'refresh_token') {
      if (spent.has(p.refresh_token)) return json(res, 400, { error: 'refresh_token_reused' });
      if (!refresh.has(p.refresh_token)) return json(res, 400, { error: 'invalid_grant' });
      refresh.delete(p.refresh_token);
      spent.add(p.refresh_token);
      return tokens(res, undefined, false);
    }
    return json(res, 400, { error: 'unsupported_grant_type' });
  }

  function sse(res, events) {
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    for (const e of events) res.write(`event: ${e.type}\ndata: ${JSON.stringify(e)}\n\n`);
    res.end();
  }

  function answer(res, body) {
    responses.push(body);
    if (body.store !== false || body.stream !== true) problems.push('responses: store/stream');
    for (const k of REFUSED) if (k in body) problems.push(`responses: ${k}`);
    if ((body.input || []).some((i) => i.role === 'system')) problems.push('responses: a system item');
    const tools = body.tools || [];
    if (tools.some((t) => t.type === 'function')) problems.push('responses: a function tool outside a namespace');
    const input = body.input || [];
    const lastUser = [...input].reverse().find((i) => i.role === 'user');
    const asked = typeof lastUser?.content === 'string' ? lastUser.content : (lastUser?.content || []).map((p) => p.text || '').join(' ');
    const usage = { input_tokens: 1200, output_tokens: 40, total_tokens: 1240 };
    if (/limit/i.test(asked)) {
      return json(res, 429, { error: { code: 'subscription_sharing_usage_limit_exceeded', message: 'Usage limit reached', param: null } });
    }
    const result = input.find((i) => i.type === 'function_call_output');
    if (/notes/i.test(asked) && tools.some((t) => t.type === 'namespace' && t.name === 'yap') && !result) {
      const reasoning = { id: 'rs_e2e', type: 'reasoning', summary: [{ type: 'summary_text', text: 'Look in the notes.' }], encrypted_content: 'gAAAA-e2e-opaque' };
      const call = { id: 'fc_e2e', type: 'function_call', status: 'completed', namespace: 'yap', name: 'search_notes', call_id: 'call_e2e', arguments: '{"query":"launch"}' };
      return sse(res, [
        { type: 'response.created', response: { status: 'in_progress' } },
        { type: 'response.reasoning_summary_text.delta', output_index: 0, delta: 'Look in the notes.' },
        { type: 'response.output_item.added', output_index: 1, item: { ...call, arguments: '' } },
        { type: 'response.function_call_arguments.delta', output_index: 1, delta: call.arguments },
        { type: 'response.completed', response: { status: 'completed', output: [reasoning, call], usage } },
      ]);
    }
    let text = 'Hello from your ChatGPT plan.';
    if (result) {
      const replayed = input.find((i) => i.type === 'reasoning');
      const ok = replayed?.encrypted_content === 'gAAAA-e2e-opaque' && !('id' in replayed) && input.some((i) => i.type === 'function_call' && i.namespace === 'yap' && i.call_id === 'call_e2e');
      if (!ok) problems.push('responses: the tool step did not send back its reasoning and call');
      text = 'I looked in your notes.';
    }
    const message = { id: 'msg_e2e', type: 'message', role: 'assistant', content: [{ type: 'output_text', text, annotations: [] }] };
    sse(res, [
      { type: 'response.created', response: { status: 'in_progress' } },
      ...text.split(' ').map((w, i) => ({ type: 'response.output_text.delta', output_index: 0, delta: (i ? ' ' : '') + w })),
      { type: 'response.completed', response: { status: 'completed', output: [message], usage } },
    ]);
  }

  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (c) => (raw += c));
    req.on('end', () => {
      const url = new URL(req.url, base);
      const bearer = (req.headers.authorization || '').replace(/^Bearer /, '');
      if (req.method === 'GET' && url.pathname === '/.well-known/openid-configuration') {
        return json(res, 200, {
          issuer: base,
          authorization_endpoint: `${base}/api/accounts/authorize`,
          token_endpoint: `${base}/api/accounts/oauth/token`,
          revocation_endpoint: `${base}/api/accounts/oauth/revoke`,
          jwks_uri: `${base}/.well-known/jwks.json`,
        });
      }
      if (req.method === 'GET' && url.pathname === '/.well-known/jwks.json') return json(res, 200, { keys: [jwk] });
      if (req.method === 'POST' && url.pathname === '/api/accounts/oauth/token') return token(res, Object.fromEntries(new URLSearchParams(raw)));
      if (req.method === 'POST' && url.pathname === '/api/accounts/oauth/revoke') {
        revoked.push(Object.fromEntries(new URLSearchParams(raw)));
        res.writeHead(200);
        return res.end();
      }
      if (url.pathname.startsWith('/v1/') && !access.has(bearer)) {
        return json(res, 401, { error: { code: 'subscription_sharing_invalid_user', message: 'no' } });
      }
      if (req.method === 'GET' && url.pathname === '/v1/models') {
        return json(res, 200, {
          models: models ?? [
            { slug: 'gpt-6.1-sol', display_name: 'GPT-6.1 Sol', visibility: 'list' },
            { slug: 'gpt-6-luna', display_name: 'GPT-6 Luna', visibility: 'list' },
            { slug: 'internal-tool', display_name: 'Hidden', visibility: 'hide' },
          ],
        });
      }
      if (req.method === 'POST' && url.pathname === '/v1/responses') return answer(res, JSON.parse(raw || '{}'));
      json(res, 404, { error: 'not found' });
    });
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  base = `http://127.0.0.1:${server.address().port}`;

  /** The consent page, allowed: where the browser goes next. */
  function approve(authUrl) {
    const q = new URL(authUrl).searchParams;
    registrations.push(Object.fromEntries(q));
    const code = `e2e-code-${crypto.randomBytes(6).toString('hex')}`;
    codes.set(code, { challenge: q.get('code_challenge'), redirectUri: q.get('redirect_uri'), nonce: q.get('nonce'), fresh: q.get('client_id') === 'dynamic_agent_client' });
    const back = new URL(q.get('redirect_uri'));
    back.searchParams.set('code', code);
    back.searchParams.set('state', q.get('state'));
    if (q.get('client_id') === 'dynamic_agent_client') back.searchParams.set('client_id', issued);
    return back.toString();
  }

  /** The consent page, declined. */
  function deny(authUrl) {
    const q = new URL(authUrl).searchParams;
    const back = new URL(q.get('redirect_uri'));
    back.searchParams.set('error', 'access_denied');
    back.searchParams.set('state', q.get('state'));
    return back.toString();
  }

  return {
    env: { YAP_CHATGPT_ISSUER: base, YAP_CHATGPT_API: `${base}/v1` },
    base,
    issued,
    approve,
    deny,
    refuseNewRegistrationCodes: (on) => {
      refuseNew = on;
    },
    registrations,
    revoked,
    responses,
    problems,
    close: () => new Promise((r) => server.close(r)),
  };
}
