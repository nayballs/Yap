// Sign in with your phone: what this service keeps on top of Better Auth's
// deviceAuthorization plugin (src/auth.ts; tables in migrations/0004).
//
//  - Where each code came from (deviceOrigin): rough place, the app's
//    version, and a keyed hash of its network. The phone shows it before
//    approving, so a code someone else sent stands out (RFC 8628 §5.4).
//  - Which sessions a phone sign-in made (phoneSession): for their first day
//    they can't delete the account or sign other devices out (src/index.ts).
//  - sweep(): clears what's done with, after each new code and on a cron.
//
// No IP address is ever stored: only Cloudflare's guess at the city and
// country, and a keyed hash of the network (IPv4 /24, IPv6 /64).
import { env } from "cloudflare:workers";
import { keyedHash, pruneCounts } from "./throttle";

const MINUTE = 60 * 1000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

type Cf = IncomingRequestCfProperties | undefined;

/** A phone code as stored: its letters, upper case (`wdjb-mjht` → `WDJBMJHT`). */
export const normalizeUserCode = (raw: string) => raw.replace(/[^A-Za-z]/g, "").toUpperCase();

/** The network an IP is on: its IPv4 /24 or IPv6 /64, or null if unparseable.
 *  A PC and a phone on one home or office connection share one public IPv4
 *  address, or one IPv6 /64, so they match; mobile data doesn't. */
