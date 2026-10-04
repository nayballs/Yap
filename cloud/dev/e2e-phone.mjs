// Local end-to-end checks for the phone sign-in hardening (cloud/).
// wrangler dev on :8851 (BETTER_AUTH_URL http://localhost:8851), mock provider
// on :8852 with MOCK_AUTO_APPROVE=1. Never production.
// Usage: npm run test:e2e [-- section...]   (= node dev/e2e-phone.mjs .dev.vars …)
// Start the servers first (see cloud/README.md "Local development"):
//   PORT=8852 MOCK_AUTO_APPROVE=1 npm run mock:provider
//   npx wrangler dev --port 8851 --local-upstream localhost:8851 --test-scheduled \n//     --var BETTER_AUTH_URL:http://localhost:8851 --var DEV_MOCK_PROVIDER:http://127.0.0.1:8852
import http from "node:http";
import crypto from "node:crypto";
import fs from "node:fs";

const BASE = "http://localhost:8851";
const ORIGIN = BASE;
const YAP_UA = "Yap/0.1.1 (Windows)";
const PHONE_UA =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1";
const PC_BROWSER_UA =
  "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Safari/537.36";
const DB_ID = "fb03bf43-1271-43c9-981b-41be5dedf590";
const SECRET = /^BETTER_AUTH_SECRET=(.*)$/m.exec(fs.readFileSync(process.argv[2], "utf8"))[1].trim();
const sections = new Set(process.argv.slice(3));
const want = (s) => sections.size === 0 || sections.has(s);

let failures = 0;
function check(name, ok, detail = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  [${detail}]` : ""}`);
  if (!ok) failures++;
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const q = (v) => (v === null ? "NULL" : typeof v === "number" ? String(v) : `'${String(v).replace(/'/g, "''")}'`);

class Jar {
  constructor() {
    this.c = new Map();
  }
  take(headers) {
    for (const sc of headers.getSetCookie()) {
      const [pair, ...attrs] = sc.split(";");
      const i = pair.indexOf("=");
      const name = pair.slice(0, i).trim();
      const value = pair.slice(i + 1).trim();
      const gone =
        value === "" ||
        attrs.some((a) => /^\s*max-age=0\s*$/i.test(a)) ||
        attrs.some((a) => {
          const m = /^\s*expires=(.*)$/i.exec(a);
          return m && new Date(m[1]) < new Date();
        });
      if (gone) this.c.delete(name);
      else this.c.set(name, value);
    }
  }
  header() {
    return [...this.c].map(([k, v]) => `${k}=${v}`).join("; ");
  }
}

/** Plain node:http, like Yap's reqwest: no Sec-Fetch-* headers (Node's fetch
 *  adds them, and Better Auth then insists on an Origin). */
function rawRequest(method, url, headers, body) {
  return new Promise((resolve, reject) => {
    const u = new URL(url);
    const h = { host: u.host, ...headers };
    if (body !== undefined) h["content-length"] = Buffer.byteLength(body);
    const r = http.request(
      { host: u.hostname === "localhost" ? "127.0.0.1" : u.hostname, port: u.port, path: u.pathname + u.search, method, headers: h },
      (res) => {
        let s = "";
        res.setEncoding("utf8");
        res.on("data", (d) => (s += d));
        res.on("end", () => {
          const out = new Headers();
          for (const [k, v] of Object.entries(res.headers)) for (const one of [].concat(v)) out.append(k, one);
          resolve({ status: res.statusCode, headers: out, text: s });
        });
      },
    );
    r.on("error", reject);
    if (body !== undefined) r.write(body);
    r.end();
  });
}

async function call(method, path, { body, headers = {}, jar, ua = YAP_UA, origin, rawBody } = {}) {
  const h = { "user-agent": ua, ...headers };
  if (jar && jar.c.size) h.cookie = jar.header();
  if (origin) h.origin = origin;
  let payload;
  if (rawBody !== undefined) payload = rawBody;
  else if (body !== undefined) {
    payload = JSON.stringify(body);
    h["content-type"] ??= "application/json";
  }
  const url = path.startsWith("http") ? path : BASE + path;
  const res = await rawRequest(method, url, h, payload);
  if (jar && url.startsWith(BASE)) jar.take(res.headers);
  let json = null;
  try {
    json = JSON.parse(res.text);
  } catch {}
  return { ...res, json };
}

