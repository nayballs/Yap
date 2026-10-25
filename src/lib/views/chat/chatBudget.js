// A chat sized to its model, the page's side (src-tauri/src/chat/budget.rs,
// compact.rs): the same token estimate Rust makes (so the message box says
// "too long" before a paste is sent), Rust's "too long" sentence known by its
// start, and the summary's words.
import { fmtCount } from '../../core/aiUsage.js';

/** Each message's own tokens besides its text (budget.rs MESSAGE_OVERHEAD). */
export const MESSAGE_OVERHEAD = 6;

/**
 * About four characters a token, with a margin (budget.rs `estimate`): ASCII
 * at 3.6 characters a token, every other character one. Pure.
 */
export function estimateTokens(text) {
  let ascii = 0;
  let other = 0;
  for (const ch of String(text ?? '')) {
    if (ch.charCodeAt(0) < 128) ascii += 1;
    else other += 1;
  }
  return Math.ceil((ascii * 10) / 36) + other;
}

/** A question's tokens as Rust counts it before sending (its text and overhead). */
export function questionTokens(text) {
  const t = String(text ?? '').trim();
  return t ? estimateTokens(t) + MESSAGE_OVERHEAD : 0;
}

/** The start Rust's "too long" sentences share (budget.rs TOO_LONG). */
export const TOO_LONG = "That's too long for ";

/** An error that's Rust's plain "too long" sentence. Pure. */
export function isTooLong(err) {
  return String(err?.message ?? err ?? '').startsWith(TOO_LONG);
}

/**
 * The line under a paste too long for the model: Rust's sentence, then how
 * big it is and the room there is. Pure.
 */
export function tooLongLine(limits, tokens) {
  const said = limits?.tooLong || "That's too long for this model.";
  return `${said} It's about ${fmtCount(tokens)} tokens; there's room for ${fmtCount(limits?.questionMax || 0)}.`;
}

/** The divider over the messages a summary stands in for. */
export const SUMMARY_DIVIDER = "Earlier messages summarised to keep this chat within the model's memory";
