<script>
  // Language Models → Dictation cleanup → **Styles by app**
  // (src-tauri/src/cleanup_styles.rs): "Match the style to the app"
  // (cfg.cleanupStyles), then a row per kind of app — Email, Work chat,
  // Personal chat, Documents, AI prompts, Terminal, Everything else — with
  // its tone (a Segmented), an example of what you say and what Yap types in
  // that tone (Wispr Flow's example cards), and under "Apps and
  // instructions" the kind's apps (take one out, add one: an app you've
  // dictated into or a site's name, moved here from any other kind) and
  // extra instructions added to the main cleanup prompt for these apps.
  // Each kind is a cleanup profile with a `category` in Settings' copy
  // (cfg.cleanupProfiles); auto-save does the saving.
  //
  //   cfg: Settings' config $state.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Segmented from '../ui/Segmented.svelte';
  import Textarea from '../ui/Textarea.svelte';
  import Button from '../ui/Button.svelte';
  import { appLabel } from '../core/appNames.js';
  import { EXAMPLES, TONE_LINES, appsOf, appsSummary, addApp, removeApp, newStyleProfile, norm } from './cleanupStyles.js';

  let { cfg } = $props();

  let catalog = $state([]);
  // Apps dictated into lately (history), for the add box's suggestions.
  let recent = $state([]);
  // What's typed in each kind's add box.
  let adding = $state({});

  onMount(async () => {
    try {
      catalog = (await invoke('cleanup_styles_catalog'))?.categories ?? [];
    } catch {
      catalog = [];
    }
    try {
      const entries = await invoke('get_history', { limit: 200 });
      const seen = new Map();
      for (const e of entries || []) {
        const app = (e.app || '').trim();
        if (app && !seen.has(app.toLowerCase())) seen.set(app.toLowerCase(), app);
      }
      recent = [...seen.values()].sort((a, b) => a.localeCompare(b));
    } catch {
      recent = [];
    }
  });

  // Every kind's profile in this copy (Rust adds them at load; a copy read
  // before that has none).
  $effect(() => {
    if (!catalog.length || !Array.isArray(cfg.cleanupProfiles)) return;
    const missing = catalog.filter((k) => !cfg.cleanupProfiles.some((p) => p.category === k.id));
    if (missing.length) cfg.cleanupProfiles = [...cfg.cleanupProfiles, ...missing.map(newStyleProfile)];
  });

  const profiles = $derived(
    Object.fromEntries((cfg.cleanupProfiles || []).filter((p) => p.category).map((p) => [p.category, p])),
  );
  const onlyRules = $derived(cfg.routingScope === 'selected_apps_only');

  function add(kind) {
    const name = (adding[kind.id] || '').trim();
    if (!name) return;
    addApp(catalog, profiles, kind, name);
    adding[kind.id] = '';
  }

  // An app's name as people know it ("slack.exe" → "Slack").
  const pretty = (name) => (/\.exe$/i.test(name) ? appLabel(name) || name : name);
</script>