function networkOf(ip: string | null | undefined): string | null {
  const text = ip?.trim().toLowerCase() ?? "";
  const v4 = /^(?:::ffff:)?(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(text);
  if (v4) {
    const octets = v4.slice(1).map(Number);
    return octets.every((n) => n <= 255) ? `v4:${octets.slice(0, 3).join(".")}` : null;
  }
  const halves = text.split("::");
  if (!text.includes(":") || halves.length > 2) return null;
  const head = halves[0] ? halves[0].split(":") : [];
  const tail = halves.length === 2 && halves[1] ? halves[1].split(":") : [];
  const missing = 8 - head.length - tail.length;
  if (halves.length === 2 ? missing < 1 : missing !== 0) return null;
  const groups = [...head, ...Array<string>(halves.length === 2 ? missing : 0).fill("0"), ...tail];
  if (!groups.every((g) => /^[0-9a-f]{1,4}$/.test(g))) return null;
  return `v6:${groups.slice(0, 4).map((g) => parseInt(g, 16).toString(16)).join(":")}`;
}

/** A request's network as a keyed hash (src/throttle.ts), or null. */
async function networkKey(req: Request): Promise<string | null> {
  const net = networkOf(req.headers.get("cf-connecting-ip"));
  return net ? keyedHash(`network:${net}`) : null;
}

/** Cloudflare's country code for a request ("T1" = Tor), null if unknown
 *  (missing, or "XX", which its types leave out). */
function countryOf(cf: Cf): string | null {
  const code: string | undefined = cf?.country;
  return code && code !== "XX" && /^[A-Z][A-Z0-9]$/.test(code) ? code : null;
}

const cityOf = (cf: Cf) => cf?.city?.trim().slice(0, 80) || null;

/** Records who asked for a new code: `req` is Yap's /device/code request. */
export async function recordOrigin(userCode: string, req: Request, now = Date.now()): Promise<void> {
  const cf = req.cf as Cf;
  await env.DB.prepare(
    `INSERT OR REPLACE INTO "deviceOrigin" ("userCode", "createdAt", "country", "city", "userAgent", "network")
     VALUES (?1, ?2, ?3, ?4, ?5, ?6)`,
  )
    .bind(
      normalizeUserCode(userCode),
      now,
      countryOf(cf),
      cityOf(cf),
      req.headers.get("user-agent")?.slice(0, 200) || null,
      await networkKey(req),
    )
    .run();
}

/** "Yap 0.1.1 on Windows" from Yap's `Yap/0.1.1 (Windows)` (src-tauri's
 *  auth.rs sends it); otherwise just "Yap". Anyone can send any user agent,
 *  so only a plain version and a known system get through. */
export function appLabel(userAgent: string | null): string {
  const ua = userAgent?.trim() ?? "";
  if (!ua.startsWith("Yap/")) return "Yap";
  const [version = "", ...more] = ua.slice(4).split(" ");
  const app = /^[0-9A-Za-z.+-]{1,32}$/.test(version) ? `Yap ${version}` : "Yap";
  const system = /^\((Windows|macOS|Linux)\)$/.exec(more.join(" "))?.[1];
  return system ? `${app} on ${system}` : app;
}

/** English country names ("GB" → "United Kingdom"); the code if there's no ICU. */
let regionNames: Intl.DisplayNames | null | undefined;
function countryName(code: string): string {
  try {
    regionNames ??= new Intl.DisplayNames(["en"], { type: "region" });
    return regionNames.of(code) ?? code;
  } catch {
    regionNames = null;
    return code;
  }
}

/** Countries whose English name takes "the" ("in the United Kingdom"). */
const WITH_THE = new Set(["GB", "US", "NL", "AE", "PH", "BS", "GM", "DO", "CF", "KY", "MV", "SC", "KM", "MH", "SB", "VG", "VI", "TC", "FO"]);

/** "near Leeds, United Kingdom", "in the United Kingdom" without a city,
 *  "" when Cloudflare doesn't know. Its guess is about the network, hence
 *  "near". */
export function placeOf(country: string | null, city: string | null): string {
  if (country === "T1") return "through the Tor network";
  if (!country) return city ? `near ${city}` : "";
  const name = countryName(country);
  return city ? `near ${city}, ${name}` : `in ${WITH_THE.has(country) ? "the " : ""}${name}`;
}

/** What the phone shows about a code before approving it. */
export interface OriginView {
  /** The app that asked, e.g. "Yap 0.1.1 on Windows". */
  label: string;
  /** e.g. "near Leeds, United Kingdom"; "" if unknown. */
  place: string;
  minutesAgo: number;
  /** Same network as the phone (null: one side unknown). */
  sameNetwork: boolean | null;
  /** Same country as the phone (null: one side unknown). */
  sameCountry: boolean | null;
}

interface OriginRow {
  createdAt: number;
  country: string | null;
  city: string | null;
  userAgent: string | null;
  network: string | null;
}

/** A still-pending code's origin, compared with `phone` (the request asking,
 *  signed in as `userId`); null when the code isn't pending, another account
 *  has claimed it (only the claimer may approve), or it wasn't recorded. */
export async function describeOrigin(
  userCode: string,
  phone: Request,
  userId: string,
  now = Date.now(),
): Promise<OriginView | null> {
  // Better Auth keeps D1 dates as ISO 8601 strings, which compare in time order.
  const row = await env.DB.prepare(
    `SELECT o."createdAt", o."country", o."city", o."userAgent", o."network"
       FROM "deviceCode" d JOIN "deviceOrigin" o ON o."userCode" = d."userCode"
      WHERE d."userCode" = ?1 AND d."status" = 'pending' AND d."expiresAt" > ?2
        AND (d."userId" IS NULL OR d."userId" = ?3)`,
  )
    .bind(normalizeUserCode(userCode), new Date(now).toISOString(), userId)
    .first<OriginRow>();
  if (!row) return null;
  const network = await networkKey(phone);
  const country = countryOf(phone.cf as Cf);
  return {
    label: appLabel(row.userAgent),
    place: placeOf(row.country, row.city),
    minutesAgo: Math.max(0, Math.floor((now - row.createdAt) / MINUTE)),
    sameNetwork: row.network && network ? row.network === network : null,
    sameCountry: row.country && country ? row.country === country : null,
  };
}

/** Where a code came from, for the new-sign-in email ("" if unknown). */
export async function originPlace(userCode: string): Promise<string> {
  const row = await env.DB.prepare(`SELECT "country", "city" FROM "deviceOrigin" WHERE "userCode" = ?1`)
    .bind(normalizeUserCode(userCode))
    .first<{ country: string | null; city: string | null }>();
  return row ? placeOf(row.country, row.city) : "";
}

const tokenKey = (token: string) => keyedHash(`session-token:${token}`);

/** Marks a session token that a phone sign-in just handed Yap. */
export async function recordPhoneSession(token: string, now = Date.now()): Promise<void> {
  await env.DB.prepare(`INSERT OR REPLACE INTO "phoneSession" ("tokenHash", "createdAt") VALUES (?1, ?2)`)
    .bind(await tokenKey(token), now)
    .run();
}

/** Whether `token` belongs to a session a phone sign-in made under a day ago. */
export async function isNewPhoneSession(token: string, now = Date.now()): Promise<boolean> {
  const row = await env.DB.prepare(`SELECT "createdAt" FROM "phoneSession" WHERE "tokenHash" = ?1`)
    .bind(await tokenKey(token))
    .first<{ createdAt: number }>();
  return row !== null && now - row.createdAt < DAY;
}

/** Clears what's done with. Runs after every new code and on the cron
 *  (wrangler.toml [triggers], every 30 minutes). */
export async function sweep(now = Date.now()): Promise<void> {
  await env.DB.batch([
    // Codes nobody finished (cancelled in Yap, never scanned). Better Auth
    // deletes a code only when Yap redeems it, or polls it after a deny or
    // expiry; these go an hour after they expire, late enough that a
    // straggling poll still hears "expired".
    env.DB.prepare(`DELETE FROM "deviceCode" WHERE "expiresAt" < ?1`).bind(new Date(now - HOUR).toISOString()),
    // Where a code came from stays as long as the code's row can (its 10
    // minutes, plus the hour above), then goes.
    env.DB.prepare(`DELETE FROM "deviceOrigin" WHERE "createdAt" < ?1`).bind(now - 70 * MINUTE),
    env.DB.prepare(`DELETE FROM "phoneSession" WHERE "createdAt" < ?1`).bind(now - 2 * DAY),
    // Browser sessions left behind. The only ones this service makes are the
    // hand-back page's (public/handoff.js) and the phone page's (device.js),
    // and both sign out when they're done; Yap's own sessions carry its
    // `Yap/…` user agent. One still here after 30 minutes was abandoned
    // mid-way (tab closed, sign-out lost) and would otherwise stay usable in
    // that browser for 30 days. A web page that needs to stay signed in
    // would have to change this.
    env.DB.prepare(`DELETE FROM "session" WHERE "userAgent" LIKE 'Mozilla/%' AND "createdAt" < ?1`).bind(
      new Date(now - 30 * MINUTE).toISOString(),
    ),
    pruneCounts(now),
  ]);
}
