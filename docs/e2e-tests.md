# End-to-end UI tests

`npm run test:app` launches a **real Yap build**, clicks through it the way a
person would, checks the results (on screen and in Yap's data files) and saves
a **named screenshot** at each key step. Any uncaught exception or unhandled
promise rejection in any of Yap's webviews fails the test that was running.

It's [Playwright](https://playwright.dev) driving the app over the Chrome
DevTools Protocol that WebView2 exposes. Every Yap window (the main window,
onboarding, the meeting notepad, the overlay) is a Playwright page. The build is the default
**stub** build (no `engines` feature: transcription returns
`[STT stub: received 1.5s of audio, engine=parakeet]`), so it needs no GPU,
Vulkan SDK or model. The suite takes about 20 seconds, plus the build.

## Running it

```bash
npm run test:app                       # build the test app, then run the suite
npm run test:app:build                 # only the build
npx playwright test -c e2e             # only the suite, against the last build
npx playwright test -c e2e -g notes    # tests whose title matches "notes"
npx playwright show-report test-results/app/report
```

The first build compiles the stub app from scratch (a few minutes); after that
it's an incremental build of seconds. It lands in its own Cargo target dir,
`src-tauri/target/e2e` (override with `YAP_E2E_TARGET_DIR`; `YAP_E2E_EXE`
points the suite at an exe directly), so it never competes with a running
`tauri dev` for its target dir.

**Safe next to your own Yap**, installed or dev. Each test instance:

- runs in **portable mode** from its own folder (`test-results/app/runs/…`):
  config, notes, history and logs live in that folder's `Data/`, never in
  `%APPDATA%\yap`;
- has its own WebView2 profile and DevTools port (both picked fresh), and a
  `config.json` seeded with hotkey F24, no autostart, no update checks, no
  local API bridge and no sound;
- runs in **test mode** (`YAP_E2E=1`, honoured by debug builds only, see
  `src-tauri/src/shell/e2e.rs`): no global keyboard/mouse hook, nothing pasted,
  typed or copied into other apps (a dictation still reaches history), no
  other app's sound silenced or paused, no
  reading the text around another app's cursor or its window's title, no
  focus stealing, no Windows notifications, no killing "orphaned" sidecars,
  and the installed app's saved window position is neither read nor written.
  A meeting recording opens no audio device: it plays `you.wav`/`them.wav`
  from `YAP_E2E_MEETING_AUDIO` (`YAP_E2E_MEETING_SPEED` times real time) or
  stays silent, and the `e2e_meeting_feed` command hands it transcript
  segments directly. `e2e_meeting_dictation`, `e2e_meeting_output_change`
  and `e2e_meeting_quiet` stage a hotkey dictation and a switch of output
  device at a point of that audio, and shorten the quiet-side warning's
  timings, without any microphone or speakers; `e2e_meeting_checkins`
  shortens the quiet-meeting countdown's and the check-ins' timings and
  keeps the player's "mic" delivering digital silence meanwhile (so the
  meeting's audio clock runs, as with a real mic). `e2e_meeting_fail
  { side?, from?, to? }` makes the live recorder's chunks over a stretch
  fail (a replay of the audio isn't affected), `e2e_meeting_start_twice
  { first, second }` starts two meetings on two threads at once, and
  `YAP_E2E_REFINE_CHUNK_MS` makes each chunk of the refine after a meeting
  take that long. For dictation,
  `e2e_dictation_mic { silent }` zeroes the real microphone's audio (a
  "muted" mic for the dead-mic watchdog), `e2e_mic_status` says whether the
  dictation mic is open (it opens on demand, `dictation/mic.rs`; `YAP_MIC_WARM_MS`
  shortens its warm window), `e2e_dictate_audio { seconds, amplitude? }` dictates a tone
  in the mic's place without needing one (with `amplitude`: that peak, in
  syllables — a whisper's 0.005 for the level meter),
  `e2e_shortcut { name }` sends Paste last / Copy last transcript's press as
  the hook would (a test run has no hook), `e2e_dictation_timing { floorMs,
  sttMs, forget }` brings "Taking longer than usual…" sooner and slows the
  stub's transcription, `e2e_dictation_cap { limitMs, warnMs }` shortens
  the 15-minute length limit, `YAP_E2E_MODEL_LOAD_MS` slows the
  startup model load down, `e2e_coaching_reset` makes every
  push-to-talk hint due again, `e2e_mic_privacy { blocked }` stands in for
  Windows' microphone privacy switches (a test run reads no registry) and
  `e2e_paste_elevated { on }` makes every paste target count as an app
  running as administrator (a test run reads no other process), and
  `e2e_crash_next_dictation` makes the next dictation panic in its paste
  step (a bug's crash, on purpose: `pipeline::Busy` must let dictation
  go), and `e2e_fail_saves { store, count }` makes a store's next `count`
  saves fail as a full disk would (`shell/saving.rs`; store ids: "notes",
  "history", "chats", "memory", "calendar"…; 0 clears), and
  `e2e_upload_pace { ms }` holds each chunk of an Upload that long once
  transcribed (the stub does one at once), so a spec can leave the view
  mid-job. The correction watch after a paste
  (`dictionary/learn.rs`) never reads another app's field in a test run:
  `e2e_learn_correction { pasted, edited, fresh }` hands Yap the edit it
  would have seen, and the rules, the prompt and the dictionary are real. It reads no installed apps'
  icons (Settings' call apps, the bar's call card): a spec that wants one
  to look installed points `YAP_E2E_APP_ICONS` at a folder of
  `<app id>.png` stand-ins (`support/icons.js` draws them). It quits when
  the suite closes its stdin, including when the suite dies;
- talks to an account service that isn't there (`YAP_AUTH_URL` points at a
  closed local port), so it's signed out and never touches your saved session;
- keeps the local API's discovery file in its run folder (`YAP_BRIDGE_FILE`,
  never `~/.yap/cli-bridge.json`) and treats `runs/<name>/ai-apps/` as the
  user profile when Settings → AI apps (MCP) edits AI apps' config files
  (`YAP_MCP_CLIENT_ROOT`): a run never adds Yap to your real Claude, Cursor…

A Yap window (1200×800) is shown while the suite runs, plus the onboarding
window for a moment, the Yap bar (the `overlay` window, a small pill above
the taskbar of the primary monitor), and in the meeting specs the meeting
notepad, docked to the right edge of the screen. Since 2026-10-25 the main
window, onboarding and the notepad go to the bottom of the z-order when shown
(`shell::window::behind_in_test_runs`; not in robot mode), behind the apps
you're working in, and still draw there (`e2e/support/yap.js` turns WebView2's
occlusion tracking off: `--disable-features=CalculateNativeWinOcclusion`). They never take the
focus, and Playwright's clicks and keys go to the webviews, not to your mouse
or keyboard. (Don't click on them while the suite runs: a real click on the
notepad's Stop, say, ends a test's meeting.) A test run's bar never reads
your cursor or the window in front of you: `bar_simulate` stands in.

## Where the results go

Everything lands in `test-results/app/` (git-ignored, cleared at the start of
each run):

| Path | What |
|------|------|
| `screenshots/<spec>/<NN-name>.png` | the named screenshots, in test order |
| `report/` | Playwright's HTML report, screenshots attached to each test |
| `playwright/` | failure details: error context and `trace.zip` (open with `npx playwright show-trace`) |
| `runs/<spec>-w<N>/` | each test instance: `stdout.log` plus `app/Data/` (its config, notes, history and `logs/yap.log.*`) |
| `fixtures/app-icons-<spec>/` | the stand-in "installed" app icons a spec hands Yap (rewritten each run) |

## CI

`.github/workflows/e2e.yml` runs the suite on every push and PR to `main`, and
on demand (`gh workflow run e2e.yml --repo nayballs/Yap`). It builds the test
app and runs the suite in two shards side by side (`--shard=n/2`, by spec file),
each uploading its `test-results/app` as **`e2e-results-1`** / **`e2e-results-2`**
on every run, pass or fail (one runner outgrew the 30-minute limit on 24 Oct 2026).
The runner has no audio devices, so the run also proves Yap starts and works
without a microphone (the stub dictation test is skipped there), and its
screen is 1024×768, so CI screenshots are a little smaller than local ones.

Hosted runners run everything elevated, and WebView2 ignores an elevated
app's `WEBVIEW2_*` variables (Microsoft's
[security notes](https://learn.microsoft.com/microsoft-edge/webview2/concepts/security#for-an-elevated-host-app-use-appropriate-override-flags)),
so the workflow sets the DevTools flag as a machine-wide WebView2 policy for
`yap.exe` instead. The suite reads the port WebView2 picked from
`DevToolsActivePort` in the instance's profile either way.

To review a run's screenshots and logs:

```bash
gh run list --workflow=e2e.yml --repo nayballs/Yap --limit 5
gh run download <run-id> --repo nayballs/Yap --name e2e-results --dir e2e-results-<run-id>
```

then open `screenshots/` (Claude can read the PNGs directly) or
`report/index.html`.

## What's covered

| Spec | Instance | Tests |
|------|----------|-------|
| `e2e/app.spec.js` | default | Home renders in portable mode · every sidebar view · every Settings page, and the nav's group names · a setting saved to `config.json` and read back after a reload · a new note survives a view switch · a dictionary entry is saved · a snippet added (multi-line text), kept across a view switch, edited, still a snippet after a Settings auto-save, removed · "Say “press enter” to send" on by default, saved off and on, the key picker with it · Account signed out with the service unreachable · onboarding opens on its first step · a stub dictation lands in the Home feed (skipped without a microphone). (What snippets and the voice command do to a dictation is unit-tested: the stub's text can't say them.) |
| `e2e/paste.spec.js` | seeded history (two dictations, then an Upload) | "Paste last transcript" (`paste_last_transcript`) returns the newest dictation — never the newer Upload — and pastes nothing in a test run (the log says so); after a deleted entry, the one before; after Clear history, "Nothing to paste yet" · Settings → Dictation → Typing into apps: "Match the text around the cursor" on by default, "Append trailing space" saying it's only for fields Yap can't read, the Win+V line, and switching it off and on saved to `config.json` · "Leave the full stop off chat messages" and "Safer pasting into terminals" on by default, auto-submit's line saying where it doesn't press Enter, each switched off and on saved to `config.json`. (The paste itself, the clipboard's privacy formats and the reading of real text fields are Rust tests: `smart_insert`, and the `--ignored` `private_clipboard_round_trip` / `reads_real_text_fields`; so are telling terminals, coding CLIs and chat apps apart and what that does to a dictation: `app_kind`, with an `--ignored` `this_machines_windows` over the PC's real windows.) |
| `e2e/dictionary.spec.js` | own instance (`dictionary`: a seeded dictionary and `dictionary_usage.json`) | the Dictionary view for hundreds of entries: a search filtering as you type (words, how they're heard, snippets' text; Esc; a row added mid-search), Ctrl+F the page's but Settings' while Settings is open · a star saved (and kept by a Settings auto-save), the four sorts, "Most used" counts, the sort remembered over a reload, Star in the row's menu · checkboxes, Shift-click, Ctrl+A, Esc, a section's box, ≈ on/off for the selection, Delete… asked and undone back in place, the Delete key, Undo after leaving the view · Import… (`e2e_dictionary_file` in place of the dialog): the preview's counts, unticking, the file's spelling, what's left out and why, Add, Undo; a text file; no file → said · Export… everything as CSV, the selection as text · 2,000 entries: open, search, a keystroke timed, more rows on scroll, Ctrl+A, a bulk delete undone · the same in the dark theme (screenshots) |
| `e2e/dictionary-list.spec.js` | none (no app) | the Dictionary view's pure parts (`views/dictionary/dictionaryList.js`, `dictionaryFile.js`): search, the sorts, CSV (quotes, `""`, line breaks, `;`/tab, headers or none) and text (`→ -> =>`, words, notes, bullets) parsing, the import's plan (new, already there, spelled differently, said twice) and choices, export round trips, both Undos |
| `e2e/screen-names.spec.js` | AI cleanup on Groq (no key) | "Spell names the way they're written on screen" (`dictionary/screen_terms.rs`) on by default, its line saying the names a dictation seems to contain go to Groq with it, Privacy & data's "Names on screen" row saying the same, off saved to `config.json` and the row then "Off". (The read, the terms and their use are Rust tests and `screen-names*.real.js`: a test run reads no other window.) |
| `e2e/push-to-talk.spec.js` | push-to-talk mode | Settings → Dictation's line on double-tap · (with a microphone:) a hold transcribing when the key comes up · a lone tap thrown away (nothing transcribed) with the "Hold F24 while you talk" Tip toast, once (`hints.json`; not again within ten minutes) · a double-tap recording hands-free past the gap, the overlay's lock, the next press transcribing, no hints after · the hint as a card on the Yap bar with the main window away · a "muted" mic (`e2e_dictation_mic`): its stream reopened once at ~3 s, "No sound from … — is it muted?" in the overlay and the full message as a toast at ~5 s, the recording going on, nothing transcribed on stop and the overlay ending on the reason · the sound coming back taking the warning back and the dictation going through |
| `e2e/mics.spec.js` | made-up microphones (`YAP_E2E_MICS`: a laptop's and a USB mic), the USB mic the old single pick, mic warm window 1.5 s | the old pick first in the order (config migrated) · Settings → Dictation → Microphone: add, move up, remove, "Not connected" after an unplug, each dictation opening the first connected one · a Bluetooth headset first in the order used, and closed at once after the dictation (never warm) · a new mic plugged in: "Contoso H2 connected · Use it for dictation?" in the window, Use it putting it first, the next dictation on it, plugged in again "Using Contoso H2 for dictation" · on the Yap bar with Not now (`micNotNow`), never asked again · never during a dictation (asked after it) · unplugged mid-dictation: it ends, "Fabrikam USB Mic disconnected · … The next one uses Northwind Audio.", the transcript in history with its audio, the next dictation on the laptop's mic · its window gone: "Your text is copied", Home's "Didn't paste · Your microphone was disconnected…" · unplugged before it heard anything: the bar says so, nothing in history · none left: "No microphone found" |
| `e2e/meeting-mic.spec.js` | made-up microphones, meeting audio from test WAVs at 4× | the meeting's mic unplugged mid-recording (`e2e_mic_unplug`): "Your microphone was disconnected · Yap is recording you with Northwind Audio now", the recording going on, the "Your mic was disconnected · now Northwind Audio" marker in the transcript and `mic` in notes.json, "You" and "Them" lines around it |
| `e2e/mic-on-demand.spec.js` | mic warm window 2.5 s (`YAP_MIC_WARM_MS`) | the dictation mic closed while Yap idles (`e2e_mic_status`) · (with a microphone:) a dictation (`e2e_dictate_audio`) opening it, the open's timing, a second one in the warm window reusing it, then closed · onboarding's mic check holding it open · "Keep the microphone ready" in Settings → Dictation (off by default) opening it and, off again, closing it · Settings search finding it by "bluetooth" |
| `e2e/model-load.spec.js` | a slow model load (`YAP_E2E_MODEL_LOAD_MS`) | a dictation started before the speech model has loaded waits for it, "Loading the speech model…" on the bar, and goes through |
| `e2e/local-on-demand.spec.js` | own instance: on-device cleanup, the model's files empty stand-ins (`support/local-llm.js` `installStandInModel`), the started model a local fake AI (`YAP_E2E_LOCAL_LLM`, "loading" for `YAP_E2E_LOCAL_LLM_LOAD_MS` 1.5 s; no llamafile runs); two more instances of its own | the on-device model loaded when it's needed, freed when idle (`ai/local_llm/on_demand.rs`): not loaded at launch, Settings → Language Models saying "Not loaded · loads when needed", "Load when Yap starts" off and "Free its memory after it's unused for" 15 minutes · a dictation starts loading it as it starts recording (`e2e_local_llm_loads`: "dictation"), its cleanup waits for it ("Loading the AI model…" on the bar, with Skip cleanup) and goes to it (the model's id), the next one goes straight to it · freed after the idle time (`e2e_local_llm_idle_ms`), never while a dictation is going on (`e2e_local_llm_busy`), logged, "Never" keeps it; the next dictation loads it again · Free memory, Load now ("Loading…", then "Loaded · ready"), "Load when Yap starts" saved (turning it on loads it), the idle choice saved · "Load when Yap starts" on: loaded at launch · cloud models only (setting on, model here): never loaded, at launch or by a dictation. Screenshots `01-not-loaded`, `02-bar-loading` … `05-load-at-launch-on`. |
| `e2e/no-mic.spec.js` | configured mic missing | starts anyway and says "No microphone found" (toast + overlay) when asked to record · with Windows' privacy switch off (`e2e_mic_privacy`), "Windows is blocking microphone access for desktop apps" with Open settings instead · Upload still transcribes a file |
| `e2e/dictation-alerts.spec.js` | seeded history (a dictation) | a dictation's alerts (`notices/alert.rs`): an app running as administrator (`e2e_paste_elevated`) — "Paste last transcript" and a dictation (with a microphone) say "Your text is copied: press Ctrl+V" as a toast, or as a Yap bar card with the main window away, nothing pasted, history kept · Windows' microphone privacy (`e2e_mic_privacy`, with a microphone): said when a recording starts (the overlay's line and a toast naming the switch), no "No sound from…" after it, Open settings (logged, never opened, in test mode), nothing transcribed on stop · the switch turned off mid-recording: the dead-mic watchdog blames it, on a bar card with the main window away · a dictation that crashes (`e2e_crash_next_dictation`, with a microphone): `e2e_dictate_audio` comes back at once (it waited out 5 minutes before), "Dictation failed — Yap hit an error. Try again" on the bar, listed in history as failed with its audio, the next dictation goes through |
| `e2e/dictation-polish.spec.js` | its own Yap, no history | Paste last / Copy last transcript's shortcuts (`e2e_shortcut`) with nothing dictated: "Nothing to paste yet" / "Nothing to copy yet" · Settings → Shortcuts: both rows unbound, the dictation hotkey and Paste last's key refused, F21 / F20 taken, found by the search, removed · after a dictation: Copy last (the command and the shortcut) → "Copied your last dictation", Paste last logged, lengths never words in the log; in another app the note is a bar card · a slowed stub transcription (`e2e_dictation_timing`): "Taking longer than usual…", no Skip · a slowed fake cleanup after a quick one: "Taking longer than usual…" + Skip cleanup (a region), clicked: the words as transcribed come back at once · an 8 s length limit (`e2e_dictation_cap`) with 15 s of tone: "4 seconds left · it'll stop at 8 seconds", the hairline, the waveform back, stopped at 8.0 s, in history, "Dictation stopped at 8 seconds" with Recover text → "Copied your dictation" · the waveform: a whisper's tone (0.005, in syllables) at 25–45 %, full scale at 85–92 % · "Hold on, still transcribing…" for a press during a transcription |
| `e2e/dictation-trust.spec.js` | a stand-in model folder, `YAP_E2E_STT_SPEED` (processor 150 ms, graphics card 450), a dictation's audio left pending in `recent-audio/` | Settings → Speech-to-Text → Speed: "not measured yet", Measure now → "Runs on: processor — fastest for your dictations on this PC (1xx ms vs 4xx ms)", `stt_speed.json` (method 3, half and half with no dictations; both devices at 3, 11 and 36 s, three warm runs each, their time for your dictations, words agreeing), the log's numbers without text, the warm engine moved; Graphics card / Processor / Automatic saved with `useGpu` following · "Use what I said": a dictation cleanup changed (the fake model, `e2e_dictation`) → the bar's "Cleaned up · Use what I said" (a region, never activated), the swap logged, once; left alone it goes after 6 s while the shortcut's command still swaps; the tray's paste as said; punctuation-only cleanup and the switch off: no offer · the tray's paste as said after a dictation cleanup left alone (cleanup off): the dictation itself, within 5 s, and Yap still answers (it froze Yap before) · Shortcuts: "Use what I said" unbound, the dictation hotkey refused, F22 taken and removed · the pending dictation listed as failed in Home, Run again → "Transcribed again", its audio still playable · a spoken dictation (needs a mic): its audio kept, Play / Stop, Run again from the row's menu, deleted with its entry · Privacy & data: the table line, the switch, Clear audio (history stays, nothing to run again) |
| `e2e/cleanup-styles.spec.js` | own instance, no graphics card for the on-device model (`YAP_E2E_GPU=none`); cleanup on a local fake AI; dictations into a stand-in app (`e2e_style_app {exe, title}`) | Settings → Language Models → Styles by app: seven kinds with their default tones, the example following a tone, a tone, an app moved from Personal to Work chat, a site and extra instructions added to Documents, Word taken out and put back, all saved; light and dark; off and on; search by "formal casual" · a dictation's cleanup told its kind just before the transcript (Outlook, Gmail by its tab's title, Slack, ChatGPT literal, PowerShell's command with the model's "Git status." pasted as "git status"), nothing for Notepad, never in the system prompt; filler alone asks no model; Terminal "As said" asks none; the person's own app rule wins; styles off, no line · onboarding recommends Cloud on this PC, with the privacy line; "Maybe later" leaves cleanup off |
| `e2e/cleanup-offer.spec.js` | own instance, a stand-in card that runs the 4B, six dictations in history, `YAP_E2E_CLEANUP_OFFER` | cleanup off: nothing turned it on; after a dictation the one tip "Let Yap tidy what you say" naming the card, Set it up → Language Models (still off), never again · none with tips off or cleanup on · onboarding recommends Private on this PC |
| `e2e/upload.spec.js` | each Upload chunk held 1.5–2 s (`e2e_upload_pace`) | a 3-chunk file transcribing: Home and back to Upload shows the file, "Transcribing… chunk n of 3" and Cancel; away again, "Transcription complete" comes on Home and the transcript waits on Upload · Cancel after coming back returns to the file, no result, no toast |
| `e2e/save-errors.spec.js` | saves made to fail (`e2e_fail_saves`) | a note's edit that couldn't be saved: "Couldn't save your note" / "The disk is full. Yap is keeping your changes…" with Try again, the text kept in Yap and not on disk, said once (no notice besides), Try again → "Saved" and notes.json holds it · two dictations whose history couldn't be saved: one notice "Couldn't save your dictation history" with Try again, which writes both |
| `e2e/config-recovery.spec.js` | a config.json with one setting of the wrong type (the dictionary a string) | set aside as `config.corrupt-<secs>`, byte for byte; every other setting kept (the suite's hotkey and switches, an API key) and written back, the dictionary reset; "Some of your settings were reset" said once (the log has the title, never the file's contents); a switch flipped in Settings saves over the repaired file, never the copy |
| `e2e/speech-models.spec.js` | three stand-in model folders | a speech model switched on the Speech-to-Text page (Use), then as the tray or onboarding does (`set_active_model`), stays when a switch is flipped in Settings (Settings' copy adopts saves from elsewhere) · deleting a model asks first, Cancel and Esc keep it, Delete removes it |
| `e2e/home-ai-edits.spec.js` | seeded history (a dictation cleanup reworded, one it only punctuated, one from before Yap kept the words as said); cleanup on a local fake AI | Home's "AI edited · N changes" (history's `said`): only on the reworded one · it opens into what changed (`region` "What AI cleanup changed", `aria-expanded` / `aria-controls`): the words taken out struck, the words put in underlined, each with its screen-reader prefix; screenshots in light and dark; Enter on the pill closes it · a dictation through the fake model (`e2e_dictation`): history keeps `said`, "AI edited · 1 change", **Paste what I said** on it right after → the swap logged (a test run touches no other app), once; then, and on an older one, "Copied what you said" + "Yap only swaps your latest dictation, right after it. Press Ctrl+V to paste it." (logged, the clipboard untouched) · **Copy what I said** (the page's clipboard stubbed) · the row's menu: Copy, Paste what I said, Copy what I said, Delete… · a punctuation-only cleanup: no pill |
| `e2e/rewrite-last.spec.js` | own instance; cleanup and the rewrite on a local fake AI; dictations to a stand-in window (`e2e_target_window`: no window has it, nothing read or pasted) | Settings → Commands: "Rewrite your last dictation when nothing is selected" on by default, saved off and on, the rewrite shortcut's line following it, Settings search finding it by "last dictation" · the shortcut with nothing selected right after a dictation (`e2e_rewrite`): on, the AI given the dictation and its rewrite logged as the last dictation's (a test run replaces nothing); off, new text written (the AI given no text, "e2e: not pasting the text"); on again, the dictation again |
| `e2e/transforms.spec.js` | own instance; cleanup and the transforms on a local fake AI; dictations and transforms to a stand-in window (`e2e_target_window`) | Settings → Commands → Transforms: Yap's five in order (no Edit or Delete), Translate's language picked (French), a shortcut on Polish (the dictation hotkey refused, F22 taken, its input-hook slot set: `e2e_transform_slots`), your own added, on a key of its own (Polish's refused), moved up, edited, deleted behind the confirm dialog (its slot gone), Polish's key removed (no slots) · Settings → Shortcuts: "Use what I said" refused on a transform's key, the Transforms link row ("2 shortcuts") going to the list · search finds it · `e2e_transform`: Polish on a selection (the AI given it under the guardrails, its preamble stripped, "e2e: not pasting the text"), the bar's "Polish ✓ · Undo" and Undo ("e2e: not undoing the transform") · Translate with nothing selected right after a dictation (the dictation given) · "Rewrite your last dictation" off: "Nothing to transform", the AI not asked · a terminal: refused, the AI not asked · the same text back: nothing pasted, "Polish: no changes needed" · your own transform's instruction · the AI failing: "Polish failed via custom" on the bar |
| `e2e/languages.spec.js` | own instance on "Whisper Large v3" (the stub engine stands in; Parakeet V2's folder installed too); cleanup and the translation on a local fake AI; dictations to a stand-in window | Settings → Dictation → My languages: French found by its own name ("franç", Enter), English by a click, "No language called…", Esc · Whisper understands both (the coverage table), Parakeet V2: "understands English only. French needs another model." and Whisper Large v3 recommended (installed), the model itself unchanged · Open Speech-to-Text and its Languages row back · the main shortcut Auto / French · a language shortcut (English → Spanish): the dictation hotkey refused, F22 taken, its input-hook slot set (`e2e_language_slots`); Polish refused on its key; the Shortcuts link row; deleted; French removed; search finds it · `e2e_language_stage` + `e2e_dictation`: the shortcut's dictation translated (not cleaned up), what was said kept for Use what I said; the AI failing: pasted as said, "Couldn't translate into Spanish"; the main shortcut: cleanup told the languages it may be in, then (Whisper heard French) that it's French; English: no line · through the stub speech engine (`e2e_dictate_audio`, skipped without a mic): the armed shortcut's dictation translated, the next one not · the nudge (`e2e_language_heard`): "You spoke French · Add it to My languages?" in the main window, Add French → My languages; within ten minutes nothing more; in another app a Yap bar card, Not now; too short or not sure enough: never; switched off: never |
| `e2e/word-diff.spec.js` | none (no app) | `lib/core/wordDiff.js` on its own: filler words out as one change, words swapped (out struck, in marked, each run one change), capitals and punctuation never a change (a dash out not shown, one in read as text), the cleaned text's line breaks kept, all of it rewritten or nothing said, a middle too big to align as one change, a 2,000-word dictation with one change aligned in its middle |
| `e2e/bar.spec.js` | default | the Yap bar (`yap.overlay`), through the debug-only `bar_simulate` (a pretend cursor on a region, fake fullscreen kinds, Esc) and `bar_debug`: idle, bottom-centre on the work area, click-through and never activated — its ex-styles read inside the app and from outside it (`support/win32.js`: GetWindowLongPtr, and WindowFromPoint on the pill showing a click would go through) — and Wispr's measured pill (40 × 8, 14 px up, a 60 × 20 hit wrapper, its colours; `flowbar-spec.md`) · the cursor on the pill opens it and makes just that clickable, then click-through again; tooltips with the shortcut, in Wispr's tooltip style · a notice card (the update card, as a demo) to Wispr's card measurements, 26 px above the pill, with its hit strip · its menu: hide for an hour (Settings shows it, Show it now) and Turn off the bar (saved; Settings adopts it and turns it back on) · Settings → General → Yap bar · a call prompt as a "Meeting detected" card (its ^ menu, ✕, an installed app's own icon — a stand-in for Teams — and Google Meet's mark), answered on the bar · fullscreen: no pill, a card over a borderless app, a card waiting out an exclusive one · ◉ starts meeting notes; the recording pill (69 × 30, half opacity, its ring, 5 bars, the stop circle; hovered, the timer) opens the notepad and ■ ends the meeting · the opt-in countdown (Esc cancels; left alone it records; the call-ended card) · dictating in the pill's place · (with a microphone:) the capsule's ✕ and ✓, a clickable region (`bar_simulate` on "dictation") that never activates the bar; ✓ transcribing as the hotkey would · ✕ → "Dictation cancelled" + Undo, nothing transcribed, Undo transcribing it, once · the tray's Cancel (`cancel_recording`) keeping it too, a new dictation dropping it, Undo gone after 10 s · with the bar off, ✓ still clickable |
| `e2e/snip.spec.js` | Chat on a local fake AI as Groq (`qwen/qwen3.8-27b`, a key) | Snip to Chat (`snip.rs`, docs/architecture/snip.md) over **fake monitors** (`e2e_snip`: a 1280×720 HDR one at 240 nits whose picture goes through the real FP16 → sRGB path, a 960×540 one at 150 %; test mode never reads the screen), the overlay made and painted but never shown, driven by posted window messages (`e2e_snip_input`), how it went from `e2e_snip_last`: a snip from Home lands in a new chat — the thumbnail with "200 × 100", the caret in the box, the three prompts, nothing sent — its PNG exactly #C0C0C0 (an 8-bit read would have clipped it) · Enter on an empty box asks "What's this?" with an `image_url` part (image first, then the text), the sent copy exact, chats.json holding the id and size, no base64 · a prompt sends in one tap; a drag the other way on the 150 % monitor exact too · Esc, right-click, a click and a twitch snip nothing · the conversation open in Chat gets the snip · GPT-OSS 20B: "can't read images.", Send, Enter and the prompts waiting, "Use Qwen3.8 27B on Groq" switching in one click and then sending · a server refusing the image (`rejectImages`): the same line, the question and screenshot back in the box, no error toast, "Send without the screenshot" · "Take a screenshot." dictated and "Snip it." to the Voice Assistant start one; "snip the ends off the beans" is dictated; switched off, "Snip." is a word · Settings: the shortcut (Win+Alt+S by default, F24 refused, F21 taken, removed), the voice switch, Keep chat screenshots (90 days, Forever) · ✂ on the bar, the shortcut again cancelling, Chat's + → Take a screenshot, Win+Alt+S in the main window (the in-page fallback) · a screenshot 40 days old swept when the setting is 7 days, "Screenshot removed after 7 days" in its place; deleting a conversation deletes its screenshots · 2026-10-25, the clipboard and the voice: a snip to Chat on the pretend clipboard too (the very PNG; `e2e_snip_clipboard {clear}`), "Copied · Ctrl+V…" in the box, Settings → Shortcuts' "Copy every snip to the clipboard" off stopping it, a pasted picture not copied back · Rust's waiting box (`chat_waiting_get`) taken by a question asked out loud (`e2e_assistant_ask`): the image part and the question to the fake AI, the box cleared, the thumbnail in the bubble; a dictation into the box (`e2e_chat_dictation`) sending both with Enter; GPT-OSS: nothing sent, the question and screenshot left in the box with the line; ✕ first: no image · "Snip." dictated → the overlay → Chat → a question listened for by itself ("Ask about this screenshot…" on the capsule, its transcript staged with `e2e_snip_question`) → sent with the image; during a call (`meeting_detect_simulate`) and after Esc in the overlay, nothing listens |
| `e2e/settings-search.spec.js` | default | Settings after the 2026-10-22 review (`lib/settings/`): the search index and the link resolver on their own (every entry well-formed, ranking, word forms, grouping by page and section, highlights, every alias to a real page) · **every index entry opened in the app and its row found** (so the index can't drift from the pages) · the search: Ctrl+F, results by page with counts in the nav and the rest dimmed, ↑/↓, Enter landing on the row (washed, its control focused), a switch working in the results, nothing found, Esc clearing then closing, `/`, a clean box after closing · old links landing (`general#meetings`, `general#screen-sharing` — also through Rust's `yap-open-settings` —, `history`, `advanced`, `general#bar`, `voice#assistant-shortcut`, `meetings#call-apps-list`, `cleanup#chat`, Memory (the "Yap remembered…" toast's Manage) and a row on it; a page that doesn't exist leaving Settings put) · Meetings' call apps one level down behind their count · 860 × 600: the icon rail (all 13 pages wholly on screen, tooltips), no sideways scroll, no squeezed rows, the rail's search · contrast measured in the app (descriptions, page lines, headings, nav labels, links, the version ≥ 4.5:1; an off switch's edge and the focus ring ≥ 3:1) and one focus ring on a switch, a select, a segmented control and a nav item |
| `e2e/polish.spec.js` | own instance (`polish`: one seeded dictation from Teams; Chat pointed at a local port nothing listens on) | the 2026-10-22 polish pass, as a new user meets Yap: the Copilot key (`lib/core/hotkeys.js`), `aiErrors.js` and `appNames.js` on their own · the hidden windows' webviews on WebView2's low memory target, the bar left alone (`e2e_webview_memory`) · Home's "1 dictation", Insights' "Teams" and "1 dictation", Upload naming Parakeet V3, the serif title on Insights / Upload / Dictionary / Integrations / Meetings · Home at 860 × 600: the stats in a row above the tip card, no sideways scroll · Settings → Shortcuts' picker offering the **Copilot key**, saved as `kb:shift+win+134`, shown as "Copilot", push-to-talk's line for it, Home greeting with it · onboarding: two local models and "More models (N)" with the Download button in view, the tray step's copy and its footer at the bottom, the shortcut picker (a Ctrl+Shift+Y combo saved whole, the Copilot key offered), Finish bringing the hidden main window up · Chat with its AI unreachable: "Couldn't reach the AI model on this PC (127.0.0.1:N)…" as a sentence, the message given back, Open Language Models · an empty Chat with no AI model saying what it needs · Integrations' endpoint list folded for developers · Home with no dictations and no speech model: "One step left: a speech model" → Settings → Speech-to-Text, the tip card starting on AI cleanup · onboarding's speech models: two cards (Parakeet V3, recommended, and Parakeet V2 for an English PC, else Whisper Large v3 Turbo), the Download button in view, "More models (12)" opening the rest · what needs doing counted once, on the title bar's bell ("Notifications (1)", its list, a click to the page that fixes it), never again on the Settings row |
| `e2e/updates.spec.js` | pointed at a local `latest.json` | a manual check finds the next patch version, downloads and verifies it, the "ready" toast and Settings → About say so, and "Restart to update" stops short of the installer (debug builds never run it) |
| `e2e/meeting-detect.spec.js` | default | call detection through the debug-only `meeting_detect_simulate` hook: a call offers to take notes and "Not now" leaves it (and the app's next call) alone · a prompt leaves with its call · a prompt left alone fades (its `fadeMs` shortened from 30 s) and counts as "Not now" · "Record notes" creates a Meetings note and records it, the call ending offers "Stop and summarise", which ends it in the note and runs the summary step · "Keep recording" carries the notes into a rejoined huddle, whose end asks again · a Discord call isn't asked about by default · "Don't ask for Teams" confirms, Settings shows Teams off, and switching it back on asks again · "Quietly" keeps the prompt out of the window · Settings → Meetings lists how Yap asks and, behind "Ask about calls in · N of M apps", every app's effective choice, in two columns, each row with the app's icon (stand-ins for Teams and Discord, `YAP_E2E_APP_ICONS`; the bundled mark or a letter for the rest), lined up and greyed with detection off · the Settings toggle turns it off and greys out the rest |
| `e2e/calendar.spec.js` | a local iCal server (`support/calendar-feed.js`) and a local fake of Google (`support/fake-google.js`, via the debug-only `YAP_GOOGLE_*_URL`) | Settings → Connectors: the three cards, the Outlook guide, a link that was reset, one that works · the Meetings view: today and the next 7 days, Conflict and Maybe, all-day and non-meetings hidden, Show more, a draft note · the reminder card, in the window and on the Yap bar (a call card: Teams' letter badge, "● In 1 min · with Tanay +1"): **Join & take notes** opens the link (recorded, never opened, in test mode) and records into a note with the meeting's name and attendees, the bar's copy goes too, and its simulated call isn't asked about again · Esc dismisses, the bar's ^ menu snoozes (a calendar mark for a meeting without a call app), a snoozed card comes back with its call, the bar's ✕ dismisses everywhere · back to back: a call during the next meeting offers to switch notes (on the bar too), and switching ends the first · Google in one click: consent with PKCE, read-only scopes, meetings listed, the ⋯ menu, Disconnect revokes the token and clears the meetings · the "Connect your calendar" dialog from every Connect calendar (the Meetings view's grey card, Integrations, the nudge after a meeting over the Meetings view): the bar's own card in its band (a picture: nothing to press or read out; its sample Google Meet call with Meet's mark), three ways in, the focus kept inside and given back, Esc / ✕ / the scrim closing it · an iCal link through it: the Outlook steps, Back, a reset link's reason in the dialog, then "Calendar connected · 7 meetings in the next 7 days" and the dialog closing on the listed meetings, no toast as well · Google through it: "Finish connecting in your browser…", Cancel, a refusal shown in the dialog, then connected with its one meeting |
| `e2e/calendar-nudge.spec.js` | Chat pointed at a local fake AI | no calendar yet: one grey "No meetings found" card whose Connect calendar opens the dialog, Continue with Google off in a build without Google's client ("Google sign-in is in the installed Yap…"), and Settings → Connectors saying the same · the "Connect your calendar" nudge once after a meeting ends, and Not now means never · past meeting notes by day, with search, Generate summary (the action plan, in the note) and the Ask bar asking Chat about your meetings · ◉ Take notes in the header starts meeting notes as the meeting shortcut does, and Stop and summarise ends them |
| `e2e/chat-streaming.spec.js` | Chat and Note Formatting pointed at a local fake AI that streams (server-sent events, paced per step) | streamed answers (`chat/stream.rs`, `streaming.js`): `closePartial` and the events' state on their own (no app) · an answer growing in place (a mid-stream screenshot: part of the list, the bold already bold, ■ in the composer), then the same bubble whole, the request asking for the stream and its usage · a tool call: the line the model wrote before it never shown, its chip, its arguments (streamed in fragments) reaching the tool whole · ■: the fake sees Yap hang up, what came stays, "· Stopped" under it and `stopped` in chats.json; stopped before anything came: the question stays, "Stopped" · a provider error mid-stream: asked again whole, the streamed text replaced · the checked answer replacing what streamed (a claimed search gets Yap's note, its "[1]" with nothing behind it gone) · a dictation starting mid-stream: the local stream steps aside ("Waiting for your dictation to finish…"), then answers · a newer question replacing the answer still coming (`stream::Halt::Superseded`): one asked out loud (`e2e_assistant_ask`) or dictated and sent (`e2e_chat_dictation` + "press enter") into the same conversation — one answer, to the newer question, the older bubble gone without an error, both questions kept (chats.json: user, user, assistant), the older stream hung up on and never asked again · a server that ignores `stream` answering whole · the notepad's Ask sheet streaming its answer (mid-stream screenshot) and its ■ |
| `e2e/chat-tools.spec.js` | Chat pointed at a local fake AI, scripted call by call | the AI Chat changing part of a note through its tools: `search_in_note` (both lines found, numbered), an `edit_note` whose text is in two places refused (nothing changed; the model told "appears 2 times … (lines 1, 3)"), the edit of the one place, the answer · the note changed there only in notes.json, the meeting notepad showing it following at once · the thread's chips (Found … / Couldn't edit … / Edited "Launch plan": "Friday." → "Monday.") and the same chips when the conversation is opened again · the Notes view showing the edit · the chat never inventing a folder: a `create_note` into a folder that doesn't exist refused (nothing saved, the model given the folders and told to leave `folder` empty), the retry filed in Personal, no new folder |
| `e2e/chat-usage.spec.js` | own instance (`chat-usage`): cleanup and Chat on a local fake AI as Groq (a test key; `YAP_E2E_NET_ALIASES` makes it api.groq.com, so its calls count as Groq's cloud), Groq's model list through `YAP_MODELS_STANDIN` (GPT-OSS 20B with a 4K context, 120B with 128K), the fake's token counts set per kind (`usage(kind, { usage, timings })`) | the AI usage tracker (`ai/usage.rs`) and Chat's context ring: a dictation's cleanup (whole) and a Chat answer (streamed, `stream_options.include_usage`) counted from their own `usage` — `ai_usage_summary` today by feature and model with the estimate at Groq's price, Insights → AI usage (today's tile, by model with "≈ $", by feature), `ai_usage.json` holding counts and never the words or the key, Groq's "Usage today" meter and `groq_usage.json` still counting both calls · the ring on a new chat: none; after an answer 1,200 + 400 of 4,096 → `data-percent` 39, the tooltip "39% of context used" on hover, the button's name; its panel ("1.6K of 4K tokens", the meter's value, "400 tokens · 120 tokens/s" from llama.cpp-style timings, today's and the week's rows), Esc giving the focus back · 90 %: the "Nearly full… A new chat answers best." line with New chat, light and dark · a switch in the picker to GPT-OSS 120B: 3 %, back to 20B: 90 % · the line's New chat: no ring, then the new chat's 64 % · See all: Insights' card in view |
| `e2e/voice-assistant.spec.js` | Chat pointed at a local fake AI, Kokoro's download from a local server | Settings → **Commands** and → **Voice Assistant**: each says what it does with an example and links to the other, Language Models' tabs (Commands / Chat & Voice Assistant) and their links back, the Shortcuts page with all four · the assistant shortcut set, refused when it clashes (the dictation key; a combo the meeting shortcut fires inside), taken from the rewrite shortcut, cleared; Chat's "Set an assistant shortcut" · a question staged with `e2e_assistant_ask` with the main window hidden: the capsule's "Ask Yap…", "Thinking…", "Speaking…" with ■, read aloud (`tts_e2e_spoken`) with spoken replies off, the answer's bar card, the conversation open in Chat continued (the question marked by voice), the card and capsule gone with the voice, a follow-up in the same conversation · asked with the window in front: Chat comes up, no card · ■, the card's ✕, a new question cutting the voice off (barge-in), the shortcut in Yap's own window stopping it · a barge-in question while the last one is still being answered: one answer, to the newer question, said once (chats.json: both questions, one answer; the older never asked again) · during a meeting: shown, never spoken · no AI model / a failing model: alerts with Open settings · spoken replies in Chat: Voice replies, ▶ / ■ / Esc, "When to speak", hands-free, barge-in · Kokoro: offer, download, voices, delete, never from the internet |
| `e2e/voice-elevenlabs.spec.js` | Chat pointed at a local fake AI; ElevenLabs a local fake (`support/fake-elevenlabs.js`, the debug-only `YAP_ELEVENLABS_URL`); Kokoro's download from a local server | Settings → Voice Assistant → Voice: "Speaks with" On this PC / ElevenLabs, the privacy line only while ElevenLabs is picked, "Get your API key" (opened through the opener), a key it won't take said so, the right key saved and masked, the account's voices (own, then defaults), George by default, another voice saved, the model (Flash v2.5 recommended, no speech-to-speech model), the month's characters (none without the permission), ▶ a voice's own sample fetched without the key and no characters used, ▶ a voice without one through the service · a Chat answer read by ElevenLabs: one `pcm_24000` request a sentence, the model, the key in its header only · quota used up with no Kokoro: one notice, silence, no second notice · a revoked key with Kokoro downloaded: Kokoro reads the whole answer (the failed sentence included), one notice with Open settings, the next answer straight to Kokoro |
| `e2e/meeting-search.spec.js` | own instance (`meeting-search`: two meetings and a note seeded); Chat pointed at a local fake AI; the engine and the search model downloaded from local stand-ins (`support/local-llm.js`, 4 MB); the embedding server a local fake (`support/fake-embed.js`: words hashed into 768 dimensions, a few sharing a sense — "staff" is "hiring", "teeth" is "dental" — via the debug-only `YAP_E2E_EMBED_URL`) | meeting search ([search.md](./architecture/search.md)): Settings → Privacy & data → Search my meetings by meaning — the switch disabled until the model is here, the size with the engine's, the download's progress held half-way, then on, every passage of the three notes read, the index's count and size, `vectors.bin` and the NOTICE on disk, What leaves this PC saying it's searched by meaning on this PC · the Meetings view's moments by meaning ("can we take on new staff" finds the hiring freeze, the speaker said once, no "nothing matches" under it), a word of the line picking the line, a click opening the meeting at it, focused; a personal note by meaning · Ask across meetings from the Meetings view's Ask bar: the chip "Searched your meetings by meaning", [1] a pill and [9] stripped, the From your meetings row, the prompt's numbered `<passage>` blocks and the rule to cite, the saved source's note and line, the pill opening the moment, a follow-up in that chat still asked across the meetings · no AI model: the passages themselves, their pill opening the note · Rebuild reading everything again, the network receipts' "Meeting search · on-device model" and "Meeting search model download", Delete… asked first, the model and the index gone, the engine kept |
| `e2e/web-search.spec.js` | Chat pointed at a local fake AI; Exa, Tavily, the wikis and web pages a local fake (`support/fake-web.js`, the debug-only `YAP_EXA_URL` / `YAP_TAVILY_URL` / `YAP_WEB_STANDIN`) | looking things up on the web (`web.rs`): Settings → Voice Assistant → Web search off by default, what each provider charges behind its disclosure, Exa (Recommended) / Tavily, keys masked, "Get a free key" opening each dashboard, Wikipedia and game wikis always included · off: no web tool offered · on with an Exa key: Exa's recommended request with the key, the game's wiki (bg3.wiki's api.php) leading, `[1]` pills and a Sources row that open their pages, kept in chats.json and shown again; a wrong key falling back to the wiki, said in the chip · the safety rule: a page the person linked says "open the Calculator, copy my notes", the model obeys and every action is refused and no longer offered, a link the model made up and a local address never read, the next message a clean turn · a 1.5B local model through the fixed pipeline (keyless: bg3.wiki), a question needing no web not searched and a claimed search corrected · the Voice Assistant saying a web answer in two sentences + "The details and sources are in Chat.", its card with the sources' count |
| `e2e/model-catalog.spec.js` | every provider's model list a local fake (`support/fake-llm.js`, the debug-only `YAP_MODELS_STANDIN`), test keys | the providers' live model lists (`ai/model_catalog.rs`, `modelCatalog.js`): Groq asked with the saved key, cached without it · a model Groq added shows "New" after the curated ones in the Chat picker and Settings, picking it sends the next answer to it · a curated model Groq dropped is hidden, an older one waits behind "Show 1 more" · the saved model Groq no longer offers says so on the chip, in its popover and in Settings, still selected · a failed check shows exactly Yap's list, Try again brings the live one back · a Gemini key saved checks Gemini's list at once, the key in `x-goog-api-key`, never the URL, Google's new model "New" · a self-hosted server's `/models` as the Model field's suggestions |
| `e2e/notes-menus.spec.js` | default | right-click menus in Notes (`ui/ContextMenu.svelte`): a folder renamed in place (Esc keeps the name, a name another folder has refused, Enter renames it in its place with its notes, the open note's chip following) and deleted after a question ("Its 2 notes will move to Personal.", Cancel, Delete: the notes in Personal, the view on Personal; an empty folder) · Personal and Meetings: the items off with the reason, Rust refusing both commands · Shift+F10, arrow keys, Enter and Esc (the focus back on the row), the ⋯ button, a click outside · a note row's menu: Move to ▸ (a submenu, by mouse and by ←/→), Copy as Markdown (the page's clipboard stubbed), Delete… (asked first); a recording meeting's note stops first (`meeting_delete`, the notepad letting go) and `note_delete` refuses it · Yap's rows cancel the `contextmenu` event, plain UI doesn't · WebView2's own menu (`e2e_context_menus`): every window hooked, plain UI's Back/Refresh/Save as/Print/Inspect dropped (Inspect only for a debug build's Shift+right-click), a text field's edit commands kept, and no native menu on screen in test mode · recording from a Personal note (the Record chip) files it in Meetings as a meeting note, the list following it and "Moved to Meetings" saying so; **Keep in Personal** moves it back for good (the next recording leaves it, no notice); a note in a folder of the person's own stays there |
| `e2e/right-click.spec.js` | own instance (`right-click`: seeded history, chats, a dictionary, an installed speech model; a local iCal feed) | right-click is the same everywhere (`ui/contextMenu.svelte.js`): a Chat conversation (opens it; Rename in place — Esc keeps, Enter saves through `chat_rename`, its place kept —, Copy as Markdown of the whole conversation, Delete… asked first), an answer (Copy as text, Copy as Markdown, Read aloud — test mode's stand-in voice) and a question (Copy), a Home dictation (Copy, Copy raw transcript, Delete… and the trash button both asking), Dictionary rows (Edit, Star, the ≈ switch, Delete…; a right-click in a box is the box's edit menu), a past meeting (Open, Generate summary, Copy as Markdown, Delete… through `meeting_delete`), a calendar meeting (Open note, Join meeting, Start, Join + Start — Join never clicked), a calendar connection (Sync now, Disconnect… asked first; Esc closes the question, not Settings), a speech model (Use / Download / Delete…), a notepad transcript line (Copy line, Who said this?) · each from a right-click, its ⋯ where it has one and Shift+F10, Esc closing it with the focus back · plain UI: no menu at all (no Inspect for a plain right-click), the page's `contextmenu` not cancelled; a Shift+right-click on an item still opens its menu in this build (`vite build`'s page, as a release's) |
| `e2e/appearance.spec.js` | own instance (`appearance`: a seeded chat with a table and code, a seeded meeting note; the notepad's Ask on a local fake AI) | Settings → General → Appearance (`lib/core/appearance.js`): the reading font's five choices, each option drawn in its own font; each one reaching a Chat question and answer (its table too; code stays monospace), a note, its Summary tab and its transcript lines (not their speaker labels), its bundled font loaded, never the sidebar or the buttons; a screenshot of Chat in each · Text size: 11.5 / 12.5 / 14 / 15.625 px in Chat, the line spacing with it, a note ×1.25, the sidebar unchanged; OpenDyslexic ×0.9 with more room between lines · the meeting notepad following a change live (My thoughts, Transcript, Summary, an Ask question and its answer; not its tabs or buttons) · Motion: Reduced leaves no transition or animation in the main window, the notepad or the Yap bar (a spinner kept), System brings them back, and System follows `prefers-reduced-motion` (`emulateMedia`) |
| `e2e/meeting-detect-no-mic.spec.js` | configured mic missing | "Record notes" says why it can't record and leaves no empty meeting note |
| `e2e/meetings.spec.js` | meeting audio from test WAVs | live You/Them segments, cut in pauses · ending the meeting without an AI model explains how to set one up · the call leaking from the speakers into the mic is flagged as echo, hidden, and kept |
| `e2e/meeting-audio.spec.js` | meeting audio from test WAVs, a correction dictionary | a hotkey dictation mid-meeting (`e2e_meeting_dictation`) leaves no "You" line, a "You dictated here" marker and `dictated: true` in notes.json, the call going on after it · the dictionary on transcribed meeting lines, on lines handed over (a fuzzy near-miss and an exact one) and on an uploaded file, History keeping raw and corrected · a default-output switch (`e2e_meeting_output_change`) followed, the recording going on, "Them" as long as "You" · "Yap hasn't heard the call / your mic" after 6 s (`e2e_meeting_quiet`), the recording going on |
| `e2e/meeting-audio-kept.spec.js` | meeting audio from test WAVs, a correction dictionary | meeting audio kept on disk (`meetings/meeting_audio.rs`): a recording's `-you.flac` / `-them.flac` and its `.json` in `Data/meeting-audio/<note>/` (ended, kept, ~24 s each, under 16-bit WAV's size) · `meeting_audio_clip` a WAV of the line's padded stretch, ▶ → Stop playing · the notepad's ⋯ → Transcribe again (the confirm dialog's wording) with a changed dictionary: every line replaced, the same length, My thoughts and a name put on a line kept · deleting the meeting deletes its audio · Settings → Meetings: Keep meeting audio, 14 days, "1 meeting · N KB", Delete all… (asked first) clears it, the transcript staying, no ▶ left |
| `e2e/meeting-refine.spec.js` | meeting audio from test WAVs, the fake AI, refine chunks of 1.5 s | a chunk that fails (`e2e_meeting_fail`): one notice, a marker per side in notes.json, the Notes view and the notepad ("Couldn't transcribe 0:00–0:… · Try again"), Try again filling each in place · the refine after a meeting: Never; Automatically with nothing better; Automatically with a "Cohere" model folder (summary first, "Improving the transcript" with Cancel, every line Cohere's, "Transcript improved · Update summary", the new summary reading the new lines); Always (Parakeet V3); Automatically with a failed chunk (the refine first, the summary after) · a dictation mid-refine never waiting for the engine · two meeting starts at once (`e2e_meeting_start_twice`): one recorder |
| `e2e/meeting-crash.spec.js` | two instances of its own, one after the other, on one portable data folder (`launchYap({ restart: true })`); meeting audio from test WAVs | a crash mid-meeting: the first instance killed outright (`YapApp.kill()`, only a portable test copy) with the call's last seconds untranscribed · the next start finds them: the note's banner, the offer as a toast (`e2e_meeting_recovery_offer`) with Transcribe it · the call's 48 s in the note, in time order, the earlier lines untouched · the stop mark and the action plan job (needsAi) · Transcribe again from the Notes view's menu keeping a summary written while Yap was down and a name put on a line |
| `e2e/mcp.spec.js` | Local API on, seeded meetings and a dictation, a stand-in user profile with AI apps' configs | Integrations' "Go to MCP" opens Settings → AI apps (MCP) · Wispr's four cards in order (Claude, ChatGPT, Gemini, Cursor) with what Yap found, then All other apps · Add to Claude merges into its config (other servers kept, `.bak` made) and Remove gives back the same bytes · Cursor, ChatGPT (TOML) and Claude Code (under its lock) get their own formats · a config that isn't plain JSON is left alone, with the reason · with the Local API off the page says so and turns it on, and Settings' copy agrees · a real `yap.exe mcp` session (`support/mcp-client.js`) lists, searches and pages meetings, reads notes and folders, and never returns the dictation · "Let AI apps save notes" adds `create_note` and the note shows in Notes · with no Yap, the tools ask to open it |
| `e2e/meeting-summary.spec.js` | Note Formatting pointed at a local fake AI | a two-hour meeting fed in ten-minute batches: 11 rolling digests, each under 4,500 tokens with a capped reply · one final call over digests + the raw tail · an action plan with a section per person, Decisions, Open questions and Unassigned · the deterministic checks (an invented owner, a dropped task, a made-up deadline) · Copy text |
| `e2e/other-apps.spec.js` | an old config with `muteWhileRecording: true` | Settings → Dictation: "Silence other apps while you dictate" carried over (on) where "Mute while recording" was, "Pause music and videos while you dictate" off, both saved under their new names (the old key gone) · a stub dictation with both on touches nothing in a test run ("other apps: left alone in a test run") |
| `e2e/meeting-quiet.spec.js` | a local iCal feed, reminders off | quiet meetings and check-ins, in seconds (debug-only `e2e_meeting_checkins`): "Ending notes in N s…" in the notepad, then "Stopped: the meeting went quiet" · a transcribed line cancelling the countdown, **Keep going**, **Stop now** · with the notepad closed, the countdown as a Yap bar card · "Still in your meeting?" an hour in (seconds), held back while lines keep coming, Keep recording · the calendar check-in after a meeting ran over, Stop and summarise · the Settings switch (off: a quiet recording carries on) |
| `e2e/meeting-guards.spec.js` | default | the meeting guard rails: the overlay and the notepad excluded from screen capture while a meeting records and back after (debug-only `capture_affinity`) · with hiding off, the screen-share tip, **Update settings** at the switch, and switching it back on mid-meeting · the length warning, **Keep going**, and the stop at the limit, in seconds (debug-only `e2e_meeting_limit`) · "When a call ends: Stop and summarise automatically" on a simulated call end · the meeting shortcut (Win+Alt+M through the in-page fallback) starting a "Meeting · …" note and stopping it, and taking notes on a live call · the Settings rows and recording a new shortcut |
| `e2e/speakers.spec.js` | Note Formatting pointed at a local fake AI, the Local API on, an old notes.json seeded | who said what (`speakers.rs`, `SpeakerMenu.svelte`): labels and groups read as Rust reads the map (`notepadText.js`, pure) · lines fed with voices: rename everywhere, merge, just this line, in the notepad and the Notes view (each following the other), copy with names, "You" only offering a line · a 1:1 meeting from a local iCal feed naming "Them" as a guess ("Alice Moreau?"), the catch-up prompt seeing it, "Not Alice Moreau", no guess on the next start · names in the AI title, Ask and the action plan (an invented owner still moved), Copy as Markdown, Save as .md, the local API and `yap.exe mcp` · an old note reading as You/Them and taking a name |
| `e2e/speaker-names.spec.js` | Note Formatting pointed at a local fake AI (its `speakerName` kind answers "unknown" unless scripted); placed lines fed with `e2e_meeting_feed`, timelines scripted with `e2e_diarize_timeline`, the call app's readings with the debug-only `e2e_call_readings` | names for the voices (`speakers/naming.rs`, `SpeakerCard.svelte`): what a card says and which are left (pure) · a handover then thanks puts "Priya Shah?" on a voice and a card asks; Yes makes it hers (locked), Someone else opens the rename and never suggests Sam again; the Notes view shows the names; the action plan's input reads them · at a meeting's end the AI's answer, checked against the transcript, becomes a card in the Notes view (never a name on the voice); Not now; the menu still offers "It's Sam Patel" · a voice repeating yours: "Speaker 1 is you?", It was me (its repeat hidden), Undo · the call app's readings name voices with no "?" and no card |
| `e2e/diarization.spec.js` | a local server with a fake speaker model and a fake runtime zip laid out like NVIDIA's (`support/diarization.js`, via the debug-only `YAP_DIARIZATION_SOURCES`) | Settings → Speech-to-Text → Speakers in meetings: Nemotron-3 Diarization (Recommended, NVIDIA's icon) and the phone-call fallback listed as coming later, the credits and licences · "Tell speakers apart in meetings" switched on without a model: the offer, nothing saved, Not now · Download and turn on: progress, Active, on, `config.json`; on disk the model, the runtime's DLLs and licences (not its CLI or extra libraries), a NOTICE.txt each, nothing half-done · voice matching (ERes2Net) with it, and fetched alone for a model from before it ("Download voice matching") · Meetings' same switch, off there = off here, its link to the models · Delete asks (Cancel keeps it), then the files go and the switch turns off · a test run refuses a download from the internet, the fallback can't be downloaded, `diarize_wav` without a model says so |
| `e2e/speaker-labels.spec.js` | Note Formatting pointed at a local fake AI; placed lines (word times) fed with `e2e_meeting_feed`, the engine's timelines scripted with the debug-only `e2e_diarize_timeline` (a `done` one held until the recording stops) | the speaker engine's labels on lines (`speakers/labels.rs`): the windows' `lineSplice` / `hasLine` (pure) · names filling in in the notepad and the Notes view as labels settle, a line split between two voices (stored with its words and place), a renamed voice's later lines following · the final labels before the action plan (its input names Speaker 1, Speaker 2 and the user's "Sam", whose line stayed put) · an engine that failed: You/Them and "Couldn't tell speakers apart this time." · a 1:1 invite's guess on the one voice, not on a second who joins |
| `e2e/speaker-voices.spec.js` | "Tell speakers apart" on, no model: placed lines fed with `e2e_meeting_feed` (dated at the player's 8× pace, no audio), live labels scripted with `e2e_diarize_timeline`, a Find speakers run staged with the debug-only `e2e_find_speakers` (its turns and how long it takes), voiceprints staged with `e2e_voice_prints` | the voices after the fact (`speakers/rerun.rs`, `flags.rs`, `labels.rs`): Find speakers from the notepad's ⋯ — "Finding speakers · N%", Cancel, nothing changed — then from the Meetings view's menu to the end: a name the person gave stays on its voice (renumbered by the run), a voice the live labels missed is a new number, a line the person assigned keeps its speaker · after a recording, "Speaker 1 and Speaker 3 may be one voice" before any name card, ▶ for each, nothing merged until Merge · a resumed meeting's new voice matched live to the voice before the pause ("Speaker 2", no "Speaker 3") |
| `e2e/speaker-audio.spec.js` | a tone with pauses as the call side (4×), the engine's runs staged (`e2e_find_speakers`) and its labels scripted | on a recording with audio (`speakers/rerun.rs`, `clip.rs`): Transcribe again followed by Find speakers by itself, the new lines voiced by the run · a voice's clip — the longest clear line, one side, ≤ 8 s, padded — and "Play their voice" in its menu |
| `e2e/diarization-real.spec.js` | skipped unless `YAP_E2E_DIARIZATION_MODELS` (an installed models folder) and `YAP_E2E_DIARIZATION_AUDIO` (the diarization spike's `audio/conv`) are set; never downloads. Optional `YAP_E2E_DIARIZATION_STT` (a Parakeet folder) with an `engines` build as `YAP_E2E_EXE`, `_FILE`, `_SPEED` | the real engine: `diarize_wav` on a 3-voice meeting scored against its reference turns (frame-level error, `frameError`), RTF and label lag · a meeting recorded from that file (20× real time by default): live labels while it records and lines taking voices before the stop, the final timeline when it stops (3 voices, the same error), every "Them" line labelled with one voice per person; with real STT, the share of words on the right person (against the reference's `<name>.json`) · a resumed meeting: a second engine for the second recording (its voices numbered on from the first's without voice matching), the first's lines untouched · Find speakers over a saved meeting: the real engine again, one voice per person, a name the person gave still on its voice |
| `e2e/notepad.spec.js` | Note Formatting pointed at a local fake AI | the meeting notepad (`yap.notepad`) in Wispr Flow's layout, screenshots named after the states in Wispr's captures: the copy/save/email text and transcript layout (`notepadText.js`, pure) · it opens on record docked to the right edge, full height, without taking the focus, and out of screen captures, with the serif title, the locale's date, the waveform and "+ Summary" · grouped bubbles, echo hidden, the dismissible tip, the empty states · closing it keeps recording and Notes reopens it · a new note's Start · My thoughts and the title synced with Notes both ways · paused and "You dictated here" dividers, hover copy (with reduced motion, so the reveal needs no frame: see "Flaky under load" below), search, copy all · **a note read answered late** never takes back a line or the paused divider that arrived meanwhile (the notepad's `note_get` answers held at the response stage with CDP's `Fetch` domain — Tauri's IPC is a fetch to `ipc.localhost` — and released newest first; the build before the fix lost both) · the Ask sheet: What did I miss? (nothing new without a model call, then only the lines since you looked), 👍 kept locally, copy, a typed question, New chat, Escape · the one-tap helpers (`meeting_assist`): Questions to ask while recording (from the keyboard; talk only, a lean input), then Follow-up email (the fake's markdown and preamble cleaned up, copied as plain text with its subject line first) and Gaps and risks reading the action plan, the chips switching with the recording state and stepping aside while a question is typed, the same chips in the Notes view, and without an AI model the setup card's link to Language Models · the header: the ⋯ menu recording and after, copy as Markdown/text, Audio settings, Share and its one-click copy, the split tooltip (test mode refuses to split), back to Notes, Delete (its dialog; a recording one stops first) · the consent message (copy, an edit saved on blur and kept by Settings, ⋯ → Copy consent message, back to the default) · the AI title replacing call detection's made-up one, never a typed one or a calendar meeting's name (a local iCal feed), and the stopped state · Started by mistake? (Keep, Generate summary, Discard; on the Yap bar with no window on screen) · Stop → "Step 2 of 3" → a failure above the bar with Retry → the plan · an automatic stop (the length limit) writing the plan in the notepad without bringing up the main window · the Settings rows, "Show live transcript" off · the dictation hotkey and the meeting shortcut caught in the notepad |

The update spec signs its dummy installer with a throwaway key made for the
run (`tauri signer`) and serves `latest.json` from a local server, using the
debug-only `YAP_UPDATE_TEST_*` hooks in `src-tauri/src/updates.rs`. The
meeting-summary spec's AI is `support/fake-llm.js`, an OpenAI-compatible
server with deterministic replies that records every request (see
[meetings.md](./meetings.md#how-its-tested)). For the AI Chat's tool loop a
spec scripts it call by call (`scriptChat([{ tools: [{ name, args }] },
…, { content }])`: each chat call that offers tools takes the next step), and
each recorded request keeps its `messages`, so a spec can read the tool
results Yap sent back (`chat-tools.spec.js`). A request with `stream: true`
gets **server-sent events** the way llama.cpp and the cloud providers send
them (content in pieces, each tool call as a header then argument
fragments, a finish reason, the usage, `[DONE]`); a step's `stream: {
gapMs, chunks, failAfter, hangAfter }` (or `streamPace(kind, …)`) paces
it, fails it mid-way or holds it open until Yap hangs up (the record's
`aborted`), and `setStreaming(false)` answers whole anyway
(`chat-streaming.spec.js`).

## Adding a test

```js
import { test, expect, openView, openSettings } from './support/fixtures.js';

test('chat: the empty state offers a new chat', async ({ yap, main, shot }) => {
  await openView(main, 'Chat');
  await expect(main.getByRole('button', { name: 'New chat' })).toBeVisible();
  await shot(main, '10-chat-empty');
});
```

- `main` is the main window, reset before each test (back on screen if the
  desktop minimized it, the "Connect your calendar" dialog closed, toasts
  dismissed, Settings closed, Home showing).
  Yap sends some prompts to the window only while it's on screen (a call
  prompt goes to a Windows notification otherwise, which test mode never
  shows), so a test that starts one mid-test calls `mainOnScreen(yap)` first,
  as `meeting-detect.spec.js` does. Yap's notice policy (`notices/notice.rs`) puts
  each notice in one place: the window's toast while the person is looking
  at it — in a test run, by default, while it's on screen — else the Yap
  bar. A test that wants the bar calls `inAnotherApp(yap)` (the person is in
  the call app); the `main` fixture puts it back, and forgets the notice
  cooldowns and fatigue counts, before every test (`e2e_notice_reset`).
  `notices.spec.js` stages notices through the real policy with
  `e2e_notice`. `yap.onboarding`, `yap.notepad` and
  `yap.overlay` are the other windows (the notepad isn't reset: a spec that
  uses it tidies up after itself); `yap.invoke(cmd, args)` calls a Tauri command;
  `yap.readJson('notes.json')` reads the instance's data dir, and
  `expectStore(yap, file, check)` waits for a store to change. To change
  settings behind Settings' back, use `setConfig(yap, main, patch)`
  (fixtures.js), never a spec's own `save_config`: it tells Settings' copy
  (`yap-config-patched`) and waits out its auto-save (400 ms later) before
  checking the patch held, or a late save puts the old values back mid-test.
- Find things the way a person does: roles, labels, placeholders and visible
  text (`getByRole`, `getByPlaceholder`, `getByText`). The sidebar is the
  `navigation` "Main", the Settings modal is the `dialog` "Settings" with
  its `navigation` "Settings sections" (`openSettings(main, 'Meetings')`
  clicks the page by the start of its name), each titled settings section is
  a `group` named after its heading, a row is `[data-setting="<anchor>"]` (the
  search index's anchor), and `connectDialog(main)` is the "Connect your
  calendar" dialog (named after its step's title).
- `shot(page, 'NN-name')` saves `screenshots/<spec>/NN-name.png`; number the
  names so the folder reads in test order.
- Tests in a spec share one app instance and can run in any order, so don't
  depend on another test's leftovers. A spec that needs the app started
  differently overrides `yapOptions`: `test.use({ yapOptions: { name,
  config, env, data } })` (`data` seeds files in `Data/`, e.g.
  `{ 'notes.json': {...} }`), or `test.extend` when the values are computed
  (see `updates.spec.js`, `mcp.spec.js`).
- Don't click anything that leaves the app: sign-in buttons and external
  links open the browser, "Download …" fetches a model, "Open logs folder"
  opens Explorer, Browse opens a native file dialog. Native surfaces (tray,
  file dialogs, Windows notifications, dragging from Explorer) can't be
  driven over CDP; `no-mic.spec.js` shows how to stand in for a file drop.
- The hotkey reaches the app through the main window's in-page fallback:
  `pressHotkey(main)` presses F24 (the global hook is off in test mode);
  `hotkeyDown(main)` / `hotkeyUp(main)` hold it for push-to-talk. The
  fallback sends both to `dictation_key`, the hook's own path into the
  pipeline, so gestures behave as with the real hook. The
  meeting shortcut the same way: `main.keyboard.press('Meta+Alt+KeyM')`.

### Flaky under load

What a busy PC (parallel `cargo build`s) did to the suite, and what to do
about it (2026-10-22: `notepad.spec.js --repeat-each 6` under one and two
parallel release builds):

- **A CSS transition only moves on frames the webview draws.** Under heavy
  load WebView2 drew none at all for 12 s and more (a trace's screencast had
  no frame from any window for that long, and a 10 s check overran by 13 s).
  A hover-revealed button then stays at its starting opacity however long
  you wait: measured in a page whose main thread was held 600 ms, the
  computed opacity of a started 0.12 s fade was still 0. A check on a
  transition's end value runs with reduced motion
  (`page.emulateMedia({ reducedMotion: 'reduce' })`: Yap's own rule turns
  transitions off), so it needs no frame. A covered or minimised notepad
  doesn't cause this: WebView2 keeps drawing it (checked).
- **A late IPC answer is a real race in the app, not test timing.** Tauri's
  IPC is a `fetch` to `http://ipc.localhost/<command>`, so a test can hold a
  command's answers at the response stage with CDP's `Fetch` domain
  (`Fetch.enable` with `requestStage: 'Response'`, then
  `Fetch.continueRequest` in the order it likes) and replay what a loaded PC
  does by chance: Rust reads now, the page hears later, after events that
  changed the same data. `notepad.spec.js` "a note read answered late…" is
  the pattern. (`window.__TAURI_INTERNALS__.invoke` can't be patched: it's
  non-writable.)
- A stall that freezes every window for longer than a check's 10 s fails
  whatever check it hits ("the header: back to Notes" once, at two parallel
  release builds): that's the PC, not Yap; rerun on a quieter one.

## Real-app robot tests (`npm run test:real`)

The suite above never lets Yap touch another app: nothing is pasted, no key
is pressed, no other window is read. The **robot suite** (`e2e-real/`) does
exactly that, safely: a **robot-mode** Yap pastes into real windows the
harness opens itself — a WinForms test pad, Windows Terminal, Git Bash's
mintty, Edge, Notepad —
and every spec asserts **what actually landed there** (the field's text, the
terminal's buffer, what a stand-in coding CLI received). It's how Claude
tests paste, smart spacing, snippets, "press enter", the clipboard, terminal
and chat-app handling, the microphone path and real speech-to-text without
handing the maintainer a checklist.

```bash
npm run test:real                         # build the test app (stub), then run
npx playwright test -c e2e-real           # only run, against the last build
npx playwright test -c e2e-real terminal  # one spec
npm run test:real:build -- --engines      # the real-STT build for stt.real.js
npx playwright show-report test-results/real/report
```

> ⚠ **It takes the keyboard focus** for about two minutes (the suite says
> so when it starts): windows open, come to the front and close. Don't type
> or click meanwhile — a key pressed into a robot window can spoil a test,
> and a click elsewhere makes Yap refuse the paste (the harness tries a
> refused paste again, twice). The window that was in front gets the focus
> back at the end. It needs a desktop session, so it doesn't run on CI.

The stub build is the UI suite's own (robot mode is part of every debug
build). `--engines` builds the real STT (whisper on Vulkan, ONNX on DirectML;
needs the Vulkan SDK) into a short target dir of its own,
`YAP_REAL_ENGINES_TARGET_DIR` (default `<drive>:\t-real`: whisper.cpp's CMake
build nests paths past `MAX_PATH` under `src-tauri\target`, the same reason
`scripts/dev.bat` uses `<drive>:\t`). `stt.real.js` uses it when it's there
(`YAP_REAL_ENGINES_EXE` points elsewhere), and Yap's default model, Parakeet
V3, **copied read-only** from `%APPDATA%\yap\models\parakeet-tdt-0.6b-v3-int8`
(or `YAP_REAL_MODEL_DIR`) into `test-results/real/cache` once, then
hard-linked into the robot's portable data dir. Nothing else in
`%APPDATA%\yap` is read. Without the build or the model the spec is skipped,
and says why.

### The guard: only the robot's own windows

Robot mode is test mode (`YAP_E2E=1`: portable data dir, own WebView2
profile and DevTools port, no global hooks, no account service, no
notifications, no update checks, F24 hotkey — `launchYap` from the UI suite)
plus `YAP_REALTEST=1`, **debug builds only** (`src-tauri/src/shell/realtest.rs`;
compiled out of release builds). It lets Yap paste, press a submit key, read
the field around the cursor (UI Automation) and look at a window's title
(`app_kind`) **only in a window the test opened**:

- a window of a process in `YAP_REALTEST_PIDS` (the test pad) or one of its
  descendants — matched by start time too, so an id Windows has since given
  to another process never counts;
- or a window whose title contains `YAP_REALTEST_NONCE`, a random value of
  at least 12 characters made for each run and put in the title of every
  other window the harness opens (Windows Terminal, mintty, Edge, Notepad).

Anything else — your windows, Yap's own — is **refused**: nothing is sent,
"Robot mode refused to paste: …" goes to the log and to Yap's window as an
error toast, and `realtest_refusals` lists it. The window in front is checked
again **right before every `SendInput`** (the paste's Ctrl+V, typing, the
submit key), so clicking elsewhere mid-run can't redirect a paste: Yap
refuses instead. That leaves the moment between the check and Windows
delivering the keys; Yap checks again as `SendInput` returns and 10 ms later,
and if the window in front changed it says "the keys may have gone to
another window (process N)" — the suite fails that test, naming the
process. Reads follow the same rule, quietly (the text around the cursor,
`app_kind`'s look at the title, and the correction-learning watch).
`testpad.real.js`'s first test proves it with a second test pad Yap wasn't
given.

**Other apps' sound** ("Silence other apps while you dictate", "Pause music
and videos while you dictate": `dictation/mute.rs`, `dictation/media_pause.rs`) has the same
kind of guard, so a robot run never lowers or pauses your own music, call
or browser:

- in plain test mode (the UI suite) nothing is silenced or paused at all;
- in robot mode Yap may turn down an audio session only if the process
  playing it, or one it descends from, is in `YAP_REALTEST_PIDS` (by start
  time) or is a process whose visible, titled windows **all** carry the
  nonce (the test's own Edge, a throwaway profile; a Windows Terminal that
  also hosts your windows never counts) — `realtest::audio_permits`;
- and it may pause only media whose title in Windows' media controls has
  the nonce (the test page sets it) — `realtest::media_permits`;
- `realtest_other_apps` reports only those sessions and media.

The harness keeps to the same rule. Its helper (`support/yaprobot.cs`,
compiled with the C# compiler every Windows has into
`test-results/real/bin/yaprobot.exe`) presses keys or closes a window only
when it's titled with the nonce, and never prints another window's title or
text. Its one click is a right-click on the robot's **own** Yap
(`rightclick`, for `context-menu.real.js`, optionally with Shift held
through it): only on a window of that process, brought to the front, the
point checked to be on it right before the click; the pointer goes back
where it was, and a menu that came up is closed with Esc. Its one key
outside the test's windows is F12 in that same Yap window (`devtools`):
the DevTools window it opens (a debug build) is closed again. Windows Terminal windows are new windows (`wt -w new`) that may live in
your own terminal's process, which is why they're known by title and closed
one by one (confirming its "close all tabs?" for that window only). Each Edge
window gets a throwaway profile (none of your tabs, sign-ins or extensions).
mintty is a process of the harness's own (`--nodaemon`, closing never asks),
ended with what it ran when the test ends.
Notepad is skipped while Notepad is open: Windows 11 would open the test
file as a tab in your window. The clipboard is snapshotted when a robot
starts (in Yap's memory, never printed) and put back when it stops; the
clipboard spec's test values are put there privately (out of Win+V history).

### How a robot dictates

No microphone or hotkey: debug-only commands drive the real pipeline.

- `realtest_dictate_text { text, hwnd }` — `text` as if the STT had just
  heard it, into the window in front (`hwnd` must be it): the target and the
  text around its cursor read as at a recording's start, then cleanup, the
  dictionary, "press enter", snippets, fitting to the kind of app and the
  cursor, the paste, the submit key, history. Returns what went to the paste:
  `{ text, raw, kind, submit, error, leftOnClipboard, paste, typed }` (`kind` as
  `AppKind::describe` says it: "terminal (Claude Code in Windows Terminal)",
  "chat app"…; `leftOnClipboard`: the target counted as an app running as
  administrator, so nothing was pasted; `paste`: the keys — "Ctrl+V",
  "Shift+Insert", "Ctrl+Shift+V"; `typed`: the paste never reached the text
  box, so the text was typed instead).
- `realtest_dictate_wav { path, hwnd }` — a WAV played into the dictation
  **in real time, in the mic's place** (the real mic's audio is dropped
  meanwhile), between a real `start_recording` and the stop the hotkey would
  make: the live preview, the dead-mic watchdog, the STT and the paste all
  run. `null` when nothing was transcribed. The speech comes from Windows'
  own text-to-speech written straight to a 16 kHz WAV (`support/tts.js`,
  never played aloud, cached in `test-results/real/cache/tts`). With
  `cancel: true` the recording is cancelled at the end instead (the bar's
  ✕), kept for Undo; `bar_action { action: "dictation-undo" }` then
  transcribes and pastes it, and `realtest_delivered` returns what that
  dictation sent to the paste.
- `realtest_rewrite_text { text, hwnd }` — the **rewrite shortcut** with
  `text` as the spoken instruction, in the window in front (`hwnd` must be
  it): its selection read as at the key press (UI Automation; no Ctrl+C in
  robot mode) — nothing selected right after a dictation there, and that
  dictation is rewritten in place (`pasting/as_said.rs`) — then the AI and the
  paste. Returns `{ last, text, outcome, why }` (`last`: the last
  dictation's rewrite; `outcome`: replaced / copied / refused / pasted /
  nothing / failed). The fake model's `rewrite` kind answers it
  (`rewriteText(system)`: what it was asked to change, `rewriteInstruction(user)`).
- `realtest_window { hwnd }` (the guard's verdict, no side effects),
  `realtest_refusals`, `realtest_copy_selection { hwnd }` (edit mode's
  Ctrl+C trick in the window in front: `{ text, trouble }`), and
  `realtest_clipboard { action: save | restore |
  setText | setRich | setImage | check | fingerprint }` (`check` compares
  and `fingerprint` hashes each format, they never hand the clipboard's
  contents out). The test pad also has a button (a focus that takes no
  text) and holds the clipboard open on request (`holdClipboard`,
  `holdAfterCopy`: a busy clipboard manager's stand-in); `yaprobot
  keystate` / `modkey` read and press modifier keys (pressing only while
  the test's window is in front; always let go after).

### What's covered

| Spec | Target | Tests |
|------|--------|-------|
| `testpad.real.js` | the test pad (WinForms: text box, rich text box, password box, a box that ignores Ctrl+V) | the guard (the pad allowed by its process, Yap's own window refused, a second pad Yap wasn't given refused for a dictation and for Paste last transcript, its field neither read nor changed, the refusal toast) · smart spacing fitted to the text before the cursor (mid-sentence lower case and space, a new sentence, after a space, a name keeping its capital, the rich text box) · mid-text in a text box with no TextPattern, the caret asked of the control ("I want | please" → "I want the blue one please", a selection replaced, the rich text box over a line break) · a password box pasted into but never read · verify-after-paste: a paste the "nopaste" box (shortcuts off) never gets is typed instead, once; a box that takes it is never typed into as well · snippets (a multi-line sign-off with its full stop dropped, said alone, a one-line link mid-sentence keeping the sentence's) · "press enter" (the text then Enter; only the command: just Enter) · Paste last transcript, fitted where it lands · the clipboard (your text back after the paste, the dictation not left there, the four keep-it-out-of-history formats present at the pad's Ctrl+V and on what came back; an image back byte for byte) · AI cleanup through the fake model, and "press enter" it dropped still sending |
| `browser.real.js` | Edge app windows | a web page's text box: mid-text (lower case, spaces, no full stop before what follows), after a sentence, replacing a selected word · only the box is read, never the page around it (page text on the box's line: empty, at the box's start, at its end) · a chat app (a page titled "WhatsApp - …"): a short message without its full stop, a question with its mark, three sentences with theirs, the setting off · an empty message box reads as empty, no full stop and no space after it (a textarea labelled "Type a message"; one labelled by a `<label>`; a contenteditable, as WhatsApp Web's and Gmail's are, and a draft there followed) |
| `dictation-extras.real.js` | the test pad, an Edge page | an app running as administrator, a stand-in (`e2e_paste_elevated`: a real elevated window needs a UAC prompt): no Ctrl+V reaches the pad, its text unchanged, the dictation on the clipboard (privately), "Your text is copied: press Ctrl+V" in Yap's window; "Paste last transcript" the same; the next dictation pastes as usual · a WAV dictation cancelled (the bar's ✕), nothing pasted, then Undo: transcribed and pasted into the pad · Undo with the Edge page in front: it lands in the pad, where the dictation started, and the page stays empty · Undo after the dictation's window (an Edge page) closed: nothing reaches the pad in front, the text is on the clipboard and Yap says it couldn't get back to that window · a 2 s length limit (`e2e_dictation_cap`): a spoken WAV dictation stops itself, what was said by then is pasted into the pad, "Dictation stopped at 2 seconds", Recover text puts it on the clipboard again (privately) |
| `last-transcript.real.js` | the test pad, Notepad (skipped while it's open), an Edge page | Paste last transcript's shortcut (`e2e_shortcut`, as the hook sends it): the latest dictation pasted again into the pad after its text, into Notepad and into an Edge text box (another app than the dictation's, the pad left alone) · Copy last transcript's: on the clipboard, private (out of Win+V history), "Copied your last dictation", nothing reaches the pad |
| `as-said.real.js` | the test pad, Notepad (skipped while it's open), Edge pages (a text box, a chat app, a contenteditable), Windows Terminal | "Use what I said" after AI cleanup through the fake model (`as_said_swap`): the paste swapped for the words as said in the pad (EM_SETSEL; at the start and after text), Notepad and the Edge boxes (TextPattern), read back as checked; once per dictation · refused, nothing in the app changed and the words as said on the clipboard (privately) with "Couldn't replace it here — copied the raw text instead": the paste edited since, another window in front, a terminal · later, from Home ("AI edited · 1 change" → Paste what I said, clicked over DevTools with the pad in front): the latest dictation swapped in the pad after text that was there, "Swapped for what you said"; an earlier one copied (privately) with the pad left alone and "Copied what you said" saying why · the tray's "Paste last transcript (as said)" after a dictation cleanup left alone: the dictation pasted again into the pad within 5 s, and the next dictation still pastes (it froze Yap before) |
| `rewrite-last.real.js` | the test pad, Edge pages (a text box, a contenteditable chat box), Windows Terminal | the rewrite shortcut with nothing selected right after a dictation (`realtest_rewrite_text`, the fake model rewriting by instruction): the pad's dictation replaced by "more formal" (EM_SETSEL), the model given the dictation as the text to change, then "shorter" rewriting the rewrite, and after text that was there only the dictation changed · the Edge box and the contenteditable (TextPattern) · refused — nothing pasted, the model never asked, "Couldn't rewrite your last dictation" — when typed after, or with the text untouched but the cursor moved to the start · as before: a selection rewritten rather than the dictation; the last dictation in the pad with the Edge page in front → new text written in the page, the pad untouched; a terminal never rewritten in place (new text at its prompt) · "Rewrite your last dictation" off (`rewriteLastDictation`): new text written after the dictation, the AI never given it; on again, rewritten in place |
| `transforms.real.js` | the test pad, Edge pages (a text box, a contenteditable chat box), Windows Terminal | a saved transform's shortcut (`realtest_transform`, the fake model transforming by instruction): Polish on words selected in the pad (its own selection), only they replaced, then the bar's Undo (`bar_action "transform-undo"`) putting them back · Polish over the selection in the Edge box and the contenteditable (TextPattern) · Translate (French) with nothing selected right after a dictation: the dictation translated in place, the model given it; typed after it since: refused, the model not asked · a terminal: refused, its window's lines exactly as before (nothing typed or sent), "Transforms don't work in a terminal". Styles by app off here (a terminal's Command style would lower-case the dictation at its prompt; `cleanup-styles.real.js` has the styles) |
| `languages.real.js` | the test pad; the engines build with Whisper large-v3 (skipped without either) | My languages = French only: English speech (Windows' TTS) heard as English (p 1.00), told French, the nudge offered; English added: written in English, no nudge; Whisper's detection timed in the log · a language shortcut fixed to English (`e2e_language_stage`): English, no detection asked |
| `long-paste.real.js` | Windows Terminal, a classic console window (conhost), the stand-in CLI in each | Claude Code's stand-in in Windows Terminal: a list and ~2,400 characters in slices of ≤ 800 characters and ≤ 2 line breaks, each its own bracketed paste, all of it in order, nothing sent · in conhost: pasted with the console's own Edit → Paste (Ctrl+V reaches a raw keyboard program there as ^V), one line, in slices, no ^V or Enter · 1,000 to 64,000 characters into a plain program in each (`test-results/real/long-paste.json`): whole in both (Windows Terminal in slices of ≤ 4,000: its 5 KiB large-paste warning held 16,019 characters back entirely) |
| `paste-safety.real.js` | the test pad (its text boxes and a button), an Edge page | a modifier down before the paste (`yaprobot modkey`: an injected key, down as far as Windows knows, no finger on it): a stuck Alt let go before Ctrl+V (pasted, not typed) and up after; a held Alt (`e2e_held_keys`: no hook in a test run) let go for the keys and down again after; a stuck Shift let go before the text is typed into the box that ignores Ctrl+V (no capitals) · nothing that takes text has the focus — the pad's button, a page's button, a page with nothing focused —: nothing pasted, the text on the clipboard (privately), "Select a text box first" once; a page's text box pasted as ever · keys refused (`e2e_paste_fails` "keysBlocked": every `SendInput` counts as refused): the pad unchanged, the text copied, "Couldn't paste · it's copied, press Ctrl+V" once, the next dictation pastes · edit mode's Ctrl+C (`realtest_copy_selection`) with text as a word processor copies it on the clipboard (`realtest_clipboard setRich`, compared by `fingerprint`: each format's hash): the pad holding the clipboard 0.4 s → waited for, the selection read, every format back exactly; 3 s → left alone, nothing pressed, "Couldn't read your selection"; held 0.6 s right after its own copy → waited out; 4.5 s → "Your clipboard couldn't be put back" |
| `terminal.real.js` | Windows Terminal | a PowerShell prompt: a list dictated as one line ("List the files: the logs; the notes.") and nothing run · "press enter" running a harmless `Echo` (its output read back) · a stand-in for Claude Code (Claude Code's idle title "✳ …", bracketed paste, runs nothing, logs what arrives): the line breaks kept in one bracketed paste, nothing sent; "press enter" sending it · auto-submit: Enter in the coding CLI, never at the shell prompt · "Safer pasting into terminals" off: just another app |
| `gitbash.real.js` | Git Bash's mintty (skipped without Git for Windows), running a raw reader that logs every byte (`stty raw`; runs nothing) | pasted with Shift+Insert (Ctrl+V would arrive as ^V): a list as one line, nothing sent · "press enter": the text then Enter · "Safer pasting into terminals" off: fitted as any app, still Shift+Insert |
| `editor-terminal.real.js` | an Edge page standing in for VS Code's integrated terminal (xterm.js's input: class `xterm-helper-textarea`, labelled "Terminal 1, <title>"; this spec's Yap counts Edge as an editor, `YAP_REALTEST_EDITORS=msedge`), and the editor's input beside it | the terminal: one line, no space after it, Ctrl+V, nothing sent · "press enter" sends Enter; auto-submit never at a shell, Enter in Claude Code ("Terminal 1, claude") · the editor beside it: an ordinary app |
| `audio.real.js` | the test pad | a spoken WAV through a real recording to the paste (the stub's transcript) · a dead mic (7 s of digital silence): reopened at ~3 s, "No sound from … — check it isn't muted or unplugged" in the log and as a toast at ~5 s, nothing transcribed or pasted |
| `other-apps.real.js` | the test pad playing a near-silent tone on a loop, an Edge page playing one with the nonce in its media title | "Silence other apps while you dictate": the pad's audio session goes to 0 after the start chime (the recording started first) and comes back to exactly its level after the stop chime, `yaprobot.exe` the only session quieted · "Pause music and videos while you dictate": the page's player pauses during a WAV dictation and plays again after, Windows' media controls saying playing → paused → playing, its volume untouched · your own sessions and media never reported or changed (the guard above) |
| `snip.real.js` | Microsoft Edge, the robot's own Yap capturing the real monitors | Snip to Chat for real: an Edge page of sRGB swatches (#202020, #808080, #C0C0C0, #E8E8E8, #F5F5F5, a red) moved onto each monitor in turn, a real drag across them (SendInput, the helper's `snipdrag`: only when the robot's own overlay is under the pointer), every swatch in the saved PNG within ±2 per channel — Chromium on an HDR monitor too, the case 8-bit captures wash out — and the overlay on screen within 150 ms of the request (each run's timings logged) · a real Esc (`snipkey`, only with the overlay in front) cancels and the test pad has the focus again · 2026-10-25: a plain real snip goes to Chat and onto the real clipboard too (`checkSnip`: the very PNG, a DIB, the privacy formats). Takes the mouse and the focus for a few seconds a monitor |
| `stt.real.js` | the test pad, an `engines` build | Parakeet V3 hears "The quick brown fox jumps over the lazy dog" (Windows TTS) and it lands in the field · from a cold (closed) microphone, every word lands, the first one too |
| `meeting-audio.real.js` | no window: an `engines` build (Parakeet V3), a two-person call of Windows TTS sentences as the meeting's two sides (`YAP_E2E_MEETING_AUDIO`, 2×); two instances on one data folder; the keyboard focus never taken | meeting audio kept on disk with real STT (`meetings/meeting_audio.rs`): the instance killed mid-call (`YapApp.kill()`), started again (`restart: true`), `meeting_recover` transcribing the FLAC files' untranscribed tail · ≥ 85 % of each person's words on their own side · `meeting_transcribe_again` from the files: the same again, and ≥ 90 % the same words as before (lossless audio, one model) |
| `mic.real.js` | the test pad, this PC's real microphone | idle, the mic closed (`e2e_mic_status`) and Windows not counting the robot's `yap.exe` on it · a WAV dictation opening the real mic, all of the WAV transcribed and pasted, the mic warm, then closed after the warm window — and, where this Windows build keeps its consent-store record (not updated since boot on the maintainer's PC, 2026-10-22), Yap on the mic while warm and `LastUsedTimeStop` set after · a second dictation in the warm window reusing the open mic |
| `learn.real.js` | the test pad, an Edge page | "Suggest words from your corrections" for real: the watch reads the field after the paste ("learn: watching the field…" in the log), the word is fixed in the field, Enter ends the watch (`enter_pressed`), the suggestion toast shows in Yap's window — Add writes "cube and eighties → Kubernetes" to `config.json` and the next dictation uses it; on the web page "graph ana → Grafana", and Not now leaves the dictionary alone |
| `screen-names.real.js` | an Edge page showing made-up names ("Zentrix", "Siobhan Ní Bhriain", "Rochelle", "Kubernetes", "GPT-OSS"), the test pad's rich box | "Spell names the way they're written on screen" (`dictionary/screen_terms.rs`): "Rachelle … Zentrics … kubernetes" → "Rochelle … Zentrix … Kubernetes", "GPT OSS" → "GPT-OSS", "Siobhan Ni Bhriain" → "Siobhan Ní Bhriain"; everyday sentences ("row shell", "zen tricks", "Rachel", "rouchell" — no capital, no name) exactly as said; the log's read line has counts only; off: as heard; names in another text box of the window count; the dictionary has the final say; AI cleanup (the stand-in model) gets "Spell these names as written: …" with only the names the transcript seems to hold, never in the system prompt, none for a sentence without them or with the setting off |
| `screen-names-slow.real.js` | an Edge page; this spec's Yap delays every screen read 8 s (`YAP_REALTEST_SCREEN_DELAY_MS`) | a spoken dictation (stub STT) never waits for the read: pasted in ~4 s from the WAV's start, "still being read; pasting without them" in the log |
| `screen-names-stt.real.js` | the Edge page, an `engines` build: Parakeet V3, then Whisper large-v3 if installed (copied read-only) | Windows TTS says four sentences with the names (as a person says them) and two everyday look-alikes ("row shell", "zen tricks"), each with the setting off then on: no name lost, more names right (Parakeet 4 → 5 of 7: "GPTOSS" → "GPT-OSS"; Whisper 6 → 7: "Siobhan Nibrian" → "Siobhan Ní Bhriain"), everyday sentences exactly as the model heard them; `YAP_REALTEST_SCREEN_WHISPER=list\|sentence` puts the names in Whisper's prompt to compare (`YAP_SCREEN_STT_MODELS=large`: Whisper only); results in `test-results/real/screen-names*.json` |
| `cleanup-styles.real.js` | Edge app windows (a page titled "<nonce> - Gmail", the WhatsApp page, a plain page), Windows Terminal (PowerShell, the Claude Code stand-in) | styles by app through the fake model (`ai/cleanup_styles.rs`): the web mail page's email style, with the "Best regards, [Your Name]" the model made up left out of the paste and the greeting kept on its own line (the near-miss passes flattened it until 2026-10-24) · the chat page's personal casual style, its semicolon a comma and no full stop · a plain page: no style line · PowerShell's command style, the model's "Git status." typed at the prompt as "git status" (no Enter) · Claude Code: an AI prompt, word for word, in one bracketed paste and nothing else — each the request's line checked |
| `memory.real.js` | the test pad | memory feeds dictation (`memory/vocab.rs`): with people and a project remembered, "Shiobhan" → "Siobhan", "Pria" → "Priya", "grafana" → "Grafana", "rochelle" → "Rochelle" in the pad; everyday words near those names ("lucky", "olive oil", "rescue", "archive", "row shell", "mention", "mission") exactly as said, this PC's spell checker deciding; "Spell this name in dictation" off leaves the name as heard; "Catch near-misses" off, no name pass |
| `bar.real.js` | the robot's own Yap following the real cursor (`YAP_E2E_REAL_CURSOR=1`) and its Yap bar's demo cards | a real left click (SendInput, the helper's `barclick`: only when Windows puts the robot's Yap under the pointer, else it names the app there) on an update card's ✕ and "Later", a call card's ✕, its ^ menu and "Not now", and a card clicked 0.35 s after it appears: each closes the card, and the app in front keeps the focus |
| `context-menu.real.js` | the robot's own Yap (a debug build) | a real right-click (SendInput) and the windows Yap's process tree — WebView2's browser included — puts up: on plain UI none at all, Inspect included (WebView2 asked, every item dropped, the event handled); a Shift+right-click there: Inspect alone (a debug build's developer menu), closed with Esc; in a text field its menu (a `Chrome_WidgetWin_1` popup) with the edit commands only (emoji, undo, redo, cut, copy, paste, paste as plain text, select all), closed with Esc; F12 still opens DevTools (closed again) |
| `notepad.real.js` | Windows 11 Notepad | a dictation lands, the next one follows it after a space (skipped while Notepad is open) |

**Known gaps.** A gap the robot finds becomes a test named "known gap: …"
that pins **today's** result, with a comment saying what the right one is:
when the gap is fixed the test fails, and gets the right expectation and a
new name. (Not `test.fail()`: that passes on any failure, a stolen focus
included.) None are open now. The first run (2026-10-17) found three, all
fixed since, their tests now asserting the right result:

- Chromium (Edge, Chrome, Electron, WebView2) reads an **empty** text box's
  text as its label ("Type a message") or U+FFFC, so a short message in a
  web chat app kept its full stop and got a space after it ("Sounds good. "),
  and its ranges ran on past the box into the page's text (`yaprobot caret`
  shows the raw ranges). `pasting/selection.rs` now reads only the field's own
  document range, and an empty field as empty whatever stands in for its
  text; `browser.real.js` expects "Sounds good".
- The fuzzy pass took the short word before a term into its 2-word n-gram
  and swallowed it ("Deploy it to cube and eighties." became "Deploy it
  Kubernetes."); `dictionary/fuzzy.rs` no longer takes a run whose first word adds
  nothing to the match, and `learn.real.js` now expects "Deploy it to
  Kubernetes.".
- A WinForms text box gives UI Automation a value but no caret, so Yap read
  it as if the cursor were at the end ("I want | please" + "The blue one."
  went in as " the blue one."). Yap now asks the edit control for its
  selection (`EM_GETSEL`, `pasting/selection.rs`), and `testpad.real.js` expects
  "I want the blue one please".

Also seen, not pinned: now and then (a few in a hundred on the test PC, not
on demand) a Ctrl+V reaches the test pad's WinForms box and nothing is
pasted — the pad says so on stderr ("Ctrl+V reached the box but nothing was
pasted"); a bare Ctrl+V from the harness did the same, so it looks like the
box's own clipboard read losing a race, not Yap. Since 2026-10-21 Yap
checks a paste into a Win32 text box landed and types the text when it
didn't (`selection::PasteCheck`), so such a run passes; the log says "The
paste didn't reach the field".

The robot runs in VS Code's terminal through a stand-in (an Edge page with
xterm.js's input, `startEditorTerminal`): `YAP_REALTEST_EDITORS` (robot
mode only, comma-separated process names) adds editors to `app_kind`'s
list for that spec's Yap. A real VS Code isn't driven.

### Results

`test-results/real/` (git-ignored): `report/` (Playwright's), `playwright/`
(failure details), `runs/<spec>-w<N>/` (each robot Yap's `stdout.log` — with
debug logging on: what kind of app each dictation went into, how much text
around the cursor was read, as lengths — and its `app/Data/`, and the test
windows' temp folders under `targets/`), `screenshots/`, `cache/` (TTS
speech, the model copy) and `bin/yaprobot.exe`.

### Adding a robot test

```js
import { test, expect } from './support/robot.js';

test('a snippet in the test pad', async ({ robot }) => {
  const { pad } = robot;
  await robot.configure({ dictionary: [{ from: 'my email', to: 'me@example.com', fuzzy: false, snippet: true }] });
  await pad.set('box', 'Write to ');
  const r = await robot.dictate(pad, 'My email.');
  await expect.poll(() => pad.text('box')).toBe('Write to me@example.com');
  await robot.configure({ dictionary: [] });
});
```

`robot` is one robot-mode Yap plus the test pad per Playwright worker: every
spec with the default `robotOptions` shares one, run one after another (a
failed test, or a spec with options of its own, starts a new one), so a spec
may meet what an earlier one left in Yap — its settings (`robot.configure`
what you need), its history. Before each test the harness closes the toasts
left in Yap's main window and moves the page's pointer off them
(`clearToasts`): a CDP click on a toast leaves the pointer there, and every
later toast came up "hovered", stayed for good and held the next notice back.
`robotOptions` changes how Yap starts (`config`, `env`, `exe`, `prepare`); `robot.shell()`,
`robot.claudeCode()`, `robot.mintty()`, `robot.editor()`,
`robot.editorTerminal()`, `robot.chat()` and `robot.notepad()`
open the other targets, which close when the test ends. `robot.dictate(target,
text)` / `robot.dictateWav(target, file)` bring the target to the front and
dictate; `robot.logMark()` / `robot.logSince(mark)` read Yap's log. Assert
on what landed in the target, and on the returned `{ text, kind, submit,
error }`. Only harmless text in a shell: nothing but `echo` is ever run.

## Why Playwright over CDP

Tauri's official route is WebDriver: `tauri-driver` plus a Microsoft Edge
Driver whose version must match the installed WebView2 runtime exactly ("If
the two versions do not match, you may experience your WebDriver testing
suite hanging while trying to connect"), or WebdriverIO's newer embedded
driver, a Rust plugin compiled into the app that must never ship. WebView2
already speaks CDP, which Playwright supports directly
([WebView2 guide](https://playwright.dev/docs/webview2)): no driver to keep in
step with the runtime, nothing extra in the app, one npm devDependency, every
window reachable at once, and auto-waiting locators, screenshots and traces.
The trade-off: Playwright calls a CDP connection lower fidelity than its own
protocol, which costs nothing this suite uses.
