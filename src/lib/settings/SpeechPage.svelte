<script>
  // Settings → Speech-to-Text (`models`; dictation/stt.rs): your model and the
  // catalogue (ModelManager.svelte), speakers in meetings (SpeakerModels,
  // speakers/diarize.rs), a link to the languages you speak (Dictation → My
  // languages, dictation/languages.rs: the main shortcut's language lives
  // there now), Translate to English, and where the model runs
  // (RunsOn.svelte, dictation/stt_speed.rs) with Unload model when idle.
  //
  //   cfg:      Settings' config $state (auto-save does the saving).
  //   langInfo: what the active model can do with languages
  //             (model_language_info; Settings keeps it current).
  //   goto:     opens another Settings page.
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Select from '../ui/Select.svelte';
  import LinkRow from '../ui/LinkRow.svelte';
  import { languageName, loadLanguageCatalog, myLanguages } from './languages.svelte.js';
  import ModelManager from './ModelManager.svelte';
  import SpeakerModels from './SpeakerModels.svelte';
  import RunsOn from './RunsOn.svelte';

  let { cfg, langInfo, goto = null } = $props();

  // The languages you speak are Dictation → My languages' (dictation/languages.rs):
  // this page links there.
  loadLanguageCatalog();
  const mine = $derived(myLanguages(cfg));
  const languagesValue = $derived.by(() => {
    if (!mine.length) return 'Auto-detect';
    const names = mine.map(languageName);
    return names.length > 2 ? `${names.slice(0, 2).join(', ')} +${names.length - 2}` : names.join(', ');
  });

  const UNLOAD_TIMEOUTS = [
    { value: 'never', label: 'Never' },
    { value: '1min', label: 'After 1 minute' },
    { value: '5min', label: 'After 5 minutes' },
    { value: '15min', label: 'After 15 minutes' },
    { value: '30min', label: 'After 30 minutes' },
  ];

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
  <LinkRow
    label="Languages"
    desc="The languages you speak, which model understands which, and a shortcut per language: Dictation → My languages"
    value={languagesValue}
    setting="language"
    onclick={() => goto?.('dictation#languages')}
  />
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
