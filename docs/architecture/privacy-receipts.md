# Network receipts and Report a problem

Two trust features (2026-10-24): **Network connections**, a list of every
connection Yap made and why (`privacy/net.rs`, Settings → Privacy & data), and
**Report a problem**, a diagnostic file the person checks before saving it
(`privacy/diagnostics.rs`, Settings → About). Logging's own rules (no user content at
`info` and above) are in [app-shell.md](./app-shell.md) (Logging).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## What the list promises

Honest, not absolute. Yap transcribes on the PC, but it also talks to the
cloud providers the person picks (Groq, OpenAI, Anthropic, Gemini,
OpenRouter, a self-hosted server), ElevenLabs, Exa/Tavily and the wikis,
model downloads, GitHub's updates, the optional account service and the
calendars. The list shows **every** connection, cloud ones included, so the
person sees what their choices send where. It never says "nothing leaves".

## `privacy/net.rs` — the recorder

- **One place.** Every reqwest request goes out through
  `net::Recorded::send_for(Purpose)` (an extension trait, in place of
  `.send()`) → `net::send`: the request is built (`build_split`), recorded
  as it leaves, executed by the same client (`execute`), and its answer
  comes back as the same `reqwest::Response` — status, headers, URL (an
  extension reqwest's `From` drops, put back), extensions, size hint and
  timeouts unchanged — with its body wrapped (`Counting`, an `http_body::Body`)
  to count the bytes as they're read. **It only observes**: nothing about a
  request changes, and a request that can't be built (never went out) isn't
  recorded. A unit test (`every_request_goes_through_the_recorder`) fails on
  any `.send()` / `client.execute(` outside `privacy/net.rs`, except `ai/eval/`.
- **What's kept** (`Receipt` → `Entry`): when (unix seconds), the **host**
  (`host_of`: the name or address, lower case; a port only off this PC — the
  on-device server takes a new one each start; **never** the path, the query,
  a user name, a header or a body), the **purpose** label, the **feature**,
  the kind (the page's icon), where it is (`place_of`), the HTTP status, and
  the bytes **sent** (the request line, headers and body when in memory) and
  **received** (the answer's head and body, as read). A request is recorded
  once, when it ends: its body read to the end or dropped (only the status
  read counts too), no answer (a failure, status 0), or dropped unanswered
  (Stop, a newer request: "stopped", not a failure). A failure is no answer,
  a body that broke off, or HTTP 400 and up.
- **Purpose** (`Purpose`) is set at the call site; its label is only Yap's
  words and service names (`provider_name` names only Yap's own provider ids,
  else "AI provider"). An AI call (`Purpose::Ai(provider)`) is labelled by
  the **feature** it ran under, a task-local (`net::within(Feature, fut)`;
  the outermost wins): "AI cleanup · Groq" (dictation/pipeline.rs and Prompt Studio's
  test), "Commands · …", "Chat · …" (chat/tools.rs `ask`, the small-model
  pipeline), "Voice Assistant · …" (chat/assistant.rs, around Chat's turn),
  "Notes · …" (Enhance, a note's Ask), "Meeting notes · …"
  (meetings/meeting_summary.rs), "Meeting brief · …" (calendar/brief.rs, around the
  meeting helpers), "Memory · …" (memory/learn.rs); unscoped: "AI model · …".
- **Every call site** (2026-10-24): `ai/llm.rs` (the one chat call), `web.rs`
  (Exa/Tavily: "Web search · Exa"; the keyless lookup: "Web search ·
  Wikipedia and wikis"; `read_page`: "Reading a web page"), `web/native.rs`
  ("Web search · Groq" / "· Google" …), `ai/model_catalog.rs` ("Model list ·
  OpenRouter", a server's own "Model list · your server"), `auth.rs`
  ("Account", every call — a signed-out Yap still makes none on its own),
  `calendar.rs` ("Calendar sync · iCal link"), `calendar/google.rs` ("Calendar
  sync · Google": tokens, events, revoke), `voice/tts/elevenlabs.rs` ("Voice ·
  ElevenLabs", "Voice list · ElevenLabs", "Voice sample · ElevenLabs"),
  `dictation/stt.rs` ("Speech model download"), `ai/local_llm.rs` ("On-device AI ·
  starting up" — the `/health` polls —, "· warming up", "On-device AI
  download"), `speakers/diarize/registry.rs` (its `download` now takes what it is:
  "Speaker model download", or Kokoro's "Voice download (Kokoro)" from
  `voice/tts/registry.rs`). By hand (`net::begin` → `Manual::answered`/`failed`;
  dropped unfinished = stopped): `updates.rs` ("Update check" — the updater
  has its own HTTP client, reqwest 0.13, so the host is its endpoint's and
  the size latest.json's —, "Update download" with the bytes the updater
  counted), `integrations/bridge.rs` ("Local API · apps on this PC": each answer to an app
  on this PC, `net::local_api`, and the startup probe of a stale bridge file)
  and the page's one connection, the **account picture** (`<img>` from the
  sign-in provider in AccountSection, ControlPanel and Settings: `onload` →
  `connections_note {purpose: "account-picture", url}`, once per picture per
  window, `src/lib/connections.js`; Rust takes only purposes on its list and
  only the host). **Not routed:** the model evaluations (`ai/eval/`, debug
  builds, the maintainer's own tool, which logs its requests itself), and the
  `cloud/` server (it's the account service, not Yap; Yap's side of it is
  `auth.rs`). `yap.exe mcp` reaches the Local API, which records it.
- **Where** (`place_of`, pure): **this PC** — loopback, `localhost`,
  `*.localhost`, `0.0.0.0`; **your network** — private, link-local, CGNAT and
  ULA addresses, `.local` / `.lan` / `.home.arpa` / `.internal` names and
  names with no dot; **the internet** — the rest. So the on-device model, a
  self-hosted server on `localhost` and the Local API show as on this PC.
- **The store** (`Store`, `connections.json`): requests of one purpose,
  feature, host and place within **10 minutes** fold into one entry (count,
  failures, bytes, first/last, the last status; the nearest entry within
  the window, so a long download finishing late doesn't move `last` back);
  **7 days** and at most **4,000** entries (the oldest go). In memory under
  one lock; written atomically (`config::atomic_write`, a corrupt file
  quarantined) by a thread every 3 s when it changed, and at exit
  (`shutdown_cleanup` → `net::flush`), which then emits `yap-connections`.
  `net::init` (lib.rs setup) names the file: until then, and in child
  processes (`yap.exe mcp`, `eval`, `diarize`) and unit tests, receipts stay
  in memory only.
- **Commands:** `connections_summary` (`summarize`, pure: today's and the
  week's totals — count, failed, sent, received, left this PC, on it —
  and by purpose, the latest first, each with its hosts: counts, bytes,
  first/last, status, features, the latest six moments), `connections_clear`,
  `connections_note`.
- **Test mode:** a request to a test run's stand-in can be recorded as the
  service it stands in for — the debug-only `YAP_E2E_NET_ALIASES`
  (`127.0.0.1:5123=api.groq.com;…`, test mode only), so the suite can show a
  cloud call without the internet.

## Settings → Privacy & data → Network connections

`settings/ConnectionsView.svelte`, one level down from Privacy & data (like
Meetings' call apps; `privacy#connections-list` opens it; Privacy & data's
**Network connections** group has the link row — "12 today", "3 left this
PC today, 9 stayed on it" — anchor `connections`, in the search index). A
back link; the one-paragraph explanation (what connects and when, that
what was in it is never kept, what "On this PC" means); two tiles (today ·
left this PC; the last 7 days · sent · received); **By purpose**: the filter
(All / Left this PC / On this PC, each with its count — a Segmented, so the
keyboard reaches it) and **Clear list…** (Yap's confirm dialog); then a row
per purpose (its kind's icon, the label, "3 requests · 1 failed · 12 KB
sent · 40 KB received · last today 14:20", a chip per place: This PC in the
success colour, Your network, Internet) that opens (`aria-expanded`) to its
hosts: the host in monospace with its chip, counts and sizes, first to
last, "for Chat, Voice Assistant", and the latest moments with their
status. Live: `yap-connections` reloads it. Colours are `--yap-*` tokens,
light and dark.

## `privacy/diagnostics.rs` — Report a problem

- **The file** (a .zip, built in memory, `Bundle::zip`): `README.txt` (what's
  in it, what's left out, that nothing was sent), `system.txt` (`system_info`:
  Yap's version, channel — dev build / nightly / stable —, installed or
  portable, speech engines or the stub; Windows' product, version and build
  from the registry, Windows 11 named as such; the processor and its
  logical cores; memory; the graphics card and its own memory; the speech
  model and where it runs; when), `settings.json`, `connections.txt`,
  `errors.txt` and `logs/yap.log.<day>.txt` for the last 3 days.
- **The settings summary** (`settings_summary`, pure, over config.json as
  `YapConfig` serialises): an **allow-list**. Copied as they are: switches,
  choices, numbers, shortcuts, model ids (`COPY`; a test keeps every name a
  real setting). Said, not shown: `ppProvider` and each scope's provider
  (Yap's ids only, else "other"); API keys as the **services that have one**
  (`ppApiKeys`, `webSearchKeys`, `ttsCloudKeys`, a scope's `apiKeys`) or
  set/not set; About you's name and note, the agent name: set/not set; the
  region: automatic/chosen; devices: default/chosen; prompts and the
  consent message: Yap's/your own; endpoints: the provider's own, or a
  server on this PC / your network / the internet (never the address); the
  dictionary as counts of corrections and snippets; app rules, nicknames,
  blocked apps as counts; profiles as a count and their providers; a cloud
  voice's model, its voice chosen/default. **Anything else is left out**
  and named under `leftOut`, so a new setting never slips in.
- **The log** (`filter_log`, pure): `INFO`, `WARN` and `ERROR` events only — a
  `DEBUG`/`TRACE` event (Debug logging's, which can hold what was said) and
  the lines it goes on over are dropped; each line scrubbed (`Scrub`): this
  PC's user folder (`%USERPROFILE%`), data folder (`<data>`), user and PC
  names (`<user>`, `<pc>`, whole words only), any `X:\Users\<name>`, email
  addresses (`<email>`), key shapes (`sk-`, `sk_`, `gsk_`, `AIza`, `xai-`,
  `tvly-`, `Bearer …`: `<key>`), long letter-and-digit tokens
  (`<redacted>`), and links cut to their host (a known service's keeps its
  path, never a query). Each day's last 4 MB is read, ≤ 1 MB kept.
  `errors.txt`: the latest 200 warnings and errors of the last 7 days.
- **The connections** (`connections_text`, pure): by purpose, counts,
  failures and sizes; hosts named only for Yap's known services
  (`KNOWN_HOSTS`: the AI providers, Exa, Tavily, ElevenLabs, GitHub, Hugging
  Face, handy.computer, the account service), else "this PC" / "your
  network" / "a host left out" (web pages, wikis, calendar links, the
  person's own servers).
- **The flow:** `diagnostics_preview` builds the file and returns its file
  list with sizes and the text of settings.json, system.txt,
  connections.txt and errors.txt, and keeps that very bundle;
  `diagnostics_save` writes it where the person picks (the dialog plugin's
  Save dialog from Rust, parented to the window, on a blocking thread; a
  test run takes `e2e_save_path` instead and never opens it) and returns
  the path, or nothing when cancelled; `diagnostics_reveal` shows it in File
  Explorer (not in test mode); `diagnostics_issue_url` is GitHub's
  new-issue page with a title and a template carrying only the Yap and
  Windows lines (`issue_url`, pure) — the page opens it through
  `open_external`. **Nothing is ever uploaded.**
- **Tests:** `a_bundle_holds_no_keys_or_personal_details` (in the spirit of
  `privacy/log_privacy.rs`): a config, a log and connections seeded with fake keys,
  a fictional name and note, dictionary words, device names, a custom
  prompt, a LAN endpoint with a key in its query, a user folder and an
  email → no file of the bundle, nor the zip, holds any of them; plus the
  summary's choices, unknown settings left out, the log's levels and
  masks, whole-word names, user folders with spaces, the connections'
  hosts, the issue page.

## Settings → About → Report a problem

`settings/ReportProblem.svelte`, its own group on About (anchor
`report-problem`, in the search index): "Report a problem…" → "Making the
file…" → **What's in the file** (a focused panel: the file list with what
each holds and its size, a preview — Settings / System / Connections / Errors
tabs over a scrollable, focusable `<pre>` —, **Save…**, Cancel, the file's
name and size) → **Saved Yap diagnostics 2026-10-24.zip**: "Nothing was
uploaded…", **Open a GitHub issue**, Show in folder, Done.

## e2e

`e2e/network-receipts.spec.js`: two fake AI servers, one aliased to
api.groq.com — a Chat answer from each shows as "Chat · Groq" (Internet,
api.groq.com) and "Chat · Custom endpoint" (This PC, 127.0.0.1), the filter
shows each alone, the link opens the list, light and dark screenshots,
connections.json holds no key, path or words, Clear asks then empties it;
Report a problem's preview (a key set but not shown, About you "set"), Save
through `e2e_save_path`, the zip unpacked and searched for the key, the name
and the note, the logs without DEBUG lines, the GitHub page's URL.
