// What went wrong with an AI call, said so a person knows what to do next.
// ai/llm.rs reports the provider's raw failure ("request failed: error sending
// request for url (http://127.0.0.1:11434/v1/chat/completions)", "HTTP 401:
// {…}"); the raw text stays in the log. Pure, shared (Chat's "Couldn't send").

const LOCAL_HOST = /^(127\.0\.0\.1|localhost|\[::1\])(:\d+)?$/i;
/** An answer cut off part-way (ai/llm.rs `CUT_OFF`, word for word). */
export const CUT_OFF = 'The answer was cut off before it finished. Try again.';
const UNREACHABLE =
  /error sending request|connection refused|dns error|failed to lookup|timed out|tcp connect|connect error|network is unreachable/;

/** Where the person sees their Claude plan's usage. */
export const CLAUDE_USAGE_URL = 'https://claude.ai/settings/usage';

/**
 * **A Chat answer on your Claude plan that didn't come** (Rust's own words,
 * ai/claude_code/events.rs `Problem::message`, ai/claude_plan.rs
 * `NOT_READY`): what helps with it — "signIn" (Claude Code isn't signed in
 * here: Sign in to Claude Code), "limit" (the plan's limit: Manage usage, or
 * this chat on an API key), "paysAnotherWay" (Claude Code here is set to pay
 * with a key: Yap stopped it). `null`: not one of those. Pure.
 */
export function claudePlanProblem(err) {
  const raw = String(err?.message ?? err ?? '');
  if (/Claude Code isn't signed in/i.test(raw)) return 'signIn';
  if (/reached your Claude plan's limit/i.test(raw)) return 'limit';
  if (/Claude Code on this PC is set to pay/i.test(raw)) return 'paysAnotherWay';
  return null;
}

/**
 * @param {unknown} err  the rejected invoke's error
 * @returns {{ text: string, settings: boolean, known: boolean }} the
 *   message; whether Settings → Language Models is where it gets fixed;
 *   and whether it's one of the failures explained here (else `text` is
 *   the raw error)
 */
export function aiError(err) {
  const raw = String(err?.message ?? err ?? '').trim();
  const lower = raw.toLowerCase();
  const host = (raw.match(/https?:\/\/([^/\s)]+)/i) || [])[1] || '';
  const said = (text, settings) => ({ text, settings, known: true });
  // Your Claude plan's problems: said as Rust says them (the buttons that
  // help go with them, Chat's toast).
  if (claudePlanProblem(raw)) return said(raw, false);
  if (UNREACHABLE.test(lower)) {
    if (LOCAL_HOST.test(host)) {
      return said(
        `Couldn't reach the AI model on this PC (${host}). Check that it's running (Ollama, LM Studio…), then try again.`,
        true,
      );
    }
    return said(`Couldn't reach ${host || 'the AI provider'}. Check your internet connection, then try again.`, false);
  }
  if (/\bhttp 40[13]\b|invalid api key|incorrect api key|invalid x-api-key|unauthorized/.test(lower)) {
    return said("The AI provider didn't accept the API key. Check it in Settings → Language Models.", true);
  }
  if (/\bhttp 404\b/.test(lower) && /model/.test(lower)) {
    return said("The AI provider doesn't offer that model any more. Pick another in Settings → Language Models.", true);
  }
  // Screenshots past what the provider takes in one request (snip/images.rs
  // `fit_for` keeps them under; this is the rest).
  if (/\bhttp 413\b|request entity too large|request_too_large|payload too large/.test(lower)) {
    return said('That was too big for the AI provider to take at once, usually the screenshots. Try fewer, or a new chat.', false);
  }
  if (/\bhttp 5\d\d\b/.test(lower)) {
    return said('The AI provider had a problem on its side. Try again in a moment.', false);
  }
  // An answer that stopped coming part-way: Rust says it so already
  // (ai/llm.rs `CUT_OFF`); the HTTP library's own words never show.
  if (/answer was cut off|error decoding response body|the connection dropped/.test(lower)) {
    return said(CUT_OFF, false);
  }
  // Yap's own words already ("No groq API key — add one in Settings → …").
  if (/api key|settings → language models/.test(lower)) return said(raw, true);
  return { text: raw || 'Something went wrong. Try again.', settings: false, known: false };
}
