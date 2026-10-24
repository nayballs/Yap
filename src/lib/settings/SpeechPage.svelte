<script>
  // Settings → Speech-to-Text (`models`; dictation/stt.rs): your model and the
  // catalogue (ModelManager.svelte), speakers in meetings (SpeakerModels,
  // speakers/diarize.rs), the language, and where the model runs (RunsOn.svelte,
  // dictation/stt_speed.rs) with Unload model when idle.
  //
  //   cfg:      Settings' config $state (auto-save does the saving).
  //   langInfo: what the active model can do with languages
  //             (model_language_info; Settings keeps it current).
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Select from '../ui/Select.svelte';
  import ModelManager from './ModelManager.svelte';
  import SpeakerModels from './SpeakerModels.svelte';
  import RunsOn from './RunsOn.svelte';

  let { cfg, langInfo } = $props();

  const UNLOAD_TIMEOUTS = [
    { value: 'never', label: 'Never' },
    { value: '1min', label: 'After 1 minute' },
    { value: '5min', label: 'After 5 minutes' },
    { value: '15min', label: 'After 15 minutes' },
    { value: '30min', label: 'After 30 minutes' },
  ];

  // Display names for the language codes the backend returns.
  const LANG_NAMES = {
    en: 'English', es: 'Spanish', fr: 'French', de: 'German', it: 'Italian',
    pt: 'Portuguese', nl: 'Dutch', pl: 'Polish', ru: 'Russian', uk: 'Ukrainian',
    zh: 'Chinese', ja: 'Japanese', ko: 'Korean', ar: 'Arabic', hi: 'Hindi',
    tr: 'Turkish', yue: 'Cantonese',
  };

  // Language dropdown: Auto + whatever the active model supports.
  const langOptions = $derived([
    { value: 'auto', label: 'Auto-detect' },
    ...(langInfo.languages || []).map((c) => ({ value: c, label: LANG_NAMES[c] || c })),
  ]);
</script>

<div class="page-h">
  <h1>Speech-to-Text</h1>
  <p>The model on this PC that turns your voice into text.</p>
</div>
<!-- Your model first (with the red badge's reason, when there's
     none), then the catalogue: ModelManager.svelte. -->
<ModelManager />
<!-- Speaker diarization's models + its switch (speakers/diarize.rs); the
     Meetings page has the switch too and links here. -->
<Group title="Speakers in meetings" id="settings-speakers">
  <Row setting="speakers">
    <SpeakerModels bind:enabled={cfg.meetingDiarization} bind:model={cfg.diarizationModel} />
  </Row>
</Group>
<Group title="Language" id="settings-stt-language">
  <Row
    label="Language"
    desc={langInfo.supportsLanguage ? 'The language you speak. Auto-detect works for most models' : 'This model uses a fixed language'}
    setting="language"
  >
    <Select bind:value={cfg.selectedLanguage} options={langOptions} ariaLabel="Language" disabled={!langInfo.supportsLanguage} />
  </Row>
  <Row setting="translate">
    <Toggle
      bind:checked={cfg.translateToEnglish}
      label="Translate to English"
      desc={langInfo.supportsTranslate ? 'Output English whatever language you speak' : 'This model can’t translate'}
      disabled={!langInfo.supportsTranslate}
    />
  </Row>
</Group>
<Group title="Performance" id="settings-performance">
  <!-- The graphics card or the processor, measured per model on
       this PC (dictation/stt_speed.rs). -->
  <RunsOn {cfg} />
  <Row label="Unload model when idle" desc="Frees memory between dictations. It loads again for the next one" setting="unload">
    <Select bind:value={cfg.modelUnloadTimeout} options={UNLOAD_TIMEOUTS} ariaLabel="Unload model when idle" />
  </Row>
</Group>
