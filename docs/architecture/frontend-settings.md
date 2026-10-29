# Frontend: Settings

`lib/settings/Settings.svelte` and its pages in `lib/settings/`, page by page. Account's
own page is [accounts.md](./accounts.md).

[Architecture index](./README.md) · [CLAUDE.md](../../CLAUDE.md)

## The shell (2026-10-22 review: structure, search, one row style)

Settings is a modal in the main window (`ControlPanel.svelte`: `embedded`,
`open`, `onclose`, `onshowview`), **always mounted** so its in-page hotkey
fallback (the dictation, assistant, meeting, read-aloud and snip keys while a Yap window has the
focus) and its auto-save run for the window's whole life. Esc closes it (a
search being typed is cleared first); the ✕ is 32 px in `--yap-fg-62`.

### The module map (split 2026-10-24)

`Settings.svelte` (~1,100 lines, from 3,829) is the modal shell: the nav,
its search and the icon rail, `goto`, which page is shown, its own
styles, and the wiring — at init it starts the window-lifetime jobs below
(`syncConfig`, `followAttention`, `followLocalLlm`,
`followHotkeyFallback`, `followDictionary`, `followOutsideChanges`) and at
mount reads the config and sets it up in the same order as before
(guardrails, version, `readConfig`, the provider's mode, the key credited,
`ensureScopes`, the on-device status, `setPristine`, the profiles
migration, `loaded`, devices, usage). What a page needs to outlive it
stays outside the page: in the shell as a bindable prop
(`meetingApps`, `privacyConnections`, `offerKokoro`, `langInfo`) or in a
store.

| Module | What it holds |
|---|---|
| `settings/configSync.svelte.js` | The config copy (`config.cfg`, `loaded`, the "Saved ✓" pulse), `base` / `pristine`, `inTurn`, `adopt` / `adoptSaved` on `yap-config-saved`, `persist`, the 400 ms auto-save (`syncConfig`, with an `onAdopted(keys)` hook) |
| `settings/configDefaults.js` | `FIELD_DEFAULTS` (a new setting's default goes here) and `cleanConfig(cfg)`, the saved payload (was `buildClean`) |
| `settings/configMerge.js` | The pure part: `merge3`, `fill`, `same`, `copy` |
| `settings/adoptions.svelte.js` | `followOutsideChanges`: the one-off adoptions (`yap-config-patched`, `yap-provider-key-saved`, a call app's choice, the on-device context and state, the Chat chip's scope, spoken replies' switch / volume / speed, the consent message, the bar turned off) |
| `settings/dictionarySync.svelte.js` | `followDictionary`: the Dictionary view's list both ways, its echo guard, learned words |
| `settings/hotkeyFallback.svelte.js` | `followHotkeyFallback`: the in-page keys (meeting, assistant, read aloud, snip, the language shortcuts, dictation, in that order) |
| `settings/languages.svelte.js` | My languages' helpers: the language list read once (`loadLanguageCatalog`, `catalog`, `languageName`), `myLanguages(cfg)` (an old config's one language too), the language shortcuts (`languageShortcuts`, `shortcutName`, `newLanguageShortcut`, `MAX_LANGUAGE_SHORTCUTS`) |
| `settings/MyLanguages.svelte` | Dictation → My languages: the search, the model coverage and recommendation, the main shortcut's language, the language shortcuts, the nudge's switch |
| `lib/core/providers.js` | The providers in one place: ids, `BASE_URLS`, `KEYED_PROVIDERS`, `PP_PROVIDERS`, `PROVIDER_MODES`, `CLOUD_TABS`, `MODEL_HINTS`, `maskKey`, the four name sets each surface uses today (`PROVIDER_LABELS` short, `SCOPE_PROVIDER_LABELS` the scope tabs', `CLEANUP_PROVIDER_LABELS` cleanup's, `PROVIDER_DESTINATIONS` where the words go), and the key store's rules (`sharedKey`, `stashCleanupKey`, `creditCleanupKey`, `liftScopeKeys`, `adoptProviderKey`, `cleanupNeedsKey`) |
| `lib/stores/attention.svelte.js` | The red count's items (`attentionItems`) kept live by `followAttention` |
| `settings/localLlm.svelte.js` | Settings' on-device model: its status, the download and its progress, switching (a curated model or one of your own, `activateOwn`), the upgrade offer, the family tab, the warm-up poll (`followLocalLlm`); Uninstall is `lib/core/localModels.js`'s, shared with the Chat model picker |
| `settings/languageModels.svelte.js` | Language Models' state that outlives the page (`lm`: the tab, cleanup's mode and cloud tab, the key field open, the Chat tab's remount count, Prompt Studio's guardrails, Smart routing's recent apps, the day's Groq usage), `providerUi`, `loadRecentApps`, `refreshUsage` |
| `settings/scopes.js` | The four scopes (`SCOPE_TABS`, `SCOPE_DEFAULTS`…), `ensureScopes(cfg)`, `scopeModelLabel` |
| `settings/cleanupPresets.js` / `settings/profiles.js` | The cleanup presets (`PP_DEFAULT_BODY` = `FIELD_DEFAULTS.ppPrompt`); profile ids and the pre-profiles migration |
| `settings/LanguageModelsPage.svelte` | The page: tabs, Dictation cleanup (mode, key, model, endpoint fields, thinking), Spelling, Styles by app, Prompt Studio, where the text goes; with `LocalModels.svelte`, `SmartRouting.svelte`, `UsageToday.svelte` |
| `settings/CommandsPage.svelte` / `VoicePage.svelte` / `SpeechPage.svelte` | The other three pages that were inline; `PageExplain.svelte` is Commands' and the Voice Assistant's example note |

