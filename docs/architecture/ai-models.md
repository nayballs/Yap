# AI models

The language-model side: the OpenAI-compatible client and its prompts, the on-device model (llamafile sidecar, GGUF headers, the graphics card), which model a call goes to, the providers' live model lists, the AI usage tracker (every call's tokens, Groq's meter, cost estimates) and the model evaluations. See also [`docs/model-evals.md`](../model-evals.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules

- **`ai/about_you.rs`** — **what every model is told about the person**
  (2026-10-21, Settings → General → About you; a tester: "It keeps finding
  things in dollars… a better way to give the model better context on the
  user"). **One Region** (2026-10-22, the maintainer: "all of that should be
  covered in the region selector only"): **Automatic** = Windows' home
  location (`detected`: `GetUserDefaultGeoName`, else `GetUserGeoID` →
  `GEO_ISO2`; no home location → the regional format's region), or a
  country picked from the list (`config.region_country`, "" = Automatic).
  **Everything else follows the country** (`resolve(detected, picked)`,
  pure): its currency (the `COUNTRIES` table: ISO code, English name, ISO
  4217; Windows' own currency settings — `GetLocaleInfoEx`
  `LOCALE_SINTLSYMBOL` "GBP" + `LOCALE_SCURRENCY` "£", the user's changes
  included — and `LOCALE_IMEASURE` only while Windows' regional format is
  for that country: a UK PC with the US format still reads £ and miles),
  its units (`Units`: km and °C; miles and °C for the UK and Crown
  Dependencies, `MILES`; US units for `IMPERIAL`: the US and its
  territories, Liberia, Myanmar), the language — Windows' language with
  the country's variety ("en" + GB → "en-GB"; no Windows language: "en-XX"
  for an English-speaking country) — and so the English spelling
  (`Spelling` British / American / Canadian; none for another language).
  **The time zone is always the PC's** (`GetDynamicTimeZoneInformation`'s
  key name → IANA through `calendar/tz.rs`' CLDR table), never a setting.
  `summary`: what follows, as Settings shows it under the picker — "Prices
  in £ · miles and °C · British spelling · UK time" (`money_short`: £, €,
  $ alone; a shared or lettered sign with its code, "$ (AUD)", "kr (SEK)").
  Until 22 Oct 2026 (never shipped) there were five pickers (country,
  language, currency, units, time zone) and a spelling toggle; their
  config fields still load and go on the next save. **The block**
  (`prompt_block(profile, now)`, pure, unit-tested), last in the system
  prompt of Chat, the Voice Assistant, the Meetings view's and the Notes
  view's Ask bars (`turn::chat_turn`, `small_model_answer`'s web
  pipeline, `note_ask`), local and cloud: "About the user: they're in the
  United Kingdom (British English); it's Wednesday 21 October 2026, 21:40 UK
  time (UTC+01:00). Give times in UK time and metric units, but miles for
  distances and speeds. Temperatures in °C: convert one a source gives in °F
  (68°F is 20°C). Prices: in £ (GBP) when you have them in £; never
  change a price's currency sign — a price in another currency stays as it
  is, labelled, like "$15 a month (US price)". Places they mention are in
  or near the United Kingdom unless they say otherwise: don't ask which
  country. With no place given ("near me", a journey, the weather), start
  from where they live if you know it. Their name is Imogen. They told Yap:
  "…"" (the places sentence, 1 Nov 2026, for any country the region names:
  with the UK here and the town in memory's "You", Qwen3.5-4B still asked
  which country "Inverness to Perth" was in; the example is "£15 a month (UK price)" for a US user,
  and no real plan's price: with "$8 a month" as the example and "Give
  prices in £", Qwen3.5-4B wrote x.ai's "$8/month" as "£8/month" 2 times in
  3 — the model evaluations, model-evals.md). **Temperatures** have their
  own sentence (`Units::temperatures`, 2026-10-23): "metric units" alone let
  a UK weather answer copy its pages' "35°F (1.5°C), feeling colder at
  30°F"; a US user's is "Temperatures in °F: convert one a source gives in
  °C (20°C is 68°F)" (and "US units (miles, pounds)"); the examples are no
  figure the evaluations' pages give. The sentence alone didn't change
  Qwen3.5-4B's web answers (told to convert, it copied the °F or got it
  wrong), so **the web text itself puts their scale first**
  (`convert_temperatures`: "35°F" → "2°C (35°F)", with `temperature_note`'s
  line beside it; `temperature_units`: not when the question names the
  other scale — web-and-apps.md), and since 2026-10-25 its speeds and
  distances too (`about_you/units.rs`: mph and miles for the UK and the US,
  km/h and km elsewhere; `TheirUnits` does all three) — only the parts known; with
  nothing known, just the date and time (its clock changes every minute, so
  it goes last and the rest of the prompt stays cacheable). `about_name`
  (≤ 60 characters) and `about_me` (≤ 300, one line). **Search** is aimed at
  the country (`web::Settings.country`: Exa's `userLocation`, Tavily's
  `country` via `tavily_country`, the providers' own searches —
  web-and-apps.md), and **prices where the person is** (`LocalPrices`,
  2026-10-22, for someone outside the US: the place as a search says it —
  "UK" —, their money, and whether a page's price can be told apart as
  theirs — not for a dollar: `tells_apart`; `local_in` / `foreign_in` read
  prices in a text, `query_names_a_place` a search that already says where
  or in what money): a price question's search names the country, results
  with only another country's prices lead to one more search, then one
  plain line (web-and-apps.md); `strip_price_offer` takes an offer to
  look for local prices out of an answer after a search, and
  `restore_price_signs` gives a price the web gave only in another money
  ("$8" written "£8") its page's sign back. **Cleanup** keeps
  the spelling (`cleanup_spelling`, `region_spelling`, on; its one switch,
  "Write in my region's spelling", is in Language Models → Dictation
  Cleanup). `about_you_status {overrides?}` (About you and that switch):
  Windows' region (Automatic), the region with the country picked, its
  summary, the block as it reads now, the spelling, the countries. **Test
  mode** never reads Windows' settings: debug-only `YAP_E2E_LOCALE` (a tag,
  or JSON `{locale, country, currency, symbol, metric, timeZone}`) stands
  in, unset = nothing (the eval's `--locale`). Kept in config.json, sent
  only to the model the person uses; accounts don't sync it (they could,
  later: accounts.md).