/** A POST with no Content-Length (chunked). */
function chunkedPost(path, body) {
  return new Promise((resolve, reject) => {
    const r = http.request(
      {
        host: "127.0.0.1",
        port: 8851,
        path,
        method: "POST",
        headers: { "content-type": "application/json", "transfer-encoding": "chunked", "user-agent": YAP_UA },
      },
      (res) => {
        let s = "";
        res.on("data", (d) => (s += d));
        res.on("end", () => resolve({ status: res.statusCode, text: s }));
      },
    );
    r.on("error", reject);
    r.write(body);
    r.end();
  });
}

async function sql(query) {
  const r = await fetch(`${BASE}/cdn-cgi/local/explorer/api/d1/database/${DB_ID}/raw`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ sql: query }),
  });
  const j = await r.json();
  if (!j.success) throw new Error(`${query}: ${JSON.stringify(j.errors)}`);
  const res = j.result[0].results;
  return res.rows.map((row) => Object.fromEntries(res.columns.map((c, i) => [c, row[i]])));
}

async function logLike(...likes) {
  const where = likes.map((l) => `message LIKE ${q(`%${l}%`)}`).join(" AND ");
  const r = await fetch(`${BASE}/cdn-cgi/local/explorer/api/local/observability/query`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ sql: `SELECT message FROM logs WHERE ${where} ORDER BY ts_ms DESC LIMIT 1` }),
  });
  const raw = (await r.json()).result?.rows?.[0]?.[0];
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw); // console.log's arguments
    return Array.isArray(parsed) ? parsed.join(" ") : String(parsed);
  } catch {
    return raw;
  }
}

async function mailTo(to, subject, tries = 40) {
  for (let i = 0; i < tries; i++) {
    const m = await logLike(`to=${to}`, subject);
    if (m) return m;
    await sleep(250);
  }
  return null;
}

async function emailSignIn(email, { ua = YAP_UA, jar, origin } = {}) {
  const send = await call("POST", "/api/auth/email-otp/send-verification-otp", { body: { email, type: "sign-in" }, ua, jar, origin });
  if (send.status !== 200) throw new Error(`send-verification-otp ${send.status} ${send.text}`);
  const mail = await mailTo(email, "Your Yap code");
  const otp = /code is (\d{6})/.exec(mail ?? "")?.[1];
  if (!otp) throw new Error(`no code mailed to ${email}`);
  const r = await call("POST", "/api/auth/sign-in/email-otp", { body: { email, otp }, ua, jar, origin });
  if (r.status !== 200) throw new Error(`sign-in/email-otp ${r.status} ${r.text}`);
  return r.json;
}

/** Follows redirects like a browser (cookies for our origin only). */
async function follow(res, jar, ua) {
  for (let i = 0; i < 10 && res.status >= 300 && res.status < 400; i++) {
    const url = new URL(res.headers.get("location"), BASE);
    res = await call("GET", url.toString(), { jar, ua });
    res.url = url.toString();
  }
  return res;
}

const b64url = (buf) => Buffer.from(buf).toString("base64url");
const pkce = () => {
  const verifier = b64url(crypto.randomBytes(32));
  return { verifier, challenge: b64url(crypto.createHash("sha256").update(verifier).digest()) };
};

/** Better Auth allows 3 /sign-in/* calls per IP per 10 s (sliding from the
 *  last one); this suite chains more, so it resets that one counter (local
 *  database only). */
const resetSignInLimit = () => sql(`DELETE FROM rateLimit WHERE key LIKE '%/sign-in/social%'`);

/** Yap's browser sign-in through the mock provider: returns Yap's session token. */
async function desktopSocialSignIn(browserJar) {
  await resetSignInLimit();
  const { verifier, challenge } = pkce();
  const state = `Nonce${crypto.randomBytes(6).toString("hex")}`;
  const start = await call(
    "GET",
    `/api/auth/electron/init-oauth-proxy?provider=microsoft&client_id=yap-desktop&code_challenge=${challenge}&state=${state}`,
    { jar: browserJar, ua: PC_BROWSER_UA },
  );
  if (start.status !== 302) throw new Error(`init-oauth-proxy ${start.status} ${start.text}`);
  const end = await follow(start, browserJar, PC_BROWSER_UA);
  const handoff = browserJar.c.get("better-auth.yap-desktop");
  if (!handoff) throw new Error(`no hand-back cookie (ended ${end.status} ${end.url})`);
  const { identifier, state: back } = JSON.parse(Buffer.from(decodeURIComponent(handoff), "base64url").toString());
  const r = await call("POST", "/api/auth/electron/token", { body: { token: identifier, state: back, code_verifier: verifier } });
  if (r.status !== 200) throw new Error(`electron/token ${r.status} ${r.text}`);
  return { token: r.json.token, user: r.json.user, landed: end.url };
}

