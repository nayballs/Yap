# Meetings

## Call detection

When a call starts in Teams, Zoom, Google Meet, a Slack huddle, Discord, Webex
or another call app, Yap asks once, "Teams call detected. Record notes?". It
records only after a click, into a new meeting note. When the call ends while Yap is
recording it, Yap asks "Stop and summarise?". Code: `src-tauri/src/meeting_detect.rs`
(+ `src/lib/meetingDetect.svelte.js` for the in-app prompts,
`src-tauri/src/win_toast.rs` for the Windows notifications).

### How a call is detected

Windows keeps a per-app record of microphone use, the same data behind the
taskbar's microphone indicator and Settings → Privacy & security → Microphone:

```
HKCU\Software\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone
  MSTeams_8wekyb3d8bbwe\                     packaged apps, by package family name
  NonPackaged\C:#Users#…#Discord.exe\        desktop apps, by exe path ('\' spelled '#')
    LastUsedTimeStart, LastUsedTimeStop      FILETIMEs; Stop == 0 → on the mic right now
```

- **Only known call apps count** (`APPS` in `meeting_detect.rs`): Teams (new
  and classic), Zoom, Webex, Slack, Discord, GoTo Meeting, WhatsApp, Signal,
  Telegram. Only their subkeys' values are read.
- **Browsers** (Chrome, Edge, Firefox, Brave, Opera, Vivaldi, Arc, …) count when
  they hold the mic **and** one of their windows' titles names a meeting:
  "Meet - abc-defg-hij", "… | Microsoft Teams", "Zoom Meeting", Webex, Slack,
  Discord, Whereby, Jitsi. Once named, the call stays with that browser while it
  holds the mic, so switching tabs doesn't end it. Titles are read only while a
  browser holds the mic and no call is named yet, and are never logged or stored.
- **Yap itself never counts.** Yap holds the mic all the time for its pre-roll,
  plus dictation and the meeting recorder, and `yap.exe` isn't a call app. On
  the development PC the only entry "on the mic" was the installed Yap.
- An entry left "on the mic" from before the last restart is ignored (a
  `LastUsedTimeStart` older than boot means an app died mid-call).

Why not OpenWhispr's approach: OpenWhispr runs a helper exe
(`resources/windows-mic-listener.c`) on WASAPI capture sessions and prompts
for **any** process using the mic except its own. On a typical PC the mic
users include dictation apps, games, OBS and voice assistants. The consent
store names the app (a package or an exe path, not just a PID) and needs no
helper process.

### Cost

A thread blocks in `RegNotifyChangeKeyValue` on the `microphone` key (subtree,
value and subkey changes), so nothing runs until some app takes or releases
the mic. A scan then reads a handful of registry values. While a call is
starting, live or ending, the detector also rescans every 2 s. Otherwise it
rescans every 60 s as a safety net, or every 5 s if the watch can't start. It
is fully local: no network, no audio, no process list.

### Timing

| | Yap | OpenWhispr |
|---|---|---|
| Call starts | app on the mic for **5 s** (**20 s** for chat apps, whose voice messages use the mic too) | mic busy ≥ 2 s (event-driven) / 2 polls 15 s apart |
| Call ends | app off the mic for **15 s** (a headset switch or a rejoin is a gap, not an ending) | — (re-arms after 60 s quiet) |
| After "Not now" | that app's calls stay quiet for **5 min** | all detections quiet for 5 min |
| While dictating | the prompt waits until the dictation finishes, then **2.5 s** | queued, flushed 2.5 s after |
| Already recording | no start prompt | suppressed in meeting mode |

### Asking

- **Once per call**, and never while Yap already records a meeting.
- Main window on screen: a sticky in-app toast ("Teams call detected", "Record
  notes? Let people know you're taking notes.", **Record notes** / **Not
  now**; its ✕ means Not now).
- Main window hidden, minimized or behind the call app: a silent Windows
  notification with Yap's logo and the same buttons. If the window is open but
  not focused, both surfaces show; answering one withdraws the other.
  Focusing the window moves a pending prompt into it.
- The call ending before anyone answers withdraws the prompt.
- Windows notifications follow Do Not Disturb and Yap's notification switch.
  Portable Yap and test runs post none, so their prompts show in the window.

