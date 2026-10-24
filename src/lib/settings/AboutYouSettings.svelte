<script>
  // Settings → General → Language and region, and About you (ai/about_you.rs,
  // 2026-10-21): what Yap tells the AI models about the person, so an answer
  // fits where they are (a tester's "It keeps finding things in dollars…").
  // - "Region" (one picker since 2026-10-22, the maintainer: "all of that should be
  //   covered in the region selector only"): "Automatic: United Kingdom
  //   (from Windows)" — Windows' home location — or any country
  //   (cfg.regionCountry, '' = Automatic). Its currency, units, language and
  //   spelling follow it; the time zone is always the PC's. Its line says
  //   what follows: "Prices in £ · miles and °C · British spelling · UK time".
  // - "What should Yap call you?" (cfg.aboutName) and "Anything the AI should
  //   know about you" (cfg.aboutMe, ≤ 300 characters);
  // - "See what the AI is told": the block itself, as it reads now;
  // - where it goes, in one line, and the way to Privacy & data.
  // The spelling's off switch is in Language Models → Dictation cleanup
  // (RegionSpelling.svelte).
  //
  //   cfg:  Settings' config $state. Auto-save does the saving.
  //   goto: opens another Settings page ("privacy").
  import { invoke } from '@tauri-apps/api/core';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Select from '../ui/Select.svelte';
  import Input from '../ui/Input.svelte';
  import Textarea from '../ui/Textarea.svelte';

  let { cfg, goto = null } = $props();
  let status = $state(null);
  let showPrompt = $state(false);

  // Re-read as the fields change (Settings' copy, before its auto-save): what
  // follows from the region, and the prompt.
  $effect(() => {
    const overrides = {
      regionCountry: cfg.regionCountry ?? '',
      aboutName: cfg.aboutName ?? '',
      aboutMe: cfg.aboutMe ?? '',
    };
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

  const windowsCountry = $derived(status?.windows?.countryName);
  const regionOptions = $derived([
    {
      value: '',
      label: windowsCountry ? `Automatic: ${windowsCountry} (from Windows)` : "Automatic (Windows doesn't say)",
    },
    ...[...(status?.countries ?? [])]
      .sort((a, b) => a.name.localeCompare(b.name))
      .map((c) => ({ value: c.code, label: c.name })),
  ]);
  const follows = $derived(
    !status ? '…' : status.summary || 'Pick your country, so answers come with your prices and units',
  );

  const aboutMax = $derived(status?.aboutMax ?? 300);
  const aboutLen = $derived([...(cfg.aboutMe ?? '')].length);
</script>

<Group title="Language and region" id="settings-language">
  <Row
    label="Region"
    hint="Prices come in your currency, distances in your units and spelling your way. Times always follow this PC's clock."
    setting="region"
  >
    {#snippet desc()}<span data-testid="region-follows">{follows}</span>{/snippet}
    <Select bind:value={cfg.regionCountry} options={regionOptions} ariaLabel="Region" wide />
  </Row>
</Group>

<Group title="About you" id="settings-about-you" desc="What the AI keeps in mind when it answers. Optional.">
  <Row label="What should Yap call you?" setting="about-name">
    <div class="name">
      <Input
        bind:value={cfg.aboutName}
        placeholder="Your first name"
        maxlength={status?.nameMax ?? 60}
        ariaLabel="What should Yap call you?"
      />
    </div>
  </Row>
  <Row setting="about-me">
    <div class="about">
      <div class="head">
        <span class="label" id="about-me-label">Anything the AI should know about you</span>
        <span class="count" class:full={aboutLen >= aboutMax}>{aboutLen}/{aboutMax}</span>
      </div>
      <Textarea
        bind:value={cfg.aboutMe}
        rows={3}
        maxlength={aboutMax}
        placeholder="I'm a teacher in Bristol and I run at weekends"
        ariaLabel="Anything the AI should know about you"
      />
      <p class="privacy">
        Kept on this PC, and sent only to the AI model you use with each question: never sent to Yap.
        <button type="button" class="set-link" aria-expanded={showPrompt} onclick={() => (showPrompt = !showPrompt)}>
          {showPrompt ? 'Hide what the AI is told' : 'See what the AI is told'}
        </button>
        {#if goto}
          · <button type="button" class="set-link" onclick={() => goto('privacy#leaves')}>Privacy & data</button>
        {/if}
      </p>
      {#if showPrompt && status}
        <p class="prompt" aria-label="What the AI is told">{status.prompt}</p>
      {/if}
    </div>
  </Row>
</Group>

<style>
  .about {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
  }
  .name {
    width: var(--yap-select-w-wide);
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
  }
  .label {
    font-size: 13.5px;
    font-weight: 560;
    color: var(--yap-fg);
  }
  .privacy,
  .count {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .count {
    font-variant-numeric: tabular-nums;
  }
  .count.full {
    color: var(--yap-danger);
    font-weight: 600;
  }
  .prompt {
    margin: 0;
    padding: 10px 12px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--yap-fg);
    white-space: pre-wrap;
  }
</style>
