<script>
  // Settings' "Select Model" list for a cloud provider (Dictation Cleanup and
  // every Language Models scope): Yap's curated models merged with what the
  // provider offers right now (modelCatalog.js — "New" for models it added
  // since Yap's list was made, the rest behind "Show N more"), a clear line
  // when the saved model is no longer offered (still selectable), and where
  // the list came from: "Gemini's current models, checked 2 h ago · Check
  // now", or why it couldn't be checked (then Yap's own list shows).
  import { onMount } from 'svelte';
  import SelectList from './ui/SelectList.svelte';
  import { PROVIDER_ICONS, MONOCHROME_PROVIDERS } from './core/providerIcons.js';
  import { PROVIDER_LABELS } from './core/providers.js';
  import { catalog, loadCatalog, checkCatalog, modelsFor } from './modelCatalog.svelte.js';
  import { ago } from './core/modelCatalog.js';

  let { provider, value = $bindable(''), disabled = false } = $props();

  let showMore = $state(false);
  let filter = $state('');
  let asking = $state(false);
  let askError = $state('');

  const merged = $derived(modelsFor(provider, value));
  const entry = $derived(catalog.providers[provider]);
  const name = $derived(PROVIDER_LABELS[provider] || provider);
  const icon = $derived(PROVIDER_ICONS[provider]);
  const mono = $derived(MONOCHROME_PROVIDERS.has(provider));

  const toOption = (i) => ({
    value: i.value,
    label: i.label,
    desc: i.desc,
    icon,
    mono,
    tag: i.missing ? 'gone' : i.tag,
  });
  const options = $derived.by(() => {
    const opts = merged.items.map(toOption);
    // A saved id Yap's list doesn't know (no live list to say otherwise):
    // shown as it is rather than silently deselected.
    if (value && !merged.live && !opts.some((o) => o.value === value)) {
      opts.push({ value, label: value, desc: 'Custom model id', icon, mono });
    }
    return opts;
  });
  const moreOptions = $derived.by(() => {
    const q = filter.trim().toLowerCase();
    return merged.more
      .filter((i) => !q || i.label.toLowerCase().includes(q) || i.value.toLowerCase().includes(q))
      .map(toOption);
  });

  async function checkNow() {
    askError = '';
    asking = true;
    try {
      await checkCatalog(provider);
    } catch (e) {
      askError = String(e);
    } finally {
      asking = false;
    }
  }

  $effect(() => {
    // Another provider: its own "more" starts folded.
    provider;
    showMore = false;
    filter = '';
    askError = '';
  });

  onMount(() => {
    loadCatalog();
  });
</script>

<div class="picker" data-provider={provider}>
  {#if merged.missing}
    <p class="gone" role="alert">
      {name} no longer offers {merged.missing.label}. Pick another model.
    </p>
  {/if}

  <SelectList bind:value {options} {disabled} />

  {#if merged.more.length > 0}
    <button type="button" class="morebtn" aria-expanded={showMore} onclick={() => (showMore = !showMore)}>
      {showMore
        ? `Show fewer ${name} models`
        : `Show ${merged.more.length} more ${name} ${merged.more.length === 1 ? 'model' : 'models'}`}
    </button>
    {#if showMore}
      {#if merged.more.length > 12}
        <input class="filter" type="search" placeholder="Find a {name} model" aria-label="Find a {name} model" bind:value={filter} />
      {/if}
      <SelectList bind:value options={moreOptions} {disabled} />
      {#if moreOptions.length === 0}<p class="status">No {name} model matches “{filter}”.</p>{/if}
    {/if}
  {/if}

  {#if entry}
    <p class="status" aria-live="polite">
      {#if entry.checking || asking}
        Checking {name}'s models…
      {:else if merged.live}
        {name}'s current models, checked {ago(entry.fetchedAt)}.
        <button type="button" class="linkbtn" onclick={checkNow} {disabled}>Check now</button>
      {:else if entry.error}
        <span class="warn">Couldn't check {name}'s models: {entry.error}.</span> Showing Yap's list.
        <button type="button" class="linkbtn" onclick={checkNow} {disabled}>Try again</button>
      {/if}
      {#if askError}<span class="warn">{askError}</span>{/if}
    </p>
  {/if}
</div>

<style>
  .picker {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .gone {
    margin: 0;
    padding: 8px 10px;
    border: 1px solid color-mix(in srgb, var(--yap-warning) 35%, var(--yap-border-subtle));
    border-radius: var(--yap-r);
    background: color-mix(in srgb, var(--yap-warning) 8%, transparent);
    color: var(--yap-warning);
    font-size: 12px;
    line-height: 1.5;
  }
  .morebtn {
    align-self: flex-start;
    border: none;
    background: none;
    padding: 2px 0;
    color: var(--yap-link);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }
  .morebtn:hover {
    text-decoration: underline;
  }
  .filter {
    width: 100%;
    box-sizing: border-box;
    padding: 6px 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12px;
  }
  .status {
    margin: 0;
    font-size: 11px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .warn {
    color: var(--yap-warning);
  }
  .linkbtn {
    border: none;
    background: none;
    padding: 0;
    color: var(--yap-link);
    font: inherit;
    cursor: pointer;
  }
  .linkbtn:hover {
    text-decoration: underline;
  }
  .linkbtn:disabled {
    color: var(--yap-muted);
    cursor: default;
  }
</style>
