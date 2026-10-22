<script>
  // Language Models → Dictation cleanup → "Write in my region's spelling"
  // (cfg.regionSpelling, on by default; about_you::cleanup_spelling): AI
  // cleanup keeps the spelling of the region picked in About you (British,
  // American, Canadian). The one switch for it, since 2026-10-22, for the
  // rare person who writes another variety than their region's; its line
  // names the variety and the region it came from.
  //
  //   cfg: Settings' config $state. Auto-save does the saving.
  import { invoke } from '@tauri-apps/api/core';
  import Toggle from './ui/Toggle.svelte';

  let { cfg, disabled = false } = $props();
  let status = $state(null);

  $effect(() => {
    const overrides = { regionCountry: cfg.regionCountry ?? '' };
    let stale = false;
    invoke('about_you_status', { overrides })
      .then((s) => {
        if (!stale) status = s;
      })
      .catch(() => {});
    return () => {
      stale = true;
    };
  });

  const desc = $derived.by(() => {
    const spelling = status?.spelling;
    const country = status?.region?.countryName;
    if (spelling && country) {
      return `${spelling} spelling, from your region (${country}): ${status.spellingExamples}. Only the spelling changes, never your words.`;
    }
    if (country) return `Your region (${country}) doesn't set an English spelling: cleanup keeps yours as it is.`;
    return 'Pick your region in General → Language and region, and cleanup spells English its way.';
  });
</script>

<Toggle
  bind:checked={cfg.regionSpelling}
  label="Write in my region's spelling"
  {desc}
  hint="Your region is in General → Language and region. Turn this off if you write another kind of English than your region's."
  {disabled}
/>