/** The phone page's provider sign-in (mock), back to /device?user_code=…. */
async function phoneSocialSignIn(jar, userCode) {
  await resetSignInLimit();
  const back = `/device?user_code=${userCode}`;
  const r = await call("POST", "/api/auth/sign-in/social", {
    body: { provider: "microsoft", callbackURL: back, errorCallbackURL: back },
    jar,
    ua: PHONE_UA,
    origin: ORIGIN,
  });
  if (r.status !== 200 || !r.json?.url) throw new Error(`sign-in/social ${r.status} ${r.text}`);
  const end = await follow({ status: 302, headers: new Headers({ location: r.json.url }) }, jar, PHONE_UA);
  return end.url;
}

const newCode = (ua = YAP_UA, headers = {}) =>
  call("POST", "/api/auth/device/code", { body: { client_id: "yap-desktop" }, ua, headers });
const poll = (deviceCode) =>
  call("POST", "/api/auth/device/token", {
    body: { grant_type: "urn:ietf:params:oauth:grant-type:device_code", device_code: deviceCode, client_id: "yap-desktop" },
  });
const bearer = (token) => ({ authorization: `Bearer ${token}` });
const signedForm = (token) =>
  encodeURIComponent(`${token}.${crypto.createHmac("sha256", SECRET).update(token).digest("base64")}`);
const origin = (jar, userCode) => call("GET", `/api/account/device-origin?user_code=${userCode}`, { jar, ua: PHONE_UA });

async function originRow(userCode) {
  for (let i = 0; i < 20; i++) {
    const rows = await sql(`SELECT * FROM deviceOrigin WHERE userCode = ${q(userCode)}`);
    if (rows.length) return rows[0];
    await sleep(150);
  }
  return null;
}

// ---------------------------------------------------------------------------

// Re-runs start clean (local database only): send limits and rate limits.
await sql(`DELETE FROM emailCodeSend`);
await sql(`DELETE FROM rateLimit`);

if (want("limits")) {
  console.log("\n== A. body limits ==");
  let r = await chunkedPost("/api/auth/sign-out", "{}");
  check("chunked POST (no content-length) to /api/auth/sign-out → 413", r.status === 413, `${r.status} ${r.text}`);
  r = await chunkedPost("/api/account/sessions/revoke", '{"id":"x"}');
  check("chunked POST to /api/account/sessions/revoke → 413", r.status === 413, `${r.status}`);
  r = await call("POST", "/api/auth/email-otp/send-verification-otp", { rawBody: JSON.stringify({ email: "a@example.com", type: "sign-in", pad: "x".repeat(5000) }), headers: { "content-type": "application/json" } });
  check("5 kB body → 413", r.status === 413, `${r.status} ${r.text}`);
  const at = (n) => {
    const base = JSON.stringify({ client_id: "yap-desktop", pad: "" });
    return JSON.stringify({ client_id: "yap-desktop", pad: "x".repeat(n - base.length) });
  };
  r = await call("POST", "/api/auth/device/code", { rawBody: at(4097), headers: { "content-type": "application/json" } });
  check("4097-byte body → 413", r.status === 413, `${r.status}`);
  r = await call("POST", "/api/auth/device/code", { rawBody: at(4096), headers: { "content-type": "application/json" } });
  check("4096-byte body passes the size check (then 400 invalid_request for the extra field)", r.status === 400 && r.json?.error === "invalid_request", `${r.status} ${r.text}`);
  r = await call("GET", "/api/auth/get-session");
  check("GETs unaffected (get-session without a session → 200 null)", r.status === 200, `${r.status} ${r.text}`);
}

if (want("devicecode")) {
  console.log("\n== A2. /device/code body ==");
  const cases = [
    ["scope", JSON.stringify({ client_id: "yap-desktop", scope: "openid" }), "application/json"],
    ["user_id", JSON.stringify({ client_id: "yap-desktop", user_id: "someone" }), "application/json"],
    ["array", "[]", "application/json"],
    ["null", "null", "application/json"],
    ["not JSON", "{", "application/json"],
    ["form-encoded", "client_id=yap-desktop", "application/x-www-form-urlencoded"],
    ["no client_id", "{}", "application/json"],
  ];
  for (const [name, rawBody, type] of cases) {
    const r = await call("POST", "/api/auth/device/code", { rawBody, headers: { "content-type": type } });
    check(`/device/code ${name} → 400 invalid_request`, r.status === 400 && r.json?.error === "invalid_request", `${r.status} ${r.text}`);
  }
  const r = await call("POST", "/api/auth/device/code", { body: { client_id: "someone-else" } });
  check("/device/code other client id → 400 invalid_client (plugin)", r.status === 400 && r.json?.error === "invalid_client", `${r.status} ${r.text}`);
}

