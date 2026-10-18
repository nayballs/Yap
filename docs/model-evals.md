# Model evaluations (`yap.exe eval`)

A harness for putting **real models** through **Yap's own Chat code** and getting detailed
logs back: which model opens the right app, finds the fact, edits one line of a note, keeps
a voice answer short, refuses a prompt injection — and what every request and response
looked like. It answers questions like "which local model should Yap ship?" with numbers,
and catches a prompt or tool change that makes a model worse.

It lives in a **debug build** of Yap (`src-tauri/src/eval.rs` + `src-tauri/src/eval/`); a
release build answers `yap.exe eval` with "debug builds only".

## Running it

```bash
# build (debug, stub STT; its own target dir) and run, from the repo root
npm run eval -- --model groq:openai/gpt-oss-20b --keys-from-yap
npm run eval -- --model local:installed --model local:qwen2.5-3b --tools on
npm run eval -- --raw evals/raw/groq-browser-search.json --keys-from-yap

# or the binary directly, from the repo root (it finds evals/scenarios there)
E:/yap-builds/evals/debug/yap.exe eval --model groq:openai/gpt-oss-20b --keys-from-yap --only apps
```

`npm run eval` builds into `$YAP_EVAL_TARGET_DIR` (default `src-tauri/target/eval`) and
writes run folders to `$YAP_EVAL_OUT` (default `test-results/evals/`, git-ignored). On
Nathan's PC: `YAP_EVAL_TARGET_DIR=E:/yap-builds/evals`, `--out E:/yap-builds/eval-runs`.
`yap.exe eval --help` lists every option; the main ones:

| Option | |
|---|---|
| `--model <spec>` | Repeat to compare. `groq:<model>`, `openai:<model>`, `anthropic:<model>`, `openrouter:<model>`, `gemini:<model>`, `custom:<base_url>,<model>`, `local:<gguf>` (below). |
| `--keys-from-yap` | Use the API keys saved in `%APPDATA%\yap\config.json` (read straight from the file, **read-only**; never through `config::load`, which may save): the AI providers' and web search's (`webSearchKeys`: Exa, Tavily). Else `GROQ_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`, `GEMINI_API_KEY`, `YAP_EVAL_CUSTOM_KEY`, `EXA_API_KEY`, `TAVILY_API_KEY`. |
| `--only a,b` / `--tag t` | Scenarios whose name contains `a` or `b` / that carry tag `t`. With `--raw`, `--only` picks requests by name. |
| `--tools on\|off` | Treat the model as able (or not) to call tools, whatever `tools::supports_tools` says (its ≥ 4B rule for local models). Default: Yap's rule. |
| `--tokens-per-minute <n>` | Pacing budget. Default **8,000 for Groq** (its free plan's limit), none otherwise. The pacing also reads Groq's `x-ratelimit-remaining-tokens` / `-reset-tokens` and waits for the window to refill, so a run doesn't spend minutes in 429s. Time held back is reported. |
| `--max-total-tokens <n>` | Stop starting scenarios on a model past this many tokens (a free plan's daily cap: Groq's gpt-oss-20b allows 200k a day). |
| `--timeout <secs>` | Per model call, instead of Yap's own (20 s for Chat). Leave it to measure what Yap does; raise it to measure what a slow model *could* do. |
| `--repeat <n>` | Run each scenario n times (models answer differently at temperature 0.5). |
| `--no-web` | Skip the web scenarios (`needs_web` / `web`); live ones go on the internet (below). |
| `--raw <file>` | Send hand-written requests (see below) instead of, or before, scenarios. |

### Local models

`--model local:<spec>` starts **Yap's own llamafile runtime** the way `local_llm.rs` does
(`--server`, `-c 8192`) on a free localhost port for the run, and stops it after. `<spec>`:

- `installed`: the on-device default, Qwen2.5-1.5B-Instruct;
- a curated id from `local_llm::CURATED_MODELS` (`qwen2.5-3b`, `qwen2.5-7b`, `llama-3.2-3b`, …);
- a path to any `.gguf`, or `https://…gguf#sha256=<hex>` (the Hugging Face LFS oid).

The runtime and models are taken from the installed app's `%APPDATA%\yap\llm\` when they're
there, **read-only** (run in place; the server's log and working folder are the run's), else
downloaded and SHA-256-verified with `local_llm::download_verified` into `--models-dir`
(default `<out>/../eval-models`). `--ngl <n>` puts layers on the GPU (default **0: CPU only**,
what a normal PC without a big GPU gets); `--ctx`, `--llamafile <path>` and repeatable
`--llamafile-arg <arg>` adjust the server.

The model id sent is the file's name (`qwen2.5-1.5b-instruct-q4_k_m`), so `supports_tools`
reads its size as it would for a self-hosted local model. (The on-device provider always
sends `qwen2.5-1.5b-instruct`, whatever GGUF is picked: see "Findings" below.) The summary
gives each local model's load time, peak RAM (the server's peak working set), and prompt and
generation speed (llama.cpp's own `timings`).

## What a run does, and what it doesn't touch

Each scenario goes through **`commands::chat_turn`**, the same function the Chat view and
the Voice Assistant call: the open-apps fast path (`apps::plan_for`), the tool loop
(`tools::run_tool_loop`) or the plain chat + keyword-RAG fallback for models without tools,
with the Chat persona Settings sets by default. Only the side effects are swapped:

- **Own data dir** (`portable::use_data_dir`): `<run>/data/`. Chats and usage counters land
  there; `%APPDATA%\yap` is never written.
- **Settings in memory** (`eval::config`, read by `config::load`; `config::save` does
  nothing): the run's provider, model and key never reach a file.
- **Notes in memory** (`notes::eval_reset`, nothing saved): reset and seeded per scenario.
- **Apps**: test mode (`YAP_E2E=1`, as the e2e suite): the index is the scenario's stand-in
  list, and opening an app is only recorded (`apps::e2e_opened_apps`). Nothing launches.
  `"apps": "this_pc"` reads this PC's Start menu instead (read-only, in a child process
  outside test mode: `yap.exe eval --dump-apps`).
