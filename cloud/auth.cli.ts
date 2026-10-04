// Schema-generation config only (`npm run schema`): the Better Auth CLI can't
// load `cloudflare:workers`, so this mirrors src/auth.ts's table-affecting
// options against an in-memory SQLite database and emits the D1 migration.
import { DatabaseSync } from "node:sqlite";
import { betterAuth } from "better-auth";
import { bearer, deviceAuthorization, emailOTP } from "better-auth/plugins";
import { electron } from "@better-auth/electron";

export const auth = betterAuth({
  database: new DatabaseSync(":memory:"),
  rateLimit: { enabled: true, storage: "database" },
  plugins: [
    emailOTP({ async sendVerificationOTP() {} }),
    bearer(),
    electron({ clientID: "yap-desktop" }),
    deviceAuthorization(), // its table only; the options live in src/auth.ts
  ],
});
