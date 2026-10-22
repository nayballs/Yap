# Web search and opening apps

Chat's and the Voice Assistant's two ways of reaching outside Yap: looking things up on the web (Yap's own search and the cloud models' own) and opening apps installed on this PC. Their Settings page is in [frontend-settings.md](./frontend-settings.md) (Voice Assistant).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

- **`web.rs`** — **looking things up on the web** (2026-10-18; Settings →
  Voice Assistant → Web search, `web_search` off by default; design in
  yap-private `docs/web-search-research.md` §7). **Backends** (`search`):
  a cloud model's **own search** (`web/native.rs`, below) by default, else
  Yap's own (`own_search`): **Exa** (`POST https://api.exa.ai/search`, `x-api-key`, its guide's
  recommended `{query, type: "auto", contents: {highlights: true}}` +
  `numResults: 5`, + `userLocation`: the person's country, ISO "GB", from
  About you — `about_you.rs`, `Settings.country`; 2026-10-21) or **Tavily** (`POST https://api.tavily.com/search`,
  Bearer, `{query, max_results: 5}`, + `country`: Tavily's lower-case name,
  "united kingdom", when it lists the person's — `about_you::tavily_country`),
  the search cache keyed by the country too; keys in `web_search_keys` (service →
  key, like `pp_api_keys`, never logged); with no key — or when the service
  fails (its error kept as `note`, said in the chip: 401/403 wrong key, Exa
  402 / Tavily 432 out of free searches, 433, 429, offline), or when it
  found nothing ("Exa found nothing." as the note) — the **keyless
  lookup**: the game's own MediaWiki (`GAME_WIKIS`: bg3.wiki, warcraft.wiki.gg,
  minecraft.wiki, Fandom wikis…; any `*.wiki.gg` / `*.fandom.com` named in
  the question) else Wikipedia in the person's language (`wiki_language`:
  the speech language, else About you's — Windows' unless changed —, else English; English again when it
  finds nothing), through `api.php` (`generator=search` + `inprop=url`, then
  TextExtracts on Wikipedia, `action=parse` HTML elsewhere → `fragment_text`
  with dom_query, infoboxes/navboxes/footnotes out → `best_passages` by
  keyword); `wiki_terms` drops the game's names and question words ("Where
  do I find Gale in Baldur's Gate 3?" → "Gale"), `wiki_retry` tries fewer.
  A keyed game question gets the wiki's best article first too. Descriptive
  User-Agent (`USER_AGENT`, contact URL), one request at a time per host
  250 ms apart, ≤ 5 results, ~700 chars each, 10-minute cache. Never a
  key-less scrape of Google/Bing/DuckDuckGo. **`read_page`**: `check_url`
  (http(s), ports 80/443, no user:pass, not `localhost`/`.local`/… or a
  local IP — `is_public`: loopback, private, link-local, CGNAT, ULA,
  mapped/NAT64/6to4 carrying one, documentation, reserved), the host
  resolved and every address checked (`resolve_public`), the connection
  pinned to them (`resolve_to_addrs`, no proxy, no cookies), redirects
  followed by hand (≤ 5, each checked), 10 s, 3 MB, HTML/plain text only;
  `page_text` = dom_smoothie (readability.js port, `TextMode::Formatted`),
  `page_part` ≈ 3,000 tokens a read with `next_offset`. **`Turn`**: only
  links the person gave (their messages, `links_in`), a search this turn
  returned (`allowed`) or an earlier answer's sources (an earlier search
  found them; `with_earlier_sources`: "tell me more about the first one"); ≤ 3 searches and ≤ 3 reads a turn (a call past the
  cap is answered "you've searched 3 times", the tool still offered);
  sources numbered as they come (`number`), `sources_for(reply)` = the ones
  cited (`citations`: [1], [2, 3], [1][4]) or all; results carry
  `DATA_NOT_INSTRUCTIONS`. Chips: `Searched the web: "q"` (+ " (bg3.wiki)"
  when keyless, + the service's error), `Read bg3.wiki/wiki/Gale`, `Didn't
  read evil.example/…: not a link from you or the search`. **Does a question
  need the web?** (`needs_web`, conservative): the person asks for it ("look
  up", "search the web", "google"), wants something current ("latest",
  "news", "price of", "coming out", "upcoming", "out yet", "who won", a year
  from last year on — "best indie games 2026"; 2026-10-21), asks about
  **prices** (`asks_about_prices`: price, cost, subscription, fees,
  cheapest, deals, discount, on sale, in stock, sold out, buy, buying,
  purchase, "how much is / does / for…" — not "how much is 15% of 80";
  a tester's "Can you check the Grok subscriptions please, for me?",
  2026-10-21; "…I am thinking of buying it", 2026-10-22), or names a known
  game; never a question about their own things (`about_own_stuff`: "my
  notes", "the meeting", "what did I note / write / say…", "we decided",
  "my list / budget", a note), a request to do something
  ("write…", "add…", "open…", "play…": a search would switch the actions off
  for the turn), nor a question about how the answer before it was made
  (`about_the_answer`: "did you look that up or guess?", "where did you get
  that?", "are you sure?"). `refers_back` spots a follow-up pointing at an
  earlier answer ("the top five in that list", "which of those…", "the
  second one"). **A model on this PC with tools** (`ai_model::runs_here`), a
  question that `needs_web`: its first step **must be a web search**
  (`tools::FirstStep::MustSearch`: `tool_choice: "required"` over all the
  tools — the next step offers the same ones, so a local model re-uses its
  cache, 2026-10-22; llama.cpp enforces "required" with a grammar, and
  ignores a named `{"type": "function", …}` choice: "Wrong type supplied
  for parameter 'tool_choice'" — and
  `tools::MUST_SEARCH_NOTE` in the question's context, saying to search what
  this message asks and leave out earlier messages' words unless it points
  back at them: "Grok subscriptions check account" carried "check" from the
  question before, 2026-10-21 — its own query, with
  the conversation in mind: "Arsenal Football Club match tonight 21 October
  2026 time"); a server that refuses or answers in words anyway gets **Yap's
  search instead** (`search_query(question)`: the asking — "can you check /
  look up / web search", "please", "for me" — left out sentence by sentence,
  a sentence of nothing else dropped; handed over as the call the
  model would have made: chip, sources, safety rule, 3-search cap). Left to
  decide, Qwen3.5-4B answered a tester's "best RPG games coming out in 2026"
  from memory (2026-10-21). Cloud models decide for themselves. **Nothing
  in the notes** (`tools::EmptyNotes`, web search on, a question not about
  the person's own things): a `search_notes` that found nothing carries
  `NOTHING_IN_NOTES_HINT` ("search the web for it next (web_search) instead
  of saying there are no notes"), and for a model on this PC a step of
  nothing but empty notes searches is followed by Yap's search for it (once
  a turn) — Qwen3.5-4B searched a tester's notes twice for "Grok
  subscriptions" and stopped. **Prices where the person is** (2026-10-22,
  someone outside the US — About you's region, `about_you::LocalPrices`;
  a tester's "Can you lookup the Grok subscription price for me I am thinking
  of buying it" from a UK PC searched "Grok subscription price 2026": Exa,
  with `userLocation` GB, found x.ai's US page, a US price list and a guide
  in $, and the answer gave the US tiers and offered to "help finding the
  UK pricing"): **a price question's search names the country**
  (`local_query`: "Grok subscription price 2026" → "… UK", every search of
  the turn, the model's — its call rewritten in the conversation too,
  `local_query` post-fix — and Yap's own; not when the query already says
  where or in what money, "… US price", "… GBP"); **results with only
  another country's prices** (`Turn::local_note`, on the pages' text, for a
  money that can be told apart — not a dollar) lead to **one more search
  for theirs** (`local_retry_query`: "… UK £"; Yap runs it straight away
  for a model on this PC, `Turn::take_local_retry`, a cloud model's result
  says `local_prices`: "None of these results gives a UK price (in £).
  Search once more for the UK price, like "…", before you answer."), then
  "These results give UK prices (in £): answer with those…" or "No UK
  prices (in £) here either. Start your answer with one short line saying
  you couldn't find UK prices, then give the prices you found as they are,
  saying which country they're for. Don't offer to look further."; the
  tool models' web line says the same (`price_note`); and an answer's
  offer to look for local prices after a search goes
  (`about_you::strip_price_offer`, in `chat_turn`, post-fix `price_offer`).
  Never a price passed off as theirs: Yap never converts or relabels one
  (the About you block's rule), and an answer's "£8" that this turn's web
  text (`Turn::texts`: results and page parts) gave only as "$8" gets its
  page's sign back (`about_you::restore_price_signs`, post-fix
  `price_sign`; not 0, not an estimate said as one — "about £6", which the
  evals' `prices_keep_their_sign` lets pass too). **Models
  that can't call tools** (`commands::small_model_answer`): the fixed
  pipeline — `needs_web`, `search_query` (the ask stripped; a price
  question's aimed at the country, and searched once more for local prices,
  `local_prices_again`, their pages first or the "couldn't find" line in
  the prompt),
  `grounded_sources` (≤ 3 sources, ≤ 1,500 tokens) + `grounded_prompt`
  (answer only from them, cite, < 80 words) with the history fitted
  (`fit_history`, ≤ 2,000) for an 8k model; no search → `NOT_SEARCHED_LINE`
  (web on) or `no_search_line` (off). **Every model's answer** then goes
  through `chats::checked_answer` (chat-and-assistant.md):
  `strip_sources_note` (a "(Yap searched the web…)" note always; a
  "Sources:" list the model wrote — numbered or linked items, and only
  after a web search the looser kinds: a bullet ending in its number, a
  site and a quoted title, any bullets to the end — what follows the list
  stays; never a list the question asked for, `asks_for_sources`: "What
  sources could I use for my essay?", references, further reading — but
  "Include your sources" asks where the answer came from, which the
  Sources row shows, so the model's copy still goes),
  `claims_browsing`
  (sentence by sentence; not a denial, an offer — "I can do a quick web
  search" —, a question, or "I searched your notes") → `NOT_SEARCHED_NOTE` /
  `NOT_SEARCHED_THAT_NOTE`. **Times**: the web tools' line says to convert a
  page's time zone to the user's (`time_note(zone)`: "…convert it to the
  user's time (UK time)…"); the date, the user's time and its UTC offset are
  in the About you block, last in the prompt (`about_you::prompt_block`).
  Chips `Searched the web…` + `Answered from 1 web
  page`. **The AI's own search** (`web/native.rs`, 2026-10-18, the maintainer's
  call: "let the cloud models use their own search tools, and only give
  local models Yap's"; `web_search_native`, on by default): a cloud chat
  model (`Settings::for_chat(cfg, provider, model, base_url, key)` →
  `NativeSearch::for_chat`) keeps the ONE tool loop and calls Yap's own
  `web_search`; running it is a short **sub-call** to the same provider,
  on the chat's endpoint and key, with nothing but its search tool
  (`NativeSearch::body`, pure; aimed at the person's country —
  `in_region`: its prompt ends "The user is in the United Kingdom: prefer
  pages and prices for there (in GBP), and say which country any other
  price is for.", and OpenAI's and Anthropic's search
  tools get `user_location {type: "approximate", country, timezone}`, as
  their docs said on 2026-10-21): **Groq** chat completions +
  `tools: [{type: "browser_search"}]`, `tool_choice: "required"`,
  `reasoning_effort: "low"`, `max_completion_tokens` 1,024, on gpt-oss
  (another Groq model searches with `openai/gpt-oss-20b`); **Gemini**
  `…/v1beta/models/<model>:generateContent` beside the chat's `/openai`
  endpoint, `tools: [{google_search: {}}]`, `x-goog-api-key` (no forcing
  exists; Gemini 2+ text models); **Anthropic** `POST /v1/messages`
  (`anthropic-version: 2023-06-01`, `x-api-key`) with the basic
  `web_search_20250305` (`max_uses: 2`; no `tool_choice` — forced is a 400
  on Opus/Sonnet 5.5 —, no temperature/thinking); **OpenAI** the
  Responses API `POST /v1/responses`, `tools: [{type: "web_search"}]`,
  `tool_choice: "required"`, `include: ["web_search_call.action.sources"]`,
  `store: false`, `reasoning.effort: "low"` on gpt-5+/o3/o4 (gpt-4o,
  gpt-4.1 not nano); **OpenRouter** chat completions + `plugins: [{id:
  "web", max_results: 5}]` (any model). Docs as read 2026-10-18 (sources in
  the module doc). Parsers (pure, fixtures from each provider's documented
  shape; Groq's from the eval spike): Groq's `executed_tools` (`browser.search`
  results, `browser.open` pages as `L<n>:` lines; `【cursor†L4-L9】` →
  those lines are the passage), Gemini's `groundingChunks` + `groundingSupports`
  (segment text = passage; byte offsets), Anthropic's `web_search_result`s +
  text `citations[].cited_text` (`page_age` → date; an `error_code` in a
  200 = failure), OpenAI's/OpenRouter's `url_citation` annotations (the
  sentence before it, or OpenRouter's `content`; the markdown link at the
  span → the mark; `utm_*` ignored when matching pages) → ≤ 5 `Hit`s (read
  or cited first) + the provider's short answer as `summary` with its
  citations renumbered to Yap's `[n]` (`render_summary`; gpt-oss's 【…】
  out). The tool result is the usual numbered `results` + `summary`, under
  `DATA_NOT_INSTRUCTIONS`, so the safety rule, the 3-search cap, chips,
  pills, the Sources row and chats.json are unchanged. Chip `Searched the
  web with Groq: "q"` (Google / Anthropic / OpenAI / OpenRouter). **Fallback**
  (unsupported model, 4xx, 429 — never waited out —, unreachable, nothing
  found): Yap's own search (`own_search`), its reason first in the `note`
  (`http_error`: "Groq's search is limiting how much Yap can search right
  now.") → `Searched the web: "q" (Exa) · Groq's search is limiting…`; not
  cached. A failure that will repeat (400/401/402/403/404/422, or Gemini's
  429 "check your plan and billing" with no retry delay = grounding not on
  a free key's plan: Gemini 3+ needs billing) is remembered 30 min per
  provider + model + endpoint (`UNAVAILABLE`, cleared by `clear_caches`):
  Yap's own search answers straight away, same reason in the chip. **Gemini's terms** (Gemini API Additional Terms, 2026-04-28): a
  grounded result may feed a follow-up answer when that answer shows its
  Links (≤ 5) — `Turn::sources_for` lists every Google link of a grounded
  turn (cited first, ≤ 5), unmodified (Google's
  `vertexaisearch.cloud.google.com/grounding-api-redirect/…`, the site from
  its title: `source_site`); the sub-call's text is never stored. Usage goes
  to `usage::record`; a model evaluation logs and paces the sub-call
  (`eval::Call`, ~4k tokens estimated). Custom/self-hosted/on-device models
  have none (Yap's own). Not for the Meetings view's Ask bar. Test mode never goes online: debug-only `YAP_EXA_URL`,
  `YAP_TAVILY_URL`, `YAP_WEB_STANDIN` (every other site as
  `<standin>/<host><path>`), else an error. `#[ignore]`d
  `real_keyless_lookup` (read-only, no key: Wikipedia + bg3.wiki).
- **`apps.rs`** — **opening apps when you ask** (2026-10-18; Settings →
  Voice Assistant → Opening apps): "open Teams" in Chat or to the Voice
  Assistant opens an app installed on this PC — only an entry of the
  index, never a command line, arguments, a file, a folder or a link.
  **Index** (`apps(refresh)`, in memory, rebuilt after `FRESH` = 5 min or
  on Refresh; read-only, no network): the Start menu's "All apps" =
  `shell:AppsFolder`'s children (`SHGetKnownFolderItem(FOLDERID_AppsFolder)`
  → `BHID_EnumItems`, on an STA thread of its own): display name, parsing
  name as the id (an AppUserModelID like `MSTeams_8wekyb3d8bbwe!MSTeams`,
  or a known-folder path `{6D809377-…}\7-Zip\7zFM.exe`), the item's ID
  list and, for a desktop app, **the Start menu shortcut behind it**
  (`System.AppUserModel.BestShortcut`, an ID list → a `.lnk`/`.url` path
  in one of the Start menu's Programs folders) plus its target and
  arguments (`System.Link.*`, read for the launcher label, never passed);
  ~240 entries → ~190 apps in ~300 ms on the test PC (204 of 205 desktop
  entries have a shortcut). Fallback: the Start menu `.lnk`s (ProgramData
  + AppData; a `.url` only with a game store's link) + App Paths. `tidy`:
  junk out (`junk` → `is_junk`: "Uninstall …"/`unins*.exe`, read-mes,
  help/`.chm`, docs, licences, `http(s)://` and `.url` links, "… on the
  Web", `.msi`, folders; a Store app keeps "Help" — Get Help; "Revo
  Uninstaller" stays; a **game store link** — `steam://`,
  `com.epicgames.launcher://`, `battlenet://`, `uplay://`, EA, GOG,
  Amazon (`LAUNCHER_LINKS`) — is an app whatever file holds it; a shortcut
  to a web page is junk), one entry per name. **Opening** (`open`): a
  packaged app (`id` has `!`) by
  `IApplicationActivationManager::ActivateApplication` (brings it forward,
  gives its pid); a desktop app by **its shortcut file** with
  `ShellExecuteExW`'s default verb, as a Start menu click (its arguments,
  working folder, run-as apply); only without one by the entry's ID list
  (`SEE_MASK_IDLIST`). ⚠ Opened by its ID list, World of Warcraft said
  "Opened" and showed nothing (2026-10-18): its launcher needs the
  shortcut's `--productcode=wow`, which nothing documents the ID list to
  carry (not confirmed by a launch; the shortcut file is the documented
  way). Then
  `bring_forward(pid, since)` (≤ 8 s: the started process's main window,
  or that of a process it started since the launch — `procs::family`, a
  launcher handing over; a Battle.net already running isn't one, and may
  stay behind the window in front). Logs the app's name and how it opened ("its
  Start menu shortcut" / "app activation" / "its app-list entry"), never a
  path. A launch Windows refuses → "Windows couldn't start X: …", never
  "Opening…". `launcher(target, args)`: "Steam" for `steam://`, "Battle.net"
  for `… Launcher.exe --productcode=…` → "Opening World of Warcraft (in
  Battle.net)." **Matching** (`find`, pure): `keys(name)` — the
  full name, without qualifiers ("(64-bit)", " - Insiders", versions),
  without the maker (`VENDORS`: "Teams"), initials ("VS Code", "WoW"), the
  last word (not a `GENERIC` one), each also with **numbers as digits**
  (`numerals`: one–twenty, II–X: "three" = "3" = "III") — vs the request
  (`FILLER` "the/app/please" out, apostrophes dropped, "my", a maker,
  `ALIASES`: calc, cmd, files, vscode, store…, numbers as digits), exact,
  a few of its words, or a near-miss (`fuzzy::spelling_distance` ≤ 0.2
  and `plain_spelling_distance` ≤ 0.3: "spottify" yes, "spotifire" no);
  nicknames first (`app_nicknames`); `CONFIDENT` 0.80, others within
  `MARGIN` 0.03 → `Several` (four Pythons, two Teams: ask, never guess);
  `apps_blocked` → `Blocked`; else **by sound** (`sound_of`:
  `fuzzy::sound_distance` ≤ 0.1 over ≥ 5 sounds, spelled ≤ 0.45 apart —
  "bolders gait" → Baldur's Gate 3) when exactly one app sounds so and no
  other scores ≥ 0.6 or sounds within 0.25; else `Nothing` + close names
  (sound-alikes first). **Fast path** (`plan_with` / `plan_for(text, cfg,
  conversation)`, before any model, in `commands::chat_turn` →
  `app_turn` and `assistant::answer`): `open_request(text)` (pure: the
  whole message is "[greeting/please/can you/try to/yes…] open|launch|
  start|play|run|fire up|boot up|load up… <app> [please/for me/now/
  again]", one sentence; `plain` = no joining words "and/about/for/to…";
  `play` — a phrase naming no app is the model's, "play the next song")
  → one app → opened, "Opening Microsoft Teams." + chip `Opened Microsoft
  Teams` (no model call: works with no AI and the 1.5B local model);
  several → "I found 2 apps for “teams”: … Which one?"; switched off →
  "You've asked Yap not to open …"; a plain name not installed → "I
  couldn't find an app called …" (+ "Did you mean …?") unless a
  tool-capable model is there; "Let Yap open apps" off → it says so; "open
  my notes", "start the meeting" (`NOT_APPS`), "open the notes about Teams"
  → the model. **Yap's own questions, answered** (a per-conversation
  memory, `recall` / `remember` / `moved_on`, in memory only): its
  question's apps (`OFFER_FOR` 5 min, gone when a message goes to the
  model) and the app it last opened or suggested (`LAST_FOR` 30 min) —
  `follow_up` (pure): after "Did you mean X?" a "yes" (`is_affirmative`:
  yes/yeah/sure/please/do it/go ahead/open it/that one/"Yes, I do.") opens
  X; after "Which one?" "the second one" / "2" / "the last one" / "3.13" /
  "the classic one" / a name opens that one (a "yes" → "Which one: A or
  B?"); "no"/"never mind"/"neither" → "Okay, I won't open anything." (no
  chip); "open it (again)" → the last app (none: the model's). **Model
  tools** (`execute_tool(name, args, question, previous)`): `open_app`
  acts only when the person's own message asks to open something
  (`asked_to_open`: open/launch/start/run/play…) or is a plain yes to the
  answer just before, which asked a question naming that app
  (`consented`: "Would you like me to open Baldur's Gate 3?" → "Yes."; not
  "Are you struggling?"), so a note saying "open X" can't (chip `Didn't
  open "X": you didn't ask to open an app`). `list_apps` takes a query
  and gives ≤ 15 names (`LIST_MAX`; no query: the count, 15, "pass a
  query" — 840 → 135 est. tokens on the test PC). **A model without
  tools** gets `NO_TOOLS_NOTE` (+ `NO_TOOLS_OPEN`: say "open" and the
  name), and an answer that claims an open the person asked for
  (`claims_unasked`: "Opening …", "I'm trying to open …", "I've
  launched …") becomes `correct_claim`'s "I can't open apps myself. Say
  “open Baldur's Gate 3” and Yap will." (a long answer keeps its text).
  Spoken "Opening …" follows the usual meeting rule (the Voice Assistant
  is off during meetings; Chat's auto-speak stays quiet; TTS reads
  "Battle.net" as "Battle dot net"). Commands `apps_list {refresh}`,
  `apps_icons {ids}` (`IShellItemImageFactory`, 32 px →
  `app_icons::encode_png`, cached per run). Test mode: nothing launched
  (recorded, debug-only `e2e_opened_apps {clear}`), index =
  `YAP_E2E_APPS` (a JSON `[{id, name, target?, args?}]`, read each call),
  no icons; unit tests never read `config.json` or this PC's apps
  (`run_tool`, a thread-local stand-in index; `games()` = the test PC's WoW /
  Classic / Battle.net / Baldur's Gate 3 / Steam entries). `#[ignore]`d
  `this_pcs_apps` (read-only: names, junk, the games' shortcuts and
  launchers, sample matches, `list_apps`' size, icons),
  `opens_notepad_for_real` (opens Notepad the app's way, closes only that
  pid's window; skipped while a Notepad runs) and
  `opens_shortcuts_for_real` (in its own temp folder, never the Start
  menu: a `.url` to `ms-settings:about` — or `calculator:` while Settings
  is open — closes only the new window; a `.lnk` to cmd.exe whose
  arguments write a file into its working folder; a `.lnk` to Notepad with
  a file, skipped while a Notepad runs).
