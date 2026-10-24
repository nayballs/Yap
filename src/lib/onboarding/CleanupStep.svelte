<script>
  // Setup's AI cleanup step (Yap's differentiator, offered up front): a
  // before/after, then Private (a model on this PC: the one recommended for
  // it first, the light one, the rest behind "More models") or Cloud (a
  // provider and your own key), whichever suits this PC best picked, with
  // what each adds to a dictation here. Once on, what it does where (styles
  // by app). Nothing is on until the person enables one.
  import { CLOUD_PROVIDERS, brandForModel, fmtSize } from './cleanupProviders.js';
  import StepHeader from './StepHeader.svelte';

  /** `setup`: onboarding's state (setupState.svelte.js). */
  let { setup } = $props();
</script>

<StepHeader title="Make it sound polished">
  Raw dictation is full of "um"s and false starts. Yap's <strong>AI cleanup</strong>
  strips filler, fixes punctuation, and resolves "no wait, I meant…" —
  <strong>privately on your PC</strong> (no account, no key, no cloud), or via
  a faster cloud provider with your own key if you prefer.
</StepHeader>

<div class="cleanup-box">
  {#if setup.cleanupEnabled}
    <div class="cleanup-done">✓ AI cleanup is on — <strong>{setup.enabledSummary}</strong>.</div>
    <!-- Styles by app (cleanup_styles.rs): what it does where, out of the box. -->
    <p class="fine styles-intro">It writes each kind of app its own way:</p>
    <ul class="styles-preview" aria-label="Styles by app">
      {#each [['Email', 'formal'], ['Work and personal chats', 'casual'], ['Documents', 'formal'], ['AI prompts', 'word for word'], ['Terminals', 'commands as said'], ['Everything else', 'your words, tidied']] as [kind, tone] (kind)}
        <li><strong>{kind}</strong> · {tone}</li>
      {/each}
    </ul>
    <button class="skip" onclick={() => (setup.cleanupEnabled = false)}>
      Choose a different model or provider →
    </button>
    <p class="fine">Or change it any time in Settings → Language Models.</p>
  {:else if setup.llmInstalling}
    <div class="cleanup-progress">
      <span>
        Downloading the {setup.llmProgress.stage === 'runtime' ? 'engine' : 'model'}…
        {setup.llmProgress.percent || 0}%
      </span>
      <div class="bar"><span style="width:{setup.llmProgress.percent || 0}%"></span></div>
    </div>
  {:else}
    <div class="cleanup-demo" aria-hidden="true">
      <div class="demo-raw">"so um, I went to the shop and uh, no wait — I mean I got milk"</div>
      <div class="demo-arrow">↓</div>
      <div class="demo-clean">"I went to the shop and got milk."</div>
    </div>

    <div class="mode-cards">
      <button
        class="mode-card"
        class:sel={setup.cleanupMode === 'local'}
        data-recommended={setup.recommendLocal ? 'yes' : 'no'}
        onclick={() => ((setup.cleanupMode = 'local'), (setup.modePicked = true))}
      >
        <span class="mode-title">🔒 Private</span>
        <span class="mode-sub">{setup.recommendLocal ? 'runs on your PC · no account' : 'runs on your PC · slower without a graphics card'}</span>
        {#if setup.recommendLocal}<span class="mode-badge">Recommended for this PC</span>{/if}
      </button>
      <button
        class="mode-card"
        class:sel={setup.cleanupMode === 'cloud'}
        data-recommended={setup.recommendLocal ? 'no' : 'yes'}
        onclick={() => ((setup.cleanupMode = 'cloud'), (setup.modePicked = true))}
      >
        <span class="mode-title">☁ Cloud</span>
        <span class="mode-sub">bring your own key · Groq's is free</span>
        {#if !setup.recommendLocal}<span class="mode-badge">{setup.advice?.cleanup === 'cloud' ? 'Quicker on this PC' : 'Recommended for this PC'}</span>{/if}
      </button>
    </div>
    {#if setup.advice}
      <!-- What each adds to a dictation here (hardware.rs, measured with yap.exe bench). -->
      <p class="fine cleanup-times" data-testid="cleanup-times" data-pick={setup.advice.cleanup}>
        On this PC, private cleanup adds about {(setup.advice.localMs / 1000).toFixed(1)} s to each dictation;
        a cloud provider about {(setup.advice.cloudMs / 1000).toFixed(1)} s.
        {#if setup.advice.cleanup === 'cloud'}Or skip it: dictation works without.{/if}
      </p>
    {/if}

    {#if setup.cleanupMode === 'local'}
      <div class="llm-list">
        {#each setup.shownLocal as m (m.id)}
          {@const b = brandForModel(m.id)}
          <label class="llm-row" class:sel={setup.localPick === m.id} data-model={m.id}>
            <input type="radio" bind:group={setup.localPick} value={m.id} name="llm" onchange={() => (setup.localPicked = true)} />
            <span class="brand" style="background:{b.color}">
              {#if b.icon}<img src={b.icon} alt="" />{:else}{b.letter}{/if}
            </span>
            <span class="llm-main">
              <span class="llm-name">
                {m.display}
                {#if m.recommended}<span class="llm-badge">Recommended for this PC</span>{/if}
              </span>
              <span class="llm-blurb">{m.blurb}</span>
            </span>
            <span class="llm-size">{m.installed ? '✓ installed' : fmtSize(m.sizeMb)}</span>
          </label>
        {/each}
        {#if setup.shownLocal.length < setup.localModels.length}
          <button class="more-models" onclick={() => (setup.allLocal = true)}>
            More models ({setup.localModels.length - setup.shownLocal.length})
          </button>
        {/if}
      </div>
      {@const pick = (setup.llm.curated || []).find((m) => m.id === setup.localPick)}
      <button class="start wide" onclick={setup.enableLocalCleanup} disabled={!pick}>
        {#if pick && !pick.installed}
          Download {pick.display} ({fmtSize(pick.sizeMb)}) & enable
        {:else if pick}
          Enable {pick.display}
        {:else}
          Enable private AI cleanup
        {/if}
      </button>
      <p class="fine llm-why" data-gpu={setup.llm.gpuCapable ? 'yes' : 'no'}>
        {#if setup.llm.gpuCapable && setup.llm.gpu}
          Your {setup.llm.gpu.name} runs Qwen3.5 4B quickly, and it can use Chat's tools: your notes, apps and the web.
        {:else}
          Qwen2.5 1.5B stays quick without a graphics card. Bigger models are slow on a processor.
        {/if}
      </p>
      <p class="fine">All of these run fully offline via llamafile. Prefer your own?
        Drop any GGUF into the models folder later (Settings → Language Models).</p>
    {:else}
      <div class="cloud-form">
        <div class="prov-grid">
          {#each CLOUD_PROVIDERS as p (p.value)}
            <button
              class="prov-chip"
              class:sel={setup.cloudProvider === p.value}
              onclick={() => {
                setup.cloudProvider = p.value;
                setup.onCloudProviderChange();
              }}
            >
              <span class="brand" style="background:{p.brand.color}">
                {#if p.brand.icon}<img src={p.brand.icon} alt="" />{:else}{p.brand.letter}{/if}
              </span>
              <span class="prov-label">{p.label}</span>
            </button>
          {/each}
        </div>
        {#if setup.cloudProvider === 'custom'}
          <input class="cloud-inp" placeholder="Base URL (https://…/v1)" bind:value={setup.cloudBaseUrl} />
        {/if}
        <input class="cloud-inp" type="password" placeholder="API key" bind:value={setup.cloudKey} />
        <input class="cloud-inp" placeholder="Model (e.g. openai/gpt-oss-20b)" bind:value={setup.cloudModel} />
      </div>
      <button class="start wide" onclick={setup.enableCloudCleanup}>Enable cloud cleanup</button>
      <p class="fine" data-privacy="cloud">Your voice never leaves this PC: Yap transcribes it here, and only the text of
        each dictation goes to the provider you pick, to be cleaned up. Your key stays on this PC. Groq's free plan
        is plenty for dictation.</p>
    {/if}
  {/if}
  {#if setup.llmError}<p class="error">{setup.llmError}</p>{/if}
</div>

<style>
  /* Cleanup step */
  .cleanup-box {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-start;
    gap: 14px;
    padding: 6px 2px;
  }
  .mode-cards {
    display: flex;
    gap: 10px;
    width: 100%;
    max-width: 440px;
  }
  .mode-card {
    position: relative;
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
    border-radius: 10px;
    padding: 14px 10px 12px;
    color: var(--yap-fg);
    font: inherit;
    cursor: pointer;
    box-shadow: var(--yap-shadow-sm);
    transition: border-color 0.12s ease;
  }
  .mode-card.sel {
    border-color: var(--yap-primary);
    background: var(--yap-primary-wash);
  }
  .mode-title {
    font-size: 14px;
    font-weight: 600;
  }
  .mode-sub {
    font-size: 11.5px;
    color: var(--yap-muted);
  }
  .mode-badge {
    position: absolute;
    top: -8px;
    right: 8px;
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
    font-size: 10px;
    font-weight: 600;
    border-radius: 5px;
    padding: 1px 6px;
  }
  .llm-list {
    width: 100%;
    max-width: 440px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .llm-row {
    display: flex;
    align-items: center;
    gap: 10px;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
    border-radius: 8px;
    padding: 8px 12px;
    cursor: pointer;
  }
  .llm-row.sel {
    border-color: var(--yap-primary);
    background: var(--yap-primary-wash);
  }
  .llm-row input {
    accent-color: var(--yap-primary);
  }
  .llm-main {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .llm-name {
    font-size: 13px;
    font-weight: 500;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .llm-badge {
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
    font-size: 10px;
    font-weight: 600;
    border-radius: 5px;
    padding: 1px 6px;
    white-space: nowrap;
  }
  .llm-blurb {
    font-size: 11.5px;
    color: var(--yap-muted-70);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .llm-size {
    flex: 0 0 auto;
    font-size: 11.5px;
    color: var(--yap-muted);
    font-variant-numeric: tabular-nums;
  }
  /* Brand badge — coloured rounded square w/ white glyph (or letter). */
  .brand {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 6px;
    color: #fff;
    font-size: 12px;
    font-weight: 700;
  }
  .brand img {
    width: 13px;
    height: 13px;
    display: block;
  }
  .cloud-form {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
    max-width: 440px;
  }
  .prov-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 6px;
  }
  .prov-chip {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
    border-radius: 8px;
    padding: 8px 10px;
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
    cursor: pointer;
    text-align: left;
    transition: border-color 0.12s ease;
  }
  .prov-chip.sel {
    border-color: var(--yap-primary);
    background: var(--yap-primary-wash);
  }
  .prov-label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .cloud-inp {
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: 7px;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    padding: 7px 10px;
  }
  .cloud-inp:focus {
    outline: none;
    border-color: var(--yap-primary);
  }
  .cleanup-demo {
    text-align: center;
    font-size: 13px;
    line-height: 1.7;
  }
  .demo-raw {
    color: var(--yap-muted);
    font-style: italic;
  }
  .demo-arrow {
    color: var(--yap-primary);
    font-size: 15px;
  }
  .demo-clean {
    color: var(--yap-fg);
    font-weight: 500;
  }
  .cleanup-done {
    color: var(--yap-success);
    font-size: 14px;
    text-align: center;
    line-height: 1.6;
  }
  .styles-intro {
    margin: 10px 0 4px;
  }
  .styles-preview {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 6px;
    max-width: 460px;
  }
  .styles-preview li {
    padding: 3px 10px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised-soft);
    border: 1px solid var(--yap-border-subtle);
    font-size: 12px;
    color: var(--yap-fg-80);
  }
  .cleanup-progress {
    width: 100%;
    max-width: 380px;
    font-size: 13px;
    color: var(--yap-muted);
    text-align: center;
  }
  .bar {
    margin-top: 8px;
    height: 6px;
    border-radius: 3px;
    background: var(--yap-raised);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--yap-primary);
    transition: width 0.2s ease;
  }
  .fine {
    color: var(--yap-muted-70);
    font-size: 11.5px;
    text-align: center;
    max-width: 380px;
    line-height: 1.5;
    margin: 0;
  }
  .more-models {
    align-self: flex-start;
    margin: 2px 0 0 4px;
    padding: 4px 6px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-fg-62);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .more-models:hover {
    color: var(--yap-fg);
    background: var(--yap-s3);
  }
  .more-models:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }

  .error {
    color: var(--yap-danger);
    font-size: 12px;
    text-align: center;
    margin: 12px 0 0;
  }
  /* The footer's buttons, as they are here too (Onboarding.svelte). */
  .skip {
    background: none;
    border: none;
    color: var(--yap-muted-70);
    font-size: 12.5px;
    cursor: pointer;
    padding: 8px 4px;
  }
  .skip:hover:not(:disabled) {
    color: var(--yap-fg);
  }
  /* Primary CTA = ink filled button, matching the main window. */
  .start {
    border: none;
    border-radius: 9px;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    font-size: 14px;
    font-weight: 600;
    padding: 11px 20px;
    cursor: pointer;
    transition: background 0.15s ease;
  }
  .start:hover:not(:disabled) {
    background: var(--yap-ink-hover);
  }
  .start:disabled {
    background: var(--yap-raised);
    color: var(--yap-muted);
    cursor: default;
  }
  .start.wide {
    width: 100%;
    max-width: 380px;
  }
</style>
