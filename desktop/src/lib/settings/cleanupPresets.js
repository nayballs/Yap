// Dictation cleanup's presets (Prompt Studio's, and Smart routing's "New
// from a preset…"). Each fills the editable "body" (tone/format). The immutable
// guardrails live in the backend (llm::BASE_PROMPT) and are always applied, so
// a preset only changes behaviour, never the refusal rules. "custom" = the user
// edited the body by hand. Keep `default` in sync with default_pp_prompt() (Rust).
export const PP_PRESETS = [
  {
    value: 'default',
    label: 'Default',
    // MUST stay byte-identical to config.rs default_pp_prompt() (the "Default vs
    // Modified" check compares them). Written with \n escapes on one line so file
    // line-endings can't diverge from Rust's LF.
    body: `Clean up the transcript using these rules.\n\nRULES:\n- Remove filler words (um, uh, er, like, you know, basically) unless meaningful.\n- Fix grammar, spelling, punctuation, and capitalization; break up run-on sentences.\n- Remove false starts, stutters, and accidental repetitions.\n- Correct obvious speech-to-text transcription errors from context.\n- Preserve the speaker's voice, tone, vocabulary, and intent.\n- Preserve technical terms, proper nouns, names, and jargon exactly as spoken — never "correct" them.\n\nSelf-corrections ("wait no", "I meant", "scratch that"): keep only the corrected version. "Actually" used for emphasis is NOT a correction.\nSpoken punctuation ("period", "comma", "new line"): convert to symbols. Use context to distinguish commands from literal mentions.\nNumbers & dates: standard written forms (January 15, 2026 / $300 / 5:30 PM). Small conversational numbers can stay as words.\nBroken phrases: reconstruct the speaker's likely intent from context. Never output a polished sentence that says nothing coherent.\nFormatting: bullets, numbered lists, or paragraph breaks only when they genuinely improve readability. Don't over-format.\n\nOUTPUT:\n- Output ONLY the cleaned text. Nothing else.\n- No commentary, labels, explanations, or preamble.\n- No questions. No suggestions. No added content.\n- Empty or filler-only input = empty output.`,
  },
  {
    value: 'email',
    label: 'Email',
    body:
      'Remove filler words and fix grammar, punctuation, and capitalization. Format the result as a clear, professional email body with complete sentences and sensible paragraphs. Keep a polite, professional tone. Don\'t add a greeting or sign-off unless it was dictated.',
  },
  {
    value: 'notes',
    label: 'Notes',
    body:
      'Remove filler words and fix grammar and punctuation. Format the result as concise notes: turn spoken lists into bullet points and trim hedging. Keep it terse and scannable.',
  },
  {
    value: 'slack',
    label: 'Slack / Chat',
    body:
      'Remove filler words and fix obvious grammar. Keep it casual and brief, like a chat message — light punctuation and a conversational tone. Don\'t over-formalize or expand.',
  },
  {
    value: 'code',
    label: 'Code / Technical',
    body:
      'Remove filler words and fix punctuation. This is technical dictation: preserve technical terms, identifiers, file names, code symbols, and casing exactly as spoken — never "correct" jargon, library, or command names. Format numbers and inline code precisely.',
  },
  { value: 'custom', label: 'Custom', body: null },
];

/** The default cleanup body (Reset, and Prompt Studio's Default-vs-Modified). */
export const PP_DEFAULT_BODY = PP_PRESETS.find((p) => p.value === 'default')?.body ?? '';

/** Smart routing's "New from a preset…" picker, in Yap's own Select. */
export const PRESET_PICKS = [
  { value: '', label: 'New from a preset…' },
  ...[
    ['default', 'Default'],
    ['email', 'Email'],
    ['notes', 'Notes'],
    ['slack', 'Slack / Chat'],
    ['code', 'Code / Technical'],
  ].map(([value, label]) => ({ value, label })),
];
