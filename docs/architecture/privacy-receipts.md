# Network receipts, Report a problem, your data

Two trust features (2026-10-24): **Network connections**, a list of every
connection Yap made and why (`privacy/net.rs`, Settings → Privacy & data), and
**Report a problem**, a diagnostic file the person checks before saving it
(`privacy/diagnostics.rs`, Settings → About). And two for the person's data
(2026-10-29, Settings → Privacy & data → Your data): **Download your data**
(`privacy/export.rs`) and **Reset app** (`privacy/reset.rs`). Logging's own
rules (no user content at `info` and above) are in
[app-shell.md](./app-shell.md) (Logging).

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
  the outermost wins): "AI cleanup · Groq" — "AI cleanup + text before the
  cursor · Groq" when a cleanup carried it (`Feature::DictationContext`,
  `ai/cleanup_context.rs`, 2026-10-25; `place_of_url` is what decides it may
  go: a model on this PC, by the same rules, a test run's stand-ins as the
  services they stand in for) — (dictation/pipeline.rs and Prompt Studio's
  test), "Commands · …", "Chat · …" (chat/tools.rs `ask`, the small-model
  pipeline), "Voice Assistant · …" (chat/assistant.rs, around Chat's turn),
  "Notes · …" (Enhance, a note's Ask), "Meeting notes · …"
  (meetings/meeting_summary.rs), "Meeting brief · …" (calendar/brief.rs, around the
  meeting helpers), "Memory · …" (memory/learn.rs); unscoped: "AI model · …".
  The AI usage tracker (`ai/usage.rs`, 2026-10-25) counts each call's
  tokens under the same scope (`net::current_feature`), with finer ones of
  its own where it needs them (a transform, a translation, speaker
  names), and places a call on this PC the way this list does
  (`place_of_url`).
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
  `voice/tts/registry.rs`, or meeting search's "Meeting search model
  download"), `search/embedder.rs` ("Meeting search · on-device model",
  `Purpose::Embeddings`: the embedding server on 127.0.0.1, on this PC;
  [search.md](./search.md)). By hand (`net::begin` → `Manual::answered`/`failed`;
  dropped unfinished = stopped): `updates.rs` ("Update check" — the updater
  has its own HTTP client, reqwest 0.13, so the host is its endpoint's and
  the size latest.json's —, "Update download" with the bytes the updater
  counted), `integrations/bridge.rs` ("Local API · apps on this PC": each answer to an app
  on this PC, `net::local_api`, and the startup probe of a stale bridge file)
  and the page's one connection, the **account picture** (`<img>` from the
  sign-in provider in AccountSection, ControlPanel and Settings: `onload` →
  `connections_note {purpose: "account-picture", url}`, once per picture per
  window, `src/lib/core/connections.js`; Rust takes only purposes on its list and
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

## `privacy/export.rs` — Download your data

- **The file** (`text_files`, pure; each store read as it is in memory, so
  what the page shows, saved or not): `README.txt` (what's in it, what's
  left out, that nothing was uploaded); `history/dictations.csv` (When — this
  PC's time —, App, Words, Text, As said, Heard, Status, Model: `history_csv`)
  and `history/history.json`, and `history/dictation-stats.json` (the stats
  Home and Insights show, `dictation/stats.rs`: a row a day, numbers and app
  names — kept when history isn't); each note as `notes/<date> <title>.md` and each
  meeting as `meetings/<date> <title>.md` (`notes::commands::export_markdown`,
  what "Save as .md" writes; `integrations::mcp::is_meeting` tells them apart;
  names made safe and unique, `file_stem`), and `notes/notes.json` (the whole
  store, folders and actions too); each chat as `chats/<date> <title>.md`
  (who and when, the model) and `chats/chats.json`; `tasks.json`; `memory/`
  — Memory's own export's Markdown (`memory::export::files`, About you on
  You.md) and `memory.json`; `dictionary/dictionary.csv` — the CSV the
  Dictionary's Export… writes (`dictionary_csv`: Heard, Written, Kind,
  Near-misses, Starred, a byte-order mark and Windows line ends; the rows
  writing one word together, as the view groups them; Import… reads it back);
  `settings.json`.
- **Settings without secrets** (`settings_export`, pure, over config.json as
  `YapConfig` serialises), three ways: by name — `SECRET_FIELDS` (`ppApiKey`,
  `ppApiKeys`, `apiKey`, `apiKeys`, `webSearchKeys`, `ttsCloudKeys`, at any
  depth: each Language Models scope's, each cleanup profile's) and any name
  with apikey, token, secret, password, session, credential, cookie or bearer
  in it, or ending in "key"/"keys" bar the keyboard's (`*Hotkey(s)`,
  `autoSubmitKey`); by look — any value the diagnostics' key shapes match
  (`sk-`, `gsk_`, `AIza`…) is left out whatever its field; and links lose
  their user name, password, query and fragment (a key can ride in one).
  What was taken out is named under `leftOut` (names only). A lint-style test
  (`every_setting_named_like_a_secret_is_left_out`) walks every field of a
  seeded config: a name with key, token, secret, password or session in it
  is either left out or a keyboard key, so a new one has to be decided
  there; `the_file_holds_every_store_and_no_secret` seeds fake keys in every
  secret field (and one pasted into the agent's name) and finds none in any
  file. The account session and the calendars' tokens are in Windows'
  Credential Manager, which the export never reads.
- **The audio** (only with the tick box, which says its size:
  `data_export_summary`): `audio/meetings/<date> <title>/*.flac` (each
  meeting's, `meeting-audio/`), `audio/dictations/<when>.wav` (the day of
  dictation audio and what Home's Keep holds, `recent-audio/`),
  `screenshots/*.png` (Chat's, `chat-images/`) — `disk_files`, stored as they
  are (they're compressed already); the text Deflated.
- **Written to disk as it goes** (`write_zip`: a big one is gigabytes),
  beside the file picked as `<name>.partial`, renamed when done; progress
  `yap-data-export {done, total}` (bytes, every 200 ms); **Cancel**
  (`data_export_cancel`) stops it and deletes the partial file. One at a
  time. A file gone since it was listed is left out. Zip64 for a file over
  4 GiB.
- **Commands**: `data_export_summary` (counts and the audio's size),
  `data_export_save {audio}` (Windows' Save dialog — `diagnostics::pick_zip_path`,
  Report a problem's, a test run's `e2e_save_path` instead — then the file;
  `{path, cancelled, bytes}`), `data_export_cancel`, `data_export_reveal`
  (Show in folder; not in test mode); debug-only `e2e_export_pace {ms}` (a
  pause after each MB, so a test can press Cancel). The log says counts and
  bytes only.

## `privacy/reset.rs` — Reset app

"Clear everything and keep my settings".

- **What goes** (`GOES`, the dialog's words and each one's files): the
  dictation history and the day of dictation audio (`history.json`,
  `recent-audio/`); every note and meeting with their audio (`notes.json`'s
  notes — the folders and actions stay —, `meeting-audio/`); every chat with
  its screenshots (`chats.json`, `chat-images/`, the ones waiting in Chat's
  box); memory (`memory.json`, and what the passes had read or were waiting
  to read); tasks (`tasks.json`); the search index (`search-index/`); usage
  stats (`dictation_stats.json` — Home's and Insights' numbers, which
  outlast clearing history but not a reset: a fresh start, and they name
  the apps dictated into; Keep history's copy in it stays —, `ai_usage.json`,
  `groq_usage.json` — Groq's request cap kept); the
  pre-meeting briefs (they quote notes and memory; `calendar.json`'s
  `briefs`). Set-aside copies of those stores (`<name>.corrupt-…`) go too.
- **What stays** (`STAYS`): config.json (every setting, the dictionary and
  snippets, the keys) and `dictionary_usage.json`; the models (`models/`,
  `llm/`); the account's sign-in and the calendars (Credential Manager,
  `calendar.json`'s connections and meetings); Yap's own records — tips
  shown (`hints.json`, `mic_tips.json`), answers to its offers
  (`notices.json`, `word_suggestions.json`, `snippet_suggestions.json`,
  `language_nudges.json`), the network receipts, caches, the log.
- **Every file the docs name is decided**: a unit test reads this folder's
  `config-and-data.md`, takes every `%APPDATA%/yap/…` and `<data>/…` it names
  and fails on one in neither list — a store added later has to be put in one
  (and the dialog's words with it).
- **Each store is emptied through its own module, under its own lock**
  (`history::clear`, `notes::reset`, `chats::reset`, `memory::reset` — its
  Undo batches dropped —, `task_store::reset`, `search::index::forget`,
  `ai::usage::store::reset` — what this Yap had counted but not written too
  —, `groq::reset`, `calendar::forget_briefs`, `meeting_audio::clear`,
  `snip::chat_images_delete_all`), so no copy held in memory writes the old
  data back with a later save; a save that fails is said (`saving.rs`, Try
  again) and the store stays empty in memory.
- **No restart**: every window lets go of each note (`yap-note-deleted`: the
  notepad hides, the quick note starts fresh), the stores' own events go
  out (`yap-history-changed`, `yap-notes-changed`, `yap-tasks-changed`,
  `yap-memory-changed`, `yap-chat-images-deleted`), then `yap-data-reset`:
  the main window shows its views afresh (`{#key}` in ControlPanel), the
  briefs' cache empties (`stores/calendar.svelte.js`). `Done.restart` is
  there for the page, never set so far.
- **Refused while a dictation or a meeting records** ("Finish the dictation
  first.", "Stop the meeting first: Yap is recording it."): what they'd save
  next would come back. Another Yap running on the same data folder (the
  installed nightly beside Yap - Dev) still holds its own copies in memory:
  quit it first.
- **Commands**: `data_reset_plan` (what goes with counts — "1 note, 1
  meeting" —, what stays, why it can't run now), `data_reset` (counts only,
  in the log too).

## Settings → Privacy & data → Your data

`settings/YourData.svelte` (its own chunk, loaded with the page), the last
group on Privacy & data: **Download your data** (anchor `download-data`, in
the search index): its row, then the tick box "Include audio and
screenshots · 1.2 GB" (none kept: off and greyed); Download… → Windows' Save
dialog → the progress (`ui/DownloadProgress.svelte`: "Saving your data…
42 %", the bytes, **Cancel**) → **Saved Yap data 2026-10-29.zip**, its size,
"Nothing was uploaded", Show in folder, Done. **Reset app** (anchor
`reset-app`): Reset app… → Yap's confirm dialog (`ui/ConfirmDialog.svelte`,
which now takes `children`): "Clear everything and keep your settings?",
what goes, each with its count, what stays, **Download your data first**
(closes it and starts the download), Cancel / **Reset app** (red) → "Yap is
reset · Your settings, dictionary and models are as they were. No restart
needed." A dictation or meeting going on: said in a toast, no dialog.

## e2e

`e2e/your-data.spec.js` (seeded history, a note, a meeting, a chat, a task,
memory, a dictionary, fake keys, made-up audio and a screenshot): the tick
box says the audio's size; Download… writes the zip through
`e2e_save_path`, every store in it (the dictation in the CSV, the meeting's
line in its Markdown, the dictionary's CSV as the view writes it, settings
naming what was left out), the audio and the screenshot, no key anywhere in
the file's bytes; a slowed one shows its progress and Cancel leaves no file;
Reset app's dialog lists what goes with counts and what stays, Download your
data first saves the file, the reset empties every store on disk, keeps
config.json (keys, dictionary, About you), Home and Notes show it at once,
and the next note and dictation are all that's saved after.

`e2e/network-receipts.spec.js`: two fake AI servers, one aliased to
api.groq.com — a Chat answer from each shows as "Chat · Groq" (Internet,
api.groq.com) and "Chat · Custom endpoint" (This PC, 127.0.0.1), the filter
shows each alone, the link opens the list, light and dark screenshots,
connections.json holds no key, path or words, Clear asks then empties it;
Report a problem's preview (a key set but not shown, About you "set"), Save
through `e2e_save_path`, the zip unpacked and searched for the key, the name
and the note, the logs without DEBUG lines, the GitHub page's URL.