- **`ai/llm/anthropic.rs`** — **Claude on Anthropic's own Messages API, for
  Chat's tool loop** (2026-10-31). A strong Claude's Chat steps
  (`speaks_messages`: provider `anthropic`, `documents::rich::strong`; the
  loop sets `ChatRequest::native`) go to `POST {base}/messages`
  (`x-api-key`, `anthropic-version: 2023-06-01`, the receipts'
  `Purpose::Ai`); every other Anthropic call (cleanup, Commands, summaries,
  the Voice Assistant's non-strong models) keeps the OpenAI-compatible
  `/chat/completions`. Why (Anthropic's OpenAI-compatibility docs, read 31
  Oct 2026): that endpoint takes function tools only (no server tools),
  answers `thinking: {type: "adaptive"}` with HTTP 400 and never returns the
  thinking, and has no prompt caching. **The request** (`body`, pure): the
  loop's OpenAI-shaped messages turned — the system prompt as one cached
  block (tools and system are the stable prefix), user text and screenshots
  (`image_url` data → base64 `image`), tool calls as `tool_use`, tool
  results as `tool_result` blocks first in the next user message, an earlier
  reply of Claude's sent back as its own blocks (`yap_anthropic`: thinking
  with its signature, server tool calls and their encrypted results, as the
  API asks); function tools as `{name, description, input_schema}`, then
  the server tools (web/native.rs `server_tools`); top-level `cache_control`
  for the conversation; `max_tokens` 16,000 (32,000 streamed; 64,000 at
  effort xhigh or max); thinking at the model's effort (`ai/effort.rs`
  below: adaptive thinking, `output_config.effort`, fast mode's `speed`;
  `display: "summarized"` on adaptive so its summary shows; since 31 Oct
  2026 a Claude question with an effort comes here even with no tools —
  the Notes view's Ask, a meeting's Ask, Chat's small path — since the
  compatible endpoint ignores `reasoning_effort`); a 400 that names
  `speed` (fast mode not on the account's plan) asks again at standard
  speed; `tool_choice` `none` on the loop's last
  call (its earlier `tool_use` blocks need the tools defined); temperature
  only where `takes_temperature`. **The answer** (`message_of`, pure): an
  OpenAI-shaped message — the text after its last server result (before it
  was "I'll look that up"), a mark after each block it cites (`MARK_OPEN`,
  numbered by the web turn: `number_marks`), its `tool_calls`, its thinking
  (`reasoning_content`), its blocks, and `yap_web` (searches, pages read,
  the pages it cites; a server tool's `error_code` said plainly). Streamed
  (`Stream`): `text_delta` as the answer, `thinking_delta` as thinking,
  `tool_use` and its `input_json_delta` (a document's section into the
  panel), each server tool as `Delta::Server` ("Searching the web…" /
  "Reading a page…"; what came before it isn't the answer), `citations_delta`
  and `signature_delta` kept. `pause_turn` is asked to go on (≤ 4); 429 and
  529 waited out as every Chat call; "too long" said plainly; a 400 naming
  `web_fetch` drops the fetch and asks again. A step with server tools may
  take 150 s. Unit-tested on the documented shapes (`anthropic::tests`);
  e2e through support/fake-llm.js's Messages API emulation.
- **`ai/chatgpt.rs`** (+ `chatgpt/`: `oauth`, `jwt`, `vault`) — **Sign in with
  ChatGPT** (2026-11-01): Yap's AI on the person's own ChatGPT plan instead of
  an API key, through OpenAI's flow for open-source apps that run on the
  person's PC. See [Sign in with ChatGPT](#sign-in-with-chatgpt-2026-11-01).
- **`ai/llm/responses.rs`** — **the ChatGPT plan on OpenAI's Responses API**
  (2026-11-01): every call on the provider "chatgpt", whatever the feature,
  from `post_chat_request`. See [Sign in with ChatGPT](#sign-in-with-chatgpt-2026-11-01).
- **`ai/claude_code.rs`** (+ `claude_code/`: `cli`, `session`, `events`,
  `fake`) — **the Claude Code engine** (2026-11-07): the person's own
  installed Claude Code CLI, driven as a child process on their own sign-in
  or API key, for Chat on the plan and Jobs. See [The Claude Code
  engine](#the-claude-code-engine-2026-11-07).
- **`ai/claude_plan.rs`** (+ `claude_plan/`: `map`, `oneshot`, `sessions`,
  `spare`, `usage`) — **Your Claude plan** (2026-11-07): the provider
  "claude-plan", Claude's models on the person's own Claude subscription
  through their own Claude Code, in every Language Models scope (Chat, and
  Dictation Cleanup, Commands and Note Formatting with a warm spare each and
  an opt-in fallback to the API key). See [Your Claude
  plan](#your-claude-plan-2026-11-07).
- **`ai/llm.rs`** — the AI cleanup client (OpenAI-compatible). Frames the transcript as
  data (delimiters + one-shot) so small models *clean* it instead of *answering* it.
  **The dictation's two messages for a hybrid model** (`cleanup_messages`,
  `split_dictation`, `splits_dictation`; 2026-10-22): the restated
  instruction (with the spelling line) as a user message of its own, then
  "Transcript: …" — the same words as the one message it was, the one-shots
  unchanged — when the model is hybrid: the on-device model whose GGUF has
  recurrent layers (`local_llm::serving_hybrid_at`, `gguf::Shape::is_hybrid`)
  or a Qwen3.5/3.6/Qwen3-Next by name (`hybrid_by_name`) on a self-hosted
  server (`llm::self_hosted`: loopback, a private address, `localhost`,
  `.local`/`.lan`, a one-word host — a cloud provider's Qwen3.5, on
  OpenRouter say, keeps the one message: its cache isn't llama.cpp's and
  nobody measured it). Such a model
  resumes its cache only from a checkpoint before the last user message, so
  it read the ~370-token instruction again for every dictation: Qwen3.5-4B
  on an RTX 5070 Ti 447 → 55 prompt tokens read, ~237 → ~187 ms a dictation
  (greedy outputs 27 of 32 the same; model-evals.md "Local AI, phase 1").
  Every other model keeps the one message: a dense model already re-used
  its cache (Qwen2.5-1.5B read 24 tokens either way) and, split, wrote
  numbers as words more often; Groq's gpt-oss changed 2 of 6.
  The system prompt is split FluidVoice-style: an immutable `BASE_PROMPT` (guardrails:
  output-only, never answer the transcript) that's always prepended via
  `build_system_prompt()` to the user's editable **body** (tone/format = a preset or
  custom text), then — in Yap's own layer, never the preset text —
  **the spelling line** ("Write in my region's spelling",
  `about_you::cleanup_spelling`: "When the transcript is in English, spell
  it the British way (honour, recognise, paralyse, metre, cancelled,
  catalogue, defence), without changing any words." — American or Canadian
  examples by region; in the system prompt, `cleanup_system_prompt`, **and**
  after the dictation's restated instruction, `cleanup_message`,
  `SpellingAt::Both`: in the system prompt alone Qwen3.5-4B changed only
  words like its examples and the 1.5B none, docs/model-evals.md) and the
  custom dictionary (`dictionary_suffix`) — only the entries the transcript
  says (`fuzzy::entries_said`, 2026-11-07: told every entry, the 4B wrote
  "ChatGPT" for "chat" and "API"; dictionary.md). **The dictation's language** (My languages,
  `dictation/languages.rs`, 2026-10-25): `languages::language_line` goes
  with the style's line and the names on screen just before the transcript
  (`cleanup_styles::before_transcript` → `llm::with_names_line`; never in the
  system prompt, which the server caches across dictations in different
  languages) — "This transcript is in French: write the cleaned text in
  French, never translating it." when Whisper heard it (or a language
  shortcut fixed it), "This transcript may be in English or French: keep it
  in the language it is in, never translating it." when My languages hold
  another than English and the model can't tell, none for English — and
  the spelling line goes only where the dictation may be English
  (`languages::cleanup_spelling`). The guardrails already said "do not
  translate", but the one-shots are English: measured in docs/model-evals.md
  "My languages". A language shortcut that translates sends the dictation
  to `transform` instead, with `languages::translate_instruction`.
  Records token/request usage (best-effort). `error_kind` (pure) gives a
  failed call's error in a few words for the log at `warn` (its kind,
  never its detail, which can quote the model's words). Also carries
  `EDIT_BASE_PROMPT` (edit/rewrite mode's guardrails; `rewrite_messages` /
  `rewrite_with_samples`),
  `TRANSFORM_BASE_PROMPT` + `transform_messages_with` / `transform` (**saved
  transforms**, `dictation/transforms.rs`, 2026-10-24: the text between `<text>`
  tags is something to transform, never a message to answer or carry out;
  keep names, numbers, dates, amounts, emails, links and code as written,
  the language and point of view, the layout; output only the result — the
  built-in's or the person's instruction in the user turn, restated after
  the text; temperature 0.2; on the Commands scope's model,
  `pipeline::commands_model`, never with the Commands prompt, which may
  answer questions; measured on the 4B and Groq's gpt-oss-20b in
  docs/model-evals.md "Saved transforms"); **the person's writing samples**
  (`ai/writing_samples.rs`, below) go at the end of the system prompt of a
  transform (`transform_with_samples`) and a rewrite (`rewrite_with_samples`:
  after the Commands body, before the selection); cleanup's
  (`cleanup_with_samples` / `cleanup_messages_full`) is for the model
  evaluations only — a dictation never sends them — and `None` is byte for
  byte the request before them (`llm::tests::the_cleanup_request_without_samples_is_byte_for_byte_as_before`
  pins cleanup's by its SHA-256), `NOTE_BASE_PROMPT` +
  `MEETING_NOTE_BASE_PROMPT` + `NOTE_DEFAULT_FRAGMENT` (note enhancement's
  guardrails/action fragment), `ACTION_PLAN_BASE_PROMPT` +
  `ACTION_PLAN_DEFAULT_FRAGMENT` (the Action Plan: never invent owners/dates)
  and `MEETING_DIGEST_PROMPT` (one rolling meeting digest), the notepad
  helpers' immutable prompts (`CATCH_UP_PROMPT`, `MEETING_ASK_PROMPT`,
  `MEETING_TITLE_PROMPT`, and the one-tap chips' `MEETING_QUESTIONS_PROMPT`,
  `MEETING_FOLLOW_UP_PROMPT` — a plain-text email, subject line first — and
  `MEETING_BLIND_SPOTS_PROMPT`; all: only what's in the meeting, never
  invent owners/dates/numbers, say when there's too little; every meeting
  prompt explains the labels through one `other_speakers!()` phrase: "You:",
  a speaker's name (a known person, like an attendee; the "Speakers besides
  You" line says which are guesses), "Speaker 2", or "Them"), `enhance_note`
  (the Actions-engine call, base prompt chosen by the caller), `chat` +
  `ChatOptions` (temperature, `max_tokens` — sent for local servers only —
  and timeout), `note_chat` (embedded per-note chat + Chat-surface turns), and
  `post_chat_message` (the tool-loop variant that returns the whole assistant
  message, incl. `tool_calls`, instead of just the text). **Chat's timeout**
  (`chat_timeout`: `note_chat`, `post_chat_message`): 20 s for a cloud model,
  **2 minutes for a server on this PC** (`LOCAL_CHAT_TIMEOUT`, a loopback
  address: Yap's sidecar, Ollama, LM Studio) — Qwen3.5-4B on a processor
  takes ~25 s a turn, its slowest calls ~50 s; the Chat view says "Still
  working (12 s): the model runs on this PC…" after 8 s. A **streamed**
  call's timeout is how long it may go quiet, not its length
  (`chat_client`, 2026-11-03: reqwest's `read_timeout`, at most
  `STREAM_LIMIT` in all; a stream cut off is asked again or said plainly,
  `CUT_OFF`: chat-and-assistant.md, Streaming). Dictation cleanup and
  edit mode keep 20 s and never wait (the raw text is the fallback). A
  rejected tool call's error starts `TOOL_CALL_REJECTED`
  (`tool_call_rejection`: Groq's `tool_use_failed`, llama.cpp's "Failed to
  parse input"), which the tool loop recovers from. **Rate limits**
  (HTTP 429 — Groq's free plan allows 8,000 tokens a minute, and one
  tool-using chat answer can ask for more): calls someone is happy to wait for
  (`chat` — meeting summaries/digests/assist —, `note_chat`,
  `post_chat_message` — the chat tool loop) wait as long as the provider says
  (`rate_limit_wait`: `retry-after-ms`, `retry-after`, else "try again in
  10.98s" in the body; else 2 s, 4 s…) and retry, ≤ 30 s in all
  (`RATE_LIMIT_PATIENCE`, ≤ 4 retries); dictation cleanup and edit mode never
  wait (the raw text is the fallback). A limit that outlasts that is said
  plainly (`rate_limited_error`: "Groq is limiting how much Yap can ask for
  right now (its free plan allows 8,000 tokens a minute). Try again in 11
  s."), not as a raw `HTTP 429` body. A 400 about the temperature is asked
  again without one; a 400 about reasoning on a call with tools (OpenAI's
  GPT-6 take tools on Chat Completions only with reasoning off) again with
  `reasoning_effort: "none"`, once each (also over Think's `"high"`, `chat/think.rs`). **A strict chat format**
  (2026-10-27, `format_refused`: llama.cpp's HTTP 400 "Unable to generate
  parser for this template", the template's `raise_exception(` in it —
  Gemma 2's "Conversation roles must alternate") is asked again with the
  conversation **taking turns** (`taking_turns`: two questions in a row — a
  question that got no answer, then the next; a question and Yap's note
  after it — one message, the earlier first, as are two answers in a row; a
  history that starts on an answer or a tool's result starts at its first
  question; tool calls and results never merged), and every later call to
  that model (by endpoint and id, `takes_turns_only`) goes so from the
  start. Every other model gets the conversation as Yap keeps it (the
  evaluations measured it so). Refused even then: said plainly by
  `chat/budget.rs` ("Gemma 2 9B IT couldn't read this conversation: its
  chat format is strict. Start a new chat, or pick another model."), never
  its Jinja. Before 2026-10-27 one failed question (Gemma 2 refused a
  replayed search) broke every later one in its chat, the unanswered
  question sending two in a row. **Too long for the model's
  context** (2026-10-25, `says_too_long`: llama.cpp's
  `exceed_context_size_error`, OpenAI's `context_length_exceeded`,
  Anthropic's "prompt is too long", Groq's "reduce the length of the
  messages", Gemini's "exceeds the maximum number of tokens", OpenRouter's
  "maximum context length", LM Studio's "context length of only") fails as
  `CONTEXT_EXCEEDED`, and a request bigger than the key's tokens a minute
  (Groq's 413) as `TOO_MUCH_A_MINUTE` + the limit: `chat/budget.rs` says
  them plainly ("That's too long for Qwen3.5 4B (64K context)."). A cloud
  provider's `x-ratelimit-limit-tokens` (its tokens a minute for this key)
  is heard on every answer (`budget::heard_rate_limit`): Chat keeps a
  request under it ([chat-and-assistant.md](./chat-and-assistant.md#long-chats-sized-to-the-model-2026-10-25)).
- **`ai/cleanup_styles.rs`** — **styles by app** (2026-10-24; Wispr Flow's
  styles per kind of app and OpenWhispr's Tone & Style of 21 Oct, by
  behaviour; the market analysis' #5): cleanup that sounds right where you
  are with no set-up. **Kinds** (`Category`): Email, Work chat, Personal
  chat, Documents, AI prompts, Terminal, Everything else, each with
  built-in apps (`built_in`: process names — Outlook/olk, Thunderbird,
  Slack, Teams, WhatsApp, Discord, Word, Notion, Obsidian, ChatGPT, Cursor,
  VS Code… — and the site a browser tab's title ends with: Gmail, Outlook,
  Google Docs, Claude, Gemini…; Edge's profile segment allowed for; a chat
  page `app_kind` found anywhere in the title), plus what `pasting/app_kind.rs`
  reads of a terminal (a shell → Terminal, a coding CLI → AI prompts).
  **Tones** (`Tone`; the kind's first): Email Formal / Casual / Natural,
  chats Formal / Casual / Very casual (Casual), Documents Formal / Casual /
  Natural, AI prompts **Literal** (no word added — no "please" —, nothing
  rephrased, never answered) / Natural, Terminal **Command** (the words as
  said, no capital or full stop) / **As said** (no AI), everything else
  Natural / Formal / Casual. **They're cleanup profiles**
  (`config::CleanupProfile` with a `category`, ids `style-<kind>`,
  `ensure_profiles` at load): the tone, the apps added (`apps`: a process
  or a site's name) and taken out (`removed_apps`), extra instructions
  (`prompt`, *added* to the main body), the per-profile model override.
  **Resolution** (`resolve(cfg, target)`, pure): the person's own app rule
  for the process first, exactly as before (`YapConfig::resolve_cleanup`;
  a rule naming a style takes that style), then "only clean up in apps with
  a rule", then the kind's style while "Match the style to the app" is on
  (`config.cleanup_styles`; `category`: a terminal's kind, then an app the
  person added — moved here from another kind —, then the built-ins kept),
  else the main body everywhere. **The prompt**: the system prompt is the
  guardrails + the main body (+ the kind's additions) as ever; the style is
  Yap's own line just before the transcript (`line`, joined with the names
  on screen by `before_transcript`; `llm::with_names_line` places it): "This
  dictation is the body of an email. Tone: formal and polished: complete
  sentences, full punctuation, contractions written out (I will, do not),
  no slang. The tone wins over keeping the speaker's wording; names keep
  their spelling. Never change the meaning or add information." So every
  kind shares the cached prompt and reads only its line and the transcript
  afresh (~60–90 tokens more; model-evals.md "Styles by app": told "an
  email", Qwen3.5-4B wrapped notes in "Dear Team," and "Best regards, [Your
  Name]"; "the body of an email" stopped it). **The answer fitted** (`fit`):
  an email's greeting and sign-off the model made up taken off
  (`drop_unsaid_frame`: short lines none of whose words were said, and
  "[placeholders]"), a command without the capital and full stop a
  sentence would have ("Git status." → "git status"), a casual message's
  semicolons as commas (not in code), a very casual one's final full stop.
  **Filler alone** ("um uh", `filler_only`) asks no model: nothing typed.
  **An answer that lost the dictation is thrown out** (`lost_the_dictation`,
  2026-10-28): under a quarter of the words said (12 or more, filler sounds
  left out, counted from the last "scratch that" / "start again" /
  "never mind"…) is an error, so the words as said go in. A tester's
  44-second dictation of 114 words came back from the on-device Qwen3.5-4B as
  "I've." and was pasted; the same request replayed nine times came back
  whole, and the lowest share of words any other cleanup in their history
  kept was 68%. **So is one that grew past it** (`grew_past_the_dictation`,
  2026-11-07): letters and digits (any script; spaces, punctuation and line
  breaks don't count) past 1.5 × the transcript's + 60, after `fit` and
  `lay_out` — an answer instead of a cleanup, a dictation written out twice,
  text it was given copied in. Over 9,812 cleanups in the model evaluations
  (every strength, lists, emails with their frames, the text before the
  cursor) the most any grew was 2.05 × a 62-letter dictation (+65, an
  email's made-up frame), none past 1.4 × + 50; `llm::error_kind` says "the
  answer added to the dictation".
  `clean` is the one call (`pipeline::clean_up`, the model evaluations'
  `--styles`). **The window** (`Target::of_window`, at the paste): its
  process (`text_injector::app_name_for`), the kind read at record start,
  and the title only for a browser (or Windows' app frame host), never
  logged; Run again goes by the history entry's process. Test mode:
  debug-only `e2e_style_app {exe, title}` stands in. **Existing installs**:
  `on_load` decides "Match the style to the app" once for a config without
  it — on, unless `pp_preset` isn't "default" (a body or preset they picked
  for everywhere stays exactly as it was); cleanup itself is never turned
  on. **The one offer** (`offer`, `cleanup_offer_status` / `_seen`;
  `lib/notices/cleanupOffer.js`): cleanup off, never shown, tips on, a PC the
  on-device model fits (`gpu::can_run_big_model`, or a model already here)
  and five dictations: one tip in the main window after a dictation or
  when it comes to the front, only while the notice policy has the person
  looking at it (`notice::window_view`, no dictation or meeting) — "Let Yap
  tidy what you say" with Set it up (Language Models) / Not now — and never
  again. A test run offers it only with `YAP_E2E_CLEANUP_OFFER`.
  `cleanup_styles_catalog` gives Settings the kinds, tones and built-in apps
  (with the names they go by). Unit-tested. **Since 25 Oct 2026** a `Plan`
  also carries the strength (`cleanup_strength.rs`) and `formatting`;
  `clean` puts Yap's lines before the transcript in this order — the
  style's (as written at the strength: `Plan::written_style`), the
  strength's, a list's or an email's layout (`cleanup_format.rs`), then the
  caller's (names on screen, the language, the text before the cursor) —
  and after `fit` lays out what the model kept on one line
  (`cleanup_format::lay_out`). `drop_unsaid_frame` also takes off a
  "Subject:" line nobody said. `main_plan` (Prompt Studio's Test: the main
  body at the strength set).
- **`ai/cleanup_strength.rs`** — **cleanup strength** (2026-10-25; Wispr
  Flow's None / Light / Medium / High, by behaviour; Language Models →
  Dictation cleanup → Strength): `Strength` from `config.cleanup_strength`
  ("light" | "medium" | "high"; missing or unknown = Medium, what cleanup
  always did — no migration: a config from before cleans exactly as it
  did). **None is cleanup off** (`post_process_enabled`): the page's None
  turns the switch off, a strength turns it on; "none" in the file asks no
  model (`resolve`). **Light** (`LIGHT_LINE`: filler sounds and stutters
  out, punctuation and capitals in, every other word as said, no grammar
  fixes), **Medium** (no line: the prompt byte for byte as before), **High**
  (`HIGH_LINE`: tighter and clearer, the meaning, every fact and the
  speaker's voice kept, nothing added). Yap's own layer, one line just
  before the transcript (`line`): the guardrails and the editable body
  (preset or the person's own) go as they are, and the cached system
  prompt is the same at every strength. **One setting for every app, not
  per style** (two knobs per kind of app would be fourteen choices for one
  idea): a tone that keeps the words (AI prompts' Literal, Terminal's
  Command) keeps them at any strength (no line), and at Light no tone
  rewords (`Strength::tone`: Formal, Casual, Very casual write as Natural;
  the kind's line stays). Light and High measured in docs/model-evals.md
  "Cleanup strength and formatting"; `YAP_EVAL_STRENGTH_LINES` tries other
  wordings in a run. "Use what I said" unchanged. The person's writing
  samples never go with a cleanup, High included (`ai/writing_samples.rs`,
  below: measured, they changed nothing). Unit-tested.
- **`ai/cleanup_format.rs`** — **smart formatting** (2026-10-25): a spoken
  list becomes a list, an email's greeting and sign-off go on lines of
  their own, a sentence that only mentions numbers stays a sentence.
  **Asked only when it's said** (`hint`, Medium and High, never for a
  Literal or Command tone): `list_cue` (pure) finds "bullet points" (or
  "bullets"), "number one … number two", ordinals in order with words
  between ("first … second", "firstly … secondly" — not after a determiner:
  "the first time", "our second office"), or "one … two … three" after a
  lead-in ("my priorities are one …", "three things one …") or at the start
  — never "I have one cat, two dogs"; `email_frame` (pure) finds a greeting
  at the start and a sign-off in the last words (with a name of up to two
  words) of a dictation into the Email kind. The lines (`NUMBERED_LINE`,
  `BULLETS_LINE`, `EMAIL_LINE`) go with the style's, before the transcript;
  each wording measured (a first one made the 4B list "one cat, two dogs",
  another write a "Subject:" line). **`lay_out`** (pure): where a line was
  sent and the model kept it on one line (the 1.5B mostly did), Rust lays
  out its own words — three or more spoken markers at clause starts as "1.
  …" lines (two stay as written), a greeting of up to four words ending in
  "," or "!" and a sign-off starting the last sentence on lines of their
  own. `YAP_EVAL_FORMAT_LINES` in a run. Unit-tested.
- **`ai/cleanup_context.rs`** — **the text before the cursor, for AI
  cleanup** (2026-10-25): a sentence or two of what's already written where
  the dictation goes, so cleanup continues it (no capital, no repeat of the
  words said again). **The read is `selection.rs`'s** — "Match the text
  around the cursor" (`smart_spacing`), at record start, in the background,
  never waited for (`selection::peek_before`, which leaves it for the paste's
  `take_context`); off, or a field Yap can't read, sends nothing; a live
  dictation's only (not Run again). **Bounded** (`bounded`, pure): the last
  two sentences, ≤ 240 characters cut at a word, never logged (a length at
  `debug`). **Where it may go** (`allowed`): a cleanup model on this PC
  (`net::place_of_url`: the on-device model, a server on `localhost`)
  whenever it's read; any other — a cloud provider, a server on the network
  — only with "Send the text before the cursor to cleanup" on
  (`cursor_context_cloud`, Privacy & data, off: it didn't help gpt-oss-20b in
  the evaluations, and it sends what's on screen). **The line** (`LINE`,
  last before the transcript): "Where it goes: right after "…" (already
  written, for reference only). Return only the cleaned transcript, so it
  follows on from that text." (2026-11-07; until then "…which is already
  written and is not part of your output. Clean up only the transcript…":
  63/72 → 66/72 on the cursor cases) — other wordings, "never to repeat"
  among them, made the 4B write the text out again (docs/model-evals.md);
  **`trim_echo`** (pure)
  takes off what a cleanup writes out again anyway (a bullet the cursor's
  line has; the text's last sentence of three words or more at the start),
  and **`copied`** (pure) throws out a cleanup that still has a run of 5
  words or more from the lines before the transcript (this one, the names
  on screen's…) that the transcript doesn't: the words as said go in
  (2026-11-07: the 4B wrote the field's earlier dictation out again after
  the new one).
  Such a cleanup's receipt is "AI cleanup + text before the cursor · Groq"
  (`net::Feature::DictationContext`); Privacy & data's "What leaves this PC"
  has its row. Measured: 4B 18/24 → 21/24, 1.5B 10/16 → 12/16 on the cursor
  cases. Test mode: debug-only `e2e_cursor_before {text}` stands in for the
  read. Unit-tested.
- **`ai/writing_samples.rs`** — **writing samples** (2026-10-29; Wispr
  Flow's "make it sound like you", by behaviour; Settings → Commands →
  Writing samples, `settings/WritingSamples.svelte`): a few things the person
  wrote, each tagged with the kind of app it suits (`Kind`: any app, Email,
  Chat — work and personal —, Documents), so rewrites sound like them.
  **Why on the Commands page**, not with Styles by app: the rewrites are what
  use them most (the rewrite shortcut, Commands by name, the transforms), and
  they work with cleanup off; Styles by app is cleanup's, shown only with it
  on. **What uses them**: the rewrite shortcut and Commands by name
  (`for_rewrite`, `pipeline/rewrite.rs`), the transforms that reword
  (`transform_uses`: Polish, Shorter and the person's own — never Fix
  grammar only, Translate or Make it a prompt) and a transform on every
  dictation (`transforms::run_on_dictation_in`); never a language
  shortcut's translation, an AI prompt or a terminal (`kind_for`: no
  samples). **Never AI cleanup**, High included: on the on-device 4B they
  changed none of ten High cleanups (cleanup keeps the words said, and the
  kind of app's tone wins, whatever the wording — the tone's sentence left
  out, 1 of 10), for ~160 more prompt tokens a dictation; `Plan::samples`
  stays for the evaluations to measure again (docs/model-evals.md
  "Writing samples"). **Which**: those tagged for the kind of app
  (`cleanup_styles::category` of the window), then the untagged ones, in the
  person's order (`pick`), each cut at a sentence to `SENT_PER_SAMPLE` (600
  characters), all under a **budget** (`budget_for`): `BUDGET_LOCAL` (800
  characters, ~200 tokens) for a model on this PC or the home network
  (`net::place_of_url`), `BUDGET_CLOUD` (2,000, ~500) for a cloud one —
  measured in docs/model-evals.md "Writing samples". **How they're sent**
  (`block`): one part at the end of the system prompt — "How the user
  writes…", each sample in `<example>` tags, then what to do with them (their
  words, sentence length, punctuation, greetings and sign-offs; never their
  facts, names or sentences; the task's own instructions first) — the stable
  prefix a server caches: the same for every call into one kind of app, a
  dictation's transcript and a selection always after it. None, and every
  prompt is byte for byte as before (`with_block`). **The file**:
  `writing_samples.json` (`Store`: the samples; unknown fields kept), read
  once, saved by `writing_samples_set` (tidied: trimmed, ≤ 12,
  each ≤ 1,500 characters, ids made; a failed save is the command's error,
  `saving.rs`); `writing_samples_get` for the page. **Privacy**: they leave
  this PC only inside a call to a cloud model, and the network receipts say
  so (`net::Feature::CommandsSamples` "Commands + your writing samples";
  a transform on every dictation's `DictationSamples` "Transform + your
  writing samples"); never logged
  (counts and lengths at `debug`). Reset app keeps them, Download your data
  has them. Unit-tested; e2e `writing-samples.spec.js`; the model
  evaluations' `--samples`.
- **`ai/local_llm.rs`** — the on-device AI cleanup sidecar: runs **llama.cpp's own
  server** (`llama-server.exe` from one of llama.cpp's official builds, kept up to
  date: `ai/local_llm/engine.rs` below; on an install from before 1 Nov 2026,
  **Mozilla llamafile** until it has moved — `engine::active`: the engine marker's
  build when its server is on disk, else `llm/llamafile.exe`) as a hidden child
  process on a free localhost port (`Engine::command`: a llama.cpp build in its own
  folder, NVIDIA's runtime first on its PATH, its card pinned with `--device` where
  a Vulkan build sees integrated graphics too), serving
  **Qwen2.5-1.5B-Instruct** (Q4_K_M GGUF) when no
  other is picked (`pp_local_model` empty = `MODEL_FILENAME`) — or a curated
  one (`CURATED_MODELS`, Q4_K_M GGUFs, SHA-pinned: since 2026-10-27
  **Qwen3.5 0.8B / 2B / 4B / 9B**, **Gemma 4 12B** and **Granite 4.1 8B**
  beside the bundled 1.5B, each bigger one the file the evaluations ran —
  none beat the 4B, which stays recommended (docs/model-evals.md, "The
  on-device download list"); the 2024 models left the list, and one already
  downloaded is served as the person's own GGUF)
  or **any user GGUF** dropped into `<data>/llm/` and picked via `pp_local_model`
  (Settings shows a model picker + "Open models folder"; switching restarts the
  sidecar). **Recommended for this PC** (`recommended_id`, from `ai/gpu.rs`):
  Qwen3.5-4B where the graphics card can run it (2026-10-18, the model
  evaluations: 94–98% of the scenarios at ~1.2 s a turn on an RTX 5070 Ti, with
  Chat's tools; the 1.5B 79–81% without them), else the 1.5B; existing installs
  keep their model and are offered the 4B **once** (`upgrade_offer`: capable
  card, on-device in use, on the 1.5B, not turned down —
  `config.local_llm_upgrade_dismissed`), never downloaded by itself. The
  curated entry's `default` = the bundled file, `needs_gpu` = only recommended
  with a card. **One server set-up** for the sidecar and the evaluations
  (`server_args`, on either engine: llama.cpp's server and llamafile's — llama.cpp
  b10441 underneath — take the same arguments but for how each is told to serve,
  `Runtime::flavour_args`: llamafile's `--server`, llama.cpp's `--no-webui
  --offline`; every flag checked against b11982's `--help` and run, 2026-11-01;
  `Plan.runtime` is a `Runtime` — `Llamafile(version)` | `LlamaCpp(build, variant)`
  — and MTP and the q8_0 KV cache need it on CUDA, `Runtime::cuda`):
  `-ngl 999 -c <context>`, **`--reasoning off`** (no thinking for
  any model: Qwen3.5 loops and times out with it on; Chat's **Think** turns it on for one
  request, without a restart: `chat_template_kwargs` and a 1,500-token
  `thinking_budget_tokens`, chat-and-assistant.md `chat/think.rs`), **`--cache-ram 0`**
  (llama.cpp's host prompt cache off: with it on, a dictation's cleanup right
  after a Chat turn took 1.3–1.4 s instead of 0.13 s on the 1.5B — the slow
  copy back of the saved prompt — and a 4B server peaked at 6.7 GB on the
  processor; Qwen3.5-4B's peak is now 3.85 GB there, 3.7 GB on the card),
  `--ctx-checkpoints 4` (Qwen3.5's recurrent-state checkpoints, ~50 MB each),
  `-lv 4` (the log names the layers that went to the GPU), `--alias <model
  id>` (2026-11-03: where the server lists its model it names it, never
  its path; its key is in its environment, never an argument —
  `local_llm/key.rs` below), MTP as the
  `Plan` says (below), and **a q8_0 KV cache only where it helps**
  (`-ctk q8_0 -ctv q8_0`, `Plan.kv_q8`, `kv_q8_helps`, 2026-10-25): on an
  NVIDIA card, llamafile 0.10.6+, a context of 32K or more
  (`KV_Q8_FROM`) whose f16 cache wouldn't fit in `card_room` while the
  q8_0 one would — then MTP is decided with it. Measured on the 4B
  (model-evals.md "KV cache q8_0"): −401 MiB on the card at 32K, −753 at
  64K, the suite 160/164 both ways and a 24K / 56K needle test 6/6 both
  ways, but Chat's generation ~25 % slower (150 → 110 tokens/s), so never
  where the f16 cache fits; never on the processor. **On the processor,
  two slots with a cache each** (`Plan.per_slot`, `per_slot_caches`,
  2026-10-25: `-np 2 --no-kv-unified`, `-c` twice the context so each slot
  has all of it; no card, 0.10.6+; and `--checkpoint-min-step 0`,
  `CHECKPOINT_MIN_STEP_PER_SLOT`, 2026-10-30: llama.cpp's default 8,192
  erased the checkpoint at an earlier request's last message once a prompt
  passed ~516 tokens, so a slot's next request re-read all but ~130 tokens —
  a Shorter transform's prompt 4.9 → 1.6 s, a cleanup after an identical
  dictation 11.3 → 0.6 s on Qwen3.5-4B; not on a card, where a long chat's
  first load took 1.2 → 3.4 s with it): with llama.cpp's one cache shared by 4
  slots a dictation's cleanup right after a long Chat turn ran 2.0–2.6×
  slower (its attention over the chat's cells; once its prompt cleared for
  the room) — now 1.0–1.1×, for the KV once more (the 1.5B +224 MiB at 8K;
  model-evals.md "The processor tier"). `log_facts` reads `n_ctx_seq`, the
  context a request has, so the context view and the restart rule see 8K,
  not 16K; `gguf::estimate` counts the second cache (`Server.per_slot`)
  and, on the processor, the context checkpoints (2026-10-30,
  `Shape::checkpoint_bytes`: up to 4 a slot, each one slot's recurrent state
  from the header's `ssm.*` sizes — the 4B's 50.25 MiB, +402 MiB for two
  slots; the server keeps them in the PC's memory, so a card's estimate
  leaves them out).
  **Qwen3.5-9B** was
  measured again for 16 GB cards and isn't offered: no better than the 4B
  (model-evals.md "Qwen3.5-9B against the 4B"). **llamafile, the runtime before
  llama.cpp's own builds** (what follows is its own machinery, kept for installs
  that haven't moved yet and as their way back; once a llama.cpp build is active
  the llamafile upgrade below never runs, and llamafile's files are never touched:
  an older Yap sharing the folder still runs them). **The runtime** (2026-10-22): **llamafile 0.10.6**
  (`RUNTIME_VERSION`, mozilla-ai's release, SHA-256 `d579f61d…` = GitHub's
  digest; over 0.10.3: CUDA graphs, a ~430 MiB smaller compute buffer, the
  June tool-call grammar fixes, checkpoints at every user message, the
  Qwen3.5 MTP fix — the 4B's cleanup ~300 → ~245 ms, generation ~116 → ~167
  tokens/s, greedy outputs identical, model-evals.md "Local AI, phase 1").
  **Which one is on disk**: the marker beside it (`llamafile.json`:
  `{version, sha256, unproven, refused, failures, mtp_failed}`, written
  after every verified download — no hashing 368 MB at a start — and, from
  a fresh install's download, *before* the runtime is renamed into place
  (`ensure_runtime` downloads to `llamafile-next.exe`, writes the marker,
  then renames: never a runtime its marker doesn't describe)); no marker =
  the 0.10.3 every install had (`UNMARKED_RUNTIME_VERSION`). What the PC
  has learnt — runtimes `refused`, per-SHA `failures` (sessions it failed
  in), `mtp_failed` — is carried from marker to marker through every swap
  and roll-back (`Marker::learnt_from`). **The upgrade for existing
  installs** (`spawn_runtime_upgrade`, from setup; `ensure_runtime` only ever
  downloads a missing runtime): 90 s after launch, then after a "not now"
  every 30 min doubling to 6 h — only while the on-device model is in use,
  not on a metered connection (`updates::metered`), not in a session that
  already went back from it, and under **the upgrade lock**
  (`UpgradeLock`: `llm/llamafile-upgrade.lock` with the owner's pid, taken
  create-new, a stale one taken over — a dev build and the installed app
  share `llm/`; `ensure_runtime` and the launch tidy take it too) — it
  checks the disk has room (`free_bytes`, `UPGRADE_ROOM` = twice the
  runtime + 256 MB, else "not now"), downloads the pinned runtime to
  `llamafile-next.exe` (verified; a file already there is hashed once a
  session and kept if whole, `next_is_whole`; a failed download is retried
  later, the reason logged), checks it runs here (`probe_version`:
  `--version`, ≤ 60 s, tied to Yap's job object; one that won't, in
  `REFUSE_AFTER` = 3 sessions, is refused and deleted), then waits until
  nothing uses the sidecar — no dictation (`pipeline_busy`), no request or
  run of requests to it under way (`requests_in_flight`, `hold` below), no
  meeting recording or action plan being written
  (`meeting_end::summary_running`) — idle, then idle again after a settle,
  never under a context restart (it takes `RESTARTING` only once idle, so a
  context change isn't held up), and under the start lock, **looking again
  once it has it** (`idle_with_locks`: a Chat turn that began while it
  waited for the lock sends it back to waiting), and never while another
  Yap's sidecar runs from this folder (`procs::sidecars_of_other_yaps`).
  Then stop, `swap_in` (the old runtime and marker kept as
  `llamafile-previous.*`, the new marker `unproven`), start. **Never without
  a working sidecar** (`start_locked` → `start_with`, unit-tested with a
  scripted start): a start Yap itself cut short — any `stop()` meanwhile
  (a model switch, the Stop button, a restart; `STOP_GEN`) or quitting
  (`shutdown`, from `shutdown_cleanup`: nothing starts or rolls back after
  it) — or one missing a file is just an error, never retried and never
  blamed on the runtime (`Failure::counts`). Any other failure (it wouldn't
  launch, exited, or wasn't ready in 120 s) is **tried once more without
  MTP and with 300 s to load** (`Attempt::AGAIN`: a new runtime's first run
  under an antivirus can take minutes). Still down, on an unproven runtime,
  once a session: the failure is counted for that SHA (`count_failure`),
  the previous runtime goes back (`roll_back`; with the new one already
  gone it just renames the previous one back, leaving `llamafile-next.exe`
  alone) and starts; the new one waits as `llamafile-next.exe` for the next
  launch, and is **refused only after 3 sessions** like this (then
  deleted, never downloaded again). If the previous one won't start
  either, the fault isn't the runtime's: the new one goes back in
  (`swap_in`) and the failure is taken back (`uncount_failure`). Once the
  new one has started, the previous one is deleted (`prove`). **At
  launch** (`tidy_runtime`, before the launch gate): a missing
  `llamafile.exe` comes back from `llamafile-previous.exe`, else from a
  whole `llamafile-next.exe` of the pinned runtime (`repair_runtime`); a
  previous one kept past a proven start goes, and so does a cut-off
  `llamafile-next.partial`. Every download (`download_for`) goes through
  the shared helper (app-shell.md `shell/download.rs`, 2026-10-25): it
  checks the disk for its size + 64 MB first, gives up on a server or
  connection silent for 60 s, **resumes** a partial file an earlier try
  left (before, any error deleted it), shows its speed and time left, and
  **Cancel** (`download_cancel {id: "llm:<curated id>"}`, the runtime's
  download part of the model's install) stops it and deletes the partial
  file; a bad hash deletes it too (`verify_into`). All of them run as `llamafile.exe`, so the orphan sweep
  (`procs::orphaned`) and the job object cover them. **Yap's own restart**
  (a new context, the swap; `PlannedRestart`): meanwhile an on-device
  cleanup gets no endpoint (`effective_endpoint_for`: the raw text; edit
  mode says the model is restarting) — never the cloud endpoint someone on
  the on-device model left configured. A test run upgrades only from its
  stand-in (`YAP_LLM_SOURCES`) when `YAP_E2E_RUNTIME_UPGRADE_MS` says when
  to look, and never runs either (e2e `local-runtime-upgrade.spec.js`).
  `local_llm_status().runtime`: `{version, pinned, upgrade: "downloading" |
  "waiting" | "", mtp}`.
  **The warm-up** (`warm_up`): right after the sidecar is healthy, one
  cleanup-sized request — the cleanup's own messages as Settings has them,
  a made-up dictation, ≤ 8 tokens — in the background, not through `ai/llm.rs`
  and not counted; the first real request (`request_starting`) cancels it.
  It leaves the cleanup's prompt in a slot: the first dictation after
  loading 599 → ~275 ms on the 4B. **Requests in flight**
  (`request_starting`, called by `llm::post_chat_request` for a call to the
  sidecar's URL): counted while they run — the context restart now waits
  for a Chat answer or digest too (`sidecar_busy`), not only a dictation.
  **Runs of requests** (`hold`, a guard for the whole run when the endpoint
  is the sidecar): a Chat turn (`turn::chat_turn` — the Voice
  Assistant's too — between its tool steps, web searches and page reads),
  the Notes Ask bar, an action plan (`run_enhance`: digests to the final
  call), `meeting_summary::prepare_final` and each live digest, and a
  notepad helper while it waits out a dictation (`chat_beside_dictation`):
  a context restart or a runtime swap never cuts one off between requests.
  **MTP** (`plan_for` → `Plan { runtime, mtp }`, `server_args` adds
  `--spec-type draft-mtp`): on llamafile 0.10.6 or newer or a llama.cpp
  CUDA build, on an **NVIDIA** card (`mtp_allowed`: measured on CUDA only; a
  Vulkan build — AMD, Intel, or an NVIDIA card on it — unmeasured, so never;
  a llama.cpp build's failures are kept in the engine's marker as
  `"b11982-cuda13/<GGUF file>"`, llamafile's in its own), for a model with the prediction layer
  (Qwen3.5), all on the card with room for it at that context (`mtp_fits`:
  `gguf::estimate` with MTP ≤ `card_room`), `config.local_mtp` on
  (default; no switch in Settings), and not failed here: a start with MTP
  that fails but comes up without it, or a model that lands (partly) on
  the processor with it (started again without it), puts
  `"<runtime>/<GGUF file>"` in the marker's `mtp_failed` (`mtp_key`) —
  never with MTP again on that runtime; a newer runtime tries again. Never
  on the processor. **The context**
  (2026-10-20, the maintainer's ask: "show what context length it's set at, and let
  me set my own"): `config.local_context` (tokens; 0 = `DEFAULT_CONTEXT`
  8,192, today's behaviour) — one setting for the one sidecar, so Chat,
  dictation cleanup and the meeting notes share it. `effective_context`
  clamps it to `MIN_CONTEXT` 2,048..`MAX_CONTEXT` 1M and to what the model
  was trained for (`trained_context`: its GGUF header's `context_length`,
  `ai/gguf.rs`, else `CuratedLlm::max_context`; a stale file falls back);
  `context_for(cfg)` is what `start()` passes as `-c`. `context_choices`:
  4K/8K/16K/32K/64K/128K/256K up to the model's maximum (and that maximum
  when it falls between; an unknown one stops at 32K). `context_view(cfg)`
  (`local_llm_status().context`): `{tokens, configured, default, max,
  serving, pending, onCard, roomMib, usedMib, options: [{tokens,
  estimateMib, over}]}` — each choice's memory from `gguf::estimate`, `over`
  = more than `card_room` (the card's own memory − 2 GiB for Windows, the
  apps on screen and Yap's speech model; never llama.cpp's "MiB free",
  which on WDDM ignores every other process), `usedMib` what the running
  server's log says (`log_facts`: the non-host `… buffer size` lines,
  `n_ctx`). **`local_llm_set_context {tokens}`** (the pickers): saved
  through `settings::change_config` (in one step, applied as a Settings save; 0 for the default), `yap-local-context-changed
  {localContext}` (Settings' copy adopts it, or its wholesale auto-save
  would put the old one back), and a running sidecar on another context
  is restarted by **`restart_when_idle`**: never while a dictation records
  or is being transcribed/cleaned up (`pipeline_busy`: `dictation_state` +
  `Pipeline::is_busy`; pure `restart_step` Wait/Settle/Go, a 1.5 s settle
  after one), a newer change supersedes an older one (generation), one
  restart at a time, then `yap-local-llm-changed {pending, error}` (the
  pickers re-read) + `yap-chat-model-changed {scope: null}` (the chips).
  `start()` itself is one at a time now (`STARTING`: a second caller finds
  it up instead of spawning another llamafile). `budget_factor(context)` /
  `budget_factor_for(provider, base_url)`: whole 8K multiples, 1–4, only
  when the call really goes to the running sidecar — what the meeting
  notes' final call and the Notes view's Ask grow by (`meetings/meeting_summary.rs`
  `FinalBudget`, `notes::commands::note_ask`). Debug-only
  `e2e_local_llm_serving {…, ctx, usedMib}` (the stand-in "restarts" on a
  change, `e2e_local_llm_restarts`) and `e2e_local_llm_busy {busy}` (a
  dictation as the guard sees one, no mic). **Model id** = the
  GGUF's name (`model_id`, `active_model_id`: `Qwen_Qwen3.5-4B-Q4_K_M`),
  what `effective_endpoint` sends and `tools::supports_tools` reads (it used to
  be `qwen2.5-1.5b-instruct` whatever ran, so a bigger model never got its
  tools). **Where it runs** (`placement`, from the log's `load_tensors:
  offloaded N/M layers to GPU`, read once ready; no line once loaded = the
  processor): a big model left on the processor (`slow_on_this_pc`) makes
  Settings say "Runs on the processor on this PC: answers will be slow — Qwen2.5
  1.5B is quicker" (+ Use / Download it). Debug-only `e2e_local_llm_serving`
  stages a running model's placement for the e2e suite (display only). Owns
  install (runtime + model download, SHA-256 verified, per-stage
  progress events naming the curated `model`, which the Chat model picker
  draws in that model's row; a test run downloads only from 127.0.0.1 — the
  debug-only `YAP_LLM_SOURCES` stand-ins, `{"runtime"|"<curated id>": {url,
  sha256}}` — and refuses anything else), process lifecycle (spawn with its
  key/health-wait/kill + orphan cleanup at
  startup — only sidecars whose parent Yap is gone, via `shell/procs.rs`, so a dev build
  never kills the installed app's; and **the model never outlives Yap**:
  `procs::end_with_yap` puts the sidecar — like the speaker engine
  (`yap.exe diarize`) and eSpeak — in a kill-on-close job object only Yap
  holds, so Windows ends it however Yap ends: Quit, a crash, End task, the
  updater's exit; never used for apps Yap opens, a link's browser or an
  installer), and `effective_endpoint()` which routes `ai/llm.rs` to the sidecar when
  provider = "ondevice": its address while it's up, the **stand-by address**
  while it's installed but not loaded (below), else (not installed) the
  configured endpoint. **What its chat format takes** (2026-10-27): once the
  sidecar is up, `hear_template_tools` asks llama.cpp's `GET /props`
  (`chat_template_caps.supports_tools`) and keeps the answer by model id
  (`template_takes_tools`, kept after the sidecar stops): a model whose
  format has no place for tools — the engine then never shows it Chat's
  tools — gets none (`tools::supports_tools`), whatever its size says.
  Gemma 2 9B's says `supports_tools: false`, `supports_system_role: false`
  (its system prompt is folded into the first question); Qwen's and
  Llama 3.1's take tools.
- **`ai/local_llm/key.rs`** — **a key for every llama.cpp server Yap starts**
  (2026-11-03, the 2 Nov security review's finding 3): the sidecar, the
  search model's embedder (search.md), an engine trial, a model
  evaluation's and `yap.exe bench`'s. They listen on 127.0.0.1, which
  anything on this PC reaches — a web page too: the page beside the chat
  refuses requests to this PC, but WebView2 never shows Yap a WebSocket,
  and a public name can resolve to 127.0.0.1 (DNS rebinding;
  chat-and-assistant.md). llama.cpp's CORS is no guard: it echoes any
  page's `Origin` with credentials allowed and answers every preflight
  (`--cors-origins` defaults to `*`). So:
  - **a key per launch** (`issue(port)` → `Issued`, forgotten when it drops
    with the server): 32 bytes from the operating system's random numbers
    (`getrandom`), 64 hex characters; never in a file, config.json, a log,
    the network receipts or the frontend (llama.cpp's own log shows its last
    four characters only: `api_keys: ****1a2b`);
  - **handed over in the environment**, `LLAMA_API_KEY` (`Key::give`;
    llama.cpp's `--api-key`, "(env: LLAMA_API_KEY)" in b11985's `--help`;
    llamafile 0.10.6 reads it too), never `--api-key` on the command line
    (every program on the PC can list it) nor an `--api-key-file` on disk; an
    inherited `LLAMA_ARG_API_KEY_FILE` is taken off;
  - **sent by every Yap caller** as `Authorization: Bearer` (`authorize`, by
    the request's address: a port Yap holds a key for on `http://127.0.0.1`,
    so a server of the person's own or the cloud never sees it): `llm.rs`'s
    one chat call (when the endpoint has no key of its own), the warm-up,
    `/props` (`hear_template_tools`), Chat's `/tokenize` (`chat/budget.rs`),
    the embeddings, the trial's requests, the evaluations' (`eval/serving.rs`
    too) and the bench's. The e2e suite's stand-in sidecar gets a key too
    (its fake takes any header);
  - **what stays open**: `/health` and `/v1/health` (the readiness checks
    need nothing else); everything else is a 401 without the key or with a
    wrong one, whatever the `Host` header says (b11985: `/props`, `/slots`,
    `/models`, `/tokenize`, chat, embeddings). llamafile 0.10.6 also lists
    its model at `/v1/models` without the key, so every server starts with
    `--alias` (the model id, or the search model's tag): its name, never its
    path, which holds the Windows user name;
  - **checked at each start** (`refuses_without`: `GET /props` without the
    key must be a 401): the sidecar and the embedder say so in the log when
    it isn't; the trial refuses that build.

  Tried against copies of the maintainer's llama.cpp b11985 (CUDA 13) and
  llamafile 0.10.6 with Qwen2.5-1.5B (`key::tests::real_server_refuses_requests_without_its_key`,
  `#[ignore]`d: the sidecar's and the embedder's set-ups, a 401 for every
  keyed path, `/health` open, the warm-up, a cleanup and the embedder's
  vectors through Yap's own code), and the b11985 copy through a real
  engine trial (`trial::tests::real_trial_of_a_build`: it refused a request
  without its key, answered, called the tool). Not tried: llamafile 0.10.3
  (an unmarked runtime, upgraded in the background; a server that ignored
  its key would still answer Yap, and the start's check would say so).
- **`ai/local_llm/engine.rs`** (+ `engine/`: `feed`, `trial`, `updater`) —
  **llama.cpp's own builds, kept up to date** (2026-11-01, the maintainer's ask:
  "detect newer engine builds, install them, prove they work, roll back on
  trouble", after Jan's Auto Update Engine and LM Studio's runtime packs). The
  sidecar ran llamafile pinned at build time: it moved only with a Yap release,
  and llamafile lags llama.cpp by months (0.10.6, 15 Sep, = llama.cpp b10441;
  llama.cpp b11960 on 28 Oct) while new models need new builds (Spark-X2.6's
  `spark2_5` needs b10828+). Now: **ggml-org/llama.cpp's official Windows
  releases** — `llama-b<N>-bin-win-<variant>-x64.zip` (the server, its DLLs —
  ggml's backends, the CPU code for every instruction set picked at run time —
  and licences; the other tools left out, `unpacked_name`; streamed, folders
  flattened, two files of one name refused, `enclosed_name` never out of the
  folder) unpacked into `llm/engine/<build>-<variant>/` through a `.unpacking`
  folder, NVIDIA's `cudart-llama-bin-win-cuda-<v>-x64.zip` once for every build
  that uses it (`cudart-<sha12>/`, put first on the server's PATH). **The
  marker** `llm/engine/engine.json` (`Marker`, config-and-data.md): never
  config.json (an older Yap's save would drop a new field), and llamafile's
  files never touched. **Which build for a PC** (`candidates`, `Hardware` from
  `gpu::this_pc` + `nvidia-smi --query-gpu=compute_cap,driver_version`):
  on an **NVIDIA card CUDA** (`cuda_flavour`: CUDA 13 from a Turing card and
  driver 580, or driver 590+ with the card unknown; else CUDA 12 from driver
  528; ~165 MB + NVIDIA's ~420 MB runtime, its size said in Settings while it
  downloads) **and Vulkan, both measured, CUDA kept when it's at least as
  fast** (`trial::choose`; 2026-11-01, the maintainer: on the RTX 5070 Ti CUDA
  generates 269 tokens/s to Vulkan's 150 and cleans up a dictation in 132 ms to
  Vulkan's 200 — llamafile's was 156 — and dictation comes first); **Vulkan**
  on any other card with memory of its own (3 GiB+; 37 MB), and the fallback
  when CUDA won't start or pass (a move from CUDA to Vulkan passes the trial at
  `trial::GOOD_ENOUGH_TPS`, 100 tokens/s); the processor's build without a
  card or with integrated graphics. The choice is kept per card
  and driver (`Pick`, `stt_speed::pc_print`; a new driver tries again,
  `to_try`). A Vulkan build that sees integrated graphics beside the card is
  pinned to the card (`--list-devices`, `parse_devices`, `device_for`: by
  DXGI's name — llama.cpp would split the model across both). **The floor**
  (`FLOOR` = **b11982**, 29 Oct 2026, every variant's files pinned with GitHub's
  SHA-256, `floor_assets`): what a first install downloads
  (`models::ensure_runtime`: no engine at all → the floor's first build for this
  PC not refused here, CUDA on NVIDIA, its progress the "runtime" stage,
  "Downloading the llama.cpp engine"; unproven until its first start) and what
  Yap runs offline; verified on the RTX 5070 Ti with Qwen3.5-4B (model-evals.md
  "llama.cpp's own builds"). **A fresh install's floor that won't start** (the
  gap closed 2026-11-01: an old driver, an odd card), with nothing to go back
  to — no build before it, no llamafile: its first start fails twice
  (`start_with` → `engine_way_back`, never held to once a session), so it's
  **refused** at once (`first_start_failed`; unless its log blames the model —
  `log_blames_model`: "error loading model", "unknown model architecture" — then
  nothing is refused) and **the floor's next build is set up**
  (`updater::after_failed_start` → `rescue`; the start says "…setting up another
  build of it. Try again in a minute.", `RESCUING_NOW`): in `floor_order` —
  **CUDA → Vulkan → the processor's on NVIDIA, Vulkan → the processor's
  elsewhere** — each downloaded and checked like any build, tried out of sight
  with the person's model under the start lock (passing = it starts and answers,
  `trial::rescue_ok`: with nothing to compare against, its tools and projector
  are the model's), kept (current, unproven, this card's pick; Settings: "CUDA
  didn't start here, so the engine runs on Vulkan", `Event::Fallback`) or
  refused (`engine.json`, so it's never downloaded again at the next launch)
  and the next tried. A download that fails stops it, refusing nothing (tried
  again at the next failed start or launch: `rescue_if_due`). **None runs**
  (`none_runs`: every one of the floor's builds for this PC refused, nothing
  current, no llamafile): said plainly — every start says `NO_ENGINE` ("The
  on-device engine won't start on this PC: Yap tried every build of llama.cpp
  it has for it. Settings → Language Models has Try again."), so do Load now,
  Chat's "didn't start" and the dictation's one notice (`on_demand::
  say_failed_once`), and Settings' engine line ("Engine: none runs on this PC",
  "None of the engine's builds started on this PC (CUDA, Vulkan and the
  processor's build tried). Updating the graphics driver may help; then Try
  again.", `Event::NoEngine` with the adapters' print); the model still counts
  as installed (`engine_installed`), so nothing offers a download again. **Try
  again** (Check now's place then; `retry_floor`) forgets the floor's refusals
  and goes through them again; a new graphics driver does at launch
  (`new_driver_retry`). A build that started here before and fails later isn't
  refused this way (a passing fault — a game on the card — mustn't move a PC to
  the processor for good). **The feed** (`feed.rs`, `Purpose::EngineCheck`):
  GitHub's releases API, 50 a page newest first until past the cooling-off;
  `b<N>` tags only (llama.cpp tags `v0.8.0` too), never drafts, every asset with
  its `sha256:` digest and uploaded whole, plain names (`parse_page`); a
  variant's files by name, CUDA by major version with its own runtime's minor
  (`pick_assets`); **only a build 3 days old** (`COOLING_OFF`, `eligible`), not
  refused here. Files come from `github.com/ggml-org/llama.cpp/releases/download/
  <tag>/<name>`, built from the tag and the name, never an address from the feed
  (`asset_url`), through `shell/download.rs` (resume, the disk checked, Cancel
  `llm-engine`; `Download::OnDeviceEngine`), each checked against its digest
  (`models::verify_into`) before it's unpacked: nothing unverified runs. **The
  updater** (`updater.rs`, from setup, `spawn`): from 2 min after launch, every
  30 min it sees whether a look is due — once a day (`CHECK_EVERY`), only while
  the on-device model is in use, not on a metered connection, not with "Keep
  the engine up to date" off (`Marker::paused`) — and takes a newer build
  (`take_due`) by itself at most **once a week** (`UPDATE_EVERY`; from llamafile
  at once; **Check now** at once). A variant at a time (the lock `engine/
  engine-update.lock`, `UpgradeLock::take_named`, held for the download, let
  go meanwhile): downloaded, then **tried** at an idle moment (`trial_idle`: no
  dictation, request, meeting or summary, nobody waiting to start the model —
  `wants_start`, a counter in `local_llm::start` — the model not loaded or
  unused 5 min, `IDLE_BEFORE_TRIAL`; under the restart and start locks,
  `runtime::idle_with_locks`; a loaded idle model is stopped for it and loaded
  again after). **The trial** (`trial.rs`, `Purpose::EngineTrial`): the engine
  running now, then the new build, each started as the sidecar would start it
  (the person's own model and context, `server_args` and `plan_for`, a key of
  its own: `local_llm/key.rs`; log `engine/trial.log`) and asked, with
  made-up words only: an answer — and a request without its key refused
  (`key::refuses_without`; a build that answers anyway counts as not
  answering, so one that stopped reading its key is never switched to) —, a valid
  tool call when its chat format takes tools (`/props`, `valid_tool_call`), its
  generation speed (llama.cpp's `timings.predicted_per_second`, the better of
  two 128-token runs), and with "Read screenshots on this PC" on, its projector
  (`vision::for_trial`) loading and a made-up 64-px picture answered. It gives
  way at once when the person needs the model (`abort`: the request dropped,
  the server ended) and is tried again at the next idle moment. **The verdict**
  (`verdict`, pure): it must start, answer, call the tool and read the picture
  wherever the current one does, and generate at `MIN_SPEED` (85 %) of the
  current engine's speed or better — relative to the engine running, so a real
  regression is caught, with room for run-to-run noise; a move from CUDA to
  the Vulkan build passes at `GOOD_ENOUGH_TPS` too. **The switch** (`decide`,
  `switch_to`): the build before kept (Go back), the new one `unproven` until
  the sidecar starts on it; a running model moves once nothing uses it
  (`restart_when_idle`). Fails: **refused** (`Marker::refused`, never taken
  again; its folder goes) and Settings says why. **A first real start that
  fails** (`local_llm::start_with` → `engine_way_back`, unit-tested): the build
  before (or llamafile) goes back in and starts, the failure counted, the new
  one kept as `retry` and put back at the next launch (`retry_at_launch`,
  `tidy_at_launch`) until `REFUSE_AFTER` (3) sessions; the one before won't
  start either: the new one stays, uncounted; nowhere to go back to: the floor's
  fallback above. **At launch** (`tidy_at_launch`, then `rescue_if_due`): the
  retry, a new driver's retry, folders no build kept needs (`tidy`), and a
  fallback cut short carried on. **Settings** (Language Models → the on-device models,
  `LocalEngine.svelte`): "Engine: llama.cpp b11982 (CUDA) · up to date" (or
  "checking…", "downloading b11990 (590 MB) · 40%", "b11990 is ready to try · waiting
  until the model isn't in use", "trying b11990 with your model…"), **Check
  now**, "Keep the engine up to date" (on), what happened last (`event_line`:
  "Updated to b11980 · faster on your card", "b11990 didn't start here: staying
  on b11960", "b11990 was slower here (72% of the speed): staying on b11960",
  "Went back to b11960 · b11980 won't be installed again"), **Go back to
  b11960** (or to llamafile; the build left refused) — `local_llm_status().engine
  State`, then `yap-local-engine` as a check moves along; commands
  `local_engine_status`, `local_engine_check`, `local_engine_set_auto {on}`,
  `local_engine_go_back`. Orphan sweep: a `llama-server.exe` is Yap's only inside
  `llm/engine/` (`procs::orphaned_under`); another Yap's server there counts for
  `other_yaps_sidecar`. Test mode: the stand-in feed `YAP_E2E_ENGINE_FEED` (its
  files at `/download/<tag>/<name>`), scripted trials `YAP_E2E_ENGINE_TRIAL`
  (nothing runs), the background look only with `YAP_E2E_ENGINE_CHECK_MS`; the
  floor's stand-ins in `YAP_LLM_SOURCES` ("engine", "cudart"). Unit-tested: build
  tags and order, the variants and their files, CUDA's flavour, the cooling-off,
  digests, the unpacking, device pinning, the verdict and the choice (CUDA when
  at least as fast), the decisions, switch / roll-back / put-back / go-back /
  retry, the floor's fallback order and refusals (`first_start_failed`,
  `next_floor`, `none_runs`, `new_driver_retry`, a log that blames the model,
  never once a session), Settings' words; e2e `local-engine.spec.js` (Check
  now, Go back, a refused build) and `local-engine-floor.spec.js` (a fresh
  install's CUDA build that won't start → Vulkan's trial fails → the
  processor's runs; then none runs, said plainly, and Try again).
- **`ai/local_llm/on_demand.rs`** — **the on-device model loaded when it's
  needed, its memory freed when it's idle** (2026-10-25; before, it started
  at launch whenever cleanup was routed to it: ~10 GB in use as soon as Yap
  opened, on an evening of cloud models only). **At launch** (and after a
  Settings save, `autostart_if_configured`) it starts only with **"Load the
  on-device model when Yap starts"** on (`local_model_at_launch`, off by
  default; Settings → Language Models, the on-device models) and something
  really routed to it (`launch_gate`, `in_use`: the cleanup's provider,
  which Chat, Commands and the meeting notes inherit; a profile an app's
  rule or a style reaches, with cleanup on; a scope switched on with it — a
  stale profile nothing reaches doesn't count). Explicit actions still
  start it: a download, choosing a model (`onLocalModelChange` now always
  starts it), Settings' **Load now** (`local_llm_start`, which also forgets
  a recent failure), the Chat picker switching to On this PC. **On first
  use**: a call routed on-device while the model isn't loaded resolves to
  `STANDBY_URL` (`http://127.0.0.1:0/v1`, a port nothing listens on;
  `effective_endpoint_for`, not in unit tests), so display code still names
  it On this PC with its model id (the tools check, the chips, a brief's
  "isn't running" plain path, memory's "stopped: put off") — and **the one
  chat call every other goes through** (`llm::post_chat_request`) loads it
  there and waits (`wake`, `LOAD_WAIT` 30 s; the start carries on past it,
  so the next call finds it up) before it sends, to the sidecar's real
  address: no path that reaches the model can miss it. A URL the sidecar
  served on before but not now (`was_ours`: every port served this session,
  `served_on`) goes the same way — a call resolved just before the model
  was freed or restarted is never sent to whatever listens there now.
  `hold` counts the stand-by address as busy (a run about to load it isn't
  freed meanwhile); `budget_factor_for` gives it the context it will load
  with. A start that fails isn't tried again on demand for `RETRY_AFTER`
  (60 s); a dictation that went without it says so **once**
  (`say_failed_once`: "The on-device AI model didn't start", Open
  settings; alert kind `aiModelFailed`, ten-minute cooldown) until it
  loads again. **Early**: a dictation starting to record
  (`pipeline/recording.rs` `load_ai_early`, at the end of `capture_target`)
  starts loading it in the background when this dictation will use it
  (`dictation_uses_it`: cleanup on and the app's routed plan on it — its
  rule's or style's profile, else the cleanup's provider — or a translating
  language shortcut, or the transform on every dictation, both the Commands
  model's), as do the Voice Assistant's key and a question dictated into
  Chat (the Chat scope's provider), the rewrite shortcut (the Commands
  model) and a transform (`transforms::run`): it loads while the person
  talks. A cheap look under the config's read lock; never for cloud
  models. Measured (2026-10-25, the 4B on an RTX 5070 Ti, llamafile 0.10.6
  with Yap's arguments and MTP, its file in Windows' cache): up 3.2–3.3 s
  after launch, the first request 0.77 s (a cold prompt; the warm-up runs
  first when the dictation is still going), the next 0.19 s — a dictation
  longer than ~3 s hides the load. **The cleanup's wait** (`pipeline/delivery.rs` → `pipeline/slow.rs`
  `wait_for_model`): when its endpoint is the stand-by address it waits
  for the model before the call, the bar saying **"Loading the AI model…"**
  after 300 ms (`slow::Stage::Loading` through the "Taking longer than
  usual" watch) with **Skip cleanup**; it won't start, or is still loading
  after 30 s: the words as said (then the cleanup's own 20 s as ever).
  Logged with the time it waited. **Idle**: a watch (`init`, from setup;
  every 20 s) stops the sidecar after **"Free its memory after it's unused
  for"** (`local_model_idle_mins`: 5 / **15** (default) / 60 minutes, 0 =
  never; another value counts as 15) with no request (`touch`: a request
  starting or ending, `hold`, a load, a dictation that will use it) —
  `idle_step` (pure): never under a request or a stream
  (`requests_in_flight`), a dictation (`context::sidecar_busy`), a start, a
  restart or a pending one, or a meeting recording whose live digests go to
  it (the Note Formatting scope, else the cleanup's provider) or an action
  plan being written; looked at again under the start lock. Logged ("On-device
  model unused for a while: freeing its memory", minutes only). The next
  use loads it again. `yap-local-llm-changed` after a load or a free
  (Settings and the Chat picker re-read). `local_llm_status().loading`: a
  start under way. Settings shows **"Loaded · ready"** (Free memory),
  **"Loading…"**, or **"Not loaded · loads when needed"** (Load now), and
  polls only while it loads. Test mode: `YAP_E2E_LOCAL_LLM` (a fake AI's
  `…/v1`) stands in for the started sidecar after
  `YAP_E2E_LOCAL_LLM_LOAD_MS` (no process; `e2e_stand_in_start`),
  `e2e_local_llm_loads` ({loads: [why…], starts, running, loading}),
  `e2e_local_llm_idle_ms {ms}` (the idle limit in ms; "Never" stays never).
  Unit-tested: the launch gate, `in_use`, `dictation_uses_it`, the idle
  decision and setting, the stand-by address, `was_ours`.
- **`ai/local_llm/uninstall.rs`** — **uninstalling an on-device model**
  (2026-10-27, the maintainer's ask after a 5.8 GB Gemma 2 9B that didn't
  suit them): `local_llm_uninstall {file}` (it replaced `local_llm_delete`,
  which deleted whatever `.gguf` name it was given, the model in use
  included), from the Chat model picker's right-click menu and Settings →
  Language Models (frontend: `core/localModels.js`). **Only a bare `.gguf`
  name** (`checked_name`: no `/` `\` `:` or `..`, no control characters,
  no space at either end) that the folder lists as a file directly inside
  `llm/` (`remove_from`) — never the llamafile engine, its marker, a
  `.partial` download or anything outside; a name already gone is done.
  **Never the model in use** (`in_use`, pure, from `UseFacts`): one
  sidecar serves one model, so the model every feature routed on-device
  uses — the configured one (`pp_local_model`, else the bundled 1.5B,
  which follows the same rule) or the one the engine serves — stays while
  anything is routed there (`models::in_use`: the cleanup's provider,
  which Chat, Commands and the meeting notes inherit, a cleanup profile, a
  Language Models scope), and the refusal **names what uses it**
  (`users` → `in_use_by`, pure; 2026-10-27: the maintainer's Chat was on
  Claude while cleanup and note formatting ran Gemma 2 on this PC, so a
  bare "Switch to another model first" made no sense in Chat's picker):
  "Dictation cleanup and note formatting use Qwen3.5 4B. Switch them to
  another model first." — dictation cleanup, the {name} style (or "N
  styles by app"), Commands, note formatting, Chat, "another feature" for
  a scope it doesn't name yet (a curated model by its name, any other by
  its file — the person's own, or one taken off the list since, like
  Gemma 2 9B). With nothing routed there it can go, unless the engine is busy
  with it (a request or run in flight, a start, a context restart, an
  on-demand load: "…in use right now. Try again in a moment."); it holds
  the start lock while it deletes (`STARTING`, so no start opens the file
  meanwhile; a start under way counts as busy). A model loaded but used by
  nothing is freed first (`stop`): Windows won't delete a file the engine
  holds open. The configured one leaves `pp_local_model` on the default
  (`change_config`; `yap-chat-model-changed {ppLocalModel: ""}`, which
  Settings' copy adopts); every list re-reads on `yap-local-llm-changed`.
  **A refused delete in plain words** (`refusal`, never the system's own
  message): another program has it open (sharing / lock violation, a
  mapped file), another Yap on this PC uses it (`runtime::other_yaps_sidecar`:
  the dev build beside the installed app), no permission, else "Couldn't
  uninstall … Try again, or delete it from the models folder." Logged with
  its size and the curated id, never its name or path. `local_llm_status`
  gives `inUseNote` (the same sentence with "it": the pickers' note), each
  installed model `inUse` (`"selected"` | `"busy"` | null) and,
  for the curated ones, `fileMb` (the file's own size: what the confirm
  says it frees). Unit-tested: the names taken and refused, the in-use rule,
  only a GGUF directly in the folder deleted (the engine and marker left),
  the refusals' words; e2e `chat-model-picker.spec.js` and
  `local-models.spec.js`.
- **`ai/local_llm/vision.rs`** — **the on-device model reading
  screenshots** (Snip to Chat's phase 3, 2026-10-28; the feature's side is
  [snip.md](./snip.md#the-on-device-model-reads-screenshots-phase-3-2026-10-28)).
  Qwen3.5 is a vision-language model: with its **projector** (a second GGUF,
  `--mmproj`) llamafile 0.10.6's server takes the same `image_url` parts as
  every cloud provider. **The projector** (`PROJECTORS`: only the 4B's so
  far, bartowski's `mmproj-Qwen_Qwen3.5-4B-f16.gguf`, 672,423,488 bytes,
  SHA-256 `659b59dd…` = its LFS oid; the 0.8B, 2B and 9B repos have theirs,
  not yet tried) is **an optional download** into `llm/vision/`
  (`install`: `shell/download.rs` — resume, speed, Cancel
  `llm-vision:<model>`, `local-vision-download-progress`; checked by
  `models::verify_into`; then `local_vision` on), offered by Settings →
  Language Models (the on-device models, "Read screenshots on this PC") and
  by Chat's "can't read images" line (`ai_model_status().imagesLocal`,
  `snip::vision::local_offer`); `remove`, and a model's Uninstall takes its
  projector too (`remove_with_model`). Never listed as a model
  (`local_llm::is_model_file`: no `mmproj-…`). **Loaded only while it's
  needed** (`ensure`, from `chat_send` and the Voice Assistant's question
  with screenshots, before `turn::prepare`): the sidecar starts without it;
  a screenshot to the on-device model sets `WANTED` and loads it — the
  model started with it, or **restarted** with it once no dictation or
  answer uses the model (`context::wait_until_idle` under the restart lock,
  then `stop` and `on_demand::wake`: a dictation meanwhile waits for the
  model as for any load) — and it stays until the model is freed for being
  idle (`forget`; also Settings' Free memory): the next load, likely a
  dictation's, is without it. Why: measured on the RTX 5070 Ti, a
  dictation's cleanup takes the same time with it loaded (back to back 176
  vs 180 ms mean, `yap.exe eval --cleanup` with and without
  `--llamafile-arg=--mmproj`), but the model takes **~0.85 GB more** on the
  card (641 MiB of weights + a 223 MiB compute buffer; llama.cpp's own worst
  case 889 MiB) and **loads 1.3 s or more slower** (3.1 → 4.4 s; up to
  8.8 s with a game on the card): a dictation that loads the model would
  wait for it. **MTP works with it** (`draft-mtp` and `--mmproj` together
  on llamafile 0.10.6: an image answer's drafts 10 of 12 kept); a start
  with the projector never notes MTP as failed (its key is blank in
  `spawn_and_wait`: the projector may be what failed). **Where**
  (`for_start` → `Load {path, on_card}`, `fits_on_card`, pure): on the card
  when the model as planned (`gguf::estimate` with its context, MTP and KV
  cache) plus the projector (`mib`: its file + `COMPUTE_MIB` 250) fit in
  `card_room`; else on the processor (`--no-mmproj-offload`: a screenshot
  answer in 2–6 s instead of 0.3–0.7 s, cleanup and MTP untouched) — it
  never pushes the model off the card. `Serving.vision` (`serving_vision`:
  `Some(on_card)` when loaded); `log_facts` adds its weights from the log
  (`in_log`: "CLIP using CUDA0 backend", "model size: 641.26 MiB"). **A
  start with it that fails**: `FAILED` for the session (`reads_images` says
  no, Chat's line says it couldn't load), the model started again without
  it if it was running (dictation keeps its cleanup). `status` →
  `local_llm_status().vision {model, modelName, sizeMb, installed, on,
  loaded, onCard, failed, cancelId}` (null for a model without one), and
  each curated model's `projectorMb` / `readsImages`. Commands
  `local_vision_install`, `local_vision_remove {model?}`, `local_vision_set
  {on}` (through `settings::change_config`); debug-only `e2e_local_vision`
  (`{ensures, wanted, loaded, failed}`). Test mode: the stand-in sidecar
  "loads" it as a start would (`e2e_stand_in_start`), and its download comes
  from `YAP_LLM_SOURCES`' `vision:<curated id>`. Unit-tested: the table,
  the card rule, the arguments, the log, the setting.
- **`ai/gguf.rs`** — a GGUF model's header, **read-only** (2026-10-20): magic,
  version 2/3, the metadata (a few scalars kept, everything else skipped by
  seeking — the tokenizer's ~250 k strings too: ~20 ms for the 4B's 10 MB
  header in a debug build) and the tensor list (where the data starts, the
  token embedding's size, whether there's an `output.weight` of its own),
  never the weights; absurd counts/lengths are errors, never a hang or a
  huge allocation. `Shape`: `context_length` (n_ctx_train), block count,
  heads, KV heads (one or per layer), key/value length,
  `full_attention_interval` (Qwen3.5: attention every 4th layer),
  `nextn_predict_layers` and the prediction blocks' bytes (`nextn_bytes`,
  from the tensor list), `feed_forward_length`, the `ssm.*` sizes, the
  vocabulary. `model_file` caches per file (size + mtime).
  **`estimate(shape, file, ctx, on_card, Server { current, mtp })`**,
  calibrated against llamafile's log on the RTX 5070 Ti — model on the card
  = the tensors' data less the token embedding (kept on the processor)
  unless the output shares it (no `output.weight`: copied to the card), and
  on 0.10.6 less the prediction block unless MTP is on; KV = attention
  layers × KV heads × (k + v) × 2 bytes per token (one unified cache for 4
  slots); recurrent state per slot (f32 conv + state, × 4 slots); compute
  on 0.10.3 = vocab × 512 × 4 + 2 KiB per token of context, on 0.10.6 = 2
  × feed-forward × 512 × 4 + embedding × 512 × 20 + 12 MiB + 1 KiB per
  token; MTP (`mtp_mib`) = the prediction block + the recurrent state × 3
  more + 36 MiB + one attention layer's KV + a draft compute buffer; a
  q8_0 KV cache (`Server.kv_q8`) = the KV × 17/32 and 3,600 bytes of
  compute per token of context more (the 4B with MTP: 4,698 / 5,546 MiB
  estimated vs 4,623 / 5,487 in the log at 32K / 64K).
  Within 1% of the log, erring high: Qwen3.5-4B on 0.10.3 3,821 / 4,637
  MiB estimated vs 3,815 / 4,641 at 8K / 32K; on 0.10.6 3,279 / 4,071 /
  5,127 vs 3,270 / 4,062 / 5,118 at 8K / 32K / 64K, with MTP 4,153 / 5,065
  / 6,281 vs 4,134 / 5,024 / 6,240; Qwen3.5-9B 5,646 vs 5,634 (8K);
  Qwen2.5-1.5B 1,472 / 2,192 vs 1,480 / 2,186 on 0.10.3 (docs/model-evals.md
  "Context length", "Local AI, phase 1"). The CUDA context itself
  (~230–250 MiB more in `nvidia-smi`) isn't counted: `card_room` leaves it
  aside. Unit-tested on synthetic headers (`tests::Writer`, the models' real
  shapes); `#[ignore]`d `this_pcs_models` reads `%APPDATA%\yap\llm`
  read-only (`YAP_GGUF_DIR` for another folder).
- **`ai/gpu.rs`** — the graphics card, for the on-device model recommended for
  this PC: DXGI (`CreateDXGIFactory1` → `EnumAdapters1` → `GetDesc1`, software
  adapters and Microsoft's Basic Render Driver left out), the card with the most
  memory of its own, cached; **`can_run_big_model`** = ≥ `BIG_MODEL_VRAM_MIB`
  (5.5 GiB, so "6 GB" cards count) of dedicated video memory — the 4B takes
  ~3.2 GiB on the card on llamafile 0.10.6 (model 2,741 MiB + 8k context's KV
  256 + recurrent state 201 + compute 72, read from the sidecar's log; 3.7
  GiB on 0.10.3; MTP, only where it fits, ~0.85 GiB more), beside Yap's speech model
  (~1–1.6 GB) and Windows (~0.5–1 GB); integrated graphics report little of
  their own (this PC's Ryzen Radeon: 2 GiB). Debug-only `YAP_E2E_GPU`
  ("none" | "<name>,<MiB>") stands in for the card in test mode. `#[ignore]`d
  `this_machine` prints this PC's cards. **Every adapter** (2026-10-24,
  `all_adapters`: DXGI's index — DirectML's device id —, software ones
  flagged) for `yap.exe bench`; **`is_small`** (< `SMALL_GPU_MIB`, 3 GiB of
  its own: integrated graphics and entry cards; a "4 GB" card isn't); and
  **`speech_adapter`** (`speech_adapter_of`, pure): the card with the most
  memory of its own when DXGI doesn't list it first — a hybrid laptop lists
  its integrated GPU first and DirectML's default device is DXGI's first, so
  Parakeet ran on the integrated GPU, not the card. lib.rs sets it at start
  (`stt::set_gpu_adapter`; never in a test run).
- **`ai/hardware.rs`** — **picks by hardware** (2026-10-24), calibrated by
  `yap.exe bench` ([`docs/benchmarks.md`](../benchmarks.md)): `Machine`
  (the best card, logical processors, memory; test mode: `YAP_E2E_GPU`,
  `YAP_E2E_CPU_THREADS`, `YAP_E2E_RAM_MIB` stand in), `tier` (pure): **card**
  (a card that isn't small), **processor** (no such card, ≥ 8 threads and ≥
  7.5 GiB), **small** (the rest). `speech_device_before_measuring` (pure):
  the graphics card only when it isn't small, else the processor —
  `stt_speed::device_choice`'s default until this PC's own timing exists
  (before 2026-10-24 always the graphics card: DirectML on a weak integrated
  GPU or on Windows' software renderer, several times slower than int8
  Parakeet on the processor, for the first minute or more). `advise` (pure)
  → `Advice {tier, speechModel (Parakeet V3 everywhere), speechDevice,
  cleanup: local | cloud, localModel (recommended_id), localMs, cloudMs,
  summary}`: on the card the on-device model (the 4B where it fits), on the
  processor tier the 1.5B, on a small PC **a cloud provider first** (the 1.5B
  adds seconds a dictation there) with both times said; the times are
  `LOCAL_MS_*` / `CLOUD_MS`, measured. `hardware_advice` (command) feeds
  onboarding: step 1 says where speech runs ("on its processor: no graphics
  card needed"), the AI cleanup step moves "Recommended" to the cloud card
  ("Quicker on this PC") on a small PC, picks it, and says what each adds to
  a dictation (`cleanup-times`). Unit-tested; e2e
  `onboarding-weak-pc.spec.js`.
- **`ai/ai_model.rs`** — which model a scope's next call goes to, resolved the
  calls' way (`Scope::Chat` → `turn::resolve_chat_endpoint`;
  `Scope::Notes` → `meeting_summary::resolve_endpoint`): `ai_model_status
  {scope}` → `{ready, error, model, host, tools (supports_tools), fromScope,
  provider (the one the scope is set to, ready or not), keyed, maskedKeys
  (each keyed provider's key as "gsk…AbCd", `mask_key` — never the key),
  servers, onDevice, context (the on-device model's, when the call goes to
  it: the chip's tooltip "Context: 8K tokens"), effort (`effort::info`:
  `{levels, default, chosen, fast}`, `null` for a model without a
  reasoning control — the chip's "Extra", the picker's Effort)}` for the Chat view's model chip + picker and the Ask
  bars' model lines; `chat_set_model {provider, model, baseUrl?, file?}` =
  the picker's switch (Chat scope only, switched on; a keyed provider, the
  installed on-device model, or a self-hosted / custom server Settings has
  the address of — `servers(cfg)`: the Chat scope's, cleanup's, any other
  scope's or cleanup profile's local/custom endpoint, one per address, its
  key taken from where it's set up (`model_catalog::endpoint_key`), never
  an address the page makes up; same provider keeps its URL + key, a new
  one gets its standard URL + the shared key; `file` picks one of the
  on-device GGUFs → `pp_local_model` (`set_local_model`, "" = the bundled
  1.5B) — the one sidecar serves it for every feature, so one serving
  another GGUF stops and starts again on it), saved through `settings::change_config` (in one step, applied as a Settings save),
  then `yap-chat-model-changed {scope, ppLocalModel?}` (Settings adopts
  both). `chat_provider_key {provider, key}` = the picker's "Connect
  Claude": `set_provider_key` saves it where Settings does (`pp_api_keys`,
  cleanup's `pp_api_key` when cleanup is on it, the Chat scope's key when
  Chat is), through `settings::change_config`, whose `model_catalog::on_config_saved`
  checks that provider's list at once; the page tells Settings' copy with
  the window event `yap-provider-key-saved` (not broadcast to every
  window). `wake_on_device` (in `chat_send` and
  `note_ask`): a chat on the on-device model starts the sidecar
  (`local_llm::ensure_started`) or says it isn't installed, never a silent
  fallback to the endpoint underneath. A config save emits
  `yap-config-saved`, which the chip re-reads on. **Which model is
  answering** (2026-11-03): `answering_line(ChatModel)` is the one line
  every prompt that answers the person's own questions carries, built from
  who answers that call (`who_answers`: `describe`'s, as the chip names it
  — the on-device model not installed is named by the endpoint underneath)
  — "You are Yap's assistant, running on {model}{where}. Asked who or what
  you are, or which model you are, say so plainly; never deny being
  {model} or say you can't tell. Don't bring it up unless asked." (~50
  tokens). `{model}` is `budget::model_label` (the on-device model's own
  name, else made from the id: the name the "too long" line and
  `other_models_note` use); `{where}` by provider: " from Anthropic",
  " from OpenAI", " from OpenAI, on the user's ChatGPT plan", " from
  Google" (Gemini), ", served by Groq", ", through OpenRouter", ", on this
  PC" (the on-device model, or a server on localhost), ", on the user's
  self-hosted server", ", on the user's own server" (custom). After the
  persona (`persona_with_answering`): a Chat turn's (`turn::Endpoint` —
  Chat, the Voice Assistant on this PC and from the phone, the Meetings
  view's Ask, Skills across the meetings, a small model's web pipeline,
  Chat's limits and context ring) and the Notes view's Ask
  (`note_prompt`); at the end of the system message
  (`messages_with_answering`, the immutable start untouched) for the
  notepad's typed Ask (`meeting_catch_up` with a question, its thread
  sized with it) and a Skill on one meeting (`meeting_skill`), on the
  meeting's model. It names the model writing the answer the person
  reads: a provider's own web search is a sub-call whose results go back
  to that model. Not dictation cleanup, Commands, transforms, "What did I
  miss?" or the one-tap helpers (they never talk about themselves).
  Unit-tested per provider kind and on each prompt; e2e `about-you`,
  `notepad`, `skills`; the model evaluations' `chat-which-model` (asked
  out loud: a greeting never names the model, "which model are you?"
  names one and never "I can't tell").
- **`ai/effort.rs`** — **effort, per model** (2026-10-31, as the Claude
  Code app's model menu has it: "Opus 5.5 Extra"): how hard a cloud model
  reasons before it answers, chosen in Chat's model picker (or Settings →
  Language Models → Chat), kept per model in `chat_effort`
  (`"<provider>:<model>"` → level; config-and-data.md) and sent **wherever
  that model answers a question**: Chat and the Voice Assistant (every
  `llm::ChatRequest::chat`, so the tool loop and `note_chat`), the Notes
  view's Ask (`note_ask`) and a meeting's typed Ask in the notepad
  (`meeting_catch_up` with a question: `chat_beside_dictation_live(…,
  asked)`). Cleanup, Commands, the meeting notes and helpers, memory and
  Chat's background summaries never come here (they call `chat_with` /
  `post_chat_request`). Levels are Claude Code's: low, medium, high, xhigh
  ("Extra") and max; a model shows only the ones it takes, and a model
  with no reasoning control has none (no Effort item). Unchosen, a model
  runs at its own default (`Caps::default`; "Default" when that isn't a
  level: OpenAI's "none", Groq's Qwen "default" — nothing sent). **The one
  table** (`caps`), from each provider's docs read 31 Oct 2026, the
  provider's own list first where it says (`CatalogModel::efforts`:
  Anthropic's `capabilities.effort`, OpenRouter's
  `reasoning.supported_efforts` and `default_effort`):
  - **Claude** — `output_config.effort` with `thinking: {type:
    "adaptive"}` (its summary shown), on the Messages API only (the
    compatible endpoint ignores an effort and 400s adaptive thinking:
    `apply` sets `ChatRequest::native`). All five on Fable 5/5.1, Mythos
    5/5.1, Opus 5.5/5/4.8/4.7, Sonnet 5.5/5, Haiku 5.5; low/medium/high/max
    on Opus 4.6, Sonnet 4.6, Mythos Preview; low to high on Opus 4.5 (the
    effort alone: it thinks to a budget, never sent — `budget_tokens` 400s
    on the newest models). Defaults: medium on Opus 5.5 and Haiku 5.5, high
    elsewhere. No effort on Haiku 4.5, Sonnet 4.5 or older. xhigh and max
    get `max_tokens` 64,000 (Anthropic: start at 64K there).
  - **Fast mode** — Claude Opus 5.5 on Anthropic's API, `speed: "fast"`
    (no beta header on Opus 5.5; Opus 5 and 4.8 are a waitlisted research
    preview, 4.8's ending 30 Nov 2026: not offered). $8 / $40 per MTok,
    twice the standard price: off unless switched on (`fast_mode`), its
    switch says so.
  - **OpenAI** — `reasoning_effort`: GPT-5.6 and GPT-6 low to max (medium),
    GPT-5.5 low to xhigh (medium), GPT-5.2–5.4 low to xhigh and GPT-5.1 low
    to high (default "none": "Default"), GPT-5 and o-series low to high
    (medium); `-chat` models none. The GPT-6 family takes tools on Chat
    Completions only with `"none"` (its model pages), so a call with tools
    sends that.
  - **Groq** — `reasoning_effort`: GPT-OSS low to high (medium), Qwen3 low
    to high ("Default"), Muse Spark Open low to xhigh (xhigh).
  - **Gemini** — `reasoning_effort` low/medium/high on 2.5 and later (its
    compatible endpoint maps it to a thinking level or budget), high.
  - **OpenRouter** — `reasoning: {effort}`: its list's levels, else the
    family's (an `anthropic/…` id by Claude's table, `openai/…`,
    `google/…`), else low to high for a model its list says reasons.

  `apply(req)` adds the fields, the time (`think::TIMEOUT`, 150 s; 300 s at
  xhigh and max) and, for Claude, the Messages API; `think::reasoned`
  marks Chat's turn so what the model thought shows folded above the
  answer (only what it sends back: Claude's summary, Groq's and
  OpenRouter's `reasoning`; OpenAI and Gemini send none). Commands
  `effort_info {provider, model}` (Settings), `chat_set_effort {provider,
  model, level|null}` and `chat_set_fast {on}` (the picker; saved through
  `settings::change_config`, the Chat status back). The on-device model
  has none: Chat's Think button is its own (`chat/think.rs`). Unit-tested
  per provider and level (`effort::tests`, `anthropic::tests`); e2e
  `chat-effort.spec.js`.
- **`ai/model_catalog.rs`** — **the AI providers' live model lists**
  (2026-10-18), so a model Google or Groq releases tomorrow shows up in
  Yap's pickers without a Yap update and a retired one stops being offered.
  Asks **the provider itself, with the key already saved** (`key_for`: the
  scope's own, `provider_api_key`'s shared one, else any scope's or cleanup
  profile's; only providers with a key, `keyed`; never a catalog of Yap's
  or a third party's): Groq `GET /openai/v1/models`, OpenAI `GET /v1/models`
  (Bearer), Anthropic `GET /v1/models` (`x-api-key` + `anthropic-version`,
  paged by `after_id`), Gemini's native `GET /v1beta/models` (the key in
  `x-goog-api-key`, never the URL; paged by `pageToken`), OpenRouter
  `GET /api/v1/models`; 15 s, off the main thread. **Keeps chat models
  only**, per provider and unit-tested on fixtures trimmed from the real
  lists (`parse_groq` … `parse_openrouter`, `chat_capable`): no embedding,
  speech/TTS/transcription, image/video/music, moderation/guard, realtime/
  live/native-audio, search-preview, computer-use, deep-research or Codex
  models; Gemini needs `generateContent` (no `-latest` aliases, `-exp`,
  custom-tools variant), Groq and OpenRouter text out (Groq: active, not a
  model `config::is_retired_groq_model`), OpenAI GPT / o-series without the
  Responses-only `-pro` and `-latest` aliases, Anthropic not `retired`
  (`deprecated` marked); dated snapshots fold into the alias the list has
  (`fold_snapshots`: `gpt-4.1-2025-04-14` → `gpt-4.1`'s `aliases`), one
  without it keeps a `base` (`claude-haiku-4-5-20251001` →
  `claude-haiku-4-5`) so a curated alias still matches. **Names**: the
  provider's (`clean_name`: "OpenAI: " / "Qwen/" cut), else `derive_name`
  ("GPT-5.5", "GPT-OSS 20B", "Llama 4 Scout 17B 16E Instruct"). Per model:
  id, name, desc (first sentence), created, context, `tools` / `thinks` when
  the provider says (Groq's `supported_features`, OpenRouter's
  `supported_parameters`, Gemini's `thinking`), `firstSeen`. **Cache**
  `<data>/model_catalog.json` (per provider: the last good list +
  `fetchedAt`, the last attempt's `checkedAt` + `error` in words — "the API
  key wasn't accepted" —, never a key; atomic, quarantined when corrupt).
  **When**: a list is fresh for a day (`FRESH_FOR`), a failed check tried
  again after an hour; `init` checks what's due 40 s after launch, then
  hourly (not in a test run); `on_config_saved` (from `save_config`) checks
  a provider at once when its key was set or changed; `model_catalog` (the
  pickers) returns the cached lists and checks a due one behind them; one
  check per provider at a time; every change is `yap-model-catalog`
  (snapshot `{providers, now, rev}`, keyed providers only; `rev` taken under
  the store lock so a late command reply never wins over a newer event).
  A failed last check = `ok: false` (the page shows the curated list).
  `tools_for(provider, model)` feeds `tools::supports_tools`: a cloud model
  its provider says can't call tools gets no note tools ("· no note tools").
  **Which models read images** (Snip to Chat, [snip.md](./snip.md)): `vision`
  where the list says it — Anthropic's `capabilities.image_input.supported`,
  Groq's and OpenRouter's input modalities — and `vision_for(provider,
  model)`; `snip::vision` adds family rules where it doesn't (Gemini, OpenAI)
  and the switch `ai_model_status` offers (`images`, `imagesSwitch`).
  **Which effort levels a model takes** (2026-10-31, `ai/effort.rs`):
  `efforts` (lowest first) where the list says — Anthropic's
  `capabilities.effort.{low…max}.supported`, OpenRouter's
  `reasoning.supported_efforts` (only Yap's five; `null`, "any", leaves it
  to Yap's table) — and OpenRouter's `default_effort`; read before the
  table, so a new model gets its own choices without an update.
  **Self-hosted / custom servers** (`llm_models {provider: local|custom,
  scope, baseUrl}`): `GET {base}/models` with the scope's key (OpenWhispr's
  OpenAICompatiblePanel reading: `data` or `models`, `id` or `name`), kept a
  minute in memory, plus what the server says of each model's **context**
  (`server_contexts`, read-only, best-effort, 3 s each: LM Studio's `GET
  /api/v0/models` `max_context_length` / `loaded_context_length`, else
  llama.cpp's own `GET /props` (`default_generation_settings.n_ctx`, the
  context it serves its one model with; 2026-10-25), else
  Ollama's `GET /api/ps` (a loaded model's `context_length`) and `POST
  /api/show` per model, ≤ 16 (`model_info.<arch>.context_length`, a
  Modelfile's `num_ctx`); `ListView.contexts [{id, max, set, app}]`; a
  server that's neither says nothing). The picker shows it first in the row
  ("8K context, set in Ollama · localhost:11434", "Up to 128K · …").
  Chat's context ring reads it too (`server_context`: what the list said,
  any age; `server_context_here`: a server on this PC is asked first when
  its list isn't in hand — one elsewhere never is). Commands `model_catalog`, `model_catalog_refresh
  {provider}` (Settings' Check now), `llm_models {provider, scope?, baseUrl?,
  force?}`. Logs: provider + counts. Test mode never contacts a provider:
  debug-only `YAP_MODELS_STANDIN` (`<standin>/<host><path>?<query>`,
  `desktop/e2e/support/fake-llm.js`); none set = nothing fetched; a self-hosted
  server only on 127.0.0.1. `#[ignore]`d `real_saved_lists`
  (`YAP_MODEL_LISTS=<dir>`) runs the parsers over saved real lists, never
  fetching.
- **`ai/usage.rs`** (+ `ai/usage/`) — **the AI usage tracker** (2026-10-25):
  every AI call's tokens, counted on this PC. **Counts only** — the
  provider, the model, on this PC or not, the feature, the prompt's and
  the answer's tokens, the on-device model's reading and writing time —
  never a word asked or answered, in the store or the log (`debug` names
  the provider, the feature and the counts). **One place**:
  `record_response`, called by `llm::post_chat_request` (the one chat call
  every other goes through, streamed or whole, the on-device model's load
  included), by a provider's own web search (`web/native.rs`, a sub-call:
  never the turn's context) and, through `record_embeddings` (the tracker
  only), by search by meaning's embeddings (`search/embedder.rs`). **The
  counts** (`parse`, pure): OpenAI-compatible `usage {prompt_tokens,
  completion_tokens, total_tokens}` (OpenAI, Groq, Anthropic's and
  Gemini's compatible endpoints, OpenRouter, llama.cpp, Ollama, LM
  Studio), Groq's `x_groq.usage` (its stream), `{input_tokens,
  output_tokens}` (Anthropic's own API with its cache's tokens as prompt,
  OpenAI's Responses API), Gemini's `usageMetadata` (thinking as answer),
  and llama.cpp's `timings`: the speed (`predicted_n` / `predicted_ms`, or
  ÷ `predicted_per_second`; the prompt's the same) and, when `usage` is
  missing, the counts (`cache_n` + `prompt_n` read, `predicted_n`
  written). A streamed answer's come back on `stream::Assembler::response`.
  **The feature**: the network receipts' scope (`net::within`, mapped:
  Dictation → `cleanup`, Chat, the Voice Assistant, Commands, Notes,
  Meetings, Brief, Memory), unless a finer one of the tracker's is set
  around the call (`usage::within`, the innermost wins: `transforms` —
  `dictation/transforms.rs`, the shortcut's and a dictation's —,
  `translation` — `dictation/languages.rs` —, `speakers` —
  `speakers/naming.rs`); embeddings are `search`; a cloud voice's request
  (spoken replies on OpenAI, Gemini or Groq, `voice/tts/hosted.rs` →
  `record_speech`: the tokens OpenAI's and Gemini's streams end with, or
  none for Groq's WAV, and the text's characters) is `voice` ("Spoken
  replies"), priced by its tokens, else by the character where the voice
  bills that way (`prices::char_cost_micros`: Groq's Orpheus, $22 a million);
  unscoped `other`. **On
  this PC** as the receipts place it (`net::place_of_url`; a test run's
  stand-in aliased to a cloud host counts as that cloud). **Where it goes**:
  `usage/groq.rs` — Groq's "Usage today" meter, exactly as before
  (`groq_usage.json`, the same shape and code: an older Yap reads and
  writes it too; `get_groq_usage`, `groq-usage`); `usage/store.rs` — the
  daily store, `ai_usage.json` (config-and-data.md): per local day, per
  provider + model + on-this-PC + feature, calls (and those that didn't
  say their tokens), prompt and answer tokens, an estimated cost and the
  llama.cpp times; kept 90 days; written by its own thread every 3 s when
  something was added and at exit (`shutdown_cleanup` → `flush`), through
  `saving.rs` (`SAVER`, "your AI usage history"); what this Yap added since
  its last write is kept apart and added to the file as it is on disk when
  another Yap (Yap - Dev beside the installed nightly) wrote it meanwhile,
  so neither's counts are lost (`subtract`, unit-tested); `yap-ai-usage`
  after each write. **`ai_usage_summary`** (`store::summarize`, pure):
  today, the last 7 days and the last 30 — tokens, on this PC and cloud,
  the estimate, the cloud tokens with no price, by model (most first; the
  on-device model named, `local_llm::display_for_id`; its speed) and by
  feature — and the on-device models' speed by day. `usage/prices.rs` —
  **the price table**: US dollars per million prompt and answer tokens at
  the providers' list prices for Claude, OpenAI, Gemini and Groq (only
  a call to that provider's own service; OpenRouter, custom and local
  servers have none), checked 2026-10-25 against the pages in `SOURCES`
  (`PRICES_AS_OF`); a price for longer prompts (`upto`: Haiku 5.5 past
  100K, Gemini Pro past 200K, OpenAI's table to 272K) and one that changes
  on a date (`from`: Gemini 4 Argon on 1 Dec 2026, the 3.x Flashes on 1 Jan
  2027) apply per call; spoken replies' cloud voices (checked 2026-10-31:
  OpenAI's `gpt-4o-mini-tts` text $0.60 / audio $12, Gemini's 3.8 Flash TTS
  $0.50 / $9 and Flash-Lite TTS $0.50 / $6, both doubling on 1 Jan 2027); a dated snapshot costs what its model does
  (`base_id`). The cost is worked out when a call is recorded (millionths
  of a dollar). Pages say it's an estimate: free plans cost nothing, cached
  prompts cost less. `usage/context.rs` — Chat's context ring
  ([chat-and-assistant.md](./chat-and-assistant.md#the-context-ring-2026-10-25)).
  **The turn's last call**: `usage::tallied` around a Chat turn keeps its
  last model call (`last_call`), which `chat_turn` keeps with the answer
  (`ChatMessage::tokens`). Unit tests never write the store (and Groq's
  meter returns early under `cfg(test)`), but the tally works there.
- **`ai/eval.rs`** (+ `ai/eval/`) — **model evaluations**, `yap.exe eval …` (main.rs;
  debug builds only): real models (Groq/OpenAI/Anthropic/OpenRouter/Gemini/
  custom, or a GGUF on Yap's own llamafile started for the run — the installed
  runtime and model read-only, else downloaded via `local_llm::models::download_verified`)
  through **`turn::chat_turn`** itself (fast path, tool loop, plain chat +
  RAG) over `evals/scenarios/*.json` (seed notes/folders, stand-in apps from
  `evals/apps/` or this PC's Start menu read-only, multi-turn user messages,
  checks on tool calls + arguments, apps "opened", notes afterwards, answer
  text/length, false claims), scored pass/fail with reasons + soft metrics.
  Side effects stay in the run: its own data dir (`portable::use_data_dir`),
  settings in memory (`eval::config`, read by `config::load`; `save` a no-op —
  keys never reach a file), notes in memory (`notes::eval_reset`), test mode
  for apps (opens recorded), the clipboard tool recorded
  (`eval::intercept_tool`); web scenarios (`needs_web` / `web`) search live
  (`eval::web_live` lets `web::Endpoints::current` use the real services in
  test mode: the run's Exa/Tavily key, else the keyless wikis) or against a
  stand-in server (`ai/eval/standin.rs`, through `YAP_EXA_URL` /
  `YAP_TAVILY_URL` / `YAP_WEB_STANDIN`). Logs per run in `<out>/<stamp>-<label>/`:
  `transcript.jsonl` (every request body incl. tool schemas, every response,
  429s/waits, tool calls + results, keys redacted), `summary.md`/`.json`.
  Paces Groq to its 8k tokens/min (and its rate-limit headers). `--raw` sends
  hand-written requests (the Groq `browser_search` spike). A local model's
  llamafile runs with the sidecar's own `local_llm::server_args` (thinking off,
  caches capped; `--llamafile-arg` after them overrides) and sends its GGUF's
  name as the model id, as the sidecar does; a voice turn is the Voice
  Assistant's (scope "assistant"). **`--cleanup`** (`ai/eval/cleanup.rs`)
  times `llm::cleanup` itself over `evals/cleanup/dictations.json` — back to
  back, after a Chat turn, after a long one — with llama.cpp's prompt tokens
  processed, and keeps every output (`cleanup.md`), then the `spelling`
  dictations (words English spells two ways, as speech-to-text writes them):
  how many come out British or American under the run's locale.
  **`--locale`** stands in for Windows' region (`YAP_E2E_LOCALE`, About you):
  without it no region reaches the prompts; a scenario's `settings.region`
  picks a country over it, as Settings' Region does (`config.region_country`;
  the US and Ireland controls of the weather scenarios, checked with
  `temperatures`: "C" | "F"; the wind and distance ones with `speeds`:
  "km/h" | "mph" and `distances`: "km" | "miles", `ai/eval/units.rs`). A
  scenario with `needs_web` and
  no `web` object runs live with the run's key (until 2026-10-21 a derived
  `Default` left them keyless: Wikipedia). `--cleanup-layout combined |
  system` tries the cleanup's prompt laid out another way (`combined` = the
  layout before 2026-10-22); `--cleanup-quick` / `--cleanup-limit n` keep a
  cloud model's tokens down. A local model's server runs Yap's `Plan` (the
  runtime's version from `--version`, MTP as the sidecar would decide on
  this PC; `--mtp on|off` overrides, never with `--ngl 0`) and is warmed up
  as the sidecar is (`--no-warm-up`); a runtime is used from the installed
  app only when its marker says it's the pinned one, else downloaded with
  its marker. `--clock-step <min>` moves the clock Chat tells the model on
  by that much each turn (`eval::now`: a real chat's turns are minutes
  apart, so the time in the prompt changes). **`--bench`**
  (`ai/eval/serving.rs`): `stream` (time to the first token, streamed vs
  not, and streamed tool calls; scenario runs stream as Chat does and
  measure the first words, `YAP_EVAL_STREAM=off` whole), `layout` (a Chat turn's prompt re-read
  with the time + notes in the system prompt vs the last user message),
  `loop` (a tool step after one that offered other tools), `embed` (an
  embedding model's speed and recall vs Chat's keyword search), `needle`
  (a long context: six facts hidden in a made-up logbook of
  `--bench-history` thousand tokens, each asked for — found, and the time
  to read it; with and without a q8_0 KV cache); **`seed.calendar`** gives
  a scenario a connected calendar (`calendar::seed`: meetings and busy time
  on days the run's clock reads as the person would say them), and
  `calendar_cited` / `calendar_not_cited` check the answer's meetings;
  `--temperature` / `--set` lay fields over every request
  (`eval::adjust_body`, in `llm::post_chat_request`). The hooks in
  `ai/llm.rs` (`eval::Call`, `eval::adjust_body`), `chat/tools.rs`, `config.rs`,
  `notes.rs` are no-ops outside a run. See
  [`docs/model-evals.md`](../model-evals.md).

## Sign in with ChatGPT (2026-11-01)

The person's ChatGPT plan pays for Yap's AI, instead of an API key. It's a provider of its own, **"chatgpt"**. The API-key providers, OpenAI's among them, don't change.

**One "ChatGPT" entry on screen** (2026-11-02): Settings' Cloud providers tabs and Chat's model picker show OpenAI's two ways as one entry, "ChatGPT" with OpenAI's mark, where they had an OpenAI and a ChatGPT one. The plan comes first, an OpenAI API key second ("Use an API key instead"); both connected, a "Your plan | API key" switch. It's a UI merge only: `chatgpt` and `openai` stay two providers everywhere underneath (config, Rust, `KEYED_PROVIDERS`, usage, effort, the voices on the OpenAI key), with no migration. See frontend-settings.md (`ChatGptProvider.svelte`) and chat-and-assistant.md (the picker).

OpenAI's flow for open-source apps that run locally is self-serve: no partner key, no review. A paid or hosted Yap would need OpenAI's say first.

### Signing in

`chatgpt_sign_in` starts it, from Settings → Language Models (any scope, or Dictation Cleanup) → Cloud providers → ChatGPT, or from the ChatGPT entry in Chat's picker (its connect card, or "Use your ChatGPT plan instead…" under the OpenAI key's models).

- **OAuth 2 with PKCE (S256)** in the system browser (`oauth.rs`).
- **The loopback listener** takes the code at `http://127.0.0.1:1455/auth/callback`:
  - any free port when 1455 is taken (only the port may change), never `localhost`;
  - one `state`, which must match;
  - a GET from loopback with the right `Host`.
- **The first time, Yap registers itself** with the account: `client_id=dynamic_agent_client`, `agent_name_hint=Yap`, and this PC's `ext_agent_host_id` (`urn:uuid:` and a v4 UUID, made once).
- **The issued `oaiapp_…` id is kept** in `chatgpt.json` *before* the code is used, so a failed exchange never registers Yap twice. Every later sign-in uses it, with the account's email as `login_hint`.
- **A refused code is authorized again, once** (2 Nov 2026). OpenAI refused the first real sign-in's code with `invalid_grant`, and Yap said "Your ChatGPT sign-in has ended". OpenAI's sign-in guide says: "discard that code and start a fresh authorization with the issued client ID". Its DevKit does that once, and so does Yap (`chatgpt.rs` `again`):
  - the browser's tab says "One more step";
  - a new tab opens on the consent page with the issued id;
  - a second refusal says "ChatGPT didn't accept this sign-in. Click Continue with ChatGPT to try again."
  - "Your sign-in has ended" is only for a refresh that's refused.
- **Scopes:** `openid profile email offline_access resource.invoke chatgpt.tokens.use.direct`, with `resource` set to `https://api.openai.com/v1`.
- **Endpoints** come from the issuer's discovery document, each checked to be on the issuer.

### The ID token

`jwt.rs` checks it with `ring`, which is already in the tree through rustls:

- RS256 against the issuer's published keys, fetched again once for an unknown `kid`;
- the issuer;
- the audience: the issued client id (a list naming others needs `azp`);
- the expiry, with 5 s of skew;
- this attempt's nonce;
- a subject.

A later sign-in must be the same account (`sub`). Another account forgets the registration and tells the person to click again, which makes a new one.

Without `chatgpt.tokens.use.direct` in the granted scopes, the person is signed in but Yap can't use the plan. The card says so and offers **Turn on plan usage** (sign in again with `prompt=consent`) or an OpenAI API key instead, on the same ChatGPT tab.

### The key store holds a marker

While signed in with the plan, the key store holds `PLAN_KEY` ("chatgpt-plan") for the provider. So every "has this provider a key?" check works as for the others: the scopes, Commands, the meeting notes, the picker's connected dot.

`set_provider_key` refuses anything else for this provider. The marker is never a credential: the calls use the sign-in's token.

### Tokens

`vault.rs` keeps them in Windows Credential Manager as one JSON value: the refresh token, the access token and its expiry, the issued client id and the account's `sub`.

- **Where:** `yap-chatgpt`, split over `yap-chatgpt-2…6` when one credential can't hold it, with Local persistence.
  - A portable Yap's names carry a hash of its data folder.
  - A model evaluation uses the real names, as it uses the maintainer's keys.
- **Never** in config.json, chatgpt.json, the logs or the webview.
- **Lifetimes:** the access token lasts an hour. The refresh token lasts 30 days and **rotates on every refresh**.
- **One refresh at a time:** `access_token()` refreshes one at a time in this process and **across Yaps** (the dev build beside the installed app, an evaluation), through a lock file in the temp folder (`UpgradeLock::take_named`).
  - Each refresh reads the newest tokens back first, so a spent refresh token is never used twice (OpenAI would answer `refresh_token_reused`).
- **After a refresh:** the new tokens are kept first, then the refreshed ID token is checked. One for another account signs Yap out.
- **A dead grant** (`invalid_grant`, `refresh_token_expired`, …) signs Yap out, and Settings says so.
- **A 401 on a call** refreshes once (`refreshed_token`) and asks again. The token it sets aside is only the one OpenAI refused, under the same lock across Yaps (2 Nov 2026): one another Yap had just refreshed is used as it is, never written back over with the spent refresh token (`set_aside`).

### Signing out

`chatgpt_sign_out` revokes the refresh token at the discovery document's `revocation_endpoint`, trying twice on a 5xx or a network error.

- The tokens are removed here either way, and the marker cleared.
- If OpenAI didn't confirm the revocation, Yap says so, with where to disconnect Yap in ChatGPT's settings.
- The registration and the host id stay.

### The calls

`responses.rs` sends `POST https://api.openai.com/v1/responses` with the access token.

**Every request:**
- `store: false` and `stream: true`. A caller that doesn't watch still gets a stream, read to its end.
- The whole history in `input`.
- The first system prompt as `instructions`. A later note in Yap's voice goes as a developer message, since system items are refused.
- None of the fields the flow refuses: `temperature`, `max_output_tokens`, `top_p`, `previous_response_id`, `metadata`, `user`, and so on. An evaluation's overrides of these are stripped too.

**Tools and a turn's steps:**
- Yap's tools go in one `yap` namespace, the only way the flow takes function tools.
- A call comes back as a `function_call` with that namespace; its result goes back as `function_call_output`.
- Each step replays the response's output items (`yap_responses`, beside the OpenAI-shaped message, as Claude's `yap_anthropic` is): reasoning with its `encrypted_content`, the calls, the text. They go back without `id` or `status`, since nothing is stored to refer to.

**Request options:**
- A named `tool_choice` becomes `required`.
- `response_format` becomes `text.format`.
- Effort (`reasoning_effort`) becomes `reasoning: {effort, summary: "auto"}`, and the summary streams as thinking.
- Screenshots go as `input_image`.

**Answers:**
- Only `response.completed` counts as an answer.
- A broken stream is asked again once (`Delta::Restart`).
- `response.incomplete` keeps what it wrote, if anything.

**The plan's errors** (`plan_error`, after OpenAI's "Errors and recovery"):

| Error | What Yap does |
|---|---|
| The usage limit (`…_usage_limit_exceeded`, also mid-stream as `response.failed`) | Says it, with "Manage usage" and chatgpt.com/settings/usage; never retried |
| Not eligible | Says it, with the API key as the way out |
| `…_unavailable`, admission's 503 | Retried twice after a pause |
| An unsupported field (`error.param`) | Drops it and asks again, up to three times |
| A plain 429 | Waited out like any provider's |

Usage (from `response.completed`) goes to the AI usage tracker as "chatgpt", with no price: it's the plan.

### The plan's models are OpenAI's

`chatgpt::as_openai` makes effort, images, context sizes and the strong models (`documents::rich`) read "chatgpt" as "openai". GPT-6 counts as a strong model since 2026-11-01.

- **The model list is the account's own:** `GET /v1/models` → `models[]` with `visibility: "list"`, in the server's order (`model_catalog.rs`). The picker shows it as it comes (`ppModels.js` `accountList`).
- **Its web search is OpenAI's own, on the plan** (`web/native.rs` `NativeSearch::plan`): the same sub-call, streamed and stateless, read from `response.completed`.

### On screen, in OpenAI's words

Following its UI rules (`ChatGptSignIn.svelte` inside `ChatGptProvider.svelte`, `stores/chatgpt.svelte.js`, `ModelChip.svelte`, `ChatModelPicker.svelte`):

- **Continue with ChatGPT** to sign in.
- **Connected · Using ChatGPT plan**, with the account and **Manage usage**.
- The one-time **"You're using your ChatGPT plan"** notice, with **Got it** (`chatgpt.json` `noticeSeen`).
- Chat's model chip says "Using ChatGPT plan", with a Manage usage link beside it. On an OpenAI API key it says "OpenAI API key" instead, since the one entry holds both.
- **Every scope** (checked 7 Nov 2026): Dictation Cleanup, Commands and Note Formatting take the plan as Chat does — every call goes through `llm::post_chat_request` to `responses.rs`, and each scope's ChatGPT tab is the same `ChatGptProvider.svelte` — with what each way is said under the switch ("Your ChatGPT plan: uses your plan's limits." / "Your OpenAI API key: billed per use."; side by side for Dictation Cleanup and Commands, `WayNote.svelte`; no speed claimed: both are OpenAI's API) and the selected model's row saying which pays. The one gap, fixed then: signing out left the marker in Commands' and Note Formatting's own key, so they looked connected (`clear_provider_key` now clears every scope).

### Receipts

- The sign-in, its keys and its refreshes: "Sign in with ChatGPT" (`Purpose::ChatGptSignIn`).
- The calls: "ChatGPT plan" (`Purpose::Ai("chatgpt")`).
- The model list: a model list.

### Tests

- The flow's pure parts are tested in each module.
- `desktop/e2e/chatgpt.spec.js` runs against a local fake of OpenAI's sign-in and Responses API (`desktop/e2e/support/fake-chatgpt.js`): PKCE, a signed ID token, rotating refresh tokens, a namespaced tool step whose reasoning must come back whole, the usage limit, revocation; and the one ChatGPT entry's states with the plan (the plan only, both with "Your plan | API key", in the picker and Settings; a key only and nothing connected in Settings). `desktop/e2e/chat-model-picker.spec.js` has the picker's nothing-connected and key-only states.
- Debug builds point at the fake with `YAP_CHATGPT_ISSUER` and `YAP_CHATGPT_API`.
- Evaluations run on the real plan with `chatgpt:<model slug>` once the maintainer has signed in.

## The Claude Code engine (2026-11-07)

`ai/claude_code.rs` (+ `claude_code/`): Yap drives the person's own installed
Claude Code CLI as a child process. Two features build on it: **Your Claude
plan** (Chat's answers on the person's plan, below) and **Jobs** (Chat hands a
task to Claude Code in one of the person's folders: [jobs.md](./jobs.md)).

**Anthropic's terms** hold for both, and are written down once, in
[jobs.md](./jobs.md#anthropics-terms): the person's own unmodified Claude
Code, their own login (plan or API key), Claude Code's own sign-in, Yap never
reading or storing Claude credentials, and never "Claude Code" in a feature's
name.

### Finding it (`cli.rs`)

- **Where**: `%USERPROFILE%\.local\bin\claude.exe` (the native install) first,
  then `PATH` with `PATHEXT` (`which`). An npm `claude.cmd` shim runs through
  `cmd /d /s /c`, and only with arguments that need no batch escaping: Yap
  never puts a prompt, JSON or a system prompt on the command line.
- **Version**: `claude --version` ("2.1.296 (Claude Code)"). Older than
  **2.1.259** (`MIN_VERSION`, the first with `--permission-prompts`) is "too
  old".
- **Signed in**: `claude auth status --json`, exit code 1 when signed out.
  Only `loggedIn`, `authMethod`, `apiProvider` and `subscriptionType` are
  read; anything else it says (an email, an organisation) is never kept.
- `status` is cached for 30 s (`refresh` probes again); the commands
  `claude_code_status {refresh?}` and `claude_code_sign_in`.
- **Sign in** (`open_sign_in`): Windows Terminal, else PowerShell, on `claude
  auth login`, never piped and never in Yap's kill-on-close job (it stays
  open if Yap quits). Test runs open nothing. The terminal itself
  (`cli/terminal.rs`, shared with a Job's Open in terminal): `wt.exe` also
  found off `PATH` (its WindowsApps alias), a console of its own with none
  of Yap's handles, out of Yap's job ([jobs.md](./jobs.md)).
- Debug builds: `YAP_CLAUDE_EXE` names the program to run instead, and
  `YAP_CLAUDE_ARGS_PREFIX` (a JSON array) the arguments before Claude Code's
  (`["fake-claude","<script.jsonl>"]`). A test run without them finds no
  Claude Code, so the e2e suite never reaches the person's own.

### A session (`session.rs`)

One process holds one conversation: `claude -p --input-format stream-json
--output-format stream-json --verbose --include-partial-messages`, then what
the `Spec` asks for. Every user message on stdin is a turn, and every turn ends
with a `result` (checked on 2.1.296: two messages, two results, one
`session_id`).

- **The `Spec`**: the folder, model (a full id), `--effort`, `--permission-mode`
  (always set: from 2.1.302 a `-p` run starts in manual mode when nothing sets
  one; `dontAsk` by default), `--permission-prompt-tool`, `--tools`,
  `--allowedTools` / `--disallowedTools`, `--mcp-config` and
  `--strict-mcp-config`, `--setting-sources`, `--settings`,
  `--append-system-prompt-file` or `--system-prompt-file` (a prompt in place
  of Claude Code's own: plan chats only), `--add-dir`, `--session-id` /
  `--resume`, `--max-budget-usd` (key runs only), `--fallback-model`,
  `--no-session-persistence` (`persist: false`: a run nothing resumes saves no
  transcript; `-p` only, checked in 2.1.296's `--help`),
  `--disable-slash-commands` (a Code chat's session: no message runs one of
  the person's own commands or skills).
- **Files, not arguments**: the settings, the MCP config and the system prompt
  are written to a folder of the run's own, `%TEMP%\yap-claude\<id>-<tag>\`,
  removed when the session goes. So nothing needs escaping for a shim and
  Windows' 32 K command-line limit never bites. Each folder names the Yap
  that made it (`yap-owner`: its process id and when it started), and at
  start (`claude_code::init`) **the sweep** removes the ones a crashed Yap
  left (`sweep_run_folders`): its Yap no longer running, or no owner and a
  day old. Only folders named as Yap names them, never a link, never another
  running Yap's (a dev build beside the installed one), never anything else
  in `%TEMP%`.
- **A run's secrets never go in a file**: the run endpoint's key is in the
  run's environment under `YAP_RUN_KEY` (`session::RUN_KEY_VAR`, through
  `Spec::env`; `RunHandle::env`), and its MCP config and hook settings name
  it as `${YAP_RUN_KEY}` (the hook lists it in `allowedEnvVars`, as an http
  hook must). Claude Code fills it in itself. An inherited `YAP_RUN_KEY` (a
  Yap started from a job's command) is taken off every run.
- **Paying is never ambiguous** (`environment`): Yap's environment with every
  `CLAUDE…` and `ANTHROPIC_…` variable taken off (`cli::clean_env`;
  `CLAUDE_CONFIG_DIR` stays, and Yap's own `YAP_…` names), then
  `ANTHROPIC_API_KEY` set for a key run and absent for a plan run. A
  `Spec::env` entry can't put any of them back. So a Bedrock, Vertex or proxy
  setup made only with environment variables doesn't reach a run. And
  `system/init`'s `apiKeySource` is checked against the way asked
  (`Problem::PaysAnotherWay`).
- **Its transcripts** (`transcripts.rs`): Claude Code saves each session in
  its own config folder (`CLAUDE_CONFIG_DIR`, else `~/.claude`) as
  `projects/<the working folder, encoded>/<session id>.jsonl`, with the
  session's own files in `<session id>/` beside it. The folder's name, read
  from 2.1.296's own code: every UTF-16 unit that isn't an ASCII letter or
  digit becomes `-` (`E:\Projects\garden-planner` →
  `E--Projects-garden-planner`), and a name over 200 is cut to 200 with `-`
  and a base-36 hash added. `remove` deletes given sessions' transcripts for
  one working folder (its folder matched without case, or one whose name
  ends as its last two parts do: `…-claude-chat`), only files named with
  those session ids (UUIDs), nothing else. Test runs look only in a
  `CLAUDE_CONFIG_DIR` they set.
- **The prompt goes over stdin** (`send_user`) as a stream-json user message,
  pictures first as base64 image blocks.
- **The plan's usage** (`get_usage`, `usage.rs`): Claude Code's
  experimental control request `{"subtype":"get_usage","skip_behaviors":true}`
  on a live session, or on a one-off probe (`usage::probe`: the one-shot
  calls' flags, nothing sent but the request, 15 s at most, always ended).
  Verified on 2.1.296 signed in on a Max plan (7 Nov): a `control_response`
  in 1.9 s with the process start, no model call. [Plan usage
  limits](#plan-usage-limits-2026-11-07) says what's read of it.
- **Stopping** (`interrupt`): the Agent SDK's control request
  (`{"type":"control_request","request_id":…,"request":{"subtype":"interrupt"}}`)
  when `system/init` lists `interrupt_receipt_v1`; 2.1.296 answers with a
  `control_response` (`success`, `{"still_queued":[]}`) before the turn's
  `result`. Otherwise stdin is closed and the process tree ended. Windows has
  no SIGINT; `kill` ends the tree.
- **Spawned like the speaker engine**: piped stdio, no window, in Yap's
  kill-on-close job (`procs::end_with_yap`), a reader thread turning stdout
  into `Event`s on a channel, stderr to the log at debug.
- The log at `info` has ids, counts and outcomes, never the prompt or an
  answer.

### Its output (`events.rs`)

A tolerant parser: one `Event` a line, any type Yap doesn't know kept whole
(`Other`), a line that isn't JSON counted by its length (`NotJson`), never
a failure. It reads `system/init` (model, tools, MCP servers, `apiKeySource`,
`capabilities`), `system/status`, `system/api_retry`,
`system/permission_denied`, the hooks' events, `stream_event` (text and
thinking deltas), `assistant` (text, `tool_use`, thinking), `user` (tool
results), `result` (`session_id`, `usage`, `total_cost_usd`, `terminal_reason`,
`permission_denials`, the final text, `modelUsage`'s context windows),
`rate_limit_event` (with its `rateLimitType` and `unifiedWindows`),
`tool_progress` and control messages. Each model call's own tokens come
from `message_start`'s `usage` (the prompt, the cache's included) and
`message_delta`'s `output_tokens`: a `result`'s `usage` is the turn's sum.

- **Problems are derived** (`Classifier`), once per turn, just before the
  event that showed them: `NotSignedIn`, `RateLimited` (with when it resets,
  or that usage credits are needed), `Billing`, `Account`, `PaysAnotherWay`.
  Each has its words (`Problem::message`: "Claude Code isn't signed in on this
  PC. Sign in to Claude Code, then try again.", "You've reached your Claude
  plan's limit (resets at 4:00 pm).") — the page matches them for its buttons
  (`core/aiErrors.js` `claudePlanProblem`).
- **Checked on 2.1.296, signed out**: a turn that can't reach Claude still
  ends with a `result`, `subtype: "success"` but `is_error: true`,
  `terminal_reason: "api_error"` and the words "Not logged in · Please run
  /login" (the `assistant` message before it carries `error:
  "authentication_failed"`): that's `NotSignedIn`, never an answer.
  `system/init` comes again before every turn. MCP and hook headers take
  `${VAR}` from the environment. An http MCP server is first probed with
  `server/discover`, then `initialize` with 2025-11-25.

### The stand-in (`fake.rs`, debug builds)

`yap.exe fake-claude <script.jsonl> [Claude Code's arguments…]` replays a
script as Claude Code would (main.rs runs it before the app). A script line is
printed as it is, with `{{session_id}}`, `{{user_text}}` and `{{cwd}}` filled
in; a line whose first key starts with `_` is an instruction: wait for the
next user message, sleep, call an http hook from its `--settings`, call a tool
of its `--mcp-config`'s server (`_mcp`; with `result_for`, the answer also
handed back as that tool call's `tool_result`, as Claude Code gives it to the
model), dump its arguments (`_dump_args`; with a file path, appended there
too, for an e2e spec) or which variables it has (never a value), write a
file in its folder, write to stderr, exit, answer `get_usage` (`_usage`:
the body to answer with from then on; none, or `null`: refused, as an
older Claude Code). `{"_for":"chat"}` (or `"job"`,
`"ask"`, `"all"`) plays the lines up to the next `_for` only in that kind of
process — a Job's run (`--permission-prompt-tool`), a plan chat's session (an
`--mcp-config`, no prompt tool), one question (neither) — so one script plays
a plan chat, the job it starts and the report's question. An interrupt is
answered as 2.1.296 does. Headers expand `${VAR}` as Claude Code does (an
MCP server's from the environment, an http hook's only for the variables in
its `allowedEnvVars`), so a run's key reaches Yap's endpoint only through the
run's environment. With `YAP_FAKE_CLAUDE_TRANSCRIPTS=1` and a
`CLAUDE_CONFIG_DIR` (both: never the person's own `~/.claude`) it keeps each
session's transcript as Claude Code does, unless started with
`--no-session-persistence`. `--version` says 2.1.296
(`YAP_FAKE_CLAUDE_VERSION` for another), `auth status` says signed in
(`YAP_FAKE_CLAUDE_LOGGED_IN=0`: signed out, exit 1). Recordings per Claude
Code version in `claude_code/fixtures/<version>/` (`real-*` recorded signed
out with an empty config folder, `synthetic-*` built from the documented
shapes) replay too; its own examples are in `fixtures/fake/`.

**Seen on a real signed-in Claude Code** (the maintainer's, 2026-11-07): a
plan chat, a job, `get_usage`. **Not yet seen**: an http `SessionStart` hook
didn't fire in a real `-p` run (not looked into).

## Your Claude plan (2026-11-07)

`ai/claude_plan.rs` (+ `claude_plan/`): Yap's AI from Claude on the person's
own Claude subscription, through their own Claude Code, in every Language
Models scope: Chat's and the Voice Assistant's answers, and (7 Nov 2026) a
dictation's cleanup, Commands and saved transforms, and the meeting notes. It
sits beside the Anthropic API key as the ChatGPT plan sits beside an OpenAI
key. Yap never calls Anthropic itself with a plan: Claude Code does, on the
person's own sign-in.

### The provider

- **"claude-plan"**, a keyed provider. Its key store holds the marker
  `PLAN_KEY` ("claude-plan") while Claude Code is found here, new enough and
  signed in (`sync`), and drops it once it isn't. So every "has this
  provider a key?" check works as for the others; `set_provider_key` takes
  nothing else for it. It's never a credential.
- **A placeholder base URL**, `claude-code://plan` (an empty one means "no
  model set up"): nothing is ever sent to it.
- **Its models** (`MODELS`, cheapest first): `claude-haiku-5-5`,
  `claude-sonnet-5-5`, `claude-opus-5-5`, `claude-fable-5-1`. Full ids, never
  aliases, so every rule keyed on a model id reads them as Anthropic's
  (`as_api`): effort (as `--effort`; no Fast mode through Claude Code),
  images, context sizes, the strong models, Latest. `model_catalog.rs` lists
  them: the CLI has no list of its own.
- **Every scope** (7 Nov 2026; until then the Chat scope only): Chat & Voice
  Assistant (Chat, the Notes view's Ask, the Voice Assistant and the
  background calls on the Chat model), Dictation Cleanup (also a question
  cleaned up, a cleanup style's or an app rule's own model), Commands (the
  rewrite shortcut, saved transforms) and Note Formatting (meeting summaries,
  the notepad's helpers). The maintainer: "dictation cleanup can be used
  through the subscription system as well as an API key".
- **Whose call it is** (`Scope`, `scope_url`): each scope's endpoint names
  its scope in the placeholder URL — `claude-code://plan/cleanup`,
  `…/commands`, `…/notes`, and Chat's bare `claude-code://plan` — set where
  the endpoint is resolved (`local_llm::effective_endpoint` and a cleanup
  profile's: cleanup; `pipeline::commands_model`: Commands;
  `meeting_summary::resolve_endpoint`: Note Formatting;
  `turn::resolve_chat_endpoint`: Chat, also on the cleanup's model). A scope
  that's off runs on the cleanup's model, under the cleanup's settings
  (Chat's stays Chat's). The call reads its warm spare, effort and fallback
  from it.
- **The key store's marker in every scope**: `set_provider_key` and
  `clear_provider_key` keep every scope on a provider in step (one key per
  provider, a plan's marker in each scope that uses it), so a sign-out takes
  the plan out of Commands and Note Formatting too, and the ChatGPT plan's
  likewise.
- **At start** (`init`): only for someone already on the plan (its marker
  set, or a scope on it), a look at Claude Code in the background, so a
  sign-out since is noticed. Nobody else's Claude Code is started by Yap on
  its own.
- `claude_plan_status {refresh?}` (Settings' card, Chat's picker; it keeps
  the marker in step) and `claude_plan_sign_in`. `on_a_key`: signed in with a
  Console API key, which isn't a plan.

### A Chat turn (`chat/turn/plan.rs`)

How a plan turn runs (a session per conversation, kept warm 10 minutes in
`claude_plan/sessions.rs` and `claude_sessions.json`; Yap's own chat prompt in
place of Claude Code's; Claude Code's web tools and Yap's read-only tools; the
job tools while Jobs is offered; its events into Chat's stream) is in
[chat-and-assistant.md](./chat-and-assistant.md#chat-on-your-claude-plan-2026-11-07),
and its job tools in [jobs.md](./jobs.md#from-a-plan-chat). A plain plan chat
runs lean, in an empty folder of Yap's (`claude\chat\` in the data folder),
with none of the person's own Claude settings.

**A plan chat in a Code chat** (7 Nov 2026, [jobs.md](./jobs.md#memory-and-the-plan-in-a-code-chat))
is a Claude Code session in the workspace, project-aware and read only, in
place of the lean setup: the workspace as its folder, the person's own
user, project and local settings (`--setting-sources user,project,local`:
their CLAUDE.md, CLAUDE.local.md and Claude Code's own memory), Claude
Code's own prompt with Yap's Code chat words added
(`--append-system-prompt-file`: it answers about this workspace's code,
reads files to, and starts a job for any change), `--tools Read,Grep,Glob`
(plus the web's with the web on; never an edit tool, Bash or PowerShell —
`--tools` is what the model gets, whatever their settings allow; 2.1.296
has no LS), `dontAsk` (reads in the workspace need no rule; anything that
would ask is denied), `blockReadsOutsideWorkingDirectories` in the run's
own `--settings`, `--disable-slash-commands`, and `--strict-mcp-config` with
Yap's endpoint only, without `search_memory`. Its prompt has no memory and
About you without its note. Another workspace is another session (the
fingerprint has the folder; `claude_sessions.json` keeps it as `cwd`). Its
events' `Read`, `Grep` and `Glob` show as "Reading app.ts", "Searching the
code · …" (`claude_plan/map.rs` `Tool::Code`), no chip.

**Claude Code's own copy of a plan chat.** A chat session has to be saved
for `--resume`, so Claude Code keeps its transcript in its own config folder
(`projects/<…-claude-chat>/<session>.jsonl`, `claude_code/transcripts.rs`),
outside Yap. **Yap deletes it with the chat**: Chat's delete
(`sessions::forget`), Delete some of your data → Chats and Reset app
(`sessions::reset`) delete the transcripts of the sessions
`claude_sessions.json` names, and a session a conversation moved on from
goes as soon as it's replaced (`sessions::put`). Each in the folder it ran
in (`Kept::cwd`): Yap's `claude/chat/`, or a Code chat's workspace, whose
projects folder also holds the person's own sessions and their jobs' —
only the files named with the chat's own session ids go there, never
another session's (a job's sessions are theirs, as if they'd run Claude
Code themselves). Settings' plan card
says it in one line: "Claude Code keeps its own copy of each plan chat; Yap
deletes it with the chat."

### Every other call (`claude_plan/oneshot.rs`)

Every plan call that isn't a Chat turn — a dictation's cleanup, a Command, a
saved transform, a meeting's summary and the notepad's helpers, and on the
Chat model memory learning, a long chat's summary, the Notes view's and the
notepad's Ask, Think, the apps' plan, Yap's own fast answers, a snip's
question, the Voice Assistant's look at the screen — arrives at
`llm::post_chat_request` as an OpenAI-shaped request and comes here, as the
ChatGPT plan's go to `llm/responses.rs`. Cleanup's own code doesn't change:
only where its call goes.

- **Lean**: `--tools ""` (no tools at all), no MCP server
  (`--strict-mcp-config` with none), `--setting-sources ""` (none of the
  person's settings, hooks or CLAUDE.md), in an empty folder of Yap's
  (`claude\ask\` in the data folder) (`ask_spec`, `call_spec`).
- **Not saved**: `--no-session-persistence`. Nothing resumes a one-shot
  call, so Claude Code keeps no transcript of it.
- **Flattened**: its system messages are the prompt (`--system-prompt-file`,
  in place of Claude Code's own), the rest one message (the earlier turns in
  a block, then the question), with that question's pictures as image
  blocks. Tools a caller offers aren't run: the answer is text.
- **A call's own words in its message** (`per_call_parts`): the parts of a
  system prompt that change with every call — the dictionary words a
  dictation says (`llm::DICTIONARY_LINE`'s paragraph) and a rewrite's
  selection (`llm::SELECTION_LINE` to the end) — go just before the
  question, so the system prompt stays the same from one dictation to the
  next into one kind of app and a warm spare fits it (below). The model
  reads the same words.
- **Effort** (`effort_for_scope`): a Chat call at Chat's choice for the
  model; a cleanup and a Command at the lowest level the model takes
  (`--effort low` on Claude 5: a dictation can't wait for thinking; none for
  a model without levels); Note Formatting at the model's own default (no
  `--effort`). Only Chat has an effort setting.
- **Models**: a scope moved onto the plan starts on its first model, Claude
  Haiku 5.5, the fastest (`MODELS` cheapest first).
- **No marker, no start**: a call with the key store's marker missing
  (Claude Code signed out, missing, too old) never starts Claude Code: it's
  the plan's problem at once (below).
- **Streamed** to whoever watches (`Delta::Content`, `Reasoning`).
- At least 60 s, since Claude Code starts first. A dictation's cleanup has
  the bar's "Taking longer than usual…" and Skip cleanup, as any.

### A warm spare per scope (`claude_plan/spare.rs`, 2026-11-07)

Starting `claude -p` costs the start-up before Claude Code reads its first
message. Measured on this PC (2.1.296): `claude --version` 31–71 ms and
`--help` 150–270 ms (the bundle alone, no session); a `-p` process's
`get_usage` answered 1.9 s after its start (verified 7 Nov, the plan's usage
probe), so a session's start is about 1.5–2 s, the time a dictation's cleanup
would wait for every time. So for **Dictation Cleanup, Commands and Note
Formatting** on the plan, Yap keeps **one Claude Code per scope already
started and waiting on stdin** (stream-json input, the one-shot's own lean
flags): a call's prompt goes to it at once, and a new spare starts behind it.

- **One call each**: a spare serves exactly one call, then its stdin is
  closed and it ends. Never a second call in the same process: no dictation
  sees another's text.
- **Started for what's coming**: Claude Code reads `--system-prompt-file` as
  it starts, so a spare is made for the system prompt the scope's last 8
  calls had most often (the latest of equals; in memory only), with the
  scope's model and effort (`Key`: model, effort, the prompt's hash). A call
  takes it only when all three match; any other call (another kind of app's
  style, another model) runs cold exactly as before, and the next spare
  follows what's most likely. With the per-call parts moved to the message,
  dictations into one kind of app always match.
- **Only while it's wanted** (`wanted`): only while that scope is set to the
  plan (Dictation Cleanup: cleanup on) and the plan is connected (the
  marker). Nothing at all the first time after Yap starts (no prompt seen
  yet): that call runs cold and the first spare starts behind it.
- **Ended**: idle 10 minutes (`IDLE`, the sweep every 30 s); started again
  at the next dictation's key press (`warm`, from `load_ai_early`, as the
  on-device model loads on demand: Dictation Cleanup for a dictation,
  Commands for a dictation or the rewrite shortcut); on sign-out (`sync`, a
  call's `NotSignedIn`); when a save takes the scope off the plan or onto
  another model or effort (`config_changed`, from `apply_config`); when the
  test stand-in changes (`set_test_program`); when Yap quits
  (`shutdown_cleanup`; the kill-on-close job ends it anyway).
- **Quietly replaced**: a spare that died, or won't take the message, is
  dropped and the call starts its own; a call never waits for a spare. One
  that wouldn't start: none tried for a minute.
- **Chat's one-off calls run cold**, as before (its turns have their own
  warm sessions); Note Formatting's calls are background ones, but a
  notepad helper is one a person waits for.
- **No window, no folders of its own**: spawned as every run is (piped,
  `CREATE_NO_WINDOW`, in the kill-on-close job), in `claude\ask\`, its run
  folder in `%TEMP%\yap-claude\` named for the run-folder sweep and removed
  when it ends.
- **Measured on the stand-in** (`spare::tests::a_spare_hides_claude_codes_start_up`,
  `YAP_FAKE_CLAUDE_START_MS=1500`): a cold call 1531–1554 ms, the same call
  on a spare under 1 ms of Yap's own (three runs). With the real Claude Code
  the model's own time remains; the start-up above is what's saved.
- Logs: `Claude plan: a one-shot call answered` with `scope`, `warm`
  (whether a spare served it) and `ms`; spares started, ended and why.
  Never a prompt.

### If the plan can't answer: the key (opt-in, 2026-11-07)

The API key stays the quickest way for every scope; the plan is the one that
uses the person's subscription. For Dictation Cleanup, Commands and Note
Formatting on the plan, Settings offers **"If your plan can't answer, use my
API key"** (`pp_plan_fallback`, a scope's `plan_fallback`; **off by
default**: it spends money), shown only on the plan with an Anthropic key
saved, never for Chat (its limit message has "Use my API key for this chat"
instead, unchanged).

- **When it's on** (`fallback_key`): a plan call that fails because of the
  plan (`oneshot::Failure::Plan`: Claude Code signed out or missing, its
  limit, paying another way, a billing or account problem, a process that
  won't start or ends before it answers) goes **once** to the Anthropic API
  on the same model id, as the same request (`llm::post_chat_request` with
  provider "anthropic"): counted as the key's usage, its receipt Anthropic's.
  Too slow, or an error the model's request got: not the plan's problem, no
  fallback.
- **When it's off**: the call fails as any does. A cleanup falls back to the
  words as said, as every failed cleanup does; a Command says it couldn't.
- The log says "Claude plan couldn't answer: this call goes to the Anthropic
  API key" with the scope, never the words.

### Usage and cost

- Every call is counted (`usage::record_response`) from the result's `usage`,
  as "claude-plan", with Claude Code's own estimate of the API price
  (`total_cost_usd`; else Anthropic's list price): on a plan, what the API
  would have charged, never spend.
- **AI usage keeps it apart**: the plan's tokens and their "API value"
  (`plan_tokens`, `plan_value_micros`), never in the month's spend.
- **An answer's byline** says which way paid: "Your Claude plan · ≈ $0.0042
  on the API"; on the key, "API key · $0.0042" (`core/aiModel.js` `paidLine`,
  `MessageTokens.costMicros`). Nothing known: the way alone. A job's cost line
  says the same ([jobs.md](./jobs.md#paying-your-plan-or-a-key)).

### Plan usage limits (2026-11-07)

`claude_code/usage.rs` (the request and its answer) and
`claude_plan/usage.rs` (when it's asked, what's kept): where the plan's
limits stand, as the Claude desktop app shows them.

- **The answer**, read tolerantly into `PlanUsage` (the subscription; the
  limits as rows: kind, label parts — "Current session", "Weekly" +
  "all models" or a scoped limit's model, "Fable" —, percent, severity,
  when it resets, active; extra usage as used, limit, currency).
  `response.response` (or `response`): `rate_limits_available` false (an API
  key, Bedrock, Vertex) → nothing; `rate_limits.limits[]` first (`kind`
  `<period>_all`, `<period>_scoped` or `<period>`: weekly, session /
  five_hour, daily, monthly; any other kind left out), the named windows
  (`five_hour`, `seven_day`, `seven_day_opus`, `seven_day_sonnet`,
  `model_scoped[]`) when it gives nothing, and the five-hour window as
  "Current session" when `limits[]` has no session row; a scoped limit at
  0 % that isn't active left out; severity the answer's word (else from the
  percent: 80 % a warning, 100 % critical); extra usage from `spend`
  (`amount_minor` and `exponent`), else `extra_usage` (minor units,
  `decimal_places`), only when it's on. Everything else (account details,
  code-named fields) is never read; a field missing or of another type is
  hidden, never an error. The shape is experimental: the fixture
  (`fixtures/2.1.296/synthetic-get-usage.json`) is synthetic, no real
  account's values, with unknown fields kept to prove they're ignored.
- **When it's asked**, never on a timer and only while the plan's marker is
  in the key store: after each plan turn on the conversation's warm session
  (in the background, once the turn lets go of it; also after the plan's
  limit stopped a turn); when a composer showing the plan chip mounts, the
  window gets the focus back, or Settings' card shows — at most once a
  minute, on a free warm session, else a one-off probe (none while every
  warm session is in a turn: that turn asks after it). An answer with
  nothing to show, a refusal or none at all: no probe for 30 minutes (and a
  session that didn't answer in 5 s isn't asked after a turn for 30
  minutes: the next question would wait behind it). **Beside a cleanup's, a
  Command's or a meeting's call** (2026-11-07, `due_with_call`): the call's
  own process gets `get_usage` right after its message, at most once a
  minute — no other process, no model call; its answer is read as the call
  runs or, after the answer, in the background for 5 s (`kept_from_call`;
  none in time changes nothing). Never one per dictation: the minute's cache
  as for the page's asks.
- **Live**: a turn's `rate_limit_event`s (a one-shot call's too) move the rows they name
  (`rateLimitType`, `unifiedWindows`: its utilization is a fraction, 1.0 at
  the limit; rejected is critical, `allowed_warning` at least a warning).
- **How full the conversation is**: after each plan turn, its last model
  call's prompt and answer (`Mapper::last_call_tokens`, what the next call
  starts from) against the model's window (`modelUsage`'s `contextWindow`,
  else Anthropic's usual, marked "about"). Yap's own context ring isn't
  shown on the plan: it reads the result's `usage`, the turn's sum over its
  calls, which overstates it.
- **In memory only**, never on disk: the latest answer and the last 50 plan
  conversations' fills (a deleted chat's goes with it). The command
  `claude_plan_usage {refresh?}` and the event `yap-claude-plan-usage` carry
  `{usage, contexts}`.
- **Not in Network connections**: it's Claude Code's own request to
  Anthropic, on the person's own sign-in.
- Logs: `info` the outcome and the rows' count, `debug` the percentages.

### Problems, in plain words

Never an answer: the turn says the problem with the buttons that help
(Chat's toast, `chatState.svelte.js`):

| Problem | Says | Buttons |
|---|---|---|
| Not signed in (or Claude Code missing, too old) | "Claude Code isn't signed in on this PC…" | Sign in to Claude Code |
| The plan's limit | "You've reached your Claude plan's limit (resets at 4:00 pm)." | Manage usage (claude.ai's usage page), **Use my API key for this chat** (an Anthropic key saved) |
| Paying another way | "Claude Code on this PC is set to pay another way…" | — (the process is stopped before it asks anything) |

In the other scopes the same problems are the call's error: a cleanup goes
in as said, a Command says it couldn't, a summary says what went wrong —
unless the scope's "If your plan can't answer, use my API key" is on (above).
The cleanup's banner says "Sign in to Claude Code" with **Sign in** while
cleanup is on the plan and Claude Code isn't ready.

### On screen

- **Settings → Language Models → every scope → Cloud providers → Claude**
  (the Anthropic tab, renamed **Claude**; Dictation Cleanup's on
  `LanguageModelsPage.svelte`, the others' on `ScopeProviderConfig.svelte`):
  the plan's card first, then the API key (`settings/ClaudePlanProvider.svelte`,
  [frontend-settings.md](./frontend-settings.md)). The card says what
  Claude Code is here: not installed (its install guide, Check again), too
  old (its version and the one Yap needs), signed in with an API key (not a
  plan), not signed in (**Sign in**), or **Connected · Using Claude plan**
  (its version and plan, Manage usage, **Use for Chat** / **Use for
  Commands**…), with the plan's usage limits under it, small. **What each
  way is** (`WayNote.svelte`), under the switch: "Your Claude plan, through
  Claude Code on this PC: uses your plan's limits. Expect slightly slower
  responses." / "Your Anthropic API key: quickest, billed per use." — for
  Dictation Cleanup and Commands both ways side by side, the one in use
  marked. The selected model's row says which pays ("Using your Claude
  plan" / "Anthropic API key"). Off Chat, with an Anthropic key saved and the
  scope on the plan: **If your plan can't answer, use my API key**.
- **Chat's usage ring** (`views/chat/PlanUsageRing.svelte`, in place of the
  chip's Manage usage on the plan): filled to the highest active limit's
  percent, coloured by its severity; hover or click opens **Plan
  usage limits** — a row per limit ("Weekly · all models · Resets Sat 3:00
  AM", the percent, a thin bar), extra usage ("Extra usage · £0.00 of
  £37.50"), last how full this chat is ("This chat · 16% of context"), and
  **See all →** to `USAGE_URL`. No numbers: no ring at all
  ([frontend.md](./frontend.md)).
- **Chat's picker**: Claude is one entry with "Your plan | API key", as
  ChatGPT is (`TAB_WAYS`, `waysState`). The chip says "Using Claude plan" or
  "Anthropic API key".
- **Receipts**: none. Yap sends nothing for the plan; Claude Code connects
  on its own, outside Network connections ([privacy-receipts.md](./privacy-receipts.md)).

### Tests

- Unit: the engine's flags, environment and files (the run key only in the
  environment, an inherited one dropped), the start-up sweep (a dead Yap's
  folders only), the parser against the recordings, the problems, the
  stand-in round trips (the test binary re-run as the stand-in:
  [build-and-test.md](./build-and-test.md#claude-code-in-tests-2026-11-07)),
  the plan's flattening, mapping, citations and sessions, a one-shot call
  not saved, the transcripts' folder names (as Claude Code's own code makes
  them) and their deletion in a fake config folder (a deleted chat's go,
  another project's stay); the usage answer over the fixture, an API key's
  answer, a refusal, no `limits[]` (the named windows), unknown kinds and
  fields, the body one level up, rate-limit events moving the rows, the
  answer matched by its request id, the fake's `_usage`, the last call's
  tokens against the turn's sum. Every scope (2026-11-07,
  `claude_plan::tests`, `oneshot::tests`, `spare::tests`): each scope's
  endpoint on the plan and whose it is, a scope that's off on the cleanup's,
  the marker out of and back into every scope, the fallback only when the
  scope says (never Chat's), a call's own words in its message, effort by
  scope; the spares against the stand-in — one call each in its own process,
  a new one behind it, another call's or a dead one never used, one that
  wouldn't start not tried again for a minute, ended when idle, on sign-out
  and when the scope changes — and the gain measured.
- E2E: `claude-plan.spec.js` (also: the stand-in keeps the chat's
  transcript in a `CLAUDE_CONFIG_DIR` of the run's own, and deleting the
  chat deletes it, another project's left), `claude-plan-problems.spec.js`,
  `claude-plan-missing.spec.js`, `claude-plan-expired.spec.js`,
  `claude-plan-limit.spec.js`, `claude-plan-usage.spec.js` on the stand-in
  ([`docs/e2e-tests.md`](../e2e-tests.md)); `claude-plan-scopes.spec.js`
  (every scope's Claude tab, a dictation cleaned and a Command run on the
  plan, two dictations on two processes — the second a spare —, usage under
  "cleanup", spares ended) and `claude-plan-scopes-signed-out.spec.js` (Sign
  in on the banner and the cards, the fallback off then on, against a fake
  Anthropic: `YAP_E2E_ANTHROPIC_API`, debug builds only). The ChatGPT plan
  beyond Chat: `chatgpt.spec.js`.
- **Needs a real signed-in check**: a cleanup and a Command on the real
  plan (the answer, `--effort low` on Haiku 5.5, the time it takes with a
  spare), `get_usage` sent beside a message on the same process (answered
  mid-turn, after it, or not at all with stdin closed), and a spare left
  waiting 10 minutes (Claude Code's own idle behaviour).
- **On a real signed-in run** (2026-11-07): a plan chat answered, and the
  plan's usage limits matched Anthropic's own figures. **Not yet confirmed**:
  the stream's finer shapes (one `assistant` event per block, ids, WebSearch
  and WebFetch results), `mcp__yap__*` under `dontAsk` with `--allowedTools`,
  `--resume` after a warm session ended and a mid-turn interrupt, real
  rate-limit events during a turn and their reset (`utilization` read as a
  fraction from the recordings; `unifiedWindows`' shape unseen), the
  pays-another-way check, whether the plan takes `claude-fable-5-1`,
  screenshots as image blocks (no e2e either), and `limits[]`' kind for the
  five-hour session (only weekly kinds were seen; the `five_hour` window
  stands in).

## Model evaluations (`npm run eval`)
How real models do on Yap's own Chat code, with every request and response
logged: `npm run eval -- --model groq:openai/gpt-oss-20b --keys-from-yap`,
`--model local:installed` (Yap's llamafile on the CPU; `--ngl 999` on the GPU),
`--only`, `--tools on`, `--raw evals/raw/groq-browser-search.json`, and
`--cleanup` (dictation cleanup latency per model), `--styles` and `--strength`
(cleanup strength, formatting and the text before the cursor, before and after),
`--samples` (writing samples: the voice, the facts and what they cost, without
and with) and `--hinglish` (Hindi in Latin letters: Yap's rules alone, cleanup
from Devanagari and from the rules' Latin).
Nothing launches, no window, the
installed app's data and `%APPDATA%\yap` are never written (keys are read from
its config.json read-only). Builds a debug stub into `$YAP_EVAL_TARGET_DIR`;
runs land in `$YAP_EVAL_OUT` / `--out` (best outside the
checkout, e.g. `<builds>/evals` and `<builds>/eval-runs`). Use it before changing a prompt, a tool or the
default local model. Guide: [`docs/model-evals.md`](../model-evals.md).
