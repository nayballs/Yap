// Limits on emailed sign-in codes, on top of Better Auth's 3 a minute per IP.
// Every code is a real email: without these, one machine could flood
// someone's inbox (180 an hour) or spend the day's sending allowance so that
// nobody else could sign in by email.
//
// The table (migrations/0002) holds keyed hashes of the address and the IP,
// never the values themselves, and forgets them after a day.
import { env } from "cloudflare:workers";

const SECOND = 1000;
const DAY = 24 * 60 * 60 * SECOND;

/** One address: a code every 30 s at most (Yap's resend button waits as
 *  long) and 10 a day. */
const ADDRESS_GAP = 30 * SECOND;
const ADDRESS_PER_DAY = 10;
/** One IP: 20 codes a day, whatever the addresses. */
const IP_PER_DAY = 20;

export interface SendKeys {
  address: string;
  ip: string;
}

async function keyedHash(value: string): Promise<string> {
  const key = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(env.BETTER_AUTH_SECRET),
    { name: "HMAC", hash: "SHA-256" },
    false,
    ["sign"],
  );
  const mac = new Uint8Array(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(value)));
  return [...mac.slice(0, 16)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

/** `email` lowercased, as Better Auth stores it. */
export async function sendKeys(email: string, ip: string | undefined): Promise<SendKeys> {
  return {
    address: `a:${await keyedHash(`address:${email}`)}`,
    ip: `i:${await keyedHash(`ip:${ip ?? "unknown"}`)}`,
  };
}

/** Seconds until another code may go out; 0 means now. */
export async function secondsUntilNextSend(keys: SendKeys, now = Date.now()): Promise<number> {
  const { results } = await env.DB.prepare(
    `SELECT key, COUNT(*) AS n, MIN(sentAt) AS first, MAX(sentAt) AS last
       FROM emailCodeSend WHERE key IN (?1, ?2) AND sentAt > ?3 GROUP BY key`,
  )
    .bind(keys.address, keys.ip, now - DAY)
    .all<{ key: string; n: number; first: number; last: number }>();

  let wait = 0;
  for (const row of results) {
    const perDay = row.key === keys.address ? ADDRESS_PER_DAY : IP_PER_DAY;
    if (row.key === keys.address) wait = Math.max(wait, row.last + ADDRESS_GAP - now);
    // The oldest send leaving the 24-hour window frees a slot.
    if (row.n >= perDay) wait = Math.max(wait, row.first + DAY - now);
  }
  return Math.ceil(Math.max(0, wait) / SECOND);
}

/** Count a code that went out, and drop rows older than a day. */
export async function recordSend(keys: SendKeys, now = Date.now()): Promise<void> {
  await env.DB.batch([
    env.DB.prepare(`INSERT INTO emailCodeSend (key, sentAt) VALUES (?1, ?3), (?2, ?3)`).bind(keys.address, keys.ip, now),
    env.DB.prepare(`DELETE FROM emailCodeSend WHERE sentAt <= ?1`).bind(now - DAY),
  ]);
}
