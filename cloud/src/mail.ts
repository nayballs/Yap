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
    // The message carries a sign-in code, so only a local server prints it.
    if (new URL(env.BETTER_AUTH_URL).hostname === "localhost") {
      console.log(`[mail:dev] to=${to}\nSubject: ${mail.subject}\n${mail.text}`);
    } else {
      console.error("[mail] RESEND_API_KEY is not set; email not sent");
    }
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

// One layout for every email, in the shape people know from Google's and
// Apple's account mail: logo, headline, the account it's about, a divider,
// the message (plus a details panel or the code), an optional button, and a
// footer saying why it was sent. Tables and inline styles only (that's what
// Gmail, Outlook and Apple Mail all render), a PNG logo (Gmail drops SVG), a
// system font stack (no webfonts in mail), Yap's warm-light palette.
const FONT = "-apple-system,'Segoe UI',Roboto,Helvetica,Arial,sans-serif";
const MONO = "ui-monospace,Consolas,'SF Mono',Menlo,monospace";
const LOGO = "https://auth.contextmirror.com/email/yap-logo.png";
const INK = "#1f1c16";
const BODY = "#3d3a33";
const MUTED = "#6e6a5f";
const HAIRLINE = "#ece8de";
const PANEL = "#faf9f6";
const AMBER = "#d98a2b";

const esc = (text: string) => text.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

interface Layout {
  /** Inbox preview line (hidden in the message itself). */
  preheader: string;
  title: string;
  /** The address the email is about, shown as an account chip. */
  account?: string;
  /** The message, as HTML (escape anything variable). */
  body: string;
  /** A details panel or the code, under the message. */
  panel?: string;
  button?: { label: string; href: string };
  /** Small print under the button / panel. */
  note?: string;
  /** Why this email was sent (footer). */
  reason: string;
}

function layout(l: Layout): string {
  const initial = esc((l.account ?? "?").trim().charAt(0).toUpperCase() || "?");
  const chip = l.account
    ? `<table role="presentation" cellpadding="0" cellspacing="0" align="center" style="margin:16px auto 0;border:1px solid #e4e0d6;border-radius:999px"><tr>
<td style="padding:4px"><div style="width:24px;height:24px;border-radius:12px;background:${AMBER};color:#ffffff;font-family:${FONT};font-size:12px;font-weight:600;line-height:24px;text-align:center">${initial}</div></td>
<td style="padding:4px 14px 4px 4px;font-family:${FONT};font-size:14px;line-height:20px;color:${BODY}">${esc(l.account)}</td>
</tr></table>`
    : "";
  const button = l.button
    ? `<table role="presentation" cellpadding="0" cellspacing="0" align="center" style="margin:28px auto 0"><tr>
<td bgcolor="#26231c" style="border-radius:999px"><a href="${esc(l.button.href)}" style="display:inline-block;padding:12px 26px;font-family:${FONT};font-size:14px;font-weight:600;line-height:20px;color:#ffffff;text-decoration:none;border-radius:999px">${esc(l.button.label)}</a></td>
</tr></table>`
    : "";
  const note = l.note
    ? `<p style="margin:${l.button ? "20px" : "24px"} 0 0;font-family:${FONT};font-size:12px;line-height:18px;color:${MUTED};text-align:center">${l.note}</p>`
    : "";
  return `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="color-scheme" content="light"><meta name="supported-color-schemes" content="light"><title>${esc(l.title)}</title></head>
<body style="margin:0;padding:0;background:#f4f2ed">
<div style="display:none;max-height:0;overflow:hidden;opacity:0;color:transparent">${esc(l.preheader)}</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="background:#f4f2ed"><tr><td align="center" style="padding:40px 16px">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="max-width:480px;background:#ffffff;border:1px solid #e4e0d6;border-radius:12px">
<tr><td align="center" style="padding:36px 36px 32px">
<img src="${LOGO}" width="48" height="48" alt="Yap" style="display:block;margin:0 auto;border:0;border-radius:11px">
<h1 style="margin:20px 0 0;font-family:${FONT};font-size:22px;font-weight:600;line-height:30px;color:${INK}">${esc(l.title)}</h1>
${chip}
<div style="height:1px;line-height:1px;font-size:1px;background:${HAIRLINE};margin:24px 0">&nbsp;</div>
<p style="margin:0;font-family:${FONT};font-size:14px;line-height:22px;color:${BODY};text-align:center">${l.body}</p>
${l.panel ?? ""}
${button}
${note}
</td></tr></table>
<p style="margin:20px auto 0;max-width:440px;font-family:${FONT};font-size:12px;line-height:18px;color:${MUTED};text-align:center">${esc(l.reason)}</p>
<p style="margin:8px 0 0;font-family:${FONT};font-size:12px;line-height:18px;color:${MUTED};text-align:center">Yap · local voice dictation · <a href="https://contextmirror.com/yap" style="color:${MUTED};text-decoration:underline">contextmirror.com/yap</a></p>
</td></tr></table>
</body></html>`;
}

