<script>
  // Setup's languages step (My languages, dictation/languages.rs): the
  // languages you speak — one click on the common ones, a search for the
  // rest — saved as they're picked (`myLanguages`), and what your speech
  // model makes of them (`my_languages_status`): when it doesn't cover one,
  // the model that does, with its download (or Use it, when it's here).
  // Someone who speaks only English is through in one click.
  import { invoke } from '@tauri-apps/api/core';
  import { MODELS } from '../core/speechModels.js';
  import { catalog, languageName, loadLanguageCatalog, myLanguages } from '../settings/languages.svelte.js';
  import StepHeader from './StepHeader.svelte';

  /** `setup`: onboarding's state (setupState.svelte.js). */
  let { setup } = $props();

  loadLanguageCatalog();

  // The PC's language first, then the most spoken ones Yap's models know.
  const pcLang = (navigator.language || 'en').slice(0, 2).toLowerCase();
  const COMMON = ['en', 'es', 'fr', 'de', 'it', 'pt', 'nl', 'pl', 'hi', 'zh', 'ja', 'ko', 'ar', 'ru', 'tr', 'uk'];
  const common = [pcLang, ...COMMON.filter((c) => c !== pcLang)].slice(0, 12);

  const mine = $derived(myLanguages(setup.cfg));
  const shown = $derived([...new Set([...common, ...mine])]);

  function toggle(code) {
    const next = mine.includes(code) ? mine.filter((c) => c !== code) : [...mine, code];
    setup.setLanguages(next);
  }

  async function onlyEnglish() {
    await setup.setLanguages(['en']);
    setup.next();
  }

  // ---- another language: a search over the list --------------------------
  let query = $state('');
  const matches = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return catalog.list
      .filter((l) => !shown.includes(l.code))
      .filter((l) => l.name.toLowerCase().includes(q) || (l.native || '').toLowerCase().includes(q))
      .slice(0, 6);
  });
  function addFound(code) {
    query = '';
    if (!mine.includes(code)) setup.setLanguages([...mine, code]);
  }

  // ---- your speech model and these languages -------------------------------
  let status = $state(null);
  $effect(() => {
    const languages = [...mine];
    const model = setup.cfg?.modelSize || '';
    if (!languages.length) {
      status = null;
      return;
    }
    let gone = false;
    invoke('my_languages_status', { languages, model })
      .then((s) => {
        if (!gone) status = s;
      })
      .catch(() => {});
    return () => {
      gone = true;
    };
  });
  const model = $derived(status?.model ?? null);
  const missing = $derived(model?.missing ?? []);
  const rec = $derived(missing.length ? (status?.recommended ?? null) : null);
  const recModel = $derived(rec ? (MODELS.find((m) => m.id === rec.id) ?? { id: rec.id, name: rec.name }) : null);

  function names(codes, last = 'and') {
    const list = (codes || []).map(languageName);
    if (list.length <= 1) return list[0] || '';
    return `${list.slice(0, -1).join(', ')} ${last} ${list.at(-1)}`;
  }

  /** What your speech model makes of them, in a sentence. */
  const coverage = $derived.by(() => {
    if (!model || !mine.length) return '';
    if (!setup.active) return 'Pick a speech model first (step 1), and Yap checks it understands them.';
    if (!missing.length) {
      return mine.length === 1 ? `${model.name} understands ${names(mine)}.` : `${model.name} understands all of them.`;
    }
    if (model.knows) return `${model.name} understands ${names(model.knows)} only: ${names(missing)} ${missing.length === 1 ? 'needs' : 'need'} another model.`;
    return `${model.name} doesn't understand ${names(missing, 'or')}.`;
  });
  const busy = $derived(!!recModel && setup.busyId === recModel.id);
</script>

<StepHeader title="Which languages do you speak?">
  Yap dictates in the languages you pick, and AI cleanup keeps each one in the
  language you spoke.
</StepHeader>

