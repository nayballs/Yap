// Transactional email via Resend's HTTP API (plain fetch, no SDK). Without a
// RESEND_API_KEY (local `wrangler dev`) messages are printed to the console
// instead, so the email-code flow is testable offline.
import { env } from "cloudflare:workers";

export interface Mail {
  subject: string;
  text: string;
  html: string;
}

export async function sendMail(to: string, mail: Mail): Promise<void> {
  if (!env.RESEND_API_KEY) {
    console.log(`[mail:dev] to=${to}\nSubject: ${mail.subject}\n${mail.text}`);
    return;
  }
  const res = await fetch("https://api.resend.com/emails", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${env.RESEND_API_KEY}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ from: env.EMAIL_FROM, to: [to], ...mail }),
  });
  // Never log the message body in production: it carries the code or link.
  if (!res.ok) console.error(`[mail] Resend HTTP ${res.status}: ${await res.text()}`);
}

// Yap's warm-light palette, inlined (email clients ignore <style> sheets and
// webfonts, hence the Georgia / system-font fallbacks).
const shell = (inner: string) => `<!doctype html>
<html><body style="margin:0;padding:32px 16px;background:#f0ede7;font-family:-apple-system,'Segoe UI',Roboto,Helvetica,Arial,sans-serif;color:#26231c">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0"><tr><td align="center">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:440px;background:#ffffff;border:1px solid #dcd7cb;border-radius:14px">
<tr><td style="padding:32px 32px 28px">
<div style="font-family:Georgia,'Palatino Linotype',serif;font-size:26px;line-height:1.2;margin:0 0 16px">Yap</div>
${inner}
</td></tr></table>
<p style="margin:16px 0 0;font-size:12px;color:#6e6a5f">Yap · local voice dictation · <a href="https://contextmirror.com/yap" style="color:#6e6a5f">contextmirror.com/yap</a></p>
</td></tr></table></body></html>`;

export function otpEmail(otp: string): Mail {
  return {
    subject: `Your Yap code: ${otp}`,
    text: `Your Yap sign-in code is ${otp}\n\nEnter it in Yap to finish signing in. It expires in 10 minutes.\n\nIf you didn't try to sign in to Yap, you can ignore this email.`,
    html: shell(`<p style="margin:0 0 20px;font-size:15px;line-height:1.5">Enter this code in Yap to finish signing in:</p>
<div style="font-family:Consolas,'SF Mono',Menlo,monospace;font-size:34px;font-weight:600;letter-spacing:10px;padding:14px 0;text-align:center;background:#faf9f6;border:1px solid #e7e3d9;border-radius:11px">${otp}</div>
<p style="margin:20px 0 0;font-size:13px;line-height:1.5;color:#6e6a5f">It expires in 10 minutes. If you didn't try to sign in to Yap, you can ignore this email.</p>`),
  };
}

/** Receipt after an account is deleted from Yap (Settings → Account). */
export function accountDeletedEmail(): Mail {
  return {
    subject: "Your Yap account has been deleted",
    text: "Your Yap account has been deleted, along with its sign-ins and sessions. Yap's dictation on your PC keeps working without an account.\n\nIf you didn't do this, contact contextmirror@proton.me.",
    html: shell(`<p style="margin:0 0 16px;font-size:15px;line-height:1.5">Your Yap account has been deleted, along with its sign-ins and sessions. Yap's dictation on your PC keeps working without an account.</p>
<p style="margin:0;font-size:13px;line-height:1.5;color:#6e6a5f">If you didn't do this, contact <a href="mailto:contextmirror@proton.me" style="color:#6e6a5f">contextmirror@proton.me</a>.</p>`),
  };
}