if (want("oauth")) {
  console.log("\n== A3. init-oauth-proxy ==");
  const { challenge } = pkce();
  const bad = [
    ["state with a space", `state=a%20b&code_challenge=${challenge}`],
    ["state with <>", `state=%3Cx%3E&code_challenge=${challenge}`],
    ["state 129 chars", `state=${"a".repeat(129)}&code_challenge=${challenge}`],
    ["challenge 42 chars", `state=abc&code_challenge=${challenge.slice(0, 42)}`],
    ["challenge 44 chars", `state=abc&code_challenge=${challenge}A`],
    ["challenge with +", `state=abc&code_challenge=${"+".repeat(43)}`],
    ["challenge padded", `state=abc&code_challenge=${challenge.slice(0, 42)}%3D`],
  ];
  for (const [name, qs] of bad) {
    const r = await call("GET", `/api/auth/electron/init-oauth-proxy?provider=microsoft&client_id=yap-desktop&${qs}`, { ua: PC_BROWSER_UA });
    check(`init-oauth-proxy ${name} → /error?error=invalid_request`, r.status === 302 && r.headers.get("location") === "/error?error=invalid_request", `${r.status} ${r.headers.get("location")}`);
  }
  const ok = await call("GET", `/api/auth/electron/init-oauth-proxy?provider=microsoft&client_id=yap-desktop&code_challenge=${challenge}&state=Nonce16chars1234-51234`, { ua: PC_BROWSER_UA });
  check("init-oauth-proxy valid (loopback-style state) → provider", ok.status === 302 && ok.headers.get("location")?.startsWith("http://127.0.0.1:8852/"), `${ok.status} ${ok.headers.get("location")?.slice(0, 60)}`);
}

