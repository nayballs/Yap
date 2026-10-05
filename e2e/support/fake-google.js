// A local stand-in for Google's OAuth endpoints and the Calendar API, for
// the one-click Google Calendar connect (src-tauri/src/calendar/google.rs,
// pointed here with the debug-only YAP_GOOGLE_*_URL variables and a fake
// client id). It checks what a real Google would: the PKCE verifier against
// the challenge, the client, the redirect address, the scopes asked for, and
// a bearer token on every API call.
//
// It plays the browser's part too: `approve(authUrl)` is the person
// clicking Allow on the consent page; it returns the address Google would
// send the browser back to (Yap's loopback listener), which the spec then
// opens with fetch.
import crypto from 'node:crypto';
import http from 'node:http';

const b64url = (buf) => buf.toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

export async function startFakeGoogle({ clientId, clientSecret, account = 'tester@example.com', events = () => [] }) {
  const codes = new Map(); // code → { challenge, redirectUri, scope }
  const access = new Map(); // access token → refresh token
  const refresh = new Set();
  const requests = []; // { method, path }
  const revoked = [];
  let lastAuth = null;

  function form(raw) {
    return Object.fromEntries(new URLSearchParams(raw));
  }

  function json(res, status, body) {
    res.writeHead(status, { 'content-type': 'application/json' });
    res.end(JSON.stringify(body));
  }

  function issue(res, refreshToken, scope) {
    const token = `ya29.e2e-${crypto.randomBytes(6).toString('hex')}`;
    access.set(token, refreshToken);
    json(res, 200, { access_token: token, expires_in: 3599, refresh_token: refreshToken, scope, token_type: 'Bearer' });
  }

  function token(res, p) {
    if (p.client_id !== clientId || (clientSecret && p.client_secret !== clientSecret)) {
      return json(res, 401, { error: 'invalid_client' });
    }
    if (p.grant_type === 'authorization_code') {
      const grant = codes.get(p.code);
      codes.delete(p.code);
      const challenge = p.code_verifier ? b64url(crypto.createHash('sha256').update(p.code_verifier).digest()) : '';
      if (!grant || grant.challenge !== challenge || grant.redirectUri !== p.redirect_uri) {
        return json(res, 400, { error: 'invalid_grant' });
      }
      const refreshToken = `1//e2e-refresh-${crypto.randomBytes(6).toString('hex')}`;
      refresh.add(refreshToken);
      return issue(res, refreshToken, grant.scope);
    }
    if (p.grant_type === 'refresh_token') {
      if (!refresh.has(p.refresh_token)) return json(res, 400, { error: 'invalid_grant' });
      return issue(res, p.refresh_token, '');
    }
    return json(res, 400, { error: 'unsupported_grant_type' });
  }

  function authed(req) {
    const t = (req.headers.authorization ?? '').replace(/^Bearer /, '');
    return access.has(t) && refresh.has(access.get(t));
  }

  const server = http.createServer((req, res) => {
    let raw = '';
    req.on('data', (c) => (raw += c));
    req.on('end', () => {
      const url = new URL(req.url, 'http://localhost');
      requests.push({ method: req.method, path: url.pathname });
      if (req.method === 'POST' && url.pathname === '/token') return token(res, form(raw));
      if (req.method === 'POST' && url.pathname === '/revoke') {
        const t = form(raw).token;
        revoked.push(t);
        refresh.delete(t);
        return json(res, 200, {});
      }
      if (url.pathname.startsWith('/calendar/v3/')) {
        if (!authed(req)) return json(res, 401, { error: { code: 401, message: 'Invalid Credentials' } });
        if (url.pathname === '/calendar/v3/users/me/calendarList') {
          return json(res, 200, {
            items: [
              { id: account, primary: true, selected: true },
              { id: 'team@group.calendar.google.com', selected: true },
            ],
          });
        }
        const m = /^\/calendar\/v3\/calendars\/([^/]+)\/events$/.exec(url.pathname);
        if (m) {
          const calendar = decodeURIComponent(m[1]);
          return json(res, 200, { summary: calendar, items: calendar === account ? events() : [] });
        }
      }
      res.writeHead(404).end();
    });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const base = `http://127.0.0.1:${server.address().port}`;

  return {
    env: {
      YAP_GOOGLE_CALENDAR_CLIENT_ID: clientId,
      ...(clientSecret ? { YAP_GOOGLE_CALENDAR_CLIENT_SECRET: clientSecret } : {}),
      YAP_GOOGLE_AUTH_URL: `${base}/o/oauth2/v2/auth`,
      YAP_GOOGLE_TOKEN_URL: `${base}/token`,
      YAP_GOOGLE_REVOKE_URL: `${base}/revoke`,
      YAP_GOOGLE_API_URL: `${base}/calendar/v3`,
    },
    requests,
    revoked,
    /** The consent page's parameters, as Yap sent them. */
    get lastAuth() {
      return lastAuth;
    },
    /** "Allow" on the consent page: where Google sends the browser next. */
    approve(authUrl) {
      const u = new URL(authUrl);
      const p = Object.fromEntries(u.searchParams);
      lastAuth = p;
      const code = `4/e2e-${crypto.randomBytes(6).toString('hex')}`;
      codes.set(code, { challenge: p.code_challenge, redirectUri: p.redirect_uri, scope: p.scope });
      const back = new URL(p.redirect_uri);
      back.searchParams.set('state', p.state);
      back.searchParams.set('code', code);
      back.searchParams.set('scope', p.scope);
      return back.toString();
    },
    /** "Cancel" on the consent page. */
    deny(authUrl) {
      const p = Object.fromEntries(new URL(authUrl).searchParams);
      const back = new URL(p.redirect_uri);
      back.searchParams.set('state', p.state);
      back.searchParams.set('error', 'access_denied');
      return back.toString();
    },
    close: () => new Promise((resolve) => server.close(resolve)),
  };
}