Since 2026-10-24 `Settings.svelte` and every part only Settings uses sit in
`src/lib/settings/` with the pages: `AboutYouSettings`, `AppearanceSettings`,
`AccountSection`, `ConnectorsSection`, `McpSection`, `MemorySection`,
`VoiceSettings`, `ElevenLabsSettings`, `WebSearchSettings`,
`OpenAppsSettings`, `KokoroOffer` (+ `kokoro.svelte.js`), `AgentNameField`,
the speech models' `ModelManager`, `ModelRow`, `SpeakerModels`,
`SpeakersToggle` (+ `diarization.svelte.js`), and the Language Models tabs'
`ScopeProviderConfig`, `ChatConfig`, `NoteFormattingConfig`,
`VoiceAgentConfig`, `PromptStudio`, `ModelPicker`, `EndpointModels`,
`RegionSpelling` and `cleanupStyles.js`. What Settings shares with other
windows is in `components/`, `stores/` and `core/`
([frontend.md](./frontend.md#the-folder-layout-2026-10-24)).

**CSS order is part of the look.** Vite bundles the CSS in import order,
and a component's scoped rule (`.offnote.svelte-x`) and `settings.css`'s
(`.settings-shell .set-note`) can tie: the later one wins. The scope tabs'
own `.offnote` / `.tools` margins have always lost to `.set-note`'s, and
cleanup's `.set-note.offnote` wins. So `LanguageModelsPage.svelte`
imports its shared components, then `settings.css`, then its own parts,
and `Settings.svelte` imports it just before `settings.css`. A change
there can move margins: compare a page before and after.

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
  `yap-meeting-detect-choice`, …; `settings/adoptions.svelte.js` and
  `dictionarySync.svelte.js`) still run; they and this agree. The copy and
  the merge are `settings/configSync.svelte.js`. e2e:
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
  calls/meeting_guard.rs `meetings#screen-sharing`, meetings/meeting.rs (quiet mic)
  `dictation#microphone`, notices/alert.rs `cleanup#chat` / `voice#voice-provider`,
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
- **Hotkeys** (`settings/hotkeys.svelte.js` + `SettingsHotkey.svelte`; their
  more keys, `moreHotkeys`, checked in the same list — see Shortcuts below): the
  six (`HOTKEY_CMD` / `HOTKEY_NAMES`: dictation hotkey, rewrite shortcut,
  meeting shortcut, assistant shortcut, read-aloud shortcut, Use-what-I-said
  shortcut) are refused when they'd fire on one
  press (`hotkeysClash`: "Already used by the …", or "Clashes with the … (Win
  + Alt + M): one press would start both"); while one is being picked
  (`hotkeyCapture.on`) its live binding is paused and the in-page fallback
  stands down. The fallback catches the meeting, assistant, read-aloud and
  dictation keys, in that order (read aloud: the page's own selection,
  `speech.svelte.js` `readPageSelection`). **The saved transforms'
  shortcuts** (Commands → Transforms) are checked against the ten and
  each other (`validateTransformHotkey`: "Already used by the Polish
  transform"), and the ten against them (`validateHotkey` reads
  `cfg.transforms`); one being picked pauses its slot
  (`onTransformCapturing` → `configure_transform_hotkey`). No in-page
  fallback for them: a transform works on another app's text. **The
  language shortcuts** (Dictation → My languages) are checked the same way
  (`validateLanguageHotkey`: "Already used by the English → Spanish
  shortcut"; the names from `settings/languages.svelte.js`, which reads the
  language list once), come before the transforms in every clash message,
  pause their slot while picked (`onLanguageCapturing` →
  `configure_language_hotkey`), and do have an in-page fallback
  (`hotkeyFallback.svelte.js` → `language_key { id, pressed }`: a dictation
  into Yap's own Chat box in French works).

## The pages

- **General** (`settings/GeneralPage.svelte`): **Language and region**
  (`#settings-language`, `AboutYouSettings.svelte`, `ai/about_you.rs`): one
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
  (`lib/core/appearance.js`, [frontend.md](./frontend.md#the-dark-theme-2026-10-22)).
  **Yap bar** (`#settings-bar`, the bar menu's Settings → `general#bar`): Show
  the Yap bar (`barEnabled`; "Turn off the bar" in its menu arrives as
  `yap-bar-changed` and is adopted), a "Hidden until 15:40 · Show it now" row
  while hidden for an hour, Hide in fullscreen apps, Colours (**Match app** /
  Light / Dark, `barTheme`, default `app`; its line says what each means),
  Position (`overlayPosition`). The search finds Theme for "dark mode"
  (`settingsIndex.js`), Colours for "bar dark".
  **Notifications** (`#settings-notifications`): "Show tips" (`showTips`),
  then one switch per kind of nice-to-know notice (2026-10-29,
  `notices/kinds.rs`, each at most a few a day): Suggestions
  (`notifySuggestions`), Feature tips (`notifyFeatureTips`, off with Show
  tips), Milestones (`notifyMilestones`), News (`notifyNews`: "Updated to
  …" after an update).
- **Dictation** (`settings/DictationPage.svelte`): **Start and stop**
  (`#settings-hotkey`): Hotkey (`hotkey`), Recording mode (Toggle /
  Push-to-talk; in push-to-talk its line says how double-tap goes hands-free),
  and in push-to-talk **Triple-tap for meeting notes** (`tripleTapMeeting`,
  off, `triple-tap-meeting`; `dictation/gesture.rs`).
  **My languages** (`#settings-languages`, `settings/MyLanguages.svelte`,
  2026-10-25; [dictation.md](./dictation.md) `dictation/languages.rs`):
  "Languages you speak" (`my-languages`) — the picked ones as chips with ✕,
  and a search box (a combobox, "Add a language"; ↑/↓, Enter, Esc) over
  `languages_catalog`'s hundred languages by English or own name ("franç"
  finds French) → `myLanguages` (a config from before shows the language the
  Speech page had picked, `myLanguages()` in `languages.svelte.js`).
  With one or more: "Your speech model" (`language-models`,
  `my_languages_status`) — one sentence on what your model makes of them
  ("Parakeet V2, your speech model, understands English only. French needs
  another model."), the model to recommend for them all (an installed one
  first) with **Open Speech-to-Text** (Yap never changes the model here),
  and a small table: each installed model × each language, ✓ or —, yours
  marked; "Main shortcut (F9)" (`main-language`): Auto or one of them →
  `selectedLanguage`, its line saying how Auto works with your model
  (Whisper hears which; Parakeet V3 recognises it by itself; Canary must
  be told, so the first; Parakeet V2 knows English only); **Language
  shortcuts** (`language-shortcuts`): rows of Speak (Any of my languages /
  each of yours) → Paste as (As spoken / Translated into each language), a
  shortcut and Delete, at most four, "Add a language shortcut"; a note when
  one translates (the model it uses, Use what I said); "Tell me when I speak
  a language I didn't pick" (`languageNudge`, `language-nudge`; its line says
  only Whisper models can tell). With Hindi among them (2026-10-29), **"Write
  in Latin letters (Hinglish)"** (`hinglish`, `#hinglish`, off; in
  `MyLanguages.svelte` itself, so onboarding's language step shows it too):
  its line gives an example, its ⓘ which speech model hears Hindi (yours, or
  "only Whisper models do (Whisper Large v3 Turbo is the one to pick)").
  **Microphone and sound** (`#settings-microphone`): Microphone — since
  2026-10-25 an **order** (`settings/MicOrder.svelte`, `micOrder`,
  `dictation/mics.rs`): numbered rows, each "In use" / "Connected" / "Not
  connected" (its name shortened, `lib/core/mics.js` `shortMicName`; the
  page's own copy of the order runs the same rules as Rust: `matches`,
  `findMic`, `micInUse`), ↑ ↓ to move and ✕ to take off, then a fixed
  "then Windows' default · <its name>" row; "Add a microphone…" lists the
  connected ones not in it (a Row with `stack`). Windows' list from
  `list_mics`, again on `yap-mics-changed`. Saved by the auto-save; every
  connected entry gets its endpoint id on the first edit (one migrated by
  name). Used from the next dictation (`mic.rs`). The first-run setup's
  mic check (`onboarding/MicStep.svelte`) picks one the same way: first in
  the order ("System default" empties it). **Mic quiz** (`mic-quiz`,
  2026-10-29; the Help menu's Microphone help lands here): "Find my best
  microphone" loads `components/MicQuiz.svelte` (`dictation/mic_quiz.rs`),
  whose "Use this one" puts a microphone first in Settings' copy of the
  order (the auto-save saves it). "Chime when recording starts and stops" (the
  volume slider and the switch on one row: `audioFeedbackVolume`,
  `soundEnabled`), "Chime plays on" (`outputDevice`), "Silence other apps while
  you dictate" (`quietOtherApps`; ⓘ: calls are never silenced), "Pause music
  and videos while you dictate" (`pauseMedia`). **While you dictate**: "Live
  preview on the Yap bar" (`streamingPartials`), "Offer what you said after AI
  cleanup" (`asSaidOffer`, off since 2026-10-25: the bar's "Cleaned up · Use what I said",
  `pasting/as_said.rs`), "Cancel and finish buttons"
  (a Select When you point at it / Always / Off → `barDictationButtons`, Off
  by default, a line per choice). **Typing into apps** (`#settings-typing`,
  Advanced's old Output): "Match the text around the cursor"
  (`smartSpacing`), "Spell names the way they're written on screen"
  (`screenNames`, `dictionary/screen_terms.rs`; its line says, with AI cleanup on a
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
  the ten global shortcuts together — Dictate, Rewrite selected text, Ask the
  Voice Assistant, Read aloud, **Use what I said** (`asSaidHotkey`, unbound
  by default; `pasting/as_said.rs`), **Paste last transcript**
  (`pasteLastHotkey`, `shortcut-paste-last`) and **Copy last transcript**
  (`copyLastHotkey`, `shortcut-copy-last`, its hint: out of Win+V history,
  the tray has both) — both unbound by default, `pasting/last.rs` —
  **Quick note** (`quickNoteHotkey`, `shortcut-quick-note`, 2026-10-28,
  unbound by default: the quick-note window, `notes/quick_note.rs`; its hint
  says where the notes go and that the tray has New quick note),
  Meeting notes, **Snip to Chat** (`snipHotkey`,
  Win+Alt+S by default, its hint naming ✂, the tray, Esc and what Windows never
  lets an app capture; `snip.rs`, [snip.md](./snip.md)) — each in keycaps with Change, a
  clash said under the one being set, and **"+ Add another"** (2026-10-25:
  `SettingsHotkey`'s `more`, up to `MORE_SHORTCUTS` 3 more keys or mouse
  buttons for the same action, `cfg.moreHotkeys[target]`, each with ✕;
  checked by the same `validateHotkey` list — one of the same action's is
  "Already used by another of this action's shortcuts" —, paused while
  picked, `onMoreCapturing` → `configure_more_hotkeys`; the first taken
  away moves the next up; the in-page fallback catches every one,
  `actionHotkeys`) — a **Language shortcuts** link row
  (`shortcut-languages`: "None set", the one's name — "English → Spanish" —
  or "N shortcuts") to Dictation → My languages — and a **Transforms** link row
  (`shortcut-transforms`: "None set", the one transform's name, or "N
  shortcuts") to Commands → Transforms, where their shortcuts are set. Their own pages show the same control
  (Read aloud: Voice Assistant → **Read aloud**, `#settings-read-aloud`). (No
  in-page fallback for Use what I said or Paste last: their paste is never
  in a Yap window; Copy last's has one, `copy_last_transcript`.) The
  search finds both new rows ("paste" now puts Paste last transcript first,
  the Dictation rows after it).
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
  `meetings/meeting_audio.rs`): "Keep meeting audio" (`meetingKeepAudio`, on), "Keep
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
  rules in `pasting/as_said.rs`, [pasting.md](./pasting.md)). **Transforms**
  (`#settings-transforms`, `settings/TransformsSettings.svelte` +
  `settings/transforms.js`, `dictation/transforms.rs`; 2026-10-24): "Rewrite the
  selected text in one press, without saying anything. With nothing
  selected, your last dictation, by the rules above." A row per transform
  (`[data-transform=<id>]`): its name ("Translate to French") and one line
  (a built-in's own, or your instruction), Translate's **language** Select
  under its name (`LANGUAGES`; its English name goes into the
  instruction), the shortcut in keycaps (clearable), ↑ / ↓ (the order is
  the tray's menu too), and for your own Edit (the row becomes a form in
  place: Name, "What it does" ≤ 600 characters, Save; Esc cancels,
  Ctrl+Enter saves) and Delete (Yap's confirm dialog: "Its shortcut goes
  with it"); Yap's five keep empty places there so every shortcut lines
  up. **Add a transform** (`[data-setting=transform-add]`) opens the same
  form (no name: the instruction's first words); at 12 a line says that's
  the most. **Run on every dictation** (`transform-every`, 2026-10-25): two
  Selects — None or a transform (`autoTransform`), and In every app / In
  email / … / Everywhere else (`autoTransformKind`, greyed while None) —, a
  line saying what runs where ("Polish runs after cleanup on each dictation
  in email, never in a terminal"), an ⓘ (one more AI call a dictation, Use
  what I said puts back your words); deleting that transform clears it. **Writing samples**
  (`#settings-writing-samples`, `settings/WritingSamples.svelte`,
  `ai/writing_samples.rs`; 2026-10-29): "Paste a few things you wrote
  yourself, so the rewrite shortcut, Commands and transforms like Polish and
  Shorter sound like you." A row per sample (`[data-sample=<id>]`): its first
  two lines, a "Kind of app it suits" Select (Any app / Email / Chat /
  Documents), Edit (the row becomes a form: the text ≤ 1,500 characters with
  a count, the kind; Esc cancels, Ctrl+Enter saves) and Delete (Yap's
  confirm dialog); **Add a writing sample** (`writing-sample-add`) opens the
  same form; at 12, a line says that's the most. A line says they stay on
  this PC and how much a rewrite carries (about 200 tokens to a model on this
  PC, 500 to a cloud one), never with Fix grammar only, Translate or Make it
  a prompt, and never with AI cleanup. Not config.json: the page reads `writing_samples_get` and saves each change
  itself (`writing_samples_set`; a failed save is `saveErrors.js`' toast). **Model and prompt**: a link row with the model in use →
  Language Models → Commands (its line: with "Answer to its name" off, the
  rewrite shortcut and transforms use the cleanup model).
- **Voice Assistant** (`voice`): "Ask Yap something by voice and hear the
  answer", the example note (names the shortcut once set) linking to
  Commands. **Assistant shortcut** (`#settings-assistant-shortcut`,
  `assistantHotkey`, clearable; a quiet line while none is set; its line
  follows the recording mode), "Show the
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
  `KokoroOffer.svelte`), How much to read aloud (`spokenLength`, a Select:
  A sentence or two / About 30 seconds / About a minute / About 2 minutes /
  The whole answer; greyed out with Speak replies off), When to speak (`speakWhen`), Talk back hands-free
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
  (a **Languages** link row — the languages you speak, "French, English", or Auto-detect — to Dictation → My languages, where the main shortcut's language is now; Translate to English) and **Performance**: **Speed**, where the
  model runs (`settings/RunsOn.svelte`, `dictation/stt_speed.rs`): the line says what
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
  (`postProcessEnabled`), then **Strength** (`cleanup-strength`, 2026-10-25,
  `ai/cleanup_strength.rs`): a Segmented None / Light / Medium / High (aria
  "Cleanup strength"), a line per strength, an ⓘ that it's one setting for
  every app and AI prompts and terminal commands keep your words; None is
  the switch off (picking it turns cleanup off, a strength turns it back on
  and saves `cleanupStrength`; the switch off and on keeps the strength; a
  config from before shows Medium); **off, one line** ("Turn on cleanup to pick a
  model…") instead of a screen of greyed choices. On: where it runs (Cloud
  providers / Local / Self-hosted — the same names on every tab;
  `ppMode`/`cloudProvider` are UI-only and resolve to `ppProvider`), the key
  banner when one is missing, the provider pills, the masked API key with "Get
  your API key", the model list (`ModelPicker.svelte`, the curated list merged
  with the provider's live one, `modelCatalog.js`), a self-hosted or custom
  endpoint's fields (`EndpointModels.svelte` suggests its server's models),
  Local's on-device model browser (Recommended for this PC, the one upgrade
  offer, the slow-on-this-PC line, the Context control `LocalContext.svelte`,
  the curated cards by family with Download / Use / Uninstall…, and the
  person's own GGUFs as cards too — Use / Uninstall…, 2026-10-27, where a
  dropdown was —; Uninstall… from a card's right-click menu or its trash
  button, off while the model is in use with what uses it ("Dictation
  cleanup uses it. Switch that to another model first", `inUseNote`), Yap's confirm with the file's size, `lib/core/localModels.js` →
  `local_llm_uninstall`: ai-models.md `ai/local_llm/uninstall.rs`), "Hide
  the model's thinking" for reasoning models.
  **Spelling** (`#settings-cleanup-spelling`, `RegionSpelling.svelte`: "Write
  in my region's spelling" → `regionSpelling`, greyed while cleanup is off).
  With cleanup on: **Styles by app** (`#settings-cleanup-styles`,
  `settings/CleanupStyles.svelte` + `lib/settings/cleanupStyles.js`, 2026-10-24;
  `ai/cleanup_styles.rs`): "Match the style to the app" (`cleanupStyles`,
  `cleanup-styles`; off: "every app gets the same cleanup"), then a card per
  kind of app from `cleanup_styles_catalog` (an icon, its name, its apps
  — "Outlook, Gmail, Thunderbird and 10 more" —, its tone as a Segmented
  bound to the kind's profile's `tone`, a line per tone, and a **You say →
  Yap types** example for that tone, Wispr's example cards; the examples are
  what Qwen3.5 4B wrote) and **Apps and instructions** (a `set-more`
  disclosure: the apps as chips with ✕ — a built-in one goes to
  `removedApps`, "Taken out: … put back" —, an add box suggesting apps
  dictated into lately — `addApp` takes it out of every other kind, so
  Discord moves from Personal to Work chat —, and "Extra instructions for
  these apps", added to the main prompt). A note when "Only clean up in
  apps with a rule" is on. The kinds' profiles ride in `cleanupProfiles`
  (`cleanConfig` keeps their fields); Smart routing's Profiles list shows only
  the person's own (`ownProfiles`), and an app rule can name a style
  ("Style: Email"). **Prompt Studio** (`PromptStudio.svelte`: View shows the
  first four lines and "Show the whole prompt", Customize — preset + body +
  Save/Reset —, Test), **Smart routing** (only clean up in apps with a rule;
  profiles with a per-profile model; app rules — "A rule wins over Styles by
  app"; Yap's own Selects), **Usage
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
  to contain go there with it —, Chat and the Voice Assistant, screenshots (kept on this PC; one goes to the AI model only with a question you send), About you, memory, meeting search (searched on this PC, by meaning once its model is here), web search,
  spoken replies, AI apps, calendar, account), each row linking to its
  setting; **Text before the cursor** (2026-10-25, `ai/cleanup_context.rs`:
  "Not used: AI cleanup is off", "Not read: …" with Match the text around
  the cursor off, "…goes with it to the cleanup model on this PC, never
  sent", "Read on this PC for each dictation, never sent", or "A sentence or
  two before the cursor goes to Groq with each dictation, for cleanup"),
  **Transform on every dictation** while one is picked (where the Commands
  model sends it, cleanup on or off), and
  under the table **"Send the text before the cursor to cleanup"**
  (`cursor-context`, `cursorContextCloud`, off; its line says what goes
  where; greyed when cleanup runs on this PC, which gets it anyway). **Network connections** (`#settings-connections`): a link row
  ("12 today" · "3 left this PC today, 9 stayed on it") one level down to
  `settings/ConnectionsView.svelte` (`privacy#connections-list` opens it
  directly; the network receipts, [privacy-receipts.md](./privacy-receipts.md)).
  **History** (`#settings-history`): "Keep dictation history"
  (`historyEnabled`) and Clear history… (asks first; the count from
  `get_stats`, re-read on `yap-history-changed`; it clears the kept audio
  too); the list itself is Home's feed. "Keep recent dictation audio"
  (`keepRecentAudio`, `dictation/recent_audio.rs`: the last 24 hours, on this PC, to run
  a dictation again; greyed while history is off) and Clear recent audio…
  ("3 dictations · 1.2 MB" from `recent_audio_status`; asks first; history
  stays). **Meetings** (`#settings-meeting-transcripts`, 2026-10-25): "Delete
  meeting transcripts after" (`meetingTranscriptDays`: Never / 30 / 90 / 365
  days, `meeting-transcripts`; notes/retention.rs) — the meeting's audio goes
  with it, and Settings → Meetings' "Keep it for" says so when it's sooner;
  "What leaves this PC"'s meeting audio row shows the sooner of the two.
  **Search my meetings by meaning** (`#settings-meeting-search`,
  `settings/MeetingSearchSettings.svelte`; [search.md](./search.md)): the
  switch (`searchByMeaning`, disabled until the model is here), Search model
  (Download with its size — the engine's too when it isn't here — and its
  progress; then name · size · where it runs, the licence link) and Search
  index (passages · notes · size, or what it's doing; Rebuild, Delete… asked
  first), from `search_status` and `yap-search-status`. **Chat screenshots** (`#settings-chat-images`): "Keep chat
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