let ctx = {};
if (want("flows")) {
  console.log("\n== B/C. normal flows + phone sign-in ==");
  // Email-code sign-in from Yap.
  const u1 = await emailSignIn("yapuser1@example.com");
  check("email-code sign-in from Yap still works", typeof u1?.token === "string" && u1.user?.email === "yapuser1@example.com");
  ctx.u1 = u1.token;

  await sleep(4000); // Better Auth: 3 /sign-in calls per 10 s per IP
  // Social sign-in through the mock provider, handed back to Yap.
  const pcBrowser = new Jar();
  const social = await desktopSocialSignIn(pcBrowser);
  check("social sign-in (mock) + hand-back still works", typeof social.token === "string" && social.user?.email === "mock.user@example.com", `landed ${social.landed}`);
  ctx.social = social.token;
  const browserSessions = await sql(`SELECT id, userAgent, createdAt FROM session WHERE userAgent LIKE 'Mozilla/%'`);
  check("the hand-back left a browser session behind (until handoff.js signs it out)", browserSessions.length >= 1, JSON.stringify(browserSessions.map((s) => s.createdAt)));
  ctx.pcBrowser = pcBrowser;

  // Phone sign-in: Yap asks for a code.
  const code = await newCode();
  check("/device/code from Yap → 200", code.status === 200, `${code.status} ${code.text}`);
  check("interval is 3", code.json?.interval === 3, `${code.json?.interval}`);
  const userCode = code.json.user_code;
  const row = await originRow(userCode);
  check("deviceOrigin row recorded", !!row, JSON.stringify(row && { ...row, network: row.network ? `${row.network.slice(0, 6)}…` : null }));
  check("deviceOrigin keeps no IP", row && !/\d+\.\d+\.\d+\.\d+|::1|127\.0\.0/.test(JSON.stringify(row)), "");
  check("deviceOrigin user agent is Yap's", row?.userAgent === YAP_UA, row?.userAgent);

  // The phone opens the link: no session yet.
  const phone = new Jar();
  let o = await origin(phone, userCode);
  check("device-origin without a session → 401", o.status === 401, `${o.status} ${o.text}`);
  check("device-origin answers Cache-Control: no-store", o.headers.get("cache-control") === "no-store", o.headers.get("cache-control"));

  await sleep(4000);
  const landed = await phoneSocialSignIn(phone, userCode);
  check("phone page provider sign-in lands back on /device?user_code=…", landed === `${BASE}/device?user_code=${userCode}`, landed);
  o = await origin(phone, userCode);
  check("device-origin signed in → 200", o.status === 200, `${o.status} ${o.text}`);
  check("device-origin label", o.json?.label === "Yap 0.1.1 on Windows", o.json?.label);
  check("device-origin minutesAgo 0", o.json?.minutesAgo === 0, `${o.json?.minutesAgo}`);
  check("device-origin sameNetwork true (both from this PC)", o.json?.sameNetwork === true, `${o.json?.sameNetwork}`);
  check("device-origin has place + sameCountry", "place" in (o.json ?? {}) && "sameCountry" in (o.json ?? {}), JSON.stringify(o.json));
  check("device-origin answer carries no IP", !/\d+\.\d+\.\d+\.\d+/.test(o.text), o.text);
  check("device-origin no-store", o.headers.get("cache-control") === "no-store", o.headers.get("cache-control"));
  ctx.originSample = o.json;

  // Approve (claim first, like device.js).
  const claim = await call("GET", `/api/auth/device?user_code=${userCode}`, { jar: phone, ua: PHONE_UA });
  check("claim → pending", claim.status === 200 && claim.json?.status === "pending", claim.text);
  const t0 = Date.now();
  const ap = await call("POST", "/api/auth/device/approve", { body: { userCode }, jar: phone, ua: PHONE_UA, origin: ORIGIN });
  check("approve → 200", ap.status === 200, `${ap.status} ${ap.text} in ${Date.now() - t0} ms`);
  const mail = await mailTo("mock.user@example.com", "New sign-in to your Yap account");
  check("approval email printed (mail:dev)", !!mail, mail ? mail.split("\n").slice(0, 4).join(" | ") : "none");
  o = await origin(phone, userCode);
  check("device-origin after approve → 404 (no longer pending)", o.status === 404, `${o.status} ${o.text}`);
  const out = await call("POST", "/api/auth/sign-out", { body: {}, jar: phone, ua: PHONE_UA, origin: ORIGIN });
  check("phone page sign-out → 200", out.status === 200, `${out.status}`);

  // Yap polls.
  const tok = await poll(code.json.device_code);
  check("/device/token → access_token", tok.status === 200 && typeof tok.json?.access_token === "string", `${tok.status} ${tok.text.slice(0, 80)}`);
  const phoneToken = tok.json.access_token;
  ctx.phoneToken = phoneToken;
  const ps = await sql(`SELECT tokenHash, createdAt FROM phoneSession`);
  const expected = crypto.createHmac("sha256", SECRET).update(`session-token:${phoneToken}`).digest().subarray(0, 16).toString("hex");
  check(
    "phoneSession row recorded: the keyed hash, never the token",
    ps.some((r) => r.tokenHash === expected) && !ps.some((r) => r.tokenHash.includes(phoneToken)),
    `${ps.length} rows`,
  );
  const me = await call("GET", "/api/auth/get-session", { headers: bearer(phoneToken) });
  check("phone session works for get-session", me.json?.user?.email === "mock.user@example.com", me.text.slice(0, 80));

  // 403 NEW_PHONE_SESSION on the three routes, in every token form.
  const forms = [
    ["raw bearer", { headers: bearer(phoneToken) }],
    ["signed bearer", { headers: bearer(signedForm(phoneToken)) }],
    ["session cookie", { headers: { cookie: `better-auth.session_token=${signedForm(phoneToken)}` }, origin: ORIGIN }],
  ];
  for (const [form, opts] of forms) {
    for (const path of ["/api/auth/delete-user", "/api/auth/revoke-other-sessions"]) {
      const r = await call("POST", path, { body: {}, ...opts });
      check(`${path} with a phone session (${form}) → 403 NEW_PHONE_SESSION`, r.status === 403 && r.json?.code === "NEW_PHONE_SESSION", `${r.status} ${r.text}`);
    }
  }
  let r = await call("POST", "/api/account/sessions/revoke", { body: { id: "whatever" }, headers: bearer(phoneToken) });
  check("/api/account/sessions/revoke with a phone session → 403 NEW_PHONE_SESSION", r.status === 403 && r.json?.code === "NEW_PHONE_SESSION", `${r.status} ${r.text}`);
  check("403 message", r.json?.message === "For your security, a sign-in made with your phone can't do this during its first day. Sign in another way to carry on.", r.json?.message);
  const still = await call("GET", "/api/auth/get-session", { headers: bearer(phoneToken) });
  check("account and phone session untouched by the refused calls", still.json?.user?.email === "mock.user@example.com");

  // A non-phone session of the same account can sign the phone session out.
  const list = await call("GET", "/api/account/sessions", { headers: bearer(social.token) });
  const phoneSessionId = list.json?.sessions?.find((s) => !s.current && s.userAgent === YAP_UA)?.id;
  check("the hand-back session lists the phone session", !!phoneSessionId, `${list.status}`);
  r = await call("POST", "/api/account/sessions/revoke", { body: { id: phoneSessionId }, headers: bearer(social.token) });
  check("a non-phone session may revoke it → removed", r.status === 200 && r.json?.removed === true, `${r.status} ${r.text}`);
  const gone = await call("GET", "/api/auth/get-session", { headers: bearer(phoneToken) });
  check("the phone session is gone", gone.status === 200 && gone.json === null, gone.text.slice(0, 60));
  r = await call("POST", "/api/auth/revoke-other-sessions", { body: {}, headers: bearer(social.token) });
  check("revoke-other-sessions with a non-phone session → 200", r.status === 200, `${r.status} ${r.text}`);
}