<div class="langs">
  <button type="button" class="only-en" onclick={onlyEnglish} disabled={!setup.cfg}>
    I only speak English
    <span class="sub">Skip this step</span>
  </button>

  <div class="or" aria-hidden="true"><span>or pick yours</span></div>

  <div class="chips" role="group" aria-label="Languages you speak">
    {#each shown as code (code)}
      <button
        type="button"
        class="chip"
        class:on={mine.includes(code)}
        aria-pressed={mine.includes(code)}
        data-language={code}
        onclick={() => toggle(code)}
        disabled={!setup.cfg}
      >
        {#if mine.includes(code)}<span class="tick" aria-hidden="true">✓</span>{/if}
        {languageName(code)}
      </button>
    {/each}
  </div>

  <div class="search">
    <input type="search" bind:value={query} placeholder="Another language…" aria-label="Find another language" autocomplete="off" spellcheck="false" />
    {#if matches.length}
      <ul class="found" aria-label="Languages found">
        {#each matches as l (l.code)}
          <li>
            <button type="button" onclick={() => addFound(l.code)}>
              {l.name}{#if l.native && l.native !== l.name}<span class="native">{l.native}</span>{/if}
            </button>
          </li>
        {/each}
      </ul>
    {:else if query.trim() && catalog.list.length}
      <p class="none">No language called “{query.trim()}”</p>
    {/if}
  </div>

  {#if coverage}
    <div class="cover" class:warn={missing.length > 0 && !!setup.active} data-testid="language-coverage" role="status">
      <p>{coverage}</p>
      {#if recModel && setup.active}
        <p class="rec">
          {recModel.name} understands {mine.length === 1 ? names(mine) : 'all your languages'}.
          <button type="button" class="get" onclick={() => setup.choose(recModel)} disabled={!!setup.busyId} data-model={recModel.id}>
            {#if busy}
              {setup.percent >= 100 ? `Setting up ${recModel.name}…` : `Downloading… ${setup.percent}%`}
            {:else if rec.installed}
              Use {recModel.name}
            {:else}
              Download {recModel.name}
            {/if}
          </button>
        </p>
        {#if busy && setup.sttLine}<p class="line">{setup.sttLine}</p>{/if}
      {:else if missing.length && setup.active}
        <p class="rec">No one speech model covers all of them: pick the one for the language you speak most in Settings → Speech-to-Text.</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .langs {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 14px;
    padding: 2px 4px;
  }
  .only-en {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    min-width: 260px;
    padding: 11px 20px;
    border: 1px solid var(--yap-border);
    border-radius: 10px;
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
    box-shadow: var(--yap-shadow-sm);
  }
  .only-en:hover:not(:disabled) {
    border-color: var(--yap-primary);
  }
  .only-en .sub {
    font-size: 12px;
    font-weight: 500;
    color: var(--yap-muted);
  }
  .or {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    max-width: 460px;
    color: var(--yap-muted);
    font-size: 12px;
  }
  .or::before,
  .or::after {
    content: '';
    flex: 1;
    height: 1px;
    background: var(--yap-border-subtle);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 7px;
    max-width: 500px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 6px 12px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
  }
  .chip:hover:not(:disabled) {
    background: var(--yap-raised-soft);
  }
  .chip.on {
    border-color: var(--yap-ink);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .tick {
    font-size: 11px;
  }
  .search {
    width: 100%;
    max-width: 300px;
  }
  .search input {
    box-sizing: border-box;
    width: 100%;
    padding: 7px 10px;
    border: 1px solid var(--yap-border);
    border-radius: 7px;
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
  }
  .found {
    list-style: none;
    margin: 6px 0 0;
    padding: 4px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 9px;
    background: var(--yap-s2);
  }
  .found button {
    display: flex;
    justify-content: space-between;
    width: 100%;
    padding: 6px 8px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .found button:hover {
    background: var(--yap-raised-soft);
  }
  .native {
    color: var(--yap-muted);
  }
  .none {
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--yap-muted);
    text-align: center;
  }
  .cover {
    width: 100%;
    max-width: 460px;
    padding: 10px 14px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 10px;
    background: var(--yap-s2);
    font-size: 13px;
    line-height: 1.5;
    color: var(--yap-fg-80);
  }
  .cover p {
    margin: 0;
  }
  .cover.warn {
    border-color: var(--yap-primary);
  }
  .rec {
    margin-top: 6px !important;
  }
  .get {
    margin-left: 6px;
    padding: 4px 11px;
    border: none;
    border-radius: 7px;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .get:disabled {
    background: var(--yap-raised);
    color: var(--yap-muted);
    cursor: default;
  }
  .line {
    margin-top: 4px !important;
    font-size: 12px;
    color: var(--yap-muted);
  }
</style>
