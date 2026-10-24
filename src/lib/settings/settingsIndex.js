// Settings search (2026-10-22 review #3; Claude's search box at the top of
// its nav, OpenWhispr's command search): a static index of every setting,
// because only the open page is mounted. Each entry names the page, the
// section heading, the label and line the row shows, extra words people
// search with, and `anchor`, the row's [data-setting] on that page (or a
// group's id without "settings-"); `fallback`, the row to land on when that
// one only shows with something switched on (Prompt Studio, with cleanup
// on). `scope` opens one of Language Models' tabs first. `toggle` = a plain
// on/off config field the result can switch in place.
//
// e2e/settings-search.spec.js opens every entry and checks its row is
// there, so the index can't drift from the pages.
import { NAV_GROUPS, PAGE_LABELS } from './nav.js';

/** @typedef {{ page: string, section: string, label: string, desc?: string, keywords?: string, anchor: string, fallback?: string, scope?: string, toggle?: string }} Entry */

/** @type {Entry[]} */
export const SETTINGS_INDEX = [
  // ---- General ----
  { page: 'general', section: 'Language and region', label: 'Region', desc: 'Prices, units and spelling follow your country', keywords: 'country currency units language location locale time zone', anchor: 'region' },
  { page: 'general', section: 'About you', label: 'What should Yap call you?', desc: 'Your first name, for the AI', keywords: 'name about you profile', anchor: 'about-name' },
  { page: 'general', section: 'About you', label: 'Anything the AI should know about you', desc: 'A line or two the AI keeps in mind', keywords: 'about me personal profile context custom instructions', anchor: 'about-me' },
  { page: 'general', section: 'Startup and updates', label: 'Start Yap when you sign in', desc: 'Yap waits in the tray, ready for your hotkey', keywords: 'login startup boot windows autostart launch', anchor: 'autostart' },
  { page: 'general', section: 'Startup and updates', label: 'Check for updates automatically', desc: 'Downloads new versions in the background', keywords: 'update upgrade version auto-update nightly', anchor: 'update-checks', toggle: 'updateChecksEnabled' },
  { page: 'general', section: 'Appearance', label: 'Theme', desc: 'Light or dark, or follow Windows', keywords: 'dark mode light mode night theme colours colors appearance system windows black', anchor: 'theme' },
  { page: 'general', section: 'Appearance', label: 'Reading font', desc: 'For chats, notes and meeting notes', keywords: 'font typeface serif sans dyslexic opendyslexic appearance', anchor: 'reading-font' },
  { page: 'general', section: 'Appearance', label: 'Text size', desc: 'For chats, notes and meeting notes', keywords: 'font size larger smaller zoom appearance', anchor: 'text-size' },
  { page: 'general', section: 'Appearance', label: 'Motion', desc: 'Reduce sliding, fading and pulsing', keywords: 'animation reduced motion accessibility appearance', anchor: 'motion' },
  { page: 'general', section: 'Yap bar', label: 'Show the Yap bar', desc: 'A small pill above the taskbar', keywords: 'bar pill overlay taskbar flow bar', anchor: 'bar' },
  { page: 'general', section: 'Yap bar', label: 'Hide in fullscreen apps', desc: 'Games and presentations stay clear', keywords: 'bar games fullscreen presentation video', anchor: 'bar-fullscreen', toggle: 'barHideFullscreen' },
  { page: 'general', section: 'Yap bar', label: 'Colours', desc: "The Yap bar's look: match Yap, light or dark", keywords: 'bar theme dark light colors match app', anchor: 'bar-colours' },
  { page: 'general', section: 'Yap bar', label: 'Position', desc: 'Where the bar and the dictation overlay sit', keywords: 'bar overlay top bottom placement', anchor: 'bar-position' },
  { page: 'general', section: 'Notifications', label: 'Show tips', desc: "Short tips, like push-to-talk's double-tap", keywords: 'tips hints coaching notifications nudge', anchor: 'tips', toggle: 'showTips' },

  // ---- Dictation ----
  { page: 'dictation', section: 'Start and stop', label: 'Hotkey', desc: 'Press it in any app to start and stop dictating', keywords: 'shortcut key keyboard f9 dictation start stop copilot key', anchor: 'hotkey' },
  { page: 'dictation', section: 'Start and stop', label: 'Recording mode', desc: 'Toggle, or hold the key while you talk', keywords: 'push-to-talk push to talk toggle hold hands-free double-tap', anchor: 'recording-mode' },
  { page: 'dictation', section: 'Microphone and sound', label: 'Microphone', desc: 'Changes apply straight away', keywords: 'mic input device audio headset', anchor: 'microphone' },
  { page: 'dictation', section: 'Microphone and sound', label: 'Keep the microphone ready', desc: 'Dictation starts instantly and catches your first word', keywords: 'mic microphone always open on in use privacy indicator bluetooth headset call quality hfp instant start first word pre-roll', anchor: 'keep-mic-ready', toggle: 'keepMicReady' },
  { page: 'dictation', section: 'Microphone and sound', label: 'Chime when recording starts and stops', desc: 'A sound, and its volume', keywords: 'sound cue beep volume chime audio feedback', anchor: 'chime', toggle: 'soundEnabled' },
  { page: 'dictation', section: 'Microphone and sound', label: 'Chime plays on', desc: 'The speakers or headset for the chime', keywords: 'output device speakers headset audio', anchor: 'chime-output' },
  { page: 'dictation', section: 'Microphone and sound', label: 'Silence other apps while you dictate', desc: 'Music, videos and games go quiet', keywords: 'mute quiet volume music sound', anchor: 'quiet-other-apps', toggle: 'quietOtherApps' },
  { page: 'dictation', section: 'Microphone and sound', label: 'Pause music and videos while you dictate', desc: "What's playing pauses, then plays again", keywords: 'media spotify youtube pause', anchor: 'pause-media', toggle: 'pauseMedia' },
  { page: 'dictation', section: 'While you dictate', label: 'Live preview on the Yap bar', desc: 'Your words show as you speak', keywords: 'partials streaming live transcription preview', anchor: 'live-preview', toggle: 'streamingPartials' },
  { page: 'dictation', section: 'While you dictate', label: 'Offer what you said after AI cleanup', desc: 'Use what I said on the Yap bar for a few seconds', keywords: 'raw transcript revert undo cleanup as said original words restore', anchor: 'as-said-offer', toggle: 'asSaidOffer' },
  { page: 'dictation', section: 'While you dictate', label: 'Cancel and finish buttons', desc: '✕ to cancel and ✓ to paste on the bar', keywords: 'cancel finish buttons bar stop', anchor: 'dictation-buttons' },
  { page: 'dictation', section: 'Typing into apps', label: 'Match the text around the cursor', desc: "Spaces, capitals and full stops fit where you're typing", keywords: 'smart spacing capitals paste pasting cursor', anchor: 'smart-spacing', toggle: 'smartSpacing' },
  { page: 'dictation', section: 'Typing into apps', label: "Spell names the way they're written on screen", desc: 'Names and terms in the window come out spelled as shown', keywords: 'names spelling screen window context proper nouns jargon terms accuracy deep context', anchor: 'screen-names', toggle: 'screenNames' },
  { page: 'dictation', section: 'Typing into apps', label: 'Leave the full stop off chat messages', desc: 'In Slack, Teams or WhatsApp, a short message ends without one', keywords: 'period full stop chat slack teams whatsapp discord', anchor: 'chat-full-stop', toggle: 'chatNoFullStop' },
  { page: 'dictation', section: 'Typing into apps', label: 'Append trailing space', desc: 'A space after each dictation', keywords: 'space paste pasting', anchor: 'trailing-space', toggle: 'appendTrailingSpace' },
  { page: 'dictation', section: 'Typing into apps', label: 'Press Enter after pasting', desc: 'Sends the message for you', keywords: 'auto-submit auto submit enter send paste', anchor: 'auto-submit', toggle: 'autoSubmit' },
  { page: 'dictation', section: 'Typing into apps', label: 'Say “press enter” to send', desc: 'Yap leaves the words out and presses the key after pasting', keywords: 'press enter voice command send return', anchor: 'voice-enter', toggle: 'voiceEnter' },
  { page: 'dictation', section: 'Typing into apps', label: 'Say “snip” to take a screenshot', desc: 'A dictation of just “snip” starts Snip to Chat', keywords: 'snip screenshot screen capture voice command picture image', anchor: 'snip-voice', toggle: 'snipVoice' },
  { page: 'dictation', section: 'Typing into apps', label: 'Send key', desc: 'Enter, Ctrl + Enter or Shift + Enter', keywords: 'auto-submit key enter ctrl shift', anchor: 'send-key' },
  { page: 'dictation', section: 'Typing into apps', label: 'Safer pasting into terminals', desc: 'One line, and Enter only when you say “press enter”', keywords: 'terminal powershell command line claude code codex paste pasting', anchor: 'terminal-mode', toggle: 'terminalMode' },
  { page: 'dictation', section: 'Typing into apps', label: 'Restore clipboard after paste', desc: "Puts back what you'd copied. Dictations never show up in Win+V history", keywords: 'clipboard copy paste win+v history', anchor: 'restore-clipboard', toggle: 'restoreClipboard' },
  { page: 'dictation', section: 'Typing into apps', label: 'Dictionary', desc: 'Words Yap should spell your way, and snippets', keywords: 'corrections snippets words spelling vocabulary jargon', anchor: 'dictionary' },

  // ---- Shortcuts ----
  { page: 'shortcuts', section: 'Global shortcuts', label: 'Dictate', desc: 'Start and stop dictating where you are typing', keywords: 'shortcut shortcuts hotkey keyboard dictation key', anchor: 'shortcut-dictation' },
  { page: 'shortcuts', section: 'Global shortcuts', label: 'Rewrite selected text', desc: 'Select text, press it and say how to change it', keywords: 'shortcut shortcuts hotkey edit rewrite commands', anchor: 'shortcut-rewrite' },
  { page: 'shortcuts', section: 'Global shortcuts', label: 'Ask the Voice Assistant', desc: 'Ask out loud from any app', keywords: 'shortcut shortcuts hotkey assistant voice', anchor: 'shortcut-assistant' },
  { page: 'shortcuts', section: 'Global shortcuts', label: 'Read aloud', desc: 'Select text in any app and press it to hear it', keywords: 'shortcut shortcuts hotkey read aloud speak selection text to speech tts dyslexia reading', anchor: 'shortcut-read' },
  { page: 'shortcuts', section: 'Global shortcuts', label: 'Use what I said', desc: 'Swap what Yap pasted for your words before AI cleanup', keywords: 'shortcut shortcuts hotkey raw transcript revert undo cleanup as said', anchor: 'shortcut-as-said' },
  { page: 'shortcuts', section: 'Global shortcuts', label: 'Meeting notes', desc: 'Take notes on a call, press again to stop', keywords: 'shortcut shortcuts hotkey meeting win alt m', anchor: 'shortcut-meeting' },
  { page: 'shortcuts', section: 'Global shortcuts', label: 'Snip to Chat', desc: 'Drag a box over any screen, into Chat', keywords: 'shortcut shortcuts hotkey snip snipping screenshot screen capture picture image win alt s', anchor: 'shortcut-snip' },

  // ---- Meetings ----
  { page: 'meetings', section: 'Call detection', label: 'Detect calls and offer to take notes', desc: 'Nothing records until you click Record notes', keywords: 'call detection teams zoom meet calls notetaker', anchor: 'call-detection', toggle: 'meetingDetection' },
  { page: 'meetings', section: 'Call detection', label: 'Ask about calls in', desc: 'Which call apps Yap asks about', keywords: 'apps teams zoom google meet slack huddle discord webex whatsapp signal telegram goto whereby jitsi', anchor: 'call-apps' },
  { page: 'meetings', section: 'Call detection', label: 'How Yap asks', desc: 'A pop-up, or quietly in the notification centre', keywords: 'pop-up popup quietly notification prompt', anchor: 'call-prompt-style' },
  { page: 'meetings', section: 'Call detection', label: 'Start notes automatically after 10 seconds', desc: 'The prompt counts down, and you can cancel', keywords: 'auto start countdown automatic', anchor: 'auto-start', toggle: 'meetingAutoStart' },
  { page: 'meetings', section: 'While a meeting records', label: 'Maximum length', desc: 'Yap stops and writes your action plan', keywords: 'maximum recording length limit hours stop', anchor: 'max-length' },
  { page: 'meetings', section: 'While a meeting records', label: 'When a call ends', desc: 'Ask, or stop and summarise', keywords: 'call ends stop summarise summary', anchor: 'call-end' },
  { page: 'meetings', section: 'While a meeting records', label: 'End notes when the meeting goes quiet', desc: 'After 10 minutes with nobody speaking', keywords: 'quiet silence idle auto stop', anchor: 'end-when-quiet', toggle: 'meetingEndWhenQuiet' },
  { page: 'meetings', section: 'While a meeting records', label: 'Hide Yap from screen sharing', desc: 'The notepad and the bar stay out of every capture', keywords: 'screen share sharing capture screenshot recording obs privacy', anchor: 'hide-from-capture', toggle: 'meetingHideFromCapture' },
  { page: 'meetings', section: 'While a meeting records', label: 'Tell speakers apart in meetings', desc: 'Speaker 1, 2, 3… for each voice', keywords: 'speakers diarization voices who said', anchor: 'speakers' },
  { page: 'meetings', section: 'While a meeting records', label: "Suggest who's speaking", desc: 'Names for the voices, for you to confirm', keywords: 'speaker names who said attendees identify', anchor: 'speaker-names', toggle: 'meetingSpeakerNames' },
  { page: 'meetings', section: 'Meeting audio', label: 'Keep meeting audio', desc: 'To transcribe a meeting again and play its lines', keywords: 'audio recording recordings keep save play playback transcribe again retranscribe crash recover', anchor: 'meeting-audio', toggle: 'meetingKeepAudio' },
  { page: 'meetings', section: 'Meeting audio', label: 'Keep it for', desc: 'How many days meeting audio stays', keywords: 'audio retention days delete old recordings storage disk space', anchor: 'meeting-audio-days' },
  { page: 'meetings', section: 'Meeting audio', label: 'Delete all meeting audio', desc: 'Every meeting’s audio goes from this PC', keywords: 'audio delete clear recordings storage disk space privacy', anchor: 'clear-meeting-audio' },
  { page: 'meetings', section: 'Notepad', label: 'Open the notepad when a meeting starts', desc: 'Your notes, the live transcript and the summary', keywords: 'notepad notes window', anchor: 'open-notepad', toggle: 'meetingOpenNotepad' },
  { page: 'meetings', section: 'Notepad', label: 'Split the screen when joining', desc: "Moves the call's window left of the notepad", keywords: 'split screen window layout', anchor: 'split-screen' },
  { page: 'meetings', section: 'Notepad', label: 'Show live transcript', desc: 'The words appear in the notepad as people talk', keywords: 'live transcript notepad', anchor: 'live-transcript', toggle: 'meetingLiveTranscript' },
  { page: 'meetings', section: 'Notepad', label: 'Meeting shortcut', desc: 'Take notes on a call, and press again to stop', keywords: 'shortcut hotkey win alt m meeting', anchor: 'meeting-shortcut' },
  { page: 'meetings', section: 'Calendar', label: 'Remind me before scheduled meetings', desc: 'A card with Join & take notes', keywords: 'calendar reminder notify notification before meeting', anchor: 'meeting-reminder' },
  { page: 'meetings', section: 'Calendar', label: 'Prepare a brief before meetings', desc: 'Last time with these people, open items and the agenda', keywords: 'brief briefing prepare prep before meeting last time open items action items agenda calendar', anchor: 'meeting-brief', toggle: 'meetingBrief' },
  { page: 'meetings', section: 'Calendar', label: 'Calendars', desc: 'Meeting names and attendees', keywords: 'calendar google outlook ical connect', anchor: 'calendars' },

  // ---- Commands ----
  { page: 'commands', section: 'Say its name', label: 'Answer to its name in any dictation', desc: '"Hey Yap, …" and the AI does what you ask', keywords: 'commands wake word hey yap agent ai', anchor: 'wake-word' },
  { page: 'commands', section: 'Say its name', label: 'Agent name', desc: 'The name Commands answer to', keywords: 'name jarvis wake word agent', anchor: 'agent-name' },
  { page: 'commands', section: 'Rewrite selected text', label: 'Rewrite shortcut', desc: 'Select text, press it and say how to change it', keywords: 'edit rewrite shortcut hotkey selection last dictation transform formal shorter translate', anchor: 'rewrite-shortcut' },
  { page: 'commands', section: 'Rewrite selected text', label: 'Rewrite your last dictation when nothing is selected', desc: 'Within a minute, in the same field', keywords: 'rewrite last dictation nothing selected transform formal shorter translate undo edit previous', anchor: 'rewrite-last', toggle: 'rewriteLastDictation' },
  { page: 'commands', section: 'Model and prompt', label: 'Commands model and prompt', desc: 'Language Models → Commands', keywords: 'model prompt language model ai', anchor: 'commands-model' },

  // ---- Voice Assistant ----
  { page: 'voice', section: 'Assistant shortcut', label: 'Assistant shortcut', desc: 'Ask out loud from any app', keywords: 'shortcut hotkey assistant voice ask', anchor: 'assistant-shortcut' },
  { page: 'voice', section: 'Assistant shortcut', label: 'Stop listening when I pause', desc: 'A pause after your question sends it', keywords: 'pause silence stop listening automatically auto end hands-free quiet voice activity', anchor: 'stop-on-pause', toggle: 'assistantStopOnPause' },
  { page: 'voice', section: 'Assistant shortcut', label: 'Show the answer on the Yap bar', desc: 'Your question and its answer above the bar', keywords: 'answer card bar', anchor: 'answer-card', toggle: 'assistantAnswerCard' },
  { page: 'voice', section: 'Read aloud', label: 'Read aloud shortcut', desc: 'Select text in any app and hear it, word by word on the Yap bar', keywords: 'read aloud shortcut hotkey selection speak text to speech tts dyslexia reading karaoke', anchor: 'read-shortcut' },
  { page: 'voice', section: 'Opening apps', label: 'Let Yap open apps when you ask', desc: '“Open Spotify”', keywords: 'open apps launch start programs', anchor: 'open-apps', toggle: 'openApps' },
  { page: 'voice', section: 'Opening apps', label: 'Nicknames', desc: 'Say a nickname, and Yap opens that app', keywords: 'open apps alias nickname', anchor: 'app-nicknames' },
  { page: 'voice', section: 'Web search', label: 'Look things up on the web', desc: 'Chat and the Voice Assistant search when a question needs it', keywords: 'web search internet google exa tavily wikipedia', anchor: 'web-search', toggle: 'webSearch' },
  { page: 'voice', section: 'Spoken replies in Chat', label: 'Speak replies', desc: 'Yap reads its answers aloud', keywords: 'read aloud tts text to speech voice replies spoken', anchor: 'speak-replies' },
  { page: 'voice', section: 'Spoken replies in Chat', label: 'When to speak', desc: 'When you asked by voice, or always', keywords: 'read aloud spoken replies', anchor: 'speak-when' },
  { page: 'voice', section: 'Spoken replies in Chat', label: 'Talk back hands-free', desc: 'A dictated question is sent and the answer read out', keywords: 'hands-free conversation talk back', anchor: 'talk-back', toggle: 'talkBackHandsFree' },
  { page: 'voice', section: 'Voice', label: 'Speaks with', desc: 'A voice on this PC, or ElevenLabs', keywords: 'elevenlabs voice tts cloud voice', anchor: 'voice-provider' },
  { page: 'voice', section: 'Voice', label: 'Voice', desc: "Kokoro's natural voices, or Windows' own", keywords: 'kokoro windows voices bella voice picker preview', anchor: 'voice' },
  { page: 'voice', section: 'Voice', label: 'Speed', desc: 'How fast Yap speaks', keywords: 'voice speed rate', anchor: 'voice-speed' },
  { page: 'voice', section: 'Voice', label: 'Volume', desc: 'How loud Yap speaks', keywords: 'voice volume loudness', anchor: 'voice-volume' },
  { page: 'voice', section: 'Model', label: 'Assistant model', desc: "The same as Chat's", keywords: 'model chat language model ai', anchor: 'assistant-model' },

  // Memory (MemorySection.svelte)
  { page: 'memory', section: 'What Yap learns from', label: 'Search and reference past chats', desc: 'Yap looks through earlier chats when you mention one', keywords: 'memory chats history recall remember conversations', anchor: 'memory-search-chats', toggle: 'memorySearchChats' },
  { page: 'memory', section: 'What Yap learns from', label: 'Learn from my chats', desc: 'Lasting facts about you, kept when a chat goes quiet', keywords: 'memory learn remember facts personal chats', anchor: 'memory-learn-chats', toggle: 'memoryLearnChats' },
  { page: 'memory', section: 'What Yap learns from', label: 'Learn from meetings', desc: "Attendees and projects, from meetings' action plans", keywords: 'memory learn meetings attendees colleagues', anchor: 'memory-learn-meetings', toggle: 'memoryLearnMeetings' },
  { page: 'memory', section: 'What Yap learns from', label: 'Include sensitive topics in memory', desc: 'Health, religion, politics and the like', keywords: 'memory sensitive private health religion politics privacy', anchor: 'memory-sensitive', toggle: 'memorySensitive' },
  { page: 'memory', section: 'You', label: 'Profile', desc: 'Who you are: where you live, your work', keywords: 'memory about me profile personal facts job', anchor: 'memory-profile' },
  { page: 'memory', section: 'You', label: 'Preferences', desc: "How you want Yap's AI to answer you", keywords: 'memory preferences style answers tone custom instructions', anchor: 'memory-preferences' },
  { page: 'memory', section: 'People', label: 'People', desc: 'The people you mention, and who they are to you', keywords: 'memory people family friends colleagues names contacts', anchor: 'memory-people' },
  { page: 'memory', section: 'Projects', label: 'Projects', desc: "What you're working on", keywords: 'memory projects work', anchor: 'memory-projects' },
  { page: 'memory', section: 'Topics', label: 'Topics', desc: 'Games, hobbies and things you come back to', keywords: 'memory topics interests hobbies games', anchor: 'memory-topics' },
  { page: 'memory', section: 'Tell Yap what to change', label: 'Tell Yap what to change', desc: 'Changes in your words, checked before they are kept', keywords: 'memory edit forget correct remember update', anchor: 'memory-tell' },
  { page: 'memory', section: 'Import, export and clear', label: 'Import memory from other AI providers', desc: 'What ChatGPT, Claude or Gemini remembers', keywords: 'memory import chatgpt claude gemini bring in', anchor: 'memory-import' },
  { page: 'memory', section: 'Import, export and clear', label: 'Export memory', desc: 'A .zip of Markdown, a file per section', keywords: 'memory export download backup markdown zip', anchor: 'memory-export' },
  { page: 'memory', section: 'Import, export and clear', label: 'Clear all memory', desc: 'Forget everything Yap remembers', keywords: 'memory clear delete forget erase reset wipe', anchor: 'memory-clear' },

  // ---- Speech-to-Text ----
  { page: 'models', section: 'Speech model', label: 'Speech model', desc: 'The model that turns your voice into text', keywords: 'speech to text transcription model parakeet whisper canary moonshine download', anchor: 'stt-model' },
  { page: 'models', section: 'Speakers in meetings', label: 'Speaker models', desc: 'For telling speakers apart in meetings', keywords: 'diarization speakers model', anchor: 'speakers' },
  { page: 'models', section: 'Language', label: 'Language', desc: 'The language you speak', keywords: 'spoken language auto-detect', anchor: 'language' },
  { page: 'models', section: 'Language', label: 'Translate to English', desc: 'Output English whatever you speak', keywords: 'translation translate', anchor: 'translate' },
  { page: 'models', section: 'Performance', label: 'Speed', desc: 'Runs on the graphics card, or the processor when it is clearly faster on this PC', keywords: 'runs on gpu cpu processor graphics card directml vulkan performance speed fast benchmark measure', anchor: 'runs-on' },
  { page: 'models', section: 'Performance', label: 'Unload model when idle', desc: 'Free memory when not dictating', keywords: 'memory ram idle unload performance', anchor: 'unload' },

  // ---- Language Models ----
  { page: 'cleanup', scope: 'cleanup', section: 'Dictation cleanup', label: 'AI cleanup', desc: 'Remove filler words, fix grammar and punctuation', keywords: 'ai cleanup text cleanup grammar filler polish groq openai anthropic gemini api key model local on-device', anchor: 'cleanup-enable', toggle: 'postProcessEnabled' },
  { page: 'cleanup', scope: 'cleanup', section: 'Dictation cleanup', label: 'Smart routing', desc: 'A cleanup style per app, with cleanup on', keywords: 'profiles app rules routing per app', anchor: 'cleanup-enable' },
  { page: 'cleanup', scope: 'cleanup', section: 'Dictation cleanup', label: "Write in my region's spelling", desc: 'British, American or Canadian spelling', keywords: 'spelling british american english colour color', anchor: 'cleanup-spelling' },
  { page: 'cleanup', scope: 'cleanup', section: 'Dictation cleanup', label: 'Prompt Studio', desc: 'View, customise and test the cleanup prompt, with cleanup on', keywords: 'prompt system prompt presets custom email notes slack code', anchor: 'prompt-studio', fallback: 'cleanup-enable' },
  { page: 'cleanup', scope: 'voiceAgent', section: 'Commands', label: 'Commands model', desc: 'The model that carries out Commands', keywords: 'commands agent model prompt', anchor: 'scope-model' },
  { page: 'cleanup', scope: 'noteFormatting', section: 'Note Formatting', label: 'Note formatting model', desc: 'Meeting summaries and note actions', keywords: 'notes meeting summary action plan model', anchor: 'scope-model' },
  { page: 'cleanup', scope: 'chat', section: 'Chat & Voice Assistant', label: 'Chat model', desc: 'The model that answers in Chat and out loud', keywords: 'chat assistant model answers', anchor: 'scope-model' },

  // ---- Connectors ----
  { page: 'connectors', section: 'Connectors', label: 'Google Calendar', desc: 'Reminders for Google meetings', keywords: 'calendar google connect', anchor: 'google-calendar' },
  { page: 'connectors', section: 'Connectors', label: 'Outlook Calendar', desc: 'Reminders for Outlook meetings', keywords: 'calendar outlook microsoft office ics', anchor: 'outlook-calendar' },
  { page: 'connectors', section: 'Connectors', label: 'Other calendar', desc: 'iCloud, Fastmail or any iCal link', keywords: 'calendar icloud fastmail ical ics webcal', anchor: 'other-calendar' },

  // ---- AI apps (MCP) ----
  { page: 'mcp', section: 'AI apps', label: 'Add Yap to your AI apps', desc: 'Claude, ChatGPT, Gemini, Cursor and more read your meetings and notes', keywords: 'mcp claude chatgpt codex gemini cursor claude code vs code windsurf ai apps', anchor: 'mcp-apps' },
  { page: 'mcp', section: 'AI apps', label: 'Let AI apps save notes to Yap', desc: 'A tool for saving notes', keywords: 'mcp write save notes', anchor: 'mcp-writes' },
  { page: 'mcp', section: 'All other apps', label: 'Command and JSON for other apps', desc: 'For any app that runs local MCP servers', keywords: 'mcp json command copy paste other apps', anchor: 'mcp-other' },

  // ---- Privacy & data ----
  { page: 'privacy', section: 'What leaves this PC', label: 'What leaves this PC', desc: 'Feature by feature, as Yap is set up', keywords: 'privacy cloud data sent private', anchor: 'leaves' },
  { page: 'privacy', section: 'Network connections', label: 'Network connections', desc: 'Every connection Yap made in the last 7 days, and why', keywords: 'network connections receipts internet traffic hosts cloud sent received requests online offline', anchor: 'connections' },
  { page: 'privacy', section: 'History', label: 'Keep dictation history', desc: 'Kept only on this PC', keywords: 'history dictations saved stats', anchor: 'history', toggle: 'historyEnabled' },
  { page: 'privacy', section: 'History', label: 'Clear history', desc: 'Removes every saved dictation', keywords: 'delete erase history', anchor: 'clear-history' },
  { page: 'privacy', section: 'History', label: 'Keep recent dictation audio', desc: 'A day of it, to run a dictation again', keywords: 'audio recordings voice run again retry lost failed recent', anchor: 'recent-audio', toggle: 'keepRecentAudio' },
  { page: 'privacy', section: 'History', label: 'Clear recent audio', desc: 'Deletes the kept dictation audio', keywords: 'delete erase audio recordings voice', anchor: 'clear-recent-audio' },
  { page: 'privacy', section: 'Chat screenshots', label: 'Keep chat screenshots', desc: '7, 30 or 90 days, or forever', keywords: 'snip screenshot screenshots images pictures delete keep retention days', anchor: 'chat-images' },
  { page: 'privacy', section: 'Local API', label: "Let apps on this PC use Yap's data", desc: 'The Local API for scripts and AI apps', keywords: 'local api bridge cli scripts http', anchor: 'local-api' },
  { page: 'privacy', section: 'Logs and data', label: 'Debug logging', desc: 'More detail in the log, for a bug report', keywords: 'debug logs diagnostics bug report troubleshooting', anchor: 'debug-logging', toggle: 'debugLogging' },
  { page: 'privacy', section: 'Logs and data', label: 'Log file', desc: 'Open the logs folder', keywords: 'logs folder', anchor: 'logs' },
  { page: 'privacy', section: 'Logs and data', label: 'Data folder', desc: 'Where Yap keeps settings, notes and models', keywords: 'appdata data folder files portable config', anchor: 'data-folder' },

  // ---- About ----
  { page: 'about', section: 'Updates', label: 'Updates', desc: 'Check for updates, release notes', keywords: 'update version check release notes', anchor: 'updates' },
  { page: 'about', section: 'Report a problem', label: 'Report a problem', desc: 'A diagnostic file you check before saving', keywords: 'bug report problem issue diagnostics diagnostic logs support crash help feedback github', anchor: 'report-problem' },
  { page: 'about', section: 'Learn more', label: 'Setup guide', desc: 'The first-run steps again', keywords: 'onboarding tutorial setup welcome', anchor: 'setup-guide' },
  { page: 'about', section: 'Learn more', label: 'Source code', desc: 'Yap on GitHub', keywords: 'github open source', anchor: 'github' },

  // ---- Account ----
  { page: 'account', section: 'Account', label: 'Account', desc: 'Sign in, devices, delete your account', keywords: 'sign in sign out login email google github discord phone devices', anchor: 'account' },
];