if (want("origin")) {
  console.log("\n== B2. device-origin variants ==");
  await sql(`DELETE FROM emailCodeSend WHERE key LIKE 'o:%'`);
  const code = await newCode();
  const userCode = code.json.user_code;
  const row = await originRow(userCode);
  const phone = new Jar();
  await emailSignIn("phone2@example.com", { ua: PHONE_UA, jar: phone, origin: ORIGIN });
  let o = await origin(phone, userCode);
  check("phone page email sign-in → device-origin 200", o.status === 200, o.text);
  const lower = await origin(phone, `${userCode.slice(0, 4).toLowerCase()}-${userCode.slice(4).toLowerCase()}`);
  check("device-origin accepts wdjb-mjht form", lower.status === 200, `${lower.status}`);
  // Once one account has claimed the code, another sees nothing.
  await sleep(4000);
  const other = new Jar();
  await phoneSocialSignIn(other, userCode);
  check("a second account sees it before anyone claims it", (await origin(other, userCode)).status === 200);
  const claimed = await call("GET", `/api/auth/device?user_code=${userCode}`, { jar: phone, ua: PHONE_UA });
  check("phone2 claims the code", claimed.json?.status === "pending" && claimed.json?.client_id === "yap-desktop", claimed.text);
  o = await origin(other, userCode);
  check("device-origin for a code another account claimed → 404", o.status === 404, `${o.status} ${o.text}`);
  o = await origin(phone, userCode);
  check("the claimer still sees it", o.status === 200, `${o.status}`);
  await call("POST", "/api/auth/sign-out", { body: {}, jar: other, ua: PHONE_UA, origin: ORIGIN });
  const phoneCountry = row?.country; // same machine, same cf
  // Another country, another network.
  await sql(`UPDATE deviceOrigin SET country = 'FR', city = 'Paris', network = 'deadbeef' WHERE userCode = ${q(userCode)}`);
  o = await origin(phone, userCode);
  check("other network → sameNetwork false", o.json?.sameNetwork === false, JSON.stringify(o.json));
  check("other country → sameCountry false (or null if this machine's country is unknown)", phoneCountry ? o.json?.sameCountry === false : o.json?.sameCountry === null, `${phoneCountry} ${o.json?.sameCountry}`);
  check("place near Paris, France", o.json?.place === "near Paris, France", o.json?.place);
  // Same country, other network.
  if (phoneCountry) {
    await sql(`UPDATE deviceOrigin SET country = ${q(phoneCountry)}, city = NULL WHERE userCode = ${q(userCode)}`);
    o = await origin(phone, userCode);
    check("same country, other network → sameCountry true, sameNetwork false", o.json?.sameCountry === true && o.json?.sameNetwork === false, JSON.stringify(o.json));
    check("country-only place", /^in /.test(o.json?.place ?? ""), o.json?.place);
  }
  // Unknown place, odd user agent, older.
  await sql(`UPDATE deviceOrigin SET country = NULL, city = NULL, network = NULL, userAgent = 'Yap/1.0 (Windows <b>)', createdAt = ${Date.now() - 3 * 60_000 - 5000} WHERE userCode = ${q(userCode)}`);
  o = await origin(phone, userCode);
  check("unknowns → place '' and nulls", o.json?.place === "" && o.json?.sameNetwork === null && o.json?.sameCountry === null, JSON.stringify(o.json));
  check("odd user agent → plain label", o.json?.label === "Yap 1.0", o.json?.label);
  check("minutesAgo 3", o.json?.minutesAgo === 3, `${o.json?.minutesAgo}`);
  await sql(`UPDATE deviceOrigin SET userAgent = 'curl/8.0' WHERE userCode = ${q(userCode)}`);
  o = await origin(phone, userCode);
  check("non-Yap user agent → 'Yap'", o.json?.label === "Yap", o.json?.label);
  // Bad and unknown codes.
  o = await origin(phone, "ABC");
  check("short code → 400", o.status === 400, `${o.status}`);
  o = await origin(phone, "BCDFGHJK");
  check("unknown code → 404", o.status === 404, `${o.status}`);
  // Deny → no longer pending.
  await call("GET", `/api/auth/device?user_code=${userCode}`, { jar: phone, ua: PHONE_UA });
  const deny = await call("POST", "/api/auth/device/deny", { body: { userCode }, jar: phone, ua: PHONE_UA, origin: ORIGIN });
  check("deny → 200", deny.status === 200, deny.text);
  o = await origin(phone, userCode);
  check("device-origin after deny → 404", o.status === 404, `${o.status}`);
  const noMail = await logLike("to=phone2@example.com", "New sign-in");
  check("no new-sign-in email for a deny", !noMail);
  // Per-IP limit: 20 per 10 minutes.
  await sql(`DELETE FROM emailCodeSend WHERE key LIKE 'o:%'`);
  const statuses = [];
  for (let i = 0; i < 21; i++) statuses.push((await origin(phone, "BCDFGHJK")).status);
  const last = await origin(phone, "BCDFGHJK");
  check("20 look-ups allowed, then 429", statuses.slice(0, 20).every((s) => s === 404) && statuses[20] === 429 && last.status === 429, statuses.join(","));
  check("429 carries Retry-After ≈ 600", Number(last.headers.get("retry-after")) > 500, last.headers.get("retry-after"));
  await sql(`DELETE FROM emailCodeSend WHERE key LIKE 'o:%'`);
  await call("POST", "/api/auth/sign-out", { body: {}, jar: phone, ua: PHONE_UA, origin: ORIGIN });
}