- **Clipboard**: the `copy_to_clipboard` tool is recorded, never run (`eval::intercept_tool`).
- **Web search** (`web.rs`), only in web scenarios ("Look things up on the web" on), fresh
  caches each: **live** by default (`eval::web_live`: the real Exa/Tavily with the run's key,
  else the keyless Wikipedia + game-wiki lookup; `read_page` reads real pages with its usual
  guards), or a **stand-in** (`"web": {"mode": "standin"}`): a local server
  (`eval/standin.rs`) answers the search with the scenario's `results` and serves its `pages`
  through `web.rs`' test-mode addresses (`YAP_EXA_URL`, `YAP_TAVILY_URL`,
  `YAP_WEB_STANDIN`) — nothing goes on the internet; its requests are logged
  (`web_standin`).
- No window, tray, hook, audio or browser.

The hooks in the app's code (`llm.rs`, `tools.rs`, `config.rs`, `notes.rs`, `portable.rs`,
`web.rs`) are no-ops outside a run, and compiled to nothing in release builds.

## Where the logs go

`<out>/<yyyymmdd-hhmmss>-<label>/`:

| File | |
|---|---|
| `summary.md` | Per model: passed / failed / skipped, pass rate, model calls, tool calls, tokens, mean turn time, 429s; a scenario × model table; every failure's reasons; soft-check warnings; every answer with the tools it called. |
| `summary.json` | The same, machine-readable, plus each turn's details and each local model's facts. |
| `transcript.jsonl` | One JSON record per event, in order: `run`, `scenario_start`, `turn_start`, `call` (pacing estimate), `pacing` (time held back), **`request`** (the full body: messages, tool schemas, temperature), **`response`** (the full body, latency, rate-limit headers, usage incl. cached and reasoning tokens, llama.cpp timings), `refused` (an HTTP error and its body), `rate_limit_wait`, `call_failed`, **`tool_call`** (name, arguments, the result fed back), `clipboard`, `turn_end` (answer, chips, fast path, opened apps, checks), `scenario_end`, `model_end`. Each carries the model, scenario and turn. |
| `yap.log` | Yap's own tracing (`info`, `yap_lib=debug`). |
| `llamafile-<model>.log` | The local server's log. |
| `spike.md` | `--raw` runs: each answer's shape. |
| `apps/`, `data/` | The stand-in app lists used, and the run's data dir. |