/** A label/value list in a soft panel ("Device: Yap 0.1.1 on Windows"). */
function details(rows: [string, string][]): string {
  const cells = rows
    .map(
      ([label, value], i) => `<tr>
<td valign="top" style="padding:${i ? "10px" : "0"} 16px 0 0;width:96px;font-family:${FONT};font-size:12px;line-height:20px;color:${MUTED};white-space:nowrap">${esc(label)}</td>
<td valign="top" style="padding:${i ? "10px" : "0"} 0 0;font-family:${FONT};font-size:14px;line-height:20px;color:${INK}">${esc(value)}</td>
</tr>`,
    )
    .join("");
  return `<table role="presentation" width="100%" cellpadding="0" cellspacing="0" style="margin:24px 0 0;background:${PANEL};border:1px solid ${HAIRLINE};border-radius:10px"><tr><td style="padding:16px 18px;text-align:left">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0">${cells}</table>
</td></tr></table>`;
}

export function otpEmail(otp: string, to: string): Mail {
  return {
    subject: `Your Yap code: ${otp}`,
    text: `Your Yap sign-in code is ${otp}\n\nEnter it in Yap to finish signing in. It expires in 10 minutes.\n\nIf you didn't ask for it, you can ignore this email: nobody can sign in without the code.`,
    html: layout({
      preheader: `${otp} is your Yap sign-in code. It expires in 10 minutes.`,
      title: "Your sign-in code",
      account: to,
      body: "Enter this code in Yap to finish signing in.",
      panel: `<div style="margin:24px 0 0;padding:18px 0;font-family:${MONO};font-size:32px;font-weight:600;letter-spacing:8px;line-height:40px;color:${INK};text-align:center;background:${PANEL};border:1px solid ${HAIRLINE};border-radius:10px">${esc(otp)}</div>`,
      note: "It expires in 10 minutes. If you didn't ask for it, you can ignore this email: nobody can sign in without the code.",
      reason: "You're receiving this because a Yap sign-in code was requested for this address.",
    }),
  };
}

/** "Sunday 4 October 2026 at 21:48 BST", in `timeZone` (an IANA name, e.g.
 *  Cloudflare's guess for the phone) or UTC. */
function formatWhen(at: Date, timeZone: string | undefined): string {
  const options: Intl.DateTimeFormatOptions = {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: "numeric",
    hour: "2-digit",
    minute: "2-digit",
    timeZoneName: "short",
  };
  try {
    return new Intl.DateTimeFormat("en-GB", { ...options, timeZone: timeZone || "UTC" }).format(at);
  } catch {
    return new Intl.DateTimeFormat("en-GB", { ...options, timeZone: "UTC" }).format(at); // unknown zone
  }
}

/** Sent when a phone approves a sign-in to Yap (src/index.ts, after
 *  /device/approve). `place` is like "near Leeds, United Kingdom" ("" when
 *  unknown); `device` like "Yap 0.1.1 on Windows". */
export function newSignInEmail(s: { to: string; device: string; place: string; at: Date; timeZone?: string }): Mail {
  const when = formatWhen(s.at, s.timeZone);
  const place = s.place ? s.place.replace(/^near /, "Near ") : "Unknown";
  const fix = "In Yap, open Settings → Account → Where you're signed in, and sign that device out.";
  return {
    subject: "New sign-in to your Yap account",
    text: `New sign-in to your Yap account (${s.to})\n\nYour phone approved a sign-in to Yap on another device.\n\nDevice: ${s.device}\nLocation: ${place}\nTime: ${when}\nApproved with: Your phone (QR code)\n\nIf this was you, you don't need to do anything.\nNot you? ${fix}\n\nReview your devices: https://auth.contextmirror.com/security`,
    html: layout({
      preheader: `${s.device} was signed in to your account with your phone.`,
      title: "New sign-in to your account",
      account: s.to,
      body: "Your phone approved a sign-in to Yap on another device. If this was you, you don't need to do anything.",
      panel: details([
        ["Device", s.device],
        ["Location", place],
        ["Time", when],
        ["Approved with", "Your phone (QR code)"],
      ]),
      button: { label: "Review your devices", href: "https://auth.contextmirror.com/security" },
      note: `<strong style="color:${BODY}">Not you?</strong> ${esc(fix)}`,
      reason: "You're receiving this email to let you know about important activity on your Yap account.",
    }),
  };
}

/** Receipt after an account is deleted from Yap (Settings → Account). */
export function accountDeletedEmail(to: string): Mail {
  return {
    subject: "Your Yap account has been deleted",
    text: `Your Yap account (${to}) has been deleted, along with its sign-ins and sessions. Yap keeps working on your PC without an account: your dictation, notes and settings were never stored with us.\n\nDidn't do this? Contact contextmirror@proton.me.`,
    html: layout({
      preheader: "Your Yap account and its sign-ins are gone. Dictation on your PC keeps working.",
      title: "Your account has been deleted",
      account: to,
      body: "Your Yap account has been deleted, along with its sign-ins and sessions. Yap keeps working on your PC without an account: your dictation, notes and settings were never stored with us.",
      note: `Didn't do this? Contact <a href="mailto:contextmirror@proton.me" style="color:${MUTED};text-decoration:underline">contextmirror@proton.me</a>.`,
      reason: "You're receiving this email to let you know about important activity on your Yap account.",
    }),
  };
}