if (want("spoof")) {
  console.log("\n== B3. client-sent cf-connecting-ip (local only) ==");
  const a = await newCode(YAP_UA, { "cf-connecting-ip": "203.0.113.9" });
  const b = await newCode(YAP_UA, { "cf-connecting-ip": "203.0.113.200" });
  const c = await newCode(YAP_UA, { "cf-connecting-ip": "198.51.100.7" });
  const [ra, rb, rc] = [await originRow(a.json.user_code), await originRow(b.json.user_code), await originRow(c.json.user_code)];
  console.log(`      networks: ${[ra, rb, rc].map((r) => r?.network?.slice(0, 8)).join(" ")} (same /24 for the first two if the header is honoured)`);
  ctx.spoof = { same: ra?.network === rb?.network, differ: ra?.network !== rc?.network };
  check("IPv4 /24: .9 and .200 share a network, 198.51.100.7 doesn't", ctx.spoof.same && ctx.spoof.differ);
}

if (want("cron")) {
  console.log("\n== C2. sweep (cron) ==");
  const hourAgo = new Date(Date.now() - 60 * 60_000).toISOString();
  const sample = await sql(`SELECT createdAt FROM session LIMIT 1`);
  check("session dates are ISO 8601 strings", sample.length === 0 || /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?Z$/.test(sample[0].createdAt), sample[0]?.createdAt);
  // A browser sign-in handed back to Yap leaves a PC-browser session and a
  // Yap one (both to be made an hour old); plus a fresh phone-browser one.
  await sleep(4000);
  await desktopSocialSignIn(new Jar());
  const fresh = new Jar();
  await emailSignIn("browser.fresh@example.com", { ua: PHONE_UA, jar: fresh, origin: ORIGIN });
  const newest = async (ua) => (await sql(`SELECT id FROM session WHERE userAgent = ${q(ua)} ORDER BY createdAt DESC LIMIT 1`))[0]?.id;
  const oldBrowser = await newest(PC_BROWSER_UA);
  const oldYap = await newest(YAP_UA);
  const freshId = await newest(PHONE_UA);
  check("have a PC-browser, a Yap and a phone-browser session", !!oldBrowser && !!oldYap && !!freshId);
  await sql(`UPDATE session SET createdAt = ${q(hourAgo)} WHERE id IN (${q(oldBrowser)}, ${q(oldYap)})`);
  // Stale rows of every kind, plus fresh ones that must stay.
  const now = Date.now();
  await sql(`INSERT INTO deviceCode (id, deviceCode, userCode, userId, expiresAt, status, clientId) VALUES ('stale1', 'stale-device-code', 'QQQQQQQQ', NULL, ${q(new Date(now - 2 * 3600_000).toISOString())}, 'pending', 'yap-desktop')`);
  await sql(`INSERT INTO deviceOrigin (userCode, createdAt) VALUES ('QQQQQQQQ', ${now - 71 * 60_000}), ('RRRRRRRR', ${now - 60 * 60_000})`);
  await sql(`INSERT INTO phoneSession (tokenHash, createdAt) VALUES ('stalehash', ${now - 3 * 86400_000}), ('freshhash', ${now - 86400_000})`);
  await sql(`INSERT INTO emailCodeSend (key, sentAt) VALUES ('o:stale', ${now - 2 * 86400_000})`);
  const res = await fetch(`${BASE}/__scheduled?cron=*/30+*+*+*+*`);
  check("/__scheduled → 200", res.status === 200, `${res.status} ${(await res.text()).slice(0, 60)}`);
  let left;
  for (let i = 0; i < 20; i++) {
    left = await sql(`SELECT id FROM session WHERE id IN (${q(oldBrowser)}, ${q(oldYap)}, ${q(freshId)})`);
    if (!left.some((r) => r.id === oldBrowser)) break;
    await sleep(200);
  }
  const ids = left.map((r) => r.id);
  check("old Mozilla/ session deleted", !ids.includes(oldBrowser));
  check("old Yap/ session kept", ids.includes(oldYap));
  check("fresh Mozilla/ session kept", ids.includes(freshId));
  check("stale deviceCode deleted", (await sql(`SELECT 1 FROM deviceCode WHERE id = 'stale1'`)).length === 0);
  const origins = (await sql(`SELECT userCode FROM deviceOrigin WHERE userCode IN ('QQQQQQQQ','RRRRRRRR')`)).map((r) => r.userCode);
  check("deviceOrigin > 70 min deleted, 60 min kept", !origins.includes("QQQQQQQQ") && origins.includes("RRRRRRRR"), origins.join(","));
  const marks = (await sql(`SELECT tokenHash FROM phoneSession WHERE tokenHash IN ('stalehash','freshhash')`)).map((r) => r.tokenHash);
  check("phoneSession > 2 days deleted, 1 day kept", !marks.includes("stalehash") && marks.includes("freshhash"), marks.join(","));
  check("look-up counts > 1 day deleted", (await sql(`SELECT 1 FROM emailCodeSend WHERE key = 'o:stale'`)).length === 0);
  await sql(`DELETE FROM deviceOrigin WHERE userCode = 'RRRRRRRR'`);
  await sql(`DELETE FROM phoneSession WHERE tokenHash = 'freshhash'`);
  await call("POST", "/api/auth/sign-out", { body: {}, jar: fresh, ua: PHONE_UA, origin: ORIGIN });
}

