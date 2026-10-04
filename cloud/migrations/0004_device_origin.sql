-- Sign in with your phone, hardening (src/phone.ts).
--
-- deviceOrigin: who asked for each phone sign-in code, shown on the phone
-- before it approves (RFC 8628 §5.4): approximate place from Cloudflare's IP
-- geolocation, the app's user agent, and `network`, a keyed hash of the
-- asker's IPv4 /24 or IPv6 /64 (never the IP itself) to tell the phone
-- whether it's on the same network. `userCode` is the code's 8 letters;
-- `createdAt` is Unix ms. Rows go after ~70 minutes (codes last 10).
CREATE TABLE IF NOT EXISTS "deviceOrigin" (
  "userCode" TEXT NOT NULL PRIMARY KEY,
  "createdAt" INTEGER NOT NULL,
  "country" TEXT,
  "city" TEXT,
  "userAgent" TEXT,
  "network" TEXT
);
CREATE INDEX IF NOT EXISTS "deviceOrigin_createdAt_idx" ON "deviceOrigin" ("createdAt");

-- phoneSession: sessions made by a phone sign-in (/device/token), as a keyed
-- hash of the session token (never the token). For their first day they
-- can't delete the account or sign other devices out. `createdAt` is Unix
-- ms; rows go after 2 days.
CREATE TABLE IF NOT EXISTS "phoneSession" (
  "tokenHash" TEXT NOT NULL PRIMARY KEY,
  "createdAt" INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS "phoneSession_createdAt_idx" ON "phoneSession" ("createdAt");