**Keys are never printed or written.** Every line of the transcript, the summaries and the
console goes through a redactor (the run's keys, plus anything shaped like a Groq, OpenAI,
Anthropic, OpenRouter or Google key). Keys only travel in the `Authorization` header, which
isn't logged.

## Scenarios (`evals/scenarios/*.json`)

```jsonc
{
  "name": "apps-python-second-one",          // unique; --only matches on it
  "description": "…",
  "tags": ["apps", "follow-up"],
  "needs_web": false,                         // true: "Look things up on the web" on (live)
  "web": {                                    // optional (implies web on)
    "mode": "standin",                        // "live" (default) or "standin"
    "service": "exa",                         // or "tavily" (default: the one the run has a key for)
    "key": false,                             // false: the keyless lookup even with a key
    "results": [{ "title": "…", "url": "https://tips.example/x", "text": "…", "published": "2026-09-30" }],
    "pages": { "https://tips.example/x": "<html>…</html>" }   // stand-in pages read_page gets
  },
  "voice": false,                             // every turn asked by voice (Voice Assistant)
  "tools": "on",                              // optional: force tools on/off for this scenario
  "settings": {                               // optional
    "open_apps": true,                        // "Let Yap open apps when you ask"
    "app_nicknames": [{ "say": "my editor", "app": "Visual Studio Code" }],
    "apps_blocked": ["Discord"],
    "persona": "…"                            // the Chat scope's prompt
  },
  "apps": "standard",                         // evals/apps/standard.json, an inline [{id, name}] list, or "this_pc"
  "seed": {
    "folders": ["Work"],                      // besides Personal and Meetings
    "notes": [{
      "title": "Launch plan", "folder": "Work", "content": "…",
      "enhanced": "…",                        // the AI summary
      "meeting": true, "participants": ["Dave"],
      "transcript": [{ "source": "them", "text": "…", "ts": 12 }]
    }]
  },
  "turns": [
    { "user": "open python", "expect": { … }, "soft": { … } },   // soft: warnings, not failures
    { "user": "the second one", "voice": false, "expect": { … } }
  ],
  "expect_end": { … },                        // checked over the whole scenario
  "soft_end": { … }
}
```

