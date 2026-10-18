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
| `--keys-from-yap` | Use the API keys saved in `%APPDATA%\yap\config.json` (read straight from the file, **read-only**; never through `config::load`, which may save). Else `GROQ_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`, `GEMINI_API_KEY`, `YAP_EVAL_CUSTOM_KEY`. |
| `--only a,b` / `--tag t` | Scenarios whose name contains `a` or `b` / that carry tag `t`. With `--raw`, `--only` picks requests by name. |
| `--tools on\|off` | Treat the model as able (or not) to call tools, whatever `tools::supports_tools` says (its ≥ 4B rule for local models). Default: Yap's rule. |
| `--tokens-per-minute <n>` | Pacing budget. Default **8,000 for Groq** (its free plan's limit), none otherwise. The pacing also reads Groq's `x-ratelimit-remaining-tokens` / `-reset-tokens` and waits for the window to refill, so a run doesn't spend minutes in 429s. Time held back is reported. |
| `--max-total-tokens <n>` | Stop starting scenarios on a model past this many tokens (a free plan's daily cap: Groq's gpt-oss-20b allows 200k a day). |
| `--timeout <secs>` | Per model call, instead of Yap's own (20 s for Chat). Leave it to measure what Yap does; raise it to measure what a slow model *could* do. |
| `--repeat <n>` | Run each scenario n times (models answer differently at temperature 0.5). |
| `--include-needs-web` | Also run scenarios marked `needs_web` (skipped until Yap has web search). |
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
- No window, tray, hook, audio or browser.

The hooks in the app's code (`llm.rs`, `tools.rs`, `config.rs`, `notes.rs`, `portable.rs`)
are no-ops outside a run, and compiled to nothing in release builds.

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
  "needs_web": false,                         // true: skipped until Yap has web search
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
| `no_false_claims` | The answer doesn't say an app was opened ("Opening Spotify", "I'll launch…", "Spotify has launched"), a note saved or changed, or text copied, when it wasn't. |
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

## Findings so far

See the run folders' `summary.md`; the first runs (18 Oct 2026) are summarised in the
commit that added the harness.
