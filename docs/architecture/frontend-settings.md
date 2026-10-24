# Frontend: Settings

`lib/Settings.svelte` and its pages in `lib/settings/`, page by page. Account's
own page is [accounts.md](./accounts.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## The shell (2026-10-22 review: structure, search, one row style)

Settings is a modal in the main window (`ControlPanel.svelte`: `embedded`,
`open`, `onclose`, `onshowview`), **always mounted** so its in-page hotkey
fallback (the dictation, assistant, meeting, read-aloud and snip keys while a Yap window has the
focus) and its auto-save run for the window's whole life. Esc closes it (a
search being typed is cleared first); the ✕ is 32 px in `--yap-fg-62`.

- **Its copy of the config, in step with config.json** (2026-10-23).
  Settings reads the config once, at mount, and auto-saves 400 ms after any
  change. Other surfaces save config.json too — the Speech-to-Text page's
  model switch (`modelStore.setActiveModel` → `set_active_model`), the
  tray's model menu, onboarding, a notice's button — and the auto-save used
  to write the whole stale copy back: the next switch flipped anywhere in
  Settings put the old speech model (and onboarding's choices) back. Now
  `base` is config.json as this copy last agreed with it (read, or saved),
  and `settings/configMerge.js`'s `merge3(base, mine, theirs)` (JSON
  three-way: what only one side changed comes from it; objects merge key by
  key; a setting changed on both sides keeps this copy's — the edit being
  made) runs both ways, one at a time (`inTurn`): on **`yap-config-saved`**
  (Rust's `config::save` sends it after every save) what changed on disk
  and not here is adopted into the copy (`adoptSaved`; a provider adopted
  sets the Language Models tab's mode, scopes remount their tabs); the
  **auto-save** (`persist`) reads config.json, puts this copy's changes onto
  it, adopts the rest, and writes nothing when the copy has nothing to add.
  A setting the person changes while a merge is under way is never
  overwritten (`adopt` skips it). The older one-off adoptions
  (`yap-config-patched`, `yap-chat-model-changed`, `yap-dictionary-learned`,
  `yap-meeting-detect-choice`, …) still run; they and this agree. e2e:
  `speech-models.spec.js` (a model switched on the Speech-to-Text page, then
  as the tray does, survives a switch flipped in Settings).

- **The nav** (`settings/nav.js` `NAV_GROUPS`, Claude-style scopes, sentence
  case): a **search box** on top ("Search settings", Ctrl F); then General ·
  Dictation · Shortcuts · Meetings | **Assistant**: Commands · Voice Assistant
  · Memory | **AI models**: Speech-to-Text ·
  Language Models | **Connections**: Connectors · AI apps (MCP) | **This PC**:
  Privacy & data · About. At the bottom the account button and a footer with
  the version and the "Saved ✓" pulse every auto-save gives (the old status
  bar under the page is gone). The active page is a white pill with the amber
  icon chip; the red attention count stays on its page's row (and on the
  title bar's bell, which names each item).
- **Page ids** (never renamed: links use them): `general`, `dictation`,
  `shortcuts`, `meetings`, `commands`, `voice`, `memory`, `models` (Speech-to-Text),
  `cleanup` (Language Models), `connectors`, `mcp`, `privacy`, `about`,
  `account`.
- **Links** — `yap-settings-goto` (window event), ControlPanel's
  `openSettings(target)` and Rust's `yap-open-settings` all go through
  `goto(target)`: `"page"`, `"page#anchor"` (scrolls to `#settings-<anchor>`,
  else the row `[data-setting=<anchor>]`, and gives a row a short amber wash,
  `.set-flash`), `"cleanup#<scope>"` (a Language Models tab), or the search's
  `{page, anchor, fallback, scope, focus}` (focus = the row's control gets the
  keyboard focus). **Old links still land** (`ALIASES`, OpenWhispr's
  `SECTION_ALIASES`): `general#meetings` → `meetings`,
  `general#screen-sharing` → `meetings#screen-sharing` (Windows notifications
  already in the notification centre carry the old string),
  `general#activation`/`#hotkey` → `dictation#hotkey`, `general#microphone`/
  `#audio` → `dictation#microphone`, `history` → `privacy#history`,
  `advanced` (and `advanced#output`) → `dictation#typing`,
  `advanced#system` → `general#startup`, `advanced#debug` → `privacy#logs`,
  `advanced#dictionary` → `dictation#dictionary`; a page that moved keeps an
  anchor asked for. A page this build doesn't have leaves Settings where it
  is. Callers today: bar.rs `general#bar`,
  meeting_guard.rs `meetings#screen-sharing`, meeting.rs (quiet mic)
  `dictation#microphone`, alert.rs `cleanup#chat` / `voice#voice-provider`,
  updates.rs `about`, auth.rs `account`, the notepad's Audio settings
  `dictation#microphone`, call detection's "Open Settings"
  `meetings#call-apps-list`, Home's history link `privacy#history`, Chat's
  `voice#assistant-shortcut` and `cleanup#chat`, the "Yap remembered…"
  toast's Manage `memory`, Memory's About-you rows `general#about-name` /
  `#region` / `#about-me`.
- **Search** (`settings/settingsIndex.js` + `SearchResults.svelte`): a static
  index of every setting — `{page, section, label, desc, keywords, anchor,
  fallback?, scope?, toggle?}`, ~110 entries — since only the open page is
  mounted. Ctrl+F (or `/` outside a text field) focuses the box; while there's
  a query the page pane shows **Results for "…"**: grouped by page (pages in
  order of their best match; inside one, by section, its heading once:
  "Dictation › Typing into apps"), the words highlighted (`highlight`, every
  form: "paste" also finds "pasting"), a plain on/off setting switchable in
  place (`toggle`), ↑/↓ to move (the box keeps the focus;
  `aria-activedescendant`), **Enter** or a click opens the page scrolled to the
  row with the wash, the row's control focused; Esc clears, a second Esc
  closes. The nav shows each page's count and dims the rest. Matching: every
  word must be found in the label (weighted most), keywords, line, section or
  page name; a word's start beats its middle; light stemming (`stems`).
  `e2e/settings-search.spec.js` opens **every index entry** and finds its row,
  so the index can't drift from the pages.
- **860 × 600** (below 960 px of window): the nav is a **58 px icon rail**
  (28 px items: all 13 pages fit at 600 px without scrolling; a 14th page
  needs that checked again — the search spec measures it) with ink tooltips
  (fixed-position, so the rail can scroll), a search button
  that opens the box over the page, a red dot for the attention count, and
  the account's avatar. Rows put a control wider than ~42% of the row on its
  own line (`ui/Row.svelte` measures it, OpenWhispr's `isCompact`), and long
  choices are Selects, so nothing squeezes to a word a line.
- **One look** (`ui/`): `Group` = a 14 px / 640 ink heading over flat rows
  with hairlines (`--yap-rule`), no card (`bare` for a group holding a card or
  its own list); `Row` = label 13.5 / 560, one grey line 12.5 in
  `--yap-muted`, the control right-aligned in one column, `setting` = its
  search/deep-link anchor, `desc` a string or a snippet; `Toggle` = an
  **ink** switch, off an outlined track (`--yap-switch-edge`, 3.4:1 on the
  page, Windows 11's); `Select` = one width (`--yap-select-w` 188 px, `wide`
  248 px for devices, voices, countries); `Segmented` = segments sized to
  their labels, the thumb measured; `Slider` ink (an `inline` one for a row's
  control column); `HotkeyInput` = the shortcut in Yap's **amber keycaps** +
  Change (+ ✕ when it can be unset), "Set a shortcut" when there's none, the
  clash message under it; `LinkRow` = every jump to another page or list
  ("Dictionary · 12 corrections · 3 snippets ›"). Content is one 700 px column
  (`--yap-settings-w`) on every page; cards only for objects (a speech model,
  a calendar connection, an AI app, Prompt Studio). Amber is kept for keycaps,
  the active page, Recommended/Beta chips, links (`--yap-link` #a85a08; dark
  #f0b04a) and focus. **One focus ring** (`--yap-focus`: 2 px of page, 2 px of
  `--yap-focus-ring`, #c2690a, dark #f0b04a) on `:focus-visible` for every
  control. An on switch is ink with a `--yap-ink-fg` knob (white on ink in
  light, dark on dark mode's cream). Thin scrollbars in Settings
  (`settings/settings.css`, the pages' shared styles under `.settings-shell`:
  page header, `.set-link`, `.set-note`, `.set-banner`, `details.set-more`
  disclosures, `.set-flash`).
- **Contrast, measured** (WCAG AA on `--yap-s1`): descriptions, page lines
  and nav group labels `--yap-muted` 5.1:1 (4.6:1 on the nav), links 4.8:1,
  headings ink 15:1, the ✕ 4.8:1, an off switch's edge 3.4:1, the focus ring
  3.8:1. Before: descriptions 3.1:1, headings 2.4:1, nav labels 2.3:1, links
  3.75:1, an off track 1.2:1. The suite measures them in the app. **In
  dark** (`e2e/theme.spec.js`, the lowest of each on the page): descriptions
  and page lines 6.8:1, labels and headings 15:1, nav group labels 7.2:1,
  nav labels 11:1, links 9.1:1, the ✕ 6.8:1, an off switch's edge 4.9:1, an
  on switch 14:1 (its knob on it 14:1), the focus ring (`#f0b04a`) 9.1:1.
- **The red badge says why** on the page: Speech-to-Text with no model (a
  banner over "Your model", whose own Download is the fix), Language Models
  with cleanup on a cloud provider and no key ("Add your Groq API key" + Add
  key, which opens and focuses the field).
- **Hotkeys** (`settings/hotkeys.svelte.js` + `SettingsHotkey.svelte`): the
  six (`HOTKEY_CMD` / `HOTKEY_NAMES`: dictation hotkey, rewrite shortcut,
  meeting shortcut, assistant shortcut, read-aloud shortcut, Use-what-I-said
  shortcut) are refused when they'd fire on one
  press (`hotkeysClash`: "Already used by the …", or "Clashes with the … (Win
  + Alt + M): one press would start both"); while one is being picked
  (`hotkeyCapture.on`) its live binding is paused and the in-page fallback
  stands down. The fallback catches the meeting, assistant, read-aloud and
  dictation keys, in that order (read aloud: the page's own selection,
  `speech.svelte.js` `readPageSelection`).

## The pages

- **General** (`settings/GeneralPage.svelte`): **Language and region**
  (`#settings-language`, `AboutYouSettings.svelte`, `about_you.rs`): one
  **Region** Select (`regionCountry`): "Automatic: United Kingdom (from
  Windows)", then every country A–Z; its line is what follows from it
  (`about_you_status`'s summary, test id `region-follows`: "Prices in £ ·
  miles and °C · British spelling · UK time"), an ⓘ that times follow the
  PC's clock. The spelling switch is in Language Models → Dictation cleanup.
  **About you** (`#settings-about-you`): "What should Yap call you?"
  (`aboutName`, ≤ 60), "Anything the AI should know about you" (`aboutMe`, 300
  characters, counted), one line on where it goes ("Kept on this PC, and sent
  only to the AI model you use with each question: never sent to Yap."), "See
  what the AI is told" (the block as it reads now) and a link to Privacy &
  data; the status re-reads as the fields change. **Startup and updates**
  (`#settings-startup`): "Start Yap when you sign in" (`autostart`,
  `set_autostart` at once; the switch goes back if Windows says no), "Check
  for updates automatically" (`updateChecksEnabled`). **Appearance**
  (`#settings-appearance`, `AppearanceSettings.svelte`; above the Yap bar
  since the theme came): **"Theme"** (System / Light / Dark, a Segmented like
  Motion → `theme`, data-setting `theme`; its line says what System means
  right now — "Follows Windows' app mode, which is dark right now", live),
  "Reading font" (each option and the closed box
  in its own font → `readingFont`), "Text size" (`readingSize`), a preview of
  the reading text, "Motion" (System / Reduced → `motion`; System's line says
  whether Windows' "Animation effects" is on). Settings applies its copy to
  the main window at once; the other windows follow the save
  (`lib/appearance.js`, [frontend.md](./frontend.md#the-dark-theme-2026-10-22)).
  **Yap bar** (`#settings-bar`, the bar menu's Settings → `general#bar`): Show
  the Yap bar (`barEnabled`; "Turn off the bar" in its menu arrives as
  `yap-bar-changed` and is adopted), a "Hidden until 15:40 · Show it now" row
  while hidden for an hour, Hide in fullscreen apps, Colours (**Match app** /
  Light / Dark, `barTheme`, default `app`; its line says what each means),
  Position (`overlayPosition`). The search finds Theme for "dark mode"
  (`settingsIndex.js`), Colours for "bar dark".
  **Notifications** (`#settings-notifications`): "Show tips" (`showTips`).
- **Dictation** (`settings/DictationPage.svelte`): **Start and stop**
  (`#settings-hotkey`): Hotkey (`hotkey`), Recording mode (Toggle /
  Push-to-talk; in push-to-talk its line says how double-tap goes hands-free).
  **Microphone and sound** (`#settings-microphone`): Microphone (`inputDevice`,
  `set_input_device` at once), "Chime when recording starts and stops" (the
  volume slider and the switch on one row: `audioFeedbackVolume`,
  `soundEnabled`), "Chime plays on" (`outputDevice`), "Silence other apps while
  you dictate" (`quietOtherApps`; ⓘ: calls are never silenced), "Pause music
  and videos while you dictate" (`pauseMedia`). **While you dictate**: "Live
  preview on the Yap bar" (`streamingPartials`), "Offer what you said after AI
  cleanup" (`asSaidOffer`, on: the bar's "Cleaned up · Use what I said",
  `as_said.rs`), "Cancel and finish buttons"
  (a Select When you point at it / Always / Off → `barDictationButtons`, Off
  by default, a line per choice). **Typing into apps** (`#settings-typing`,
  Advanced's old Output): "Match the text around the cursor"
  (`smartSpacing`), "Spell names the way they're written on screen"
  (`screenNames`, `screen_terms.rs`; its line says, with AI cleanup on a
  cloud provider, that the names a dictation seems to contain go there with
  it — the ⓘ that the screen's text never leaves the PC), "Leave the full stop off chat messages"
  (`chatNoFullStop`), "Append trailing space" (its line "Only where Yap can't
  read the text around the cursor" while the first is on), "Press Enter after
  pasting" (`autoSubmit`; "In a terminal, only for coding tools like Claude
  Code" while terminal pasting is on), "Say “press enter” to send"
  (`voiceEnter`), **"Say “snip” to take a screenshot"** (`snipVoice`, on:
  a dictation of just "snip" starts Snip to Chat; `snip-voice`), "Send key" (`autoSubmitKey`, while either is on), "Safer
  pasting into terminals" (`terminalMode`), "Restore clipboard after paste"
  (dictations never show up in Win+V history), and the **Dictionary** link row
  (its counts; it leaves Settings for the Dictionary view, `onshowview`).
- **Shortcuts** (`settings/ShortcutsPage.svelte`, OpenWhispr's Hotkeys page):
  the seven global shortcuts together — Dictate, Rewrite selected text, Ask the
  Voice Assistant, Read aloud, **Use what I said** (`asSaidHotkey`, unbound
  by default; `as_said.rs`), Meeting notes, **Snip to Chat** (`snipHotkey`,
  Win+Alt+S by default, its hint naming ✂, the tray, Esc and what Windows never
  lets an app capture; `snip.rs`, [snip.md](./snip.md)) — each in keycaps with Change, a
  clash said under the one being set. Their own pages show the same control
  (Read aloud: Voice Assistant → **Read aloud**, `#settings-read-aloud`). (No
  in-page fallback for Use what I said: its paste is never in a Yap window.)
- **Meetings** (`settings/MeetingsPage.svelte`, Wispr's Notetaker page and
  order): **Call detection** (`#settings-meetings`): "Detect calls and offer
  to take notes" (`meetingDetection`), **"Ask about calls in · 8 of 12 apps ›"**
  (a link row with the first apps' icons, `CallAppIcon`) opening one level
  down the list of call apps (`meetings#call-apps-list`, a back link to
  Meetings): two columns of [the app's icon — its own as installed, else its
  mark or letter] [name] [switch], the effective choice (`meetingDetectApps[app]`,
  else the app's default); greyed with a line while detection is off; a
  `yap-meeting-detect-choice` from Rust is adopted into Settings' copy). "How
  Yap asks" (Pop-up / Quietly → `meetingDetectStyle`), "Start notes
  automatically after 10 seconds" (`meetingAutoStart`). **While a meeting
  records**: Maximum length (`meetingMaxMinutes`), When a call ends (Ask me /
  Stop and summarise → `meetingCallEnd`), "End notes when the meeting goes
  quiet" (`meetingEndWhenQuiet`), "Hide Yap from screen sharing"
  (`meetingHideFromCapture`, in `#settings-screen-sharing`), "Tell speakers
  apart in meetings" (`SpeakersToggle`, the same switch as Speech-to-Text's,
  with a link to the models), "Suggest who's speaking"
  (`meetingSpeakerNames`). **Meeting audio** (`#settings-meeting-audio`,
  `meeting_audio.rs`): "Keep meeting audio" (`meetingKeepAudio`, on), "Keep
  it for" (1 / 7 / 14 / 30 / 90 days → `meetingAudioDays`, greyed while
  off), "Delete all meeting audio" ("2 meetings · 180 MB" from
  `meeting_audio_status`, kept current by `yap-meeting-audio`; Delete all… →
  the confirm dialog → `meeting_audio_clear`). Privacy & data's "What
  leaves this PC" has a Meeting audio row ("Never leaves this PC: kept here
  for 14 days…"). **Notepad**: "Open the notepad when a meeting
  starts" (`meetingOpenNotepad`), "Split the screen when joining"
  (`meetingSplitScreen`, greyed with a reason while the first is off), "Show
  live transcript" (`meetingLiveTranscript`), Meeting shortcut
  (`meetingHotkey`, clearable). **Calendar**: "Remind me before scheduled
  meetings" (Right before / 1 / 2 minutes / Never → `meetingReminder`) and a
  Calendars link row to Connectors. Last, the consent line.
- **Commands** (`commands`): "Tell Yap's AI what to write, and it types the
  result where you are", with an example note (`role="note"`) linking to the
  Voice Assistant. **Say its name** (`#settings-wake-word`): "Answer to its
  name in any dictation" (`llmScopes.voiceAgent.enabled`), Agent name
  (`AgentNameField.svelte`: saves itself 0.8 s after typing stops or as you
  leave it, and keeps the name in the dictionary). **Rewrite selected text**:
  the Rewrite shortcut (`editHotkey`), its line following the switch below
  (on: "With nothing selected within a minute of a dictation, it rewrites
  that dictation where it is; otherwise it writes where your cursor is";
  off: "With nothing selected, it writes where your cursor is"), and
  **"Rewrite your last dictation when nothing is selected"** ·
  "Within a minute, in the same field" (`#rewrite-last`,
  `rewriteLastDictation`, on; in the search index with its switch; the
  rules in `as_said.rs`, [pasting.md](./pasting.md)). **Model and prompt**: a
  link row with the model in use → Language Models → Commands.
- **Voice Assistant** (`voice`): "Ask Yap something by voice and hear the
  answer", the example note (names the shortcut once set) linking to
  Commands. **Assistant shortcut** (`#settings-assistant-shortcut`,
  `assistantHotkey`, clearable; a quiet line while none is set; its line
  follows the recording mode and the pause setting), "Stop listening when I
  pause" (`assistantStopOnPause`, row `stop-on-pause`; `pause.rs`: its line
  says "Asking hands-free (a double-tap)…" in push-to-talk), "Show the
  answer on the Yap bar" (`assistantAnswerCard`). **Opening apps** (`OpenAppsSettings.svelte`,
  `#settings-opening-apps`: `openApps`, Nicknames `appNicknames`, Apps Yap
  found with a switch each → `appsBlocked`). **Web search**
  (`WebSearchSettings.svelte`, `#settings-web-search`: `webSearch`; "Search
  with" — The AI's own search (Recommended, `webSearchNative`) / Exa / Tavily
  (`webSearchService`); with the AI's own search, **"What each provider
  charges for its search"** behind a disclosure (Groq, Gemini, Anthropic,
  OpenAI, OpenRouter, as published 18 Oct 2026) and "Yap's search" (Exa /
  Tavily); the service's key masked in `webSearchKeys` with "Get a free key";
  Wikipedia and reference wikis, always; where the search words go, a link to
  Privacy & data, and the safety rule). **Spoken replies in Chat**: Speak
  replies (`speakReplies`; switching it on without Kokoro offers it,
  `KokoroOffer.svelte`), When to speak (`speakWhen`), Talk back hands-free
  (`talkBackHandsFree`). **Voice** (`VoiceSettings.svelte`): Speaks with (On
  this PC / ElevenLabs → `ttsProvider`, `#settings-voice-provider`; with
  ElevenLabs, its line and `ElevenLabsSettings.svelte`: the key, the voices
  with ▶, the model, the month's characters), Engine (Kokoro / Windows voices
  → `ttsEngine`), Kokoro's download row (`ModelRow`), Voice (Kokoro's 54 in
  accent × gender groups, or Windows' → `ttsVoice` / `ttsWindowsVoice`) with ▶
  Preview, Speed (`ttsRate`) and Volume (`ttsVolume`, `tts_volume_set` live)
  as inline sliders, and the credits behind a "Credits" disclosure. **Model**:
  a link row → Language Models → Chat & Voice Assistant.
  `yap-voice-replies` and `yap-voice-volume` (Chat's) are adopted.
- **Memory** (`memory`, `MemorySection.svelte`, [memory.md](./memory.md#settings--memory)):
  **What Yap learns from** (the four switches, flat; a note when the Chat
  model is small: Yap learns carefully with it, by the small path), **You** (Profile and Preferences
  as rows that open in place, then About you's name, region and note as link
  rows to their rows in General), **People / Projects / Topics** (a card of
  objects each: a row per item opening to edit, "Add a person"), **Tell Yap
  what to change** (the box, Suggest changes, the diff), **Import, export and
  clear**, the privacy line. With no AI model, a note at the top.
- **Speech-to-Text** (`models`, `ModelManager.svelte`): **Your model** first —
  a card with its name, size, languages and "In use", or "Not downloaded" +
  Download (never "Active" for a model that isn't on this PC), and Change
  (down to the catalogue); with no model at all, the red badge's reason in a
  banner above it. **More models**: vendor pills (All / NVIDIA / OpenAI /
  Community) over every other model, each with a quiet Download (or Use, for
  one installed), delete behind Yap's confirm dialog. **Speakers in meetings**
  (`#settings-speakers`, `SpeakerModels.svelte` + `diarization.svelte.js`: the
  switch, the speaker models' rows, credits behind a disclosure). **Language**
  (Language, Translate to English) and **Performance**: **Speed**, where the
  model runs (`settings/RunsOn.svelte`, `stt_speed.rs`): the line says what
  Automatic chose and why, with each device's time for your dictations
  (`everydayMs`) — "Runs on: graphics card — fastest for your dictations on
  this PC (150 ms vs 280 ms)", "Runs on: processor — fastest for your
  dictations on this PC (400 ms vs 630 ms)", "…— the processor is only
  slightly faster for your dictations (…)", "…— far faster than the
  processor here", "…— the graphics card didn't run this model correctly
  here" — or "(not measured on this PC yet)", "(your choice). Automatic
  picks the processor here (150 ms vs 450 ms)"; a Select Automatic /
  Graphics card / Processor → `sttDevice`, with `useGpu` following it, and
  Measure now / Measure again; the ⓘ has each device's warm time at the three
  dictation lengths and its first run after loading, the rule — the graphics
  card unless the processor takes 20 % less time on everyday dictations,
  the long length shown but not counted — and that a meeting or a busy app
  slows the processor path; it follows `yap-stt-speed`) and Unload model when idle. (It replaced the GPU
  acceleration switch, 2026-10-22.)
- **Language Models** (`cleanup`): tabs Dictation Cleanup / Commands /
  Note Formatting / Chat & Voice Assistant (scope keys `cleanup`,
  `voiceAgent`, `noteFormatting`, `chat`; `cleanup#<scope>` opens one).
  **Dictation cleanup** (`#settings-cleanup`): "Clean up dictations with AI"
  (`postProcessEnabled`); **off, one line** ("Turn on cleanup to pick a
  model…") instead of a screen of greyed choices. On: where it runs (Cloud
  providers / Local / Self-hosted — the same names on every tab;
  `ppMode`/`cloudProvider` are UI-only and resolve to `ppProvider`), the key
  banner when one is missing, the provider pills, the masked API key with "Get
  your API key", the model list (`ModelPicker.svelte`, the curated list merged
  with the provider's live one, `modelCatalog.js`), a self-hosted or custom
  endpoint's fields (`EndpointModels.svelte` suggests its server's models),
  Local's on-device model browser (Recommended for this PC, the one upgrade
  offer, the slow-on-this-PC line, the Context control `LocalContext.svelte`,
  bring-your-own GGUFs), "Hide the model's thinking" for reasoning models.
  **Spelling** (`#settings-cleanup-spelling`, `RegionSpelling.svelte`: "Write
  in my region's spelling" → `regionSpelling`, greyed while cleanup is off).
  With cleanup on: **Prompt Studio** (`PromptStudio.svelte`: View shows the
  first four lines and "Show the whole prompt", Customize — preset + body +
  Save/Reset —, Test), **Smart routing** (only clean up in apps with a rule;
  profiles with a per-profile model; app rules; Yap's own Selects), **Usage
  today**, and one line on where the text goes with a link to Privacy & data.
  The other tabs (`VoiceAgentConfig`, `NoteFormattingConfig`, `ChatConfig`,
  sharing `ScopeProviderConfig`): their switch first; off, one line; on, the
  model and the prompt. Commands' tab links to the Commands page for the name
  and shortcut; Chat's to the Voice Assistant page.
- **Connectors** (`ConnectorsSection.svelte`, Wispr's): a card per source —
  Google Calendar (the browser, "Finish in your browser…" + Cancel; a build
  without the client says so), Outlook Calendar (the guided ICS form), Other
  calendar (an iCal link) — each connection a row with ⋯ Sync now /
  Disconnect; then the **AI apps (MCP)** link row.
- **AI apps (MCP)** (`McpSection.svelte`, Wispr's): a card each for Claude /
  ChatGPT / Gemini / Cursor ("Add to X" → Added ✓ · Remove), **All other
  apps** (Claude Code / VS Code / Windsurf + the command and JSON), "Let AI
  apps save notes to Yap" (`mcpAllowWrites`), the Local-API-off warning with
  "Turn on the Local API", and one line on where their reads go with a link to
  Privacy & data.
- **Privacy & data** (`settings/PrivacyPage.svelte`, OpenWhispr's Privacy &
  Data, Claude's Privacy; replaces History and gathers the scattered privacy
  notes): **What leaves this PC** — a table by feature as Yap is set up now
  (your voice, recent dictation audio, AI cleanup, names on screen — read on this PC for each
  dictation, or with cleanup on a cloud provider the names a dictation seems
  to contain go there with it —, Chat and the Voice Assistant, screenshots (kept on this PC; one goes to the AI model only with a question you send), About you, memory, web search,
  spoken replies, AI apps, calendar, account), each row linking to its
  setting. **Network connections** (`#settings-connections`): a link row
  ("12 today" · "3 left this PC today, 9 stayed on it") one level down to
  `settings/ConnectionsView.svelte` (`privacy#connections-list` opens it
  directly; the network receipts, [privacy-receipts.md](./privacy-receipts.md)).
  **History** (`#settings-history`): "Keep dictation history"
  (`historyEnabled`) and Clear history… (asks first; the count from
  `get_stats`, re-read on `yap-history-changed`; it clears the kept audio
  too); the list itself is Home's feed. "Keep recent dictation audio"
  (`keepRecentAudio`, `recent_audio.rs`: the last 24 hours, on this PC, to run
  a dictation again; greyed while history is off) and Clear recent audio…
  ("3 dictations · 1.2 MB" from `recent_audio_status`; asks first; history
  stays). **Chat screenshots** (`#settings-chat-images`): "Keep chat
  screenshots" (`chatImageDays`: 7 / 30 / 90 days / Forever, `chat-images`;
  "4 screenshots · 1.2 MB" from `chat_images_usage`; Snip to Chat, [snip.md](./snip.md)).
  **Local API** (`#settings-local-api`): `bridgeEnabled` (the
  Integrations view follows it, `yap-bridge-changed`). **Logs and data**
  (`#settings-logs`): Debug logging (`debugLogging`; ⓘ: what you dictate can
  appear in logs), the log file (Copy path, Open logs folder) and where the
  data folder is (portable-aware).
- **About** (`settings/AboutPage.svelte`): the version and one line on what
  Yap does (Parakeet era wording), "Your voice never leaves this PC" with a
  link to Privacy & data, **Updates** (the shared store's states: Check for
  updates, Downloading…, "Yap X is ready to install" + Restart to update,
  release notes; a line on automatic checks linking to General), **Report a
  problem** (`settings/ReportProblem.svelte`, row `report-problem`: the
  diagnostic file's contents shown first, then Save…, then "Open a GitHub
  issue" / Show in folder; [privacy-receipts.md](./privacy-receipts.md)),
  **Learn more** (Show setup guide again, the GitHub link).
- **Account** (`AccountSection.svelte`, [accounts.md](./accounts.md)): while
  Yap asks the service which sign-ins it offers, placeholder buttons and
  "Checking sign-in options…".
