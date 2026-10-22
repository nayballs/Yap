<script>
  // Settings → General → About you (about_you.rs, 2026-10-21): what Yap
  // tells the AI models about the person, so an answer fits where they are
  // (Nathan's "It keeps finding things in dollars…").
  // - "Region" (one picker since 2026-10-22, Nathan: "all of that should be
  //   covered in the region selector only"): "Automatic: United Kingdom
  //   (from Windows)" — Windows' home location — or any country
  //   (cfg.regionCountry, '' = Automatic). Its currency, units, language and
  //   spelling follow it; the time zone is always the PC's. One quiet line
  //   under it says what follows: "Prices in £ · miles and °C · British
  //   spelling · UK time".
  // - "What should Yap call you?" (cfg.aboutName) and "Anything the AI should
  //   know about you" (cfg.aboutMe, ≤ 300 characters);
  // - "See what the AI is told": the block itself, as it reads now;
  // - the privacy line.
  // The spelling's off switch is in Language Models → Dictation cleanup
  // (RegionSpelling.svelte).
  //
  //   cfg: Settings' config $state. Auto-save does the saving.
  import { invoke } from '@tauri-apps/api/core';
  import Group from './ui/Group.svelte';
  import Row from './ui/Row.svelte';
  import Select from './ui/Select.svelte';
  import Tooltip from './ui/Tooltip.svelte';
  import Input from './ui/Input.svelte';
  import Textarea from './ui/Textarea.svelte';

  let { cfg } = $props();
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

<div id="settings-about-you">
  <Group title="About you">
    <Row>
      <div class="region">
        <div class="head">
          <span class="label" id="region-label">Region</span>
          <Tooltip
            text="Yap tells the AI where you are, so prices come in your currency, distances in your units and spelling your way. Times always follow this PC's clock."
          />
        </div>
        <div class="pick">
          <Select bind:value={cfg.regionCountry} options={regionOptions} ariaLabel="Region" />
        </div>
        <p class="follows" data-testid="region-follows">{follows}</p>
      </div>
    </Row>
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
        <div class="head">
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
  .region,
  .about,
  .told {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
  }
  .pick {
    width: 300px;
    max-width: 100%;
  }
  .pick :global(.select-wrap) {
    max-width: none;
    width: 100%;
  }
  .name {
    width: 220px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .about .head {
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
  }
  .label {
    font-size: 13px;
    font-weight: 650;
    color: var(--yap-fg);
  }
  .desc,
  .follows,
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
  .follows,
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