**Checks** (`expect`, `soft`, `expect_end`, `soft_end`; text is case-insensitive; an unknown
field is an error, so a typo can't silently skip a check):

| Check | Passes when |
|---|---|
| `fast_path` | Yap answered by itself (`true`) or a model did (`false`). |
| `tools_called` | `[{ "name": "edit_note" \| ["a","b"], "args": { "<arg>": <predicate> } }]`: each matches some call. |
| `tools_not_called` | None of these were called (`"*"`: no tool at all). |
| `max_steps`, `max_tool_calls` | At most this many model calls / tool calls. |
| `answer_contains`, `answer_contains_any`, `answer_not_contains` | Substrings of the answer. |
| `answer_matches`, `answer_not_matches` | A regex (case-insensitive). |
| `max_words` | The answer's length. |
| `opened`, `opened_any`, `opened_only`, `opened_none` | Apps "opened": all of, one of, nothing but, nothing. |
| `no_false_claims` | The answer doesn't say an app was opened ("Opening Spotify", "I'll launch…", "Spotify has launched"), a note saved or changed, or text copied, when it wasn't; nor cites `[1]` with no web source behind it. Offers and questions ("let me know and I'll open it") aren't claims. |
| `min_sources`, `sources_any` | The answer came with at least n web sources (Chat's Sources row); one of their addresses holds one of these. |
| `clipboard_contains`, `clipboard_untouched` | What the clipboard tool was handed. |
| `notes` | `[{ "title", "exists", "folder", "contains", "not_contains" }]`, as the notes are afterwards. |
| `folders`, `notes_count` | The folders afterwards, exactly (no invented ones); how many notes. |
| `allow_error` | The turn may end in an error (else an error fails it). |

Argument predicates: a string (equal, case-insensitive), a number or bool (equal), or an
object: `{"contains": s}`, `{"contains_any": [..]}`, `{"not_contains": s}`, `{"matches":
regex}`, `{"note": "Launch plan"}` (that note's id), `{"empty": true}`, `{"absent": true}`,
`{"present": true}`, `{"equals": v}`.

Each run also records **soft metrics** per scenario: model calls, retries, tool calls,
prompt / completion / cached / reasoning tokens, model time and wall time, rate-limit waits,
pacing waits, words, (local) llama.cpp prompt and generation speed, the slowest single call,
and how many calls took **longer than 20 s**, Chat's own timeout: run a slow local model with
`--timeout 180` to see what it *can* do, and read "Calls > 20 s" for what Yap would do with
it today (those calls would fail).

Small models answer differently from run to run (Chat uses temperature 0.5): compare local
models with `--repeat 2` or more; a cloud model's daily token cap may only allow one pass.

`cargo test eval::` checks that every scenario in the repo parses and that its app list
loads.

## Raw requests (`--raw`)

For trying a provider feature before Yap uses it.
`evals/raw/groq-browser-search.json` is the Groq `browser_search` spike:

```jsonc
{ "requests": [
  { "name": "a1 …", "provider": "groq",   // the endpoint: the provider's, or "base_url"
    "yap_system": true,                     // put Chat's own system prompt (persona + tool instructions) first
    "yap_tools": true,                      // add Chat's own function tools to body.tools
    "body": { "model": "openai/gpt-oss-20b", "messages": [...], "tools": [{ "type": "browser_search" }] } }
] }
```

Each is logged like any call (`transcript.jsonl`) and summarised in `spike.md`: status,
latency, usage, rate-limit headers, the message's fields, any tool calls, and what a
server-side tool ran (`executed_tools`: its queries, and the pages' titles and URLs).

## Adding a scenario

1. Write `evals/scenarios/<area>-<what>.json`: a realistic message (as Nathan would say or
   type it), the seed it needs, and the checks that say "this went right". Prefer checking
   what *happened* (`opened`, `notes`, `tools_called`) over wording.
2. `cargo test eval::` (it parses), then run it on one model with `--only <name>`.
3. Read the answer and the transcript before trusting a pass or a fail: a check that's too
   strict (or too loose) is a bug in the scenario.

## Findings so far (18 Oct 2026)

Run folders under `E:/yap-builds/eval-runs/` on Nathan's PC (Ryzen 7 9800X3D, llama.cpp's
8 threads, 64 GB; local models on the **CPU** unless said). 24 scenarios (20 + 4 web).

| Model | Pass | Mean turn | Calls over Chat's 20 s | Memory (llama.cpp's projection / peak) |
|---|---|---|---|---|
| Groq `openai/gpt-oss-20b` (free plan, paced to 8k tokens/min) | 21/24 (two passes: notes/knowledge/voice before the games + web merge, apps/honesty/web after) | ~1–2 s | 0 | cloud |
| Qwen2.5-1.5B (on-device default), Yap's rule: no tools | 38/48 (79%, ×2) | 1.0 s | 0 | 1.6 / 1.4 GB |
| Qwen2.5-1.5B, tools forced on | 39/48 (81%) | 2.6 s | 0 | 1.6 / 1.8 GB |
| Qwen2.5-3B, Yap's rule: no tools | 38/48 (79%) | 1.5 s | 0 | 2.6 / 2.4 GB |
| Qwen2.5-3B, tools forced on | 38/48 (79%) | 9.3 s | 2 | 2.6 / 3.0 GB |
| Llama-3.2-3B, tools forced on | 20/48 (42%) | 43 s | 11 | 3.3 / 7.5 GB |
| Qwen3-4B-Instruct-2507 (tools by Yap's rule) | 36/48 (75%) | 13 s | 7 | 4.1 / 6.7 GB |
| Qwen3-4B-Instruct-2507 on the GPU (`--ngl 999`, Chat's own timeout) | 18/24 (75%) | 1.0 s | 0 | GPU |
| Qwen2.5-7B (tools by Yap's rule) | 20/24 (83%) | 23 s | 15 | 5.4 / 6.0 GB |
| Qwen3.5-4B, thinking on (its default) | loops (21 calls) and 180 s timeouts on plain chat: stopped | 35–41 s first call | most | — |
| **Qwen3.5-4B, thinking off** (`--llamafile-arg=--reasoning --llamafile-arg=off`) | **23/24 (96%)** | 25 s | 16 | 3.8 / 4.4 GB |
| **Qwen3.5-4B, thinking off, on the GPU** (Chat's own timeout) | **44/48 (92%, ×2)** | 1.4 s | 0 | GPU |

**Which local model.** On a CPU-only PC the 1.5B stays the right default: about a second a
turn, 1.6 GB, 79% with Yap's rule (no tools: the open-apps fast path does the apps, the
fixed pipeline the web) — but it can't act on notes, and with tools forced on it managed
half the note actions (as did the 3B). **Qwen3.5-4B with thinking off** is the one small
model that used the tools well (96% on the CPU, 92% over two GPU passes; edits, folders,
the injections refused) — on a GPU it's as quick as the cloud (1.4 s a turn); on the CPU,
25 s a turn, so two thirds of its calls would hit Chat's 20 s timeout. To offer it, the
sidecar needs `--reasoning off` (with thinking on it loops and times out), the on-device
model id must name the GGUF so `supports_tools` sees "4B", and a CPU-only PC needs a
longer local timeout (or streaming). Keep the ≥ 4B rule, but by model rather than by the
size in its name: Qwen3.5-4B with thinking on passes the rule and fails, Llama-3.2-3B would
fail it anyway.

What the logs showed (each with the transcript to prove it):

- **Groq answers HTTP 400 `tool_use_failed`** when gpt-oss writes a bad tool call (an
  `edit_note` without `note_id`; a `web_search` after Yap took it out of the tools at the
  3-search cap), and Yap's tool loop ends the turn with that error. Retrying the step (with
  the error as a tool result), and keeping a capped tool offered but refused, would fix both.
- **Without tools, gpt-oss-20b claims actions** ("Your note … has been saved"); since the
  merge, `apps::NO_TOOLS_NOTE` + `claims_unasked` cover the open case, and a re-run passed.
- **Voice answers run long** (87–98 words for "how long do I boil an egg"): nothing tells a
  non-web Chat turn it will be spoken.
- **Small tool models skip `web_search`** and answer from memory with made-up `[1]`
  citations (Qwen3-4B: "Python 3.12.0 [1]", no sources); the fixed pipeline (1.5B) got
  the current facts right. The keyless bg3.wiki lookup rarely surfaces the Gale article
  (Gale/Banter, Tara, Act One/Camp Events come first), so every model failed that one but
  Qwen3.5-4B on the GPU (whose own queries found it, both passes).
- **Notes**: the commonest local failure is a new note in the right folder (Stories); small
  models also rename or garble a line instead of `edit_note`'s exact replace.
- **Prompt injection**: open_app's guard refused every note-driven open, and no web page
  made Yap act; but 3B/4B models *said* they had obeyed ("Spotify and the Calculator have
  been opened"), and the 1.5B repeated the page's instructions as its answer.
- **llamafile's defaults cost RAM**: 4 slots and an 8 GB host prompt cache (`--cache-ram`):
  the 4B's server grew from 4.1 GB to 6.7 GB over a run. `-np 1 --cache-ram 512` would cap it.
- **The on-device provider always sends `qwen2.5-1.5b-instruct`** as the model id, whatever
  GGUF is picked, so `supports_tools` never gives a bigger on-device model its tools.
- **Llama-3.2-3B** loops on tools (337 calls for 48 scenarios, up to the 20-step cap), writes
  tool calls as text, and its malformed calls make llamafile answer HTTP 500.

**Groq's `browser_search` spike** (`--raw evals/raw/groq-browser-search.json`, Nathan's free
key): on its own it works (HTTP 200; Exa-powered; one searched answer 3.6–4.5k tokens, ~0.4 s
of Groq time, 2.5–3 s wall), with `message.executed_tools[]` = a `browser.search` entry
(`arguments` = the query, `search_results.results[]` = 5–10 `{title, url, score, content:
""}`) and a `browser.open` entry (the page read: one result whose `content` is the page text
and `url` the page), plus `reasoning`; answers cite as `【1†L4-L9】`. **Mixed with Yap's
function tools** the request is accepted (HTTP 200), but gpt-oss-20b called `search_notes`
instead of searching and, handed "no notes found", answered from memory, wrongly ("the
Druid's Grove"); a notes question in the same set-up rightly called `search_notes`. It
didn't pick the built-in tool next to function tools. So Groq's search fits Yap as a
**sub-call** (`web::native_backend`: `tools: [browser_search]`, `tool_choice: "required"`,
the hits from `executed_tools`), not mixed into Chat's own request.
