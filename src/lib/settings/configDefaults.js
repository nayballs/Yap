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
  asSaidOffer: true,
  // Snip to Chat (snip.rs): its shortcut, saying "snip", keeping screenshots.
  snipHotkey: 'kb:alt+win+83',
  snipVoice: true,
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
  barEnabled: true,
  barHideFullscreen: true,
  barTheme: 'app',
  barDictationButtons: 'off',
  // General → Appearance (AppearanceSettings.svelte, lib/core/appearance.js).
  theme: 'system',
  readingFont: 'default',
  readingSize: 'default',
  motion: 'system',
  inputDevice: null,
  dictionary: [],
  selectedLanguage: 'auto',
  translateToEnglish: false,
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
  // config.rs default_pp_prompt(), byte for byte (cleanupPresets.js).
  ppPrompt: PP_DEFAULT_BODY,
  routingScope: 'all_apps',
  appRoutes: [],
  cleanupProfiles: [],
  // Styles by app (settings/CleanupStyles.svelte, ai/cleanup_styles.rs).
  cleanupStyles: true,
  llmScopes: {},
  agentName: '',
  editHotkey: '',
  // The rewrite shortcut with nothing selected rewrites the last
  // dictation (pasting/as_said.rs).
  rewriteLastDictation: true,
  assistantHotkey: '',
  readHotkey: '',
  assistantAnswerCard: true,
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