if (want("v6")) {
  console.log("\n== B4. IPv6 /64 and odd addresses ==");
  const net = async (ip) => {
    const c = await newCode(YAP_UA, { "cf-connecting-ip": ip });
    if (c.status !== 200) throw new Error(`${ip}: ${c.status} ${c.text}`);
    return (await originRow(c.json.user_code))?.network ?? null;
  };
  const a = await net("2001:db8:1:2::5");
  const b = await net("2001:0db8:0001:0002:ffff:0:0:9");
  const c = await net("2001:db8:1:3::5");
  check("IPv6: same /64 (compressed vs full form) → same network", a && a === b, `${a?.slice(0, 8)} ${b?.slice(0, 8)}`);
  check("IPv6: another /64 → another network", c && c !== a, `${c?.slice(0, 8)}`);
  await sleep(61_000); // /device/code: 5 a minute per IP (each spoofed IP is its own bucket, but stay polite)
  const m = await net("::ffff:203.0.113.77");
  const v4 = await net("203.0.113.1");
  check("IPv4-mapped IPv6 → its IPv4 /24", m && m === v4, `${m?.slice(0, 8)} ${v4?.slice(0, 8)}`);
  const junk = await net("not-an-ip");
  check("unparseable address → no network", junk === null, `${junk}`);
}

if (want("delete")) {
  console.log("\n== C3. a non-phone session may still delete its account ==");
  const token = ctx.u1 ?? (await emailSignIn("yapuser2@example.com")).token;
  const email = ctx.u1 ? "yapuser1@example.com" : "yapuser2@example.com";
  const r = await call("POST", "/api/auth/delete-user", { body: {}, headers: bearer(token) });
  check("delete-user with an email-code session → 200 (passes the phone-session guard)", r.status === 200, `${r.status} ${r.text}`);
  const mail = await mailTo(email, "has been deleted");
  check("deletion receipt printed", !!mail);
}

console.log(`\n${failures ? `${failures} FAILED` : "all passed"}`);
if (ctx.originSample) console.log("sample device-origin:", JSON.stringify(ctx.originSample));
process.exit(failures ? 1 : 0);