### Record notes

Creates a note through the notes API (`notes::create` + `notes::mark_meeting`):

- title **"Teams call · 5 Oct, 14:30"** (local time),
- folder **Meetings**, `noteType` **meeting**, `source` **meeting**,

then starts the existing recorder (`meeting_start`: mic = "You", system audio =
"Them"). With the window visible the note opens in Notes. With it hidden, a
Windows notification says "Taking notes on your Teams call" and offers **Open
note**. If recording can't start (no microphone, no speakers), the note is
deleted and the prompt says why.

### When the call ends

If Yap is recording that call, it asks "Teams call ended. Stop recording and
summarise your notes?" with **Stop and summarise** / **Keep recording**. "That
call" means the recording its prompt started, or one started during the call.
**Stop and summarise** opens the note and stops it from the Notes view, so the
usual Meeting Notes summary runs exactly as after a manual stop. If the page
hasn't stopped it after 8 s, Yap stops it directly. A call that starts while
Yap still records the note "Record notes" started (a rejoin, or after **Keep
recording**) carries that recording on. If the same app rejoins while this
prompt is up, the prompt is withdrawn, and the call's own end asks again. A
recording started by hand before the call, such as an in-person meeting, is
never offered for stopping just because a call ended.

Yap asks rather than auto-stopping, following OpenWhispr: its engine never
ends a meeting recording by itself, and its calendar "meeting ended" event
isn't acted on. The mic signal also can't tell a finished call from one
moved to a phone, a breakout room or a dropped connection that's about to
rejoin. Auto-stopping on a guess would cut meetings short and summarise half a
transcript. The person decides both ends: nothing records without a click,
and nothing stops without one.

### Setting

Settings → General → Meetings: **Detect calls and offer to take notes**
(`meetingDetection`, default **on**, like OpenWhispr's
`notifyMeetingDetection`). It only reads Windows' own record of mic use,
locally, and only ever asks, and the prompt is how people find out notes
exist. An always-visible line under it reminds people of consent: "Recording a call? Let
people know you're taking notes." Switching it off withdraws any prompt
(a recording in progress carries on) and stops all detection work.

### Testing

- Unit tests (`cargo test --lib meeting_detect`): consent-store key names → apps,
  dictation apps/games/Yap never matching, browser titles, the stale-entry rule,
  the start/end debounce, note titles, prompt wording, and the notification XML
  (built into a WinRT toast, never shown).
- Read-only check of this PC's microphone record:
  `cargo test --lib meeting_detect::tests::this_machine -- --ignored --nocapture`
  (prints exe/package names and recognised meetings only, never window titles).
- e2e (`npm run test:app`): `e2e/meeting-detect.spec.js` and
  `e2e/meeting-detect-no-mic.spec.js` drive the flow through the debug-only
  `meeting_detect_simulate { appId, active }` hook (no debounce; test mode reads no
  registry). They cover the prompt, Not now, a prompt withdrawn with its call, Record
  notes (with devices: the note and the recording; without: the error and no
  note left behind), Stop and summarise, and the Settings toggle.

### Limits

- Apps not in `APPS`/browser titles aren't detected (record manually from a note).
- Exe names for Webex and GoTo installs vary by version. Only Teams, Discord
  and a browser have been seen in the consent store on the development PC.
- A meeting tab in a browser is named from the windows' titles, i.e. the
  active tab of each window, so a call started in a background tab is named
  once its tab is shown.
- Windows only. Other platforms build but don't detect.

Sources: OpenWhispr `src/helpers/meetingDetectionEngine.js`,
`audioActivityDetector.js`, `meetingProcessDetector.js`,
`resources/windows-mic-listener.c`, `src/components/MeetingNotificationOverlay.tsx`,
`src/stores/settingsStore.ts` (`notifyMeetingDetection` default);
Microsoft, [RegNotifyChangeKeyValue](https://learn.microsoft.com/en-us/windows/win32/api/winreg/nf-winreg-regnotifychangekeyvalue);
the same consent-store technique in
[automattermostatus #17](https://gitlab.com/matclab/automattermostatus/-/issues/17)
(Teams' packaged entry sits directly under `microphone`).