{#snippet kindIcon(id)}
  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#if id === 'email'}
      <rect x="3" y="5" width="18" height="14" rx="2" /><path d="m3.5 7 8.5 6 8.5-6" />
    {:else if id === 'workChat'}
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /><path d="M8 9h8M8 13h5" />
    {:else if id === 'personalChat'}
      <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /><path d="M9 9.5h.01M15 9.5h.01M9.5 12.5a3.5 3.5 0 0 0 5 0" />
    {:else if id === 'documents'}
      <path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" /><path d="M14 3v6h6M8 13h8M8 17h6" />
    {:else if id === 'aiPrompts'}
      <path d="M11 3.5l1.7 4.3 4.3 1.7-4.3 1.7L11 15.5l-1.7-4.3L5 9.5l4.3-1.7z" /><path d="M18 14.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z" />
    {:else if id === 'terminal'}
      <rect x="3" y="4" width="18" height="16" rx="2" /><path d="m7 9 3 3-3 3M13 15h4" />
    {:else}
      <circle cx="6" cy="12" r="1.4" /><circle cx="12" cy="12" r="1.4" /><circle cx="18" cy="12" r="1.4" />
    {/if}
  </svg>
{/snippet}

<Group title="Styles by app" id="settings-cleanup-styles" desc="Yap writes in the right tone for where you are: emails formal, chats casual, AI prompts word for word">
  <Row setting="cleanup-styles">
    <Toggle
      bind:checked={cfg.cleanupStyles}
      label="Match the style to the app"
      desc={cfg.cleanupStyles
        ? 'Each kind of app below gets its own tone'
        : 'Off: every app gets the same cleanup, the prompt below'}
    />
  </Row>
  {#if cfg.cleanupStyles && onlyRules}
    <p class="note" role="note">
      “Only clean up in apps with a rule” is on (Smart routing, below): only apps with a rule are cleaned up. A rule can
      name a style.
    </p>
  {/if}
  {#if cfg.cleanupStyles && catalog.length}
    <div class="kinds" data-setting="cleanup-style-kinds">
      {#each catalog as kind (kind.id)}
        {@const p = profiles[kind.id]}
        {#if p}
          {@const tone = kind.tones.some((t) => t.id === p.tone) ? p.tone : kind.defaultTone}
          {@const apps = appsOf(kind, p)}
          {@const ex = EXAMPLES[kind.id]}
          <section class="kind" aria-labelledby="style-{kind.id}" data-kind={kind.id}>
            <div class="head">
              <span class="icon">{@render kindIcon(kind.id)}</span>
              <div class="title">
                <h3 id="style-{kind.id}">{kind.name}</h3>
                <p class="apps">{kind.id === 'other' ? 'Every other app' : appsSummary(apps.map((a) => pretty(a.name)))}</p>
              </div>
              <div class="tone">
                <Segmented
                  bind:value={p.tone}
                  options={kind.tones.map((t) => ({ value: t.id, label: t.label }))}
                  label="{kind.name} tone"
                />
              </div>
            </div>
            <p class="tone-line">{TONE_LINES[tone] || ''}</p>
            {#if ex}
              <div class="example" role="group" aria-label="Example: {kind.name}, {kind.tones.find((t) => t.id === tone)?.label}">
                <div class="said">
                  <span class="lab">You say</span>
                  <span class="said-text">“{ex.said}”</span>
                </div>
                <div class="typed">
                  <span class="lab">Yap types</span>
                  <span class="typed-text">{ex.tones[tone] ?? ''}</span>
                </div>
              </div>
            {/if}
            <details class="set-more">
              <summary>{kind.id === 'other' ? 'Instructions' : 'Apps and instructions'}</summary>
              <div class="more-body">
                {#if kind.id !== 'other'}
                  <ul class="chips" aria-label="{kind.name} apps">
                    {#each apps as app (app.name)}
                      <li class="chip">
                        <span>{pretty(app.name)}</span>
                        <button
                          class="x"
                          title="Take {pretty(app.name)} out of {kind.name}"
                          aria-label="Take {pretty(app.name)} out of {kind.name}"
                          onclick={() => removeApp(p, app)}>×</button
                        >
                      </li>
                    {/each}
                  </ul>
                  {#if (p.removedApps || []).length}
                    <p class="removed">
                      Taken out:
                      {#each p.removedApps as r, i (r)}
                        {i ? ', ' : ''}{r}
                        <button
                          class="link"
                          aria-label="Put {r} back in {kind.name}"
                          onclick={() => (p.removedApps = p.removedApps.filter((x) => norm(x) !== norm(r)))}>put back</button
                        >
                      {/each}
                    </p>
                  {/if}
                  <div class="add">
                    <input
                      class="add-input"
                      list="style-recent-{kind.id}"
                      placeholder="An app (slack.exe) or a site's name (Basecamp)"
                      aria-label="Add an app to {kind.name}"
                      bind:value={adding[kind.id]}
                      onkeydown={(e) => e.key === 'Enter' && add(kind)}
                    />
                    <datalist id="style-recent-{kind.id}">
                      {#each recent as r (r)}<option value={r}>{pretty(r)}</option>{/each}
                    </datalist>
                    <Button variant="secondary" size="sm" disabled={!(adding[kind.id] || '').trim()} onclick={() => add(kind)}>Add</Button>
                  </div>
                  <p class="hint">An app added here leaves the kind it was in. A site goes by the name its tab ends with (“Inbox – Gmail”).</p>
                {/if}
                <label class="instr-label" for="style-instr-{kind.id}">Extra instructions for {kind.id === 'other' ? 'every other app' : 'these apps'}</label>
                <div id="style-instr-{kind.id}">
                  <Textarea
                    bind:value={p.prompt}
                    rows={2}
                    ariaLabel="Extra instructions for {kind.name}"
                    placeholder={kind.id === 'email' ? 'e.g. Sign off with “Best wishes”.' : 'Added to the cleanup prompt for these apps'}
                  />
                </div>
              </div>
            </details>
          </section>
        {/if}
      {/each}
    </div>
  {/if}
</Group>

<style>
  .note {
    margin: 6px 0 10px;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-muted);
  }
  .kinds {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 10px;
  }
  .kind {
    background: var(--yap-s2);
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r-lg);
    padding: 12px 14px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 11px;
    flex-wrap: wrap;
  }
  .icon {
    flex: none;
    width: 30px;
    height: 30px;
    display: grid;
    place-items: center;
    border-radius: var(--yap-r);
    background: var(--yap-raised-soft);
    color: var(--yap-fg-80);
  }
  .icon :global(svg) {
    width: 17px;
    height: 17px;
  }
  .title {
    flex: 1 1 180px;
    min-width: 0;
  }
  h3 {
    margin: 0;
    font-size: 13.5px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .apps {
    margin: 1px 0 0;
    font-size: 12px;
    color: var(--yap-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tone {
    flex: none;
  }
  .tone-line {
    margin: 8px 0 0 41px;
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .example {
    margin: 8px 0 0 41px;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
  }
  .said,
  .typed {
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 8px 10px;
    border-radius: var(--yap-r);
    font-size: 12.5px;
    line-height: 1.5;
  }
  .said {
    background: var(--yap-raised-soft);
    color: var(--yap-fg-62);
  }
  .typed {
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
    color: var(--yap-fg);
  }
  .said-text {
    font-style: italic;
  }
  .typed-text {
    white-space: pre-wrap;
  }
  .lab {
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.02em;
    color: var(--yap-muted);
  }
  details.set-more {
    margin: 9px 0 0 41px;
  }
  .chips {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px 4px 2px 9px;
    border-radius: var(--yap-r-full);
    background: var(--yap-raised-soft);
    border: 1px solid var(--yap-border-subtle);
    font-size: 12px;
    color: var(--yap-fg-80);
  }
  .x {
    border: 0;
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
    font-size: 14px;
    line-height: 1;
    width: 18px;
    height: 18px;
    border-radius: var(--yap-r-full);
  }
  .x:hover {
    color: var(--yap-fg);
    background: var(--yap-s3);
  }
  .x:focus-visible,
  .link:focus-visible,
  .add-input:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .removed {
    margin: 8px 0 0;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .link {
    border: 0;
    background: none;
    padding: 0 2px;
    color: var(--yap-link);
    font: inherit;
    cursor: pointer;
  }
  .add {
    display: flex;
    gap: 6px;
    margin-top: 10px;
  }
  .add-input {
    flex: 1;
    min-width: 0;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    color: var(--yap-fg);
    padding: 5px 8px;
    font: inherit;
    font-size: 12.5px;
  }
  .hint {
    margin: 6px 0 0;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .instr-label {
    display: block;
    margin: 12px 0 4px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg-80);
  }
  @media (max-width: 720px) {
    .example {
      grid-template-columns: 1fr;
    }
  }
</style>
