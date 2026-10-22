# AI models

The language-model side: the OpenAI-compatible client and its prompts, the on-device model (llamafile sidecar, GGUF headers, the graphics card), which model a call goes to, the providers' live model lists, Groq usage and the model evaluations. See also [`docs/model-evals.md`](../model-evals.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## Modules

- **`about_you.rs`** — **what every model is told about the person**
  (2026-10-21, Settings → General → About you; Nathan: "It keeps finding
  things in dollars… a better way to give the model better context on the
  user"). **Region and language from Windows** (`detected`): the regional
  format (`GetUserDefaultLocaleName`, "en-GB"), the home location
  (`GetUserDefaultGeoName`, else `GetUserGeoID` → `GEO_ISO2`), the currency
  (`GetLocaleInfoEx` `LOCALE_SINTLSYMBOL` "GBP" + `LOCALE_SCURRENCY` "£", the
  user's own changes included), `LOCALE_IMEASURE` (metric / US) and the time
  zone (`GetDynamicTimeZoneInformation`'s key name → IANA through
  `calendar/tz.rs`' CLDR table). Settings' changes (`config.region_country /
  _language / _currency / _units / _time_zone`, empty = Windows') go on top
  (`resolve`, pure): a picked country brings its own currency (the
  `COUNTRIES` table: ISO code, English name, ISO 4217) and units (`IMPERIAL`:
  US and its territories, Liberia, Myanmar) unless those are picked too; the
  language and the clock stay Windows' until picked. `summary`: "United
  Kingdom · English (UK) · £ GBP · metric · UK time"; `Spelling` British /
  American / Canadian from an English language tag's region (else an
  English-speaking country; none for another language). **The block**
  (`prompt_block(profile, now)`, pure, unit-tested), last in the system
  prompt of Chat, the Voice Assistant, the Meetings view's and the Notes
  view's Ask bars (`commands::chat_turn`, `small_model_answer`'s web
  pipeline, `note_ask`), local and cloud: "About the user: they're in the
  United Kingdom (British English); it's Wednesday 21 October 2026, 21:40 UK
  time (UTC+01:00). Give times in UK time and metric units. Prices: in £ (GBP)
  when you have them in £; never change a price's currency sign — a price in
  another currency stays as it is, labelled, like "$15 a month (US price)".
  Their name is Nathan. They told Yap: "…"" (the example is "£15 a month (UK
  price)" for a US user, and no real plan's price: with "$8 a month" as the
  example and "Give prices in £", Qwen3.5-4B wrote x.ai's
  "$8/month" as "£8/month" 2 times in 3 — the model evaluations,
  model-evals.md) — only
  the parts known; with nothing known, just the date and time (its clock
  changes every minute, so it goes last and the rest of the prompt stays
  cacheable). `about_name` (≤ 60 characters) and `about_me` (≤ 300, one
  line). **Search** is aimed at the country (`web::Settings.country`: Exa's
  `userLocation`, Tavily's `country` via `tavily_country`, the providers'
  own searches — web-and-apps.md); **cleanup** keeps the spelling
  (`cleanup_spelling`, `region_spelling`, on). `about_you_status {overrides?}`
  (Settings): Windows' region, "Automatic" for the picked country, the
  result, its summary, the block as it reads now, the spelling line, the
  pickers' lists. **Test mode** never reads Windows' settings: debug-only
  `YAP_E2E_LOCALE` (a tag, or JSON `{locale, country, currency, symbol,
  metric, timeZone}`) stands in, unset = nothing (the eval's `--locale`).
  Kept in config.json, sent only to the model the person uses; accounts
  don't sync it (they could, later: accounts.md).
- **`llm.rs`** — the AI cleanup client (OpenAI-compatible). Frames the transcript as
  data (delimiters + one-shot) so small models *clean* it instead of *answering* it.
  **The dictation's two messages for a hybrid model** (`cleanup_messages`,
  `split_dictation`, `splits_dictation`; 2026-10-22): the restated
  instruction (with the spelling line) as a user message of its own, then
  "Transcript: …" — the same words as the one message it was, the one-shots
  unchanged — when the model is hybrid: the on-device model whose GGUF has
  recurrent layers (`local_llm::serving_hybrid_at`, `gguf::Shape::is_hybrid`)
  or a Qwen3.5/3.6/Qwen3-Next by name (`hybrid_by_name`). Such a model
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
  **the spelling line** ("Use my region's spelling",
  `about_you::cleanup_spelling`: "When the transcript is in English, spell
  it the British way (honour, recognise, paralyse, metre, cancelled,
  catalogue, defence), without changing any words." — American or Canadian
  examples by region; in the system prompt, `cleanup_system_prompt`, **and**
  after the dictation's restated instruction, `cleanup_message`,
  `SpellingAt::Both`: in the system prompt alone Qwen3.5-4B changed only
  words like its examples and the 1.5B none, docs/model-evals.md) and the
  custom dictionary. Records token/request usage (best-effort). Also carries
  `EDIT_BASE_PROMPT` (edit/rewrite mode's guardrails), `NOTE_BASE_PROMPT` +
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
  working (12 s): the model runs on this PC…" after 8 s. Dictation cleanup and
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
  `reasoning_effort: "none"`, once each.
- **`local_llm.rs`** — the on-device AI cleanup sidecar: runs **Mozilla llamafile**
  (llama.cpp, single-file OpenAI-compatible server) as a hidden child process on a
  free localhost port, serving **Qwen2.5-1.5B-Instruct** (Q4_K_M GGUF) when no
  other is picked (`pp_local_model` empty = `MODEL_FILENAME`) — or a curated
  one (`CURATED_MODELS`, incl. **Qwen3.5-4B**, bartowski's Q4_K_M, SHA-pinned)
  or **any user GGUF** dropped into `<data>/llm/` and picked via `pp_local_model`
  (Settings shows a model picker + "Open models folder"; switching restarts the
  sidecar). **Recommended for this PC** (`recommended_id`, from `gpu.rs`):
  Qwen3.5-4B where the graphics card can run it (2026-10-18, the model
  evaluations: 94–98% of the scenarios at ~1.2 s a turn on an RTX 5070 Ti, with
  Chat's tools; the 1.5B 79–81% without them), else the 1.5B; existing installs
  keep their model and are offered the 4B **once** (`upgrade_offer`: capable
  card, on-device in use, on the 1.5B, not turned down —
  `config.local_llm_upgrade_dismissed`), never downloaded by itself. The
  curated entry's `default` = the bundled file, `needs_gpu` = only recommended
  with a card. **One server set-up** for the sidecar and the evaluations
  (`server_args`): `-ngl 999 -c <context>`, **`--reasoning off`** (no thinking for
  any model: Qwen3.5 loops and times out with it on), **`--cache-ram 0`**
  (llama.cpp's host prompt cache off: with it on, a dictation's cleanup right
  after a Chat turn took 1.3–1.4 s instead of 0.13 s on the 1.5B — the slow
  copy back of the saved prompt — and a 4B server peaked at 6.7 GB on the
  processor; Qwen3.5-4B's peak is now 3.85 GB there, 3.7 GB on the card),
  `--ctx-checkpoints 4` (Qwen3.5's recurrent-state checkpoints, ~50 MB each),
  `-lv 4` (the log names the layers that went to the GPU), and MTP as the
  `Plan` says (below). **The runtime** (2026-10-22): **llamafile 0.10.6**
  (`RUNTIME_VERSION`, mozilla-ai's release, SHA-256 `d579f61d…` = GitHub's
  digest; over 0.10.3: CUDA graphs, a ~430 MiB smaller compute buffer, the
  June tool-call grammar fixes, checkpoints at every user message, the
  Qwen3.5 MTP fix — the 4B's cleanup ~300 → ~245 ms, generation ~116 → ~167
  tokens/s, greedy outputs identical, model-evals.md "Local AI, phase 1").
  **Which one is on disk**: the marker beside it (`llamafile.json`:
  `{version, sha256, unproven, refused}`, written after every verified
  download — no hashing 368 MB at a start); no marker = the 0.10.3 every
  install had (`UNMARKED_RUNTIME_VERSION`). **The upgrade for existing
  installs** (`spawn_runtime_upgrade`, from setup; `ensure_runtime` only ever
  downloads a missing runtime): 90 s after launch, then after a "not now"
  every 30 min doubling to 6 h — only while the on-device model is in use
  and not on a metered connection (`updates::metered`), it downloads the
  pinned runtime to `llamafile-next.exe` (verified; a file already there is
  hashed once and kept if whole; a failed download is retried later, the
  reason logged), checks it runs here (`probe_version`: `--version`, ≤ 60 s,
  tied to Yap's job object), then waits until nothing uses the sidecar — no
  dictation (`pipeline_busy`), no request to it under way
  (`requests_in_flight`), no meeting recording or action plan being written
  (`meeting_end::summary_running`) — idle, then idle again after a settle,
  never under a context restart (it takes `RESTARTING` only once idle, so a
  context change isn't held up), and swaps it in under the start lock:
  stop, `swap_in` (the old runtime and marker kept as
  `llamafile-previous.*`, the new marker `unproven`), start. **Never without
  a working sidecar** (`start_locked`): the first start on an unproven
  runtime that fails puts the previous one back (`roll_back`: the new SHA
  noted as `refused`, never downloaded again) and starts it; if that fails
  too, the fault isn't the runtime's and the new one goes back in
  (`swap_in`); once the new one has started, the previous one is deleted
  (`prove`). Both run as `llamafile.exe`, so the orphan sweep
  (`procs::orphaned`) and the job object cover them. A test run upgrades
  only from its stand-in (`YAP_LLM_SOURCES`) when
  `YAP_E2E_RUNTIME_UPGRADE_MS` says when to look, and never runs either
  (e2e `local-runtime-upgrade.spec.js`). `local_llm_status().runtime`:
  `{version, pinned, upgrade: "downloading" | "waiting" | "", mtp}`.
  **The warm-up** (`warm_up`): right after the sidecar is healthy, one
  cleanup-sized request — the cleanup's own messages as Settings has them,
  a made-up dictation, ≤ 8 tokens — in the background, not through `llm.rs`
  and not counted; the first real request (`request_starting`) cancels it.
  It leaves the cleanup's prompt in a slot: the first dictation after
  loading 599 → ~275 ms on the 4B. **Requests in flight**
  (`request_starting`, called by `llm::post_chat_request` for a call to the
  sidecar's URL): counted while they run — the context restart now waits
  for a Chat answer or digest too (`sidecar_busy`), not only a dictation.
  **MTP** (`plan_for` → `Plan { runtime, mtp }`, `server_args` adds
  `--spec-type draft-mtp`): on 0.10.6 or newer, for a model with the
  prediction layer (Qwen3.5), all on a card with room for it at that
  context (`mtp_fits`: `gguf::estimate` with MTP ≤ `card_room`),
  `config.local_mtp` on (default; no switch in Settings); never on the
  processor — a model that lands (partly) on the processor with it starts
  again without it for the session. **The context**
  (2026-10-20, Nathan's ask: "show what context length it's set at, and let
  me set my own"): `config.local_context` (tokens; 0 = `DEFAULT_CONTEXT`
  8,192, today's behaviour) — one setting for the one sidecar, so Chat,
  dictation cleanup and the meeting notes share it. `effective_context`
  clamps it to `MIN_CONTEXT` 2,048..`MAX_CONTEXT` 1M and to what the model
  was trained for (`trained_context`: its GGUF header's `context_length`,
  `gguf.rs`, else `CuratedLlm::max_context`; a stale file falls back);
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
  through `save_config` (0 for the default), `yap-local-context-changed
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
  notes' final call and the Notes view's Ask grow by (`meeting_summary.rs`
  `FinalBudget`, `commands::note_ask`). Debug-only
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
  sha256}}` — and refuses anything else), process lifecycle (spawn/health-wait/kill + orphan cleanup at
  startup — only sidecars whose parent Yap is gone, via `procs.rs`, so a dev build
  never kills the installed app's; and **the model never outlives Yap**:
  `procs::end_with_yap` puts the sidecar — like the speaker engine
  (`yap.exe diarize`) and eSpeak — in a kill-on-close job object only Yap
  holds, so Windows ends it however Yap ends: Quit, a crash, End task, the
  updater's exit; never used for apps Yap opens, a link's browser or an
  installer), and `effective_endpoint()` which routes `llm.rs` to the sidecar when
  provider = "ondevice" (falls back to the configured endpoint if it's down).
- **`gguf.rs`** — a GGUF model's header, **read-only** (2026-10-20): magic,
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
  more + 36 MiB + one attention layer's KV + a draft compute buffer.
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
- **`gpu.rs`** — the graphics card, for the on-device model recommended for
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
  `this_machine` prints this PC's cards.
- **`ai_model.rs`** — which model a scope's next call goes to, resolved the
  calls' way (`Scope::Chat` → `commands::resolve_chat_endpoint`;
  `Scope::Notes` → `meeting_summary::resolve_endpoint`): `ai_model_status
  {scope}` → `{ready, error, model, host, tools (supports_tools), fromScope,
  provider (the one the scope is set to, ready or not), keyed, maskedKeys
  (each keyed provider's key as "gsk…AbCd", `mask_key` — never the key),
  servers, onDevice, context (the on-device model's, when the call goes to
  it: the chip's tooltip "Context: 8K tokens")}` for the Chat view's model chip + picker and the Ask
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
  another GGUF stops and starts again on it), saved through `save_config`,
  then `yap-chat-model-changed {scope, ppLocalModel?}` (Settings adopts
  both). `chat_provider_key {provider, key}` = the picker's "Connect
  Claude": `set_provider_key` saves it where Settings does (`pp_api_keys`,
  cleanup's `pp_api_key` when cleanup is on it, the Chat scope's key when
  Chat is), through `save_config`, whose `model_catalog::on_config_saved`
  checks that provider's list at once; the page tells Settings' copy with
  the window event `yap-provider-key-saved` (not broadcast to every
  window). `wake_on_device` (in `chat_send` and
  `note_ask`): a chat on the on-device model starts the sidecar
  (`local_llm::ensure_started`) or says it isn't installed, never a silent
  fallback to the endpoint underneath. `save_config` emits
  `yap-config-saved`, which the chip re-reads on. Unit-tested.
- **`model_catalog.rs`** — **the AI providers' live model lists**
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
  **Self-hosted / custom servers** (`llm_models {provider: local|custom,
  scope, baseUrl}`): `GET {base}/models` with the scope's key (OpenWhispr's
  OpenAICompatiblePanel reading: `data` or `models`, `id` or `name`), kept a
  minute in memory, plus what the server says of each model's **context**
  (`server_contexts`, read-only, best-effort, 3 s each: LM Studio's `GET
  /api/v0/models` `max_context_length` / `loaded_context_length`, else
  Ollama's `GET /api/ps` (a loaded model's `context_length`) and `POST
  /api/show` per model, ≤ 16 (`model_info.<arch>.context_length`, a
  Modelfile's `num_ctx`); `ListView.contexts [{id, max, set, app}]`; a
  server that's neither says nothing). The picker shows it first in the row
  ("8K context, set in Ollama · localhost:11434", "Up to 128K · …"). Commands `model_catalog`, `model_catalog_refresh
  {provider}` (Settings' Check now), `llm_models {provider, scope?, baseUrl?,
  force?}`. Logs: provider + counts. Test mode never contacts a provider:
  debug-only `YAP_MODELS_STANDIN` (`<standin>/<host><path>?<query>`,
  `e2e/support/fake-llm.js`); none set = nothing fetched; a self-hosted
  server only on 127.0.0.1. `#[ignore]`d `real_saved_lists`
  (`YAP_MODEL_LISTS=<dir>`) runs the parsers over saved real lists, never
  fetching.
- **`usage.rs`** — daily Groq usage tracker (tokens summed locally + requests from
  `x-ratelimit-*` headers), persisted to `groq_usage.json`, auto-resets at midnight
  UTC; powers the `get_groq_usage` command + `groq-usage` event.
- **`eval.rs`** (+ `eval/`) — **model evaluations**, `yap.exe eval …` (main.rs;
  debug builds only): real models (Groq/OpenAI/Anthropic/OpenRouter/Gemini/
  custom, or a GGUF on Yap's own llamafile started for the run — the installed
  runtime and model read-only, else downloaded via `local_llm::download_verified`)
  through **`commands::chat_turn`** itself (fast path, tool loop, plain chat +
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
  stand-in server (`eval/standin.rs`, through `YAP_EXA_URL` /
  `YAP_TAVILY_URL` / `YAP_WEB_STANDIN`). Logs per run in `<out>/<stamp>-<label>/`:
  `transcript.jsonl` (every request body incl. tool schemas, every response,
  429s/waits, tool calls + results, keys redacted), `summary.md`/`.json`.
  Paces Groq to its 8k tokens/min (and its rate-limit headers). `--raw` sends
  hand-written requests (the Groq `browser_search` spike). A local model's
  llamafile runs with the sidecar's own `local_llm::server_args` (thinking off,
  caches capped; `--llamafile-arg` after them overrides) and sends its GGUF's
  name as the model id, as the sidecar does; a voice turn is the Voice
  Assistant's (scope "assistant"). **`--cleanup`** (`eval/cleanup.rs`)
  times `llm::cleanup` itself over `evals/cleanup/dictations.json` — back to
  back, after a Chat turn, after a long one — with llama.cpp's prompt tokens
  processed, and keeps every output (`cleanup.md`), then the `spelling`
  dictations (words English spells two ways, as speech-to-text writes them):
  how many come out British or American under the run's locale.
  **`--locale`** stands in for Windows' region (`YAP_E2E_LOCALE`, About you):
  without it no region reaches the prompts. A scenario with `needs_web` and
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
  (`eval/serving.rs`): `stream` (time to the first token, streamed vs
  not, and streamed tool calls), `layout` (a Chat turn's prompt re-read
  with the time + notes in the system prompt vs the last user message),
  `loop` (a tool step after one that offered other tools), `embed` (an
  embedding model's speed and recall vs Chat's keyword search);
  `--temperature` / `--set` lay fields over every request
  (`eval::adjust_body`, in `llm::post_chat_request`). The hooks in
  `llm.rs` (`eval::Call`, `eval::adjust_body`), `tools.rs`, `config.rs`,
  `notes.rs` are no-ops outside a run. See
  [`docs/model-evals.md`](../model-evals.md).

## Model evaluations (`npm run eval`)
How real models do on Yap's own Chat code, with every request and response
logged: `npm run eval -- --model groq:openai/gpt-oss-20b --keys-from-yap`,
`--model local:installed` (Yap's llamafile on the CPU; `--ngl 999` on the GPU),
`--only`, `--tools on`, `--raw evals/raw/groq-browser-search.json`, and
`--cleanup` (dictation cleanup latency per model). Nothing launches, no window, the
installed app's data and `%APPDATA%\yap` are never written (keys are read from
its config.json read-only). Builds a debug stub into `$YAP_EVAL_TARGET_DIR`;
runs land in `$YAP_EVAL_OUT` / `--out` (Nathan's PC: `E:/yap-builds/evals`,
`E:/yap-builds/eval-runs`). Use it before changing a prompt, a tool or the
default local model. Guide: [`docs/model-evals.md`](../model-evals.md).
