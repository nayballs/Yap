// Settings' config copy: the defaults it starts from and the payload it
// saves (settings/configSync.svelte.js reads, merges and saves it).
//
// FIELD_DEFAULTS: for fields a stale config.json (or an in-flight backend)
// might not include yet, so the pages' bindings always have something to
// bind to. A new setting's default goes here (and in config.rs); a setting
// whose saved value must change once goes in config::migrate_once.
import { PP_DEFAULT_BODY } from './cleanupPresets.js';

export const FIELD_DEFAULTS = {
  recordingMode: 'toggle',
  // Settings → General → Notifications: each kind on its own (notices/kinds.rs).
  notifySuggestions: true,
  notifyMilestones: true,
  notifyFeatureTips: true,
  notifyNews: true,
  quietOtherApps: false,
  pauseMedia: false,
  keepMicReady: false,
  appendTrailingSpace: false,
  smartSpacing: true,
  screenNames: true,
  chatNoFullStop: true,
  autoSubmit: false,
  voiceEnter: true,
  terminalMode: true,
  restoreClipboard: true,
  startHidden: false,
  autostart: false,
  audioFeedbackVolume: 1.0,
  soundEnabled: true,
  useGpu: true,
  // Settings → Speech-to-Text → Runs on (dictation/stt_speed.rs): auto | gpu | cpu.
  sttDevice: 'auto',
  // A day of dictation audio, to run one again (dictation/recent_audio.rs).
  keepRecentAudio: true,
  // "Use what I said" (pasting/as_said.rs): its shortcut and the bar's offer.
  asSaidHotkey: '',
  asSaidOffer: false,
  // Paste last / Copy last transcript (pasting/last.rs): unbound.
  pasteLastHotkey: '',
  copyLastHotkey: '',
  // Quick note (notes/quick_note.rs): unbound.
  quickNoteHotkey: '',
  // More shortcuts per action ("+ Add another", dictation/input_hook.rs), by
  // the first one's key: { hotkey: ['mouse:4'] }.
  moreHotkeys: {},
  // Push-to-talk's triple-tap starts meeting notes (dictation/gesture.rs).
  tripleTapMeeting: false,
  // Snip to Chat (snip.rs): its shortcut, saying "snip", keeping screenshots.
  snipHotkey: 'kb:alt+win+83',
  snipVoice: true,
  // …then listen for the question; every snip to Chat on the clipboard too.
  snipListen: true,
  snipCopy: true,
  chatImageDays: 30,
  snipSaveDir: '',
  streamingPartials: true,
  historyEnabled: true,
  meetingDetection: true,
  meetingAutoStart: false,
  meetingDetectApps: {},
  meetingDetectStyle: 'popup',
  meetingHideFromCapture: true,
  meetingMaxMinutes: 120,
  meetingCallEnd: 'ask',
  meetingEndWhenQuiet: true,
  meetingHotkey: 'kb:alt+win+77',
  meetingOpenNotepad: true,
  meetingSplitScreen: false,
  meetingLiveTranscript: true,
  // Meeting audio kept on disk (meetings/meeting_audio.rs): to transcribe again, play a line.
  meetingKeepAudio: true,
  meetingAudioDays: 14,
  // Delete meeting transcripts after N days (notes/retention.rs): 0 = keep; 30 | 90 | 365.
  meetingTranscriptDays: 0,
  // The refined transcript after a meeting (meetings/meeting_refine.rs): auto | always | never.
  meetingRefine: 'auto',
  meetingDiarization: false,
  diarizationModel: 'nemotron-3-diarization',
  meetingReminder: '15s',
  meetingBrief: true,
  // Memory (MemorySection.svelte, memory.rs): the switches only.
  memorySearchChats: true,
  memoryLearnChats: true,
  memoryLearnMeetings: false,
  memorySensitive: false,
  // "Search by meaning" (search/): off until its model is in.
  searchByMeaning: false,
  barEnabled: true,
  barHideFullscreen: true,
  barTheme: 'app',
  barDictationButtons: 'off',
  // The quick note's button on the bar (notes/quick_note.rs; off).
  barQuickNote: false,
  // General → Appearance (AppearanceSettings.svelte, lib/core/appearance.js).
  theme: 'system',
  readingFont: 'default',
  readingSize: 'default',
  motion: 'system',
  inputDevice: null,
  // Settings → Dictation → Microphone (MicOrder.svelte, dictation/mics.rs):
  // the microphones in order ([{ id, name }]; empty: Windows' default), and
  // the ones answered "Not now" to.
  micOrder: [],
  micNotNow: [],
  dictionary: [],
  selectedLanguage: 'auto',
  translateToEnglish: false,
  // My languages (dictation/languages.rs): the languages you speak, the
  // language shortcuts, and "Tell me when I speak a language I didn't pick".
  myLanguages: [],
  languageShortcuts: [],
  languageNudge: true,
  modelUnloadTimeout: 'never',
  outputDevice: null,
  overlayPosition: 'bottom',
  autoSubmitKey: 'enter',
  updateChecksEnabled: true,
  debugLogging: false,
  postProcessEnabled: false,
  ppProvider: 'groq',
  ppBaseUrl: 'https://api.groq.com/openai/v1',
  ppApiKey: '',
  ppApiKeys: {},
  ppModel: 'openai/gpt-oss-20b',
  ppPreset: 'default',
  ppDisableThinking: false,
  // The on-device model loads on first use, freed after this many idle
  // minutes (0 = never); at launch only when asked (ai/local_llm/on_demand.rs).
  localModelAtLaunch: false,
  localModelIdleMins: 15,
  // "Read screenshots on this PC" (ai/local_llm/vision.rs): once its
  // projector is downloaded. On: downloading it is the choice.
  localVision: true,
  // config.rs default_pp_prompt(), byte for byte (cleanupPresets.js).
  ppPrompt: PP_DEFAULT_BODY,
  routingScope: 'all_apps',
  appRoutes: [],
  cleanupProfiles: [],
  // Styles by app (settings/CleanupStyles.svelte, ai/cleanup_styles.rs).
  cleanupStyles: true,
  // Cleanup strength (ai/cleanup_strength.rs): light | medium | high; None
  // is cleanup off.
  cleanupStrength: 'medium',
  // The text before the cursor to a cleanup model off this PC
  // (ai/cleanup_context.rs; Privacy & data).
  cursorContextCloud: false,
  // A transform on every dictation (dictation/transforms.rs): its id, and
  // the kind of app ('' = every app).
  autoTransform: '',
  autoTransformKind: '',
  llmScopes: {},
  agentName: '',
  editHotkey: '',
  // The rewrite shortcut with nothing selected rewrites the last
  // dictation (pasting/as_said.rs).
  rewriteLastDictation: true,
  assistantHotkey: '',
  readHotkey: '',
  assistantAnswerCard: true,
  spokenLength: '1m',
  openApps: true,
  appsBlocked: [],
  appNicknames: [],
  webSearch: false,
  webSearchNative: true,
  webSearchService: 'exa',
  webSearchKeys: {},
  speakReplies: false,
  speakWhen: 'voice',
  talkBackHandsFree: true,
  ttsEngine: 'kokoro',
  ttsVoice: 'af_bella',
  ttsWindowsVoice: '',
  ttsRate: 1.0,
  ttsVolume: 0.5,
  ttsProvider: 'local',
  ttsCloudKeys: {},
  ttsCloud: {},
  // About you (AboutYouSettings.svelte, ai/about_you.rs): the region's country,
  // '' = Automatic (Windows'); its spelling's switch is in Dictation
  // cleanup (RegionSpelling.svelte).
  aboutName: '',
  aboutMe: '',
  regionCountry: '',
  regionSpelling: true,
};

