<script>
  // Settings → General → About you (about_you.rs, 2026-10-21): what Yap
  // tells the AI models about the person, so an answer fits where they are
  // (Nathan's "It keeps finding things in dollars…").
  // - "Region and language": what Windows says (regional format, home
  //   location, currency, measurement, time zone), as one line —
  //   "United Kingdom · English (UK) · £ GBP · metric · UK time" — with
  //   Change to pick each (cfg.regionCountry / regionLanguage /
  //   regionCurrency / regionUnits / regionTimeZone; empty = follow
  //   Windows, a picked country brings its own currency and units);
  // - "What should Yap call you?" (cfg.aboutName) and "Anything the AI should
  //   know about you" (cfg.aboutMe, ≤ 300 characters);
  // - "Use my region's spelling" (cfg.regionSpelling): AI cleanup keeps
  //   British or American spelling;
  // - "See what the AI is told": the block itself, as it reads now;
  // - the privacy line.
  //
  //   cfg: Settings' config $state. Auto-save does the saving.
  import { invoke } from '@tauri-apps/api/core';
  import Group from './ui/Group.svelte';
  import Row from './ui/Row.svelte';
  import Select from './ui/Select.svelte';
  import Toggle from './ui/Toggle.svelte';
  import Button from './ui/Button.svelte';
  import Input from './ui/Input.svelte';
  import Textarea from './ui/Textarea.svelte';

  let { cfg } = $props();
  let status = $state(null);
  let changing = $state(false);
  let showPrompt = $state(false);

  // Re-read as the fields change (Settings' copy, before its auto-save): the
  // summary, what "Automatic" means for the country picked, the prompt.
  $effect(() => {
    const overrides = {
      regionCountry: cfg.regionCountry ?? '',
      regionLanguage: cfg.regionLanguage ?? '',
      regionCurrency: cfg.regionCurrency ?? '',
      regionUnits: cfg.regionUnits ?? '',
      regionTimeZone: cfg.regionTimeZone ?? '',
      aboutName: cfg.aboutName ?? '',
      aboutMe: cfg.aboutMe ?? '',
      regionSpelling: cfg.regionSpelling !== false,
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

  const ZONES = (() => {
    try {
      return Intl.supportedValuesOf('timeZone');
    } catch {
      return [];
    }
  })();
  const city = (z) => (z.split('/').slice(1).join(' / ') || z).replace(/_/g, ' ');
  const money = (r) => (!r?.currency ? 'not known' : r.symbol && r.symbol !== r.currency ? `${r.symbol} ${r.currency}` : r.currency);
  const units = (r) => (r?.metric === false ? 'US units' : r?.metric ? 'metric' : 'not known');

  const overridden = $derived(
    ['regionCountry', 'regionLanguage', 'regionCurrency', 'regionUnits', 'regionTimeZone'].some((k) => (cfg[k] ?? '') !== ''),
  );
  const win = $derived(status?.windows ?? {});
  const countryOptions = $derived([
    { value: '', label: `Windows: ${win.countryName ?? 'not set'}` },
    ...[...(status?.countries ?? [])].sort((a, b) => a.name.localeCompare(b.name)).map((c) => ({ value: c.code, label: c.name })),
  ]);
  const languageOptions = $derived.by(() => {
    const list = (status?.languages ?? []).map((l) => ({ value: l.tag, label: l.name }));
    const picked = cfg.regionLanguage ?? '';
    if (picked && !list.some((l) => l.value === picked)) list.unshift({ value: picked, label: picked });
    return [{ value: '', label: `Windows: ${win.languageName ?? 'not set'}` }, ...list];
  });
  const currencyOptions = $derived.by(() => {
    const list = (status?.currencies ?? []).map((c) => ({
      value: c.code,
      label: c.symbol !== c.code ? `${c.symbol} ${c.code} · ${c.name}` : `${c.code} · ${c.name}`,
    }));
    const picked = cfg.regionCurrency ?? '';
    if (picked && !list.some((c) => c.value === picked)) list.unshift({ value: picked, label: picked });
    return [{ value: '', label: `Automatic: ${money(status?.auto)}` }, ...list];
  });
  const unitOptions = $derived([
    { value: '', label: `Automatic: ${units(status?.auto)}` },
    { value: 'metric', label: 'Metric (km, kg, °C)' },
    { value: 'imperial', label: 'US units (miles, pounds, °F)' },
  ]);
  const zoneOptions = $derived([
    { value: '', label: `Windows: ${win.timeZoneLabel ?? 'not set'}${win.timeZone ? ` (${win.timeZone})` : ''}` },
    ...ZONES.map((z) => ({ value: z, label: city(z), group: z.includes('/') ? z.split('/')[0] : 'Other' })),
  ]);

  const aboutMax = $derived(status?.aboutMax ?? 300);
  const aboutLen = $derived([...(cfg.aboutMe ?? '')].length);
  const spelling = $derived(status?.region?.spelling);

  function useWindows() {
    cfg.regionCountry = '';
    cfg.regionLanguage = '';
    cfg.regionCurrency = '';
    cfg.regionUnits = '';
    cfg.regionTimeZone = '';
  }
</script>

<div id="settings-about-you">
  <Group title="About you">
    <Row
      label="Region and language"
      desc={status ? status.summary || "Windows doesn't say: pick your country" : '…'}
      hint="Yap tells the AI where you are, so prices come in your currency, times in your time zone and units the way you use them. It follows Windows' region settings until you change something here."
    >
      <Button variant="secondary" size="sm" onclick={() => (changing = !changing)}>
        {changing ? 'Done' : 'Change'}
      </Button>
    </Row>
    {#if changing}
      <Row>
        <div class="pickers" role="group" aria-label="Region and language">
          <Select bind:value={cfg.regionCountry} options={countryOptions} label="Country" />
          <Select bind:value={cfg.regionLanguage} options={languageOptions} label="Language" />
          <Select bind:value={cfg.regionCurrency} options={currencyOptions} label="Currency" />
          <Select bind:value={cfg.regionUnits} options={unitOptions} label="Units" />
          <Select bind:value={cfg.regionTimeZone} options={zoneOptions} label="Time zone" />
          <p class="small">
            {#if overridden}
              Windows says: {status?.windowsSummary || 'nothing'}.
              <button type="button" class="link" onclick={useWindows}>Use Windows' settings</button>
            {:else}
              From Windows' region settings. A country you pick brings its own currency and units.
            {/if}
          </p>
        </div>
      </Row>
    {/if}
    <Row label="What should Yap call you?" desc="Optional">
      <div class="name">
        <Input
          bind:value={cfg.aboutName}
          placeholder="Your first name"
          maxlength={status?.nameMax ?? 60}
          ariaLabel="What should Yap call you?"
        />
      </div>
    </Row>
    <Row>
      <div class="about">
        <div class="abouthead">
          <span class="label" id="about-me-label">Anything the AI should know about you</span>
          <span class="count" class:full={aboutLen >= aboutMax}>{aboutLen}/{aboutMax}</span>
        </div>
        <span class="desc">Optional. A line or two the AI keeps in mind when it answers.</span>
        <Textarea
          bind:value={cfg.aboutMe}
          rows={3}
          maxlength={aboutMax}
          placeholder="I'm in Manchester, I play PC games, I work day shifts"
          ariaLabel="Anything the AI should know about you"
        />
      </div>
    </Row>
    <Row>
      <Toggle
        bind:checked={cfg.regionSpelling}
        label="Use my region's spelling"
        desc={spelling
          ? `AI cleanup writes ${spelling} English: ${status?.spellingExamples ?? ''}`
          : 'AI cleanup keeps your English spelling once Yap knows which you use'}
        hint="Only the spelling changes, never your words. It applies when AI cleanup is on (Language Models → Dictation Cleanup)."
      />
    </Row>
    <Row>
      <div class="told">
        <button type="button" class="link" aria-expanded={showPrompt} onclick={() => (showPrompt = !showPrompt)}>
          {showPrompt ? 'Hide what the AI is told' : 'See what the AI is told'}
        </button>
        {#if showPrompt && status}
          <p class="prompt" aria-label="What the AI is told">{status.prompt}</p>
        {/if}
        <p class="privacy">
          Kept on this PC. Yap sends it to the AI model you use, with each question: a cloud provider (Groq, OpenAI,
          Anthropic…) sees it, a model on this PC keeps it here. It's never sent to Yap.
        </p>
      </div>
    </Row>
  </Group>
</div>

<style>
  .pickers {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
  }
  /* One column of equal pickers. */
  .pickers :global(.select-wrap) {
    width: 260px;
  }
  .name {
    width: 220px;
  }
  .about,
  .told {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }
  .abouthead {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
  }
  .label {
    font-size: 13px;
    font-weight: 500;
    color: var(--yap-fg);
  }
  .desc,
  .small,
  .privacy,
  .count {
    font-size: 11.5px;
    line-height: 1.55;
    color: var(--yap-muted);
  }
  .count.full {
    color: var(--yap-warning);
    font-weight: 600;
  }
  .small,
  .privacy {
    margin: 0;
  }
  .link {
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    font-size: 12px;
    color: var(--yap-primary);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
    align-self: flex-start;
  }
  .link:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 2px;
    border-radius: 3px;
  }
  .prompt {
    margin: 0;
    padding: 9px 11px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    font-size: 12px;
    line-height: 1.55;
    color: var(--yap-fg);
    white-space: pre-wrap;
  }
</style>
