// Stand-in for Microsoft's sign-in page, so the whole browser → Yap handoff
// can be tested locally without real OAuth credentials. Run it with
// `npm run mock:microsoft`, and in .dev.vars set
//   MICROSOFT_CLIENT_ID=mock
//   MICROSOFT_CLIENT_SECRET=mock
//   DEV_MICROSOFT_AUTHORITY=http://127.0.0.1:8790
// (src/auth.ts ignores the authority unless the Worker runs on localhost).
//
// Implements the two endpoints Better Auth's Microsoft provider calls, with
// PKCE and redirect_uri checked like the real thing. The ID token is unsigned:
// the provider only decodes it in the authorization-code flow.
import { createHash, randomBytes } from "node:crypto";
import { createServer } from "node:http";

const PORT = Number(process.env.PORT || 8790);
const ORIGIN = `http://127.0.0.1:${PORT}`;
const PERSONAL_TENANT = "9188040d-6c67-4c5b-b112-36a304b66dad";
const CODE_TTL_MS = 60_000;

/** code → what /token needs to check it and mint the ID token. */
const codes = new Map();

const b64url = (data) => Buffer.from(data).toString("base64url");
const sha256 = (data) => createHash("sha256").update(data).digest();
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

/** Stable GUID-shaped object id per email, so repeat sign-ins hit one account. */
function oidFor(email) {
  const h = sha256(`mock-oid:${email}`).toString("hex");
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20, 32)}`;
}

function send(res, status, body, headers = {}) {
  res.writeHead(status, headers);
  res.end(body);
}

const json = (res, status, obj) =>
  send(res, status, JSON.stringify(obj), { "content-type": "application/json", "cache-control": "no-store" });

function redirect(res, target, params) {
  const url = new URL(target);
  for (const [k, v] of Object.entries(params)) url.searchParams.set(k, v);
  send(res, 302, "", { location: url.toString() });
}

function authorizePage(q) {
  const hidden = ["client_id", "redirect_uri", "state", "code_challenge", "code_challenge_method"]
    .map((k) => `<input type="hidden" name="${k}" value="${esc(q.get(k) ?? "")}">`)
    .join("");
  return `<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1"><title>Mock Microsoft sign-in</title></head>
<body style="font-family:system-ui,sans-serif;max-width:420px;margin:48px auto;padding:0 16px;color:#1b1b1b">
<p style="font-size:13px;color:#a4262c;font-weight:600">LOCAL TEST ONLY: not Microsoft</p>
<h1 style="font-size:24px;font-weight:600">Sign in</h1>
<form method="get" action="/approve">${hidden}
<label>Email<br><input name="email" value="mock.user@example.com" style="width:100%;padding:8px;margin:4px 0 12px"></label>
<label>Name<br><input name="name" value="Mock User" style="width:100%;padding:8px;margin:4px 0 12px"></label>
<label style="display:block;margin-bottom:16px"><input type="checkbox" name="verified" value="1" checked> Verified email (xms_edov)</label>
<button name="action" value="approve" style="padding:8px 20px">Continue</button>
<button name="action" value="cancel" style="padding:8px 20px">Cancel</button>
</form></body></html>`;
}

async function readForm(req) {
  let body = "";
  for await (const chunk of req) body += chunk;
  return new URLSearchParams(body);
}

createServer(async (req, res) => {
  const url = new URL(req.url, ORIGIN);
  const q = url.searchParams;
  try {
    // 1. Authorize: Better Auth sends the browser here. MOCK_AUTO_APPROVE=1
    //    skips the form (hands-free tests) as MOCK_EMAIL, verified.
    if (req.method === "GET" && url.pathname.endsWith("/oauth2/v2.0/authorize")) {
      if (!q.get("redirect_uri") || !q.get("state")) return send(res, 400, "missing redirect_uri/state");
      if (process.env.MOCK_AUTO_APPROVE === "1") {
        q.set("email", process.env.MOCK_EMAIL || "mock.user@example.com");
        q.set("name", "Mock User");
        q.set("verified", "1");
        q.set("action", "approve");
        url.pathname = "/approve";
      } else {
        return send(res, 200, authorizePage(q), { "content-type": "text/html; charset=utf-8" });
      }
    }

    // 2. The form: issue a code (or an error, like a user hitting Cancel).
    if (req.method === "GET" && url.pathname === "/approve") {
      const back = q.get("redirect_uri");
      if (q.get("action") === "cancel") {
        return redirect(res, back, {
          error: "access_denied",
          error_description: "The user cancelled the sign-in.",
          state: q.get("state"),
        });
      }
      const code = b64url(randomBytes(24));
      codes.set(code, {
        clientId: q.get("client_id"),
        redirectUri: back,
        challenge: q.get("code_challenge"),
        email: q.get("email").trim().toLowerCase(),
        name: q.get("name").trim(),
        verified: q.get("verified") === "1",
        expires: Date.now() + CODE_TTL_MS,
      });
      return redirect(res, back, { code, state: q.get("state") });
    }

    // 3. Token: Better Auth's server swaps the code (with the PKCE verifier).
    if (req.method === "POST" && url.pathname.endsWith("/oauth2/v2.0/token")) {
      const form = await readForm(req);
      const code = form.get("code");
      const entry = codes.get(code);
      codes.delete(code); // single use
      if (!entry || entry.expires < Date.now()) {
        return json(res, 400, { error: "invalid_grant", error_description: "unknown or expired code" });
      }
      if (form.get("redirect_uri") !== entry.redirectUri) {
        return json(res, 400, { error: "invalid_grant", error_description: "redirect_uri mismatch" });
      }
      if (entry.challenge && b64url(sha256(form.get("code_verifier") ?? "")) !== entry.challenge) {
        return json(res, 400, { error: "invalid_grant", error_description: "PKCE verification failed" });
      }
      const now = Math.floor(Date.now() / 1000);
      const oid = oidFor(entry.email);
      const claims = {
        iss: `${ORIGIN}/${PERSONAL_TENANT}/v2.0`,
        aud: entry.clientId,
        sub: oid,
        oid,
        tid: PERSONAL_TENANT,
        email: entry.email,
        name: entry.name,
        preferred_username: entry.email,
        iat: now,
        nbf: now,
        exp: now + 3600,
        ...(entry.verified && { xms_edov: true }),
      };
      const idToken = [
        b64url(JSON.stringify({ alg: "RS256", typ: "JWT", kid: "mock" })),
        b64url(JSON.stringify(claims)),
        b64url("mock-signature"),
      ].join(".");
      return json(res, 200, {
        token_type: "Bearer",
        scope: "openid profile email",
        expires_in: 3600,
        access_token: b64url(randomBytes(24)),
        id_token: idToken,
      });
    }

    send(res, 404, "not found");
  } catch (err) {
    console.error(err);
    send(res, 500, "mock error");
  }
}).listen(PORT, "127.0.0.1", () => console.log(`Mock Microsoft sign-in on ${ORIGIN} (local testing only)`));
