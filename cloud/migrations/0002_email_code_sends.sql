-- Emailed sign-in codes, for the send limits in src/throttle.ts. `key` is a
-- keyed hash of an address ("a:…") or an IP ("i:…"); rows last a day.
CREATE TABLE IF NOT EXISTS "emailCodeSend" (
  "key" TEXT NOT NULL,
  "sentAt" INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS "emailCodeSend_key_sentAt_idx" ON "emailCodeSend" ("key", "sentAt");
CREATE INDEX IF NOT EXISTS "emailCodeSend_sentAt_idx" ON "emailCodeSend" ("sentAt");
