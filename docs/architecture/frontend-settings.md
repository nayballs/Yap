# Frontend: Settings

`lib/Settings.svelte`, page by page (Commands, Voice Assistant, General, Speech-to-Text, Language Models, History, Advanced, About, Connectors, MCP, Account). Account's own page is [accounts.md](./accounts.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

- **`lib/Settings.svelte`** — the settings surface, now rendered **inside the
  ControlPanel's modal** (`embedded` prop; ✕ closes). Grouped sidebar (App / AI models / Data / Connections / System):
  two pages for two different things (2026-10-18; both were "Voice
  Agent"), each opening with one sentence and an example in a note
  (`role="note"`) that links to the other: **Commands** (App, after
  General; id `commands`): "Tell Yap's AI what to write, and it types the
  result where you are" — *Say "Hey Jarvis, reply that I'll be late" in
  any app; Yap writes it and types it there* (the agent's name, live); **Say
  its name** (`#settings-wake-word`: the Commands scope's enable switch
  `llmScopes.voiceAgent.enabled` "Answer to its name in any dictation", the
  agent name — `AgentNameField.svelte`), **Rewrite selected text** (the
  **Rewrite shortcut**, `editHotkey`, moved here from General →
  Activation), **Model and prompt** → Language Models → Commands. **Voice
  Assistant** (id `voice`): "Ask Yap something by voice and hear the
  answer, like a voice assistant" — the example names the shortcut once
  set; **Assistant shortcut** (`#settings-assistant-shortcut`,
  `ui/HotkeyInput` `assistantHotkey`, clearable, "Set a shortcut", with a
  gentle "Set an assistant shortcut…" line while none is set; its line
  follows the recording mode), **Opening apps** (`OpenAppsSettings.svelte`,
  `#settings-opening-apps`: "Let Yap open apps when you ask" `openApps`;
  **Nicknames** — "Say … → App to open" with a type-ahead of the apps,
  `appNicknames`, duplicates refused, ✕ to remove; **Apps Yap found** —
  `apps_list`, a count, Refresh, a search box, two columns of the app's
  own icon (`apps_icons`, else its letter) + a switch each, off = "Yap
  won't open this one" `appsBlocked`; the first 40, then Show all),
  **Web search** (`WebSearchSettings.svelte`, `#settings-web-search`:
  "Look things up on the web" `webSearch`, off; **"Search with"**, a radio
  list: **The AI's own search** (Recommended, `webSearchNative`, the
  default: "Cloud models (Groq, Google Gemini, Anthropic, OpenAI,
  OpenRouter) search with their provider's built-in search, on the API key
  Chat already uses. Local and self-hosted models use Yap's search below.")
  / **Exa** / **Tavily** (every model, `webSearchService`); with the AI's
  own search, **"What each provider charges for its search"** (Groq: no
  charge on the free plan but ~4,000 of its 8,000 tokens a minute per
  searched answer, paid $5/1k + $1/1k pages; Gemini 3+ billing only,
  5,000 a month free then $14/1k — on a free key Yap's own search answers —,
  2.5 Flash/Flash-Lite where still offered 500 a day free, free tier used
  to improve Google's products; Anthropic and OpenAI $10/1k + tokens; OpenRouter the
  provider's price or $0.007 a search; as published 18 Oct 2026) and
  **"Yap's search"** (Exa (Recommended) / Tavily, for local and
  self-hosted models and when a provider's search is unavailable); each
  key masked in `webSearchKeys` with "Get a free key →" (its dashboard via
  `open_external`) and its free allowance; "Wikipedia and reference wikis"
  (no key needed), always included; where the search words go (a cloud
  model's provider, else the service / Wikipedia); the explain line adds a
  web example while it's on), **Spoken replies in Chat** ("Speak replies" `speakReplies`;
  "When to speak" Segmented When I asked by voice / Always `speakWhen`;
  "Talk back hands-free" `talkBackHandsFree`, greyed while replies are off),
  switching "Speak replies" on while Kokoro isn't downloaded offers it
  under the switch (`KokoroOffer.svelte`, "Download Kokoro (379 MB)" / Not
  now; replies stay on, Windows voices meanwhile; not offered while
  ElevenLabs speaks), **Voice**
  (`VoiceSettings.svelte`: **Speaks with** Segmented On this PC /
  ElevenLabs `ttsProvider` (`#settings-voice-provider`, the fallback
  notice's Open settings target); with **ElevenLabs** picked, the plain
  line "The words Yap speaks are sent to ElevenLabs." under it, then
  `ElevenLabsSettings.svelte`: the masked API key `ttsCloudKeys.elevenlabs`
  (edit / done, as the web search keys) with "Get your API key →" (their
  API-keys page via the opener) and a status line ("Key works · 37
  voices", "ElevenLabs didn't accept it: …"); the account's voices as a
  radio list — Your voices, then ElevenLabs voices; name + accent ·
  gender · age; a ▶ / ■ per voice (`tts_cloud_preview`, its sample) —
  saved as `ttsCloud.elevenlabs {voice, voiceName}`; **Model** Select
  (Flash v2.5 · Recommended first; `ttsCloud.elevenlabs.model`); the
  month's characters with a bar when the key may read them; and what
  happens when ElevenLabs doesn't answer (Yap's own voice steps in, or, with
  no Kokoro downloaded, nothing is read). The lists load with the key as
  typed, 600 ms after typing stops, and when the page opens. On this PC:
  **Engine** Segmented Kokoro / Windows voices
  `ttsEngine`, "Natural voices, made on this PC. Recommended"; Kokoro's
  **download row** (`ModelRow`: Kokoro-82M · size · 54 voices ·
  Recommended, Download → progress → Active, delete via `ui/ConfirmDialog`;
  in a stub build "In the full Yap build"); the **voice picker** following
  the engine — Kokoro's 54 in `<optgroup>`s British English / American
  English / Other languages × Female / Male (`kokoroOptions` in
  `kokoro.svelte.js`, "Bella (default)", `ttsVoice`), or Windows' with its
  default first (`ttsWindowsVoice`) — **▶ Preview** / ■ Stop →
  `tts_preview` with the shown engine, voice and speed (off until Kokoro's
  downloaded); Speed slider 0.5–2.0× `ttsRate`; Volume slider `ttsVolume`
  (also in Chat's + menu, beside the Voice replies switch); the credit line: Kokoro
  Apache-2.0 from onnx-community, misaki Apache-2.0, eSpeak NG GPL-3.0 run
  separately, licences and the source link saved with them;
  `kokoro.svelte.js` is the download store) and **Model** → Language
  Models → Chat & Voice Assistant. `yap-voice-replies` (Chat's toggle) is
  adopted into the config copy. **Hotkeys**: the four (`HOTKEY_CMD` /
  `HOTKEY_NAMES`: dictation hotkey, rewrite shortcut, meeting shortcut,
  assistant shortcut) are refused when they'd fire on one press
  (`hotkeysClash`: "Already used by the …", or "Clashes with the … (Win +
  Alt + M): one press would start both"); the in-page fallback catches the
  meeting, assistant and dictation keys, in that order. **Language Models**
  tabs read Dictation Cleanup / **Commands** (`VoiceAgentConfig.svelte`,
  scope `voiceAgent`, "Name and shortcut in Commands") / Note Formatting /
  **Chat & Voice Assistant** (`ChatConfig.svelte`, scope `chat`, a line
  linking to the Voice Assistant page).
  **General** (hotkey, recording mode — in push-to-talk its line says how
  double-tap goes hands-free, "More shortcuts" naming the rewrite and
  assistant shortcuts with buttons to Commands / Voice Assistant; then
  **About you** (`AboutYouSettings.svelte`, `#settings-about-you`,
  `about_you.rs`, 2026-10-21): "Region and language" as one line from
  `about_you_status` ("United Kingdom · English (UK) · £ GBP · metric · UK
  time") with **Change** → Country / Language / Currency / Units / Time zone
  Selects (first option "Windows: United Kingdom" or "Automatic: £ GBP" —
  what a picked country brings —, the zones from `Intl.supportedValuesOf`
  grouped by region; `regionCountry` … `regionTimeZone`, empty = Windows'),
  "Windows says: … · Use Windows' settings" once something's picked; "What
  should Yap call you?" (`aboutName`, ≤ 60) and "Anything the AI should know
  about you" (`aboutMe`, a 300-character textarea with a count); "Use my
  region's spelling" (`regionSpelling`: "AI cleanup writes British English:
  honour, recognise, paralyse, metre…"); "See what the AI is told" (the
  block, as it reads now) and the privacy line ("Kept on this PC. Yap sends
  it to the AI model you use, with each question: a cloud provider … sees
  it, a model on this PC keeps it here. It's never sent to Yap."); the
  status re-reads as the fields change, before the auto-save), mic, sound+volume, then "Silence other apps
  while you dictate" (`quietOtherApps`) and "Pause music and videos while
  you dictate" (`pauseMedia`) where "Mute while recording" was; **Yap bar** group
  (`#settings-bar`, the bar menu's Settings → "general#bar"): Show the Yap bar
  (`barEnabled`; "Turn off the bar" in its menu arrives as `yap-bar-changed`
  and is adopted), a "Hidden until 15:40 · Show it now" row while hidden for an
  hour, Hide in fullscreen apps (`barHideFullscreen`), the live-preview toggle,
  "Cancel and finish buttons while dictating" (`ui/Segmented` When you point
  at it / Always / Off → `barDictationButtons`, Off by default, a line per
  choice)
  and Position (`overlayPosition`) — the dictation overlay itself is always on,
  it's the hot-mic indicator; **Notifications** group
  (`#settings-notifications`): "Show tips" (`showTips`, on: coaching hints
  and the calendar nudge; questions, alerts and errors always show);
  **Meetings** group (`#settings-meetings`, the
  target of `yap-settings-goto` "general#meetings"): "Detect calls and offer to
  take notes" (`meetingDetection`), then — disabled while it's off — "How Yap
  asks" (`ui/Segmented` Pop-up / Quietly → `meetingDetectStyle`, a one-line
  explanation of the choice) and "Ask about calls in" (a two-column list,
  one row per call app from the `meetingDetect.apps` snapshot: [its icon,
  `CallAppIcon` at 20 px — the real one as installed, else its mark or
  letter; greyed with the switch while detection is off] [name] [switch],
  showing the effective choice; a flip writes `meetingDetectApps[app]`; a
  `yap-meeting-detect-choice` from Rust is adopted into Settings' config copy
  so auto-save can't undo it), then, in Wispr's order: the guard rails
  "When a call ends" (Select: Ask me / Stop and summarise automatically →
  `meetingCallEnd`; disabled while detection is off), "Maximum recording
  length" (Select 1–4 hours / No limit → `meetingMaxMinutes`), "End notes
  when the meeting goes quiet" (`meetingEndWhenQuiet`, where Wispr has its
  auto-end switch: after the length), "Hide Yap's
  meeting windows from screen sharing" (`meetingHideFromCapture`, wrapped in
  `#settings-screen-sharing` — the screen-share tip's "Update settings"
  target); the meeting notepad's "Open the notepad when a meeting starts"
  (`meetingOpenNotepad`) and "Split the screen when joining"
  (`meetingSplitScreen`, greyed out while the first is off); "Meeting
  shortcut" (`ui/HotkeyInput`, clearable → `meetingHotkey`; paused while
  capturing via `configure_meeting_hotkey`; the three hotkeys can't clash);
  "Notify before scheduled meetings start" (Select: Right before / 1 minute /
  2 minutes / Never → `meetingReminder`) with a "Calendar" row under it
  (Connected: <account> + Manage, or Connect calendar → Connectors);
  "Show live transcript" (`meetingLiveTranscript`, the notepad's live lines)
  + "Tell speakers apart in meetings" (`SpeakersToggle`, the same switch as
  below, + a link to the models) + an always-visible consent line, "Recording a
  call? Let people know you're taking notes."), **Speech-to-Text** (`ModelManager` + GPU +
  language/translate, and **Speakers in meetings** (`#settings-speakers`,
  `SpeakerModels.svelte` + `diarization.svelte.js`, `diarize.rs`): the speaker
  models in `ModelRow`s (Download / Active / delete — the active one too;
  the fallback greyed "For phone-only calls — coming later", status
  `unavailable`), the **"Tell speakers apart in meetings"** switch (Beta chip,
  "It all runs on this PC"; `SpeakersToggle.svelte`: off until a model is
  installed — switching it on without one offers "Download and turn on" /
  Not now with a progress bar instead; deleting the active model turns it
  off), and where the files come from with their licences (OpenMDW-1.1,
  Apache-2.0)), **Language Models** (OpenWhispr-style: enable toggle → mode
  selector Cloud Providers/Local/Self-Hosted → provider pill tabs (Groq/Anthropic/
  OpenAI/OpenRouter/Custom, brand icons) → API Key (masked + "Get your API key"
  link) → Select Model rows (`ModelPicker.svelte`: `ppModels.js`'s curated
  list merged with the provider's live one by `modelCatalog.js` — curated it
  still lists, then "New" (released after `CURATED_AT`, by the provider's
  date or, for Gemini's dateless list, not in the registry's `seen`; 120
  days), the rest behind "Show N more" with a filter past 12, the saved
  model it no longer lists kept with "No longer offered" and a line saying
  so, "Groq's current models, checked 2 h ago · Check now" or "Couldn't
  check…: … Showing Yap's list. · Try again"; no key, offline or a failed
  check = exactly the curated list; switching provider picks its first model
  it still offers; `ui/SelectList.svelte` draws the tags); a self-hosted or
  custom endpoint's Model field suggests its server's `/models`
  (`EndpointModels.svelte`, a `<datalist>` + "3 models on this server"), as
  do the cleanup profiles' model fields;
  UI-only `ppMode`/`cloudProvider` state resolves to the unchanged `ppProvider`
  contract. Local = the on-device model browser (family tabs, Download / Use /
  delete; each card's size line ends with its maximum, "2.8 GB · up to
  256K"; above the cards the same **Context** control as the Chat picker's,
  `LocalContext.svelte`, adding "Shared by Chat, dictation cleanup and
  meeting notes."; Settings adopts `yap-local-context-changed` and re-reads
  on `yap-local-llm-changed`): **"Recommended for this PC"** on `local_llm_status`'s pick (Qwen3.5
  4B with a capable card, else the 1.5B; `isDefault` = the model an empty
  `ppLocalModel` means), and under its heading one quiet line at most — the
  4B offered once (`upgrade`: "Your NVIDIA GeForce RTX 5070 Ti can run Qwen3.5
  4B…" Download / No thanks → `localLlmUpgradeDismissed`), or, a big model
  left on the processor (`slowOnThisPc`, `placement`), "Runs on the processor
  on this PC: answers will be slow — Qwen2.5 1.5B Instruct is quicker." + Use
  it (read afresh whenever the section opens). Below it, **Prompt Studio** (`PromptStudio.svelte`, OpenWhispr port):
  View (full effective prompt via `get_base_prompt`) / Customize (preset +
  body + Save/Reset) / Test tabs. Plus usage meter + profiles w/ per-profile
  model override + per-app rules), **History** (stats
  dashboard + recent list + enable/clear), **Advanced** (output toggles —
  "Match the text around the cursor" (`smartSpacing`) first, "Leave the full
  stop off chat messages" (`chatNoFullStop`), then "Append
  trailing space", whose line reads "Only where Yap can't read the text around
  the cursor" while the first is on, auto-submit (its line adds "In a terminal,
  only for coding tools like Claude Code" while terminal pasting is on), "Say
  “press enter” to send" + the key, "Safer pasting into terminals"
  (`terminalMode`), and "Restore clipboard after
  paste", which says dictations never show up in Win+V history — system,
  dictionary), **About** (version + an **Updates** card on the shared update
  store: "Last checked …" / Check for updates, Downloading… %, "Yap X is ready to
  install" + Restart to update, release notes + release-page link, the
  "Check for updates automatically" toggle), **Connectors** (first in the
  **Connections** group; `ConnectorsSection.svelte`, laid out like Wispr's
  Settings → Connectors: serif title + one line on what a calendar gives you,
  a rounded light card per connector — **Google Calendar** ("Reminders for
  Google meetings before they begin", Connect → the browser, "Finish in your
  browser…" + Cancel; a build without the client says "Google sign-in is in
  the installed Yap. In this build, use a private iCal link below, under
  Other calendar."), **Outlook
  Calendar** (Connect → the guided publish-your-calendar ICS form), **Other
  calendar** (an iCal link, help for Google/iCloud/Fastmail) — each
  connection a row with its account or link host, "Synced …" or the error,
  "Connect again" when Google needs it, and a ⋯ menu Sync now / Disconnect;
  then a plain row with the AI apps' logos, "Give your AI access to your
  meeting transcripts and notes" + **Go to MCP**), **MCP** (in the same
  **Connections** group, between Data and System; `McpSection.svelte`, laid out like Wispr's Settings →
  MCP: serif title, a light card each for Claude / ChatGPT / Gemini / Cursor with
  "Allow X to access your meeting notes and transcripts" + **Add to X** (→ Added ✓
  · Remove), the ChatGPT card's "desktop app and Codex, not the web" note, then
  **All other apps:** with one-click rows for Claude Code / VS Code / Windsurf +
  the command and JSON, the "Let AI apps save notes to Yap" switch
  (`cfg.mcpAllowWrites`), a Local-API-off warning with "Turn on the Local API",
  and the privacy line; it re-reads when it comes into view, as Settings stays
  mounted), and **Account** (bottom of the
  sidebar — `AccountSection.svelte`). The status bar's update link follows the
  same store (Check for updates → Restart to update / Downloading… / Up to date ✓).