// The cleaned config payload the backend expects (trimmed dictionary,
// numeric fields, null device sentinels). Does NOT mutate `cfg` — so calling
// it from the auto-save effect can't re-trigger the effect.
export function cleanConfig(cfg) {
  return {
    ...cfg,
    inputDevice: cfg.inputDevice || null,
    micOrder: (Array.isArray(cfg.micOrder) ? cfg.micOrder : [])
      .filter((m) => m && (m.id || m.name))
      .map((m) => ({ id: String(m.id || ''), name: String(m.name || '') })),
    micNotNow: (Array.isArray(cfg.micNotNow) ? cfg.micNotNow : []).filter((m) => m && (m.id || m.name)),
    outputDevice: cfg.outputDevice || null,
    audioFeedbackVolume: Number(cfg.audioFeedbackVolume),
    ttsRate: Number(cfg.ttsRate) || 1,
    ttsVolume: Number.isFinite(Number(cfg.ttsVolume)) ? Number(cfg.ttsVolume) : 0.5,
    dictionary: cfg.dictionary
      .map((e) => ({
        from: (e.from || '').trim(),
        to: (e.to || '').trim(),
        fuzzy: e.fuzzy ?? true,
        // Snippets (DictionaryView) ride along in this copy: keep the flag,
        // or this auto-save would turn them into corrections.
        snippet: !!e.snippet,
        // A star too (the Dictionary view's), written only when set.
        ...(e.starred ? { starred: true } : {}),
      }))
      .filter((e) => e.from),
    appRoutes: (cfg.appRoutes || [])
      .map((r) => ({
        app: (r.app || '').trim(),
        label: (r.label || '').trim() || (r.app || '').trim(),
        profileId: r.profileId || '',
        prompt: r.prompt || '',
      }))
      .filter((r) => r.app),
    cleanupProfiles: (cfg.cleanupProfiles || [])
      .map((p) => ({
        // A style's kind, tone and apps (ai/cleanup_styles.rs) ride along.
        ...p,
        id: p.id,
        name: (p.name || '').trim() || 'Untitled',
        prompt: p.prompt || '',
        provider: p.provider || '',
        baseUrl: (p.baseUrl || '').trim(),
        model: (p.model || '').trim(),
        apiKey: p.apiKey || '',
      }))
      .filter((p) => p.id),
  };
}