const PAGE_ORDER = [...NAV_GROUPS.flatMap((g) => g.items.map((i) => i.id)), 'account'];

/** Lower case, straight quotes, no stray punctuation at the edges. */
function norm(s) {
  return String(s || '')
    .toLowerCase()
    .replace(/[“”«»]/g, '"')
    .replace(/[‘’]/g, "'")
    .replace(/\s+/g, ' ')
    .trim();
}

/**
 * The forms of a word to look for: "paste" also finds "pasting" and
 * "pasted", "notes" finds "note".
 */
export function stems(word) {
  const w = word.replace(/^["'(]+|["'),.?!:;]+$/g, '');
  if (!w) return [];
  const out = new Set([w]);
  if (w.length > 4 && w.endsWith('e')) out.add(w.slice(0, -1));
  if (w.length > 5 && w.endsWith('ing')) out.add(w.slice(0, -3));
  if (w.length > 4 && w.endsWith('ed')) out.add(w.slice(0, -2));
  if (w.length > 3 && w.endsWith('s') && !w.endsWith('ss')) out.add(w.slice(0, -1));
  return [...out];
}

/** The query's words, each with its forms. */
function terms(query) {
  return norm(query)
    .split(' ')
    .map(stems)
    .filter((s) => s.length);
}

/** Where a word's forms first appear in a text, or -1. */
function find(text, forms) {
  let best = -1;
  for (const f of forms) {
    const i = text.indexOf(f);
    if (i >= 0 && (best < 0 || i < best)) best = i;
  }
  return best;
}

/**
 * Search the index. Every word must be found somewhere in the entry (its
 * label, line, section, page or keywords); a match in the label counts most,
 * one at a word's start more than one inside it.
 * @returns {(Entry & { score: number })[]} best first
 */
export function searchSettings(query, index = SETTINGS_INDEX) {
  const ts = terms(query);
  if (!ts.length) return [];
  const out = [];
  index.forEach((e, i) => {
    const fields = [
      [norm(e.label), 6],
      [norm(e.keywords), 3],
      [norm(e.desc), 2],
      [norm(e.section), 1.5],
      [norm(PAGE_LABELS[e.page]), 1],
    ];
    let score = 0;
    for (const forms of ts) {
      let best = 0;
      for (const [text, w] of fields) {
        const at = find(text, forms);
        if (at < 0) continue;
        const wordStart = at === 0 || /[\s\-("“/]/.test(text[at - 1]);
        best = Math.max(best, w * (wordStart ? 1 : 0.5));
      }
      if (!best) return;
      score += best;
    }
    const label = norm(e.label);
    const q = norm(query);
    if (label.startsWith(q)) score += 4;
    else if (label.includes(q)) score += 2;
    out.push({ ...e, score, order: i });
  });
  return out.sort((a, b) => b.score - a.score || a.order - b.order);
}

/**
 * Results grouped by page: pages in the order of their best result; inside
 * a page, by section (in the order of each section's best result), so a
 * section's heading shows once; best first within a section.
 * @returns {{ page: string, label: string, results: (Entry & {score:number})[] }[]}
 */
export function groupResults(results) {
  const groups = new Map();
  for (const r of results) {
    if (!groups.has(r.page)) groups.set(r.page, { page: r.page, label: PAGE_LABELS[r.page] ?? r.page, sections: new Map() });
    const g = groups.get(r.page);
    if (!g.sections.has(r.section)) g.sections.set(r.section, []);
    g.sections.get(r.section).push(r);
  }
  return [...groups.values()]
    .map((g) => ({ page: g.page, label: g.label, results: [...g.sections.values()].flat() }))
    .sort(
      (a, b) =>
        Math.max(...b.results.map((r) => r.score)) - Math.max(...a.results.map((r) => r.score)) ||
        PAGE_ORDER.indexOf(a.page) - PAGE_ORDER.indexOf(b.page)
    );
}

/** How many results each page has (the nav's counts). */
export function countByPage(results) {
  const counts = {};
  for (const r of results) counts[r.page] = (counts[r.page] ?? 0) + 1;
  return counts;
}

/**
 * A text cut into plain and matched parts, for <mark>: every place one of
 * the query's words (in any of its forms) appears.
 * @returns {{ text: string, hit: boolean }[]}
 */
export function highlight(text, query) {
  const src = String(text || '');
  const low = src.toLowerCase().replace(/[“”]/g, '"').replace(/[‘’]/g, "'");
  const ranges = [];
  for (const forms of terms(query)) {
    for (const f of forms) {
      if (f.length < 2) continue;
      let i = low.indexOf(f);
      while (i >= 0) {
        ranges.push([i, i + f.length]);
        i = low.indexOf(f, i + f.length);
      }
    }
  }
  if (!ranges.length) return [{ text: src, hit: false }];
  ranges.sort((a, b) => a[0] - b[0] || b[1] - a[1]);
  const merged = [];
  for (const r of ranges) {
    const last = merged[merged.length - 1];
    if (last && r[0] <= last[1]) last[1] = Math.max(last[1], r[1]);
    else merged.push([...r]);
  }
  const parts = [];
  let at = 0;
  for (const [a, b] of merged) {
    if (a > at) parts.push({ text: src.slice(at, a), hit: false });
    parts.push({ text: src.slice(a, b), hit: true });
    at = b;
  }
  if (at < src.length) parts.push({ text: src.slice(at), hit: false });
  return parts;
}
