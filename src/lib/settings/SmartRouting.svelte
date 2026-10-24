<script>
  // Settings → Language Models → Dictation cleanup → Smart routing: only
  // clean up in apps with a rule, reusable profiles (each with its own model
  // if it likes), and a profile per app (Yap's own Selects). The person's own
  // profiles; Styles by app's (a profile with a `category`) are
  // CleanupStyles.svelte's, and a rule can name one ("Style: Email").
  //
  //   cfg: Settings' config $state (auto-save does the saving).
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Select from '../ui/Select.svelte';
  import Button from '../ui/Button.svelte';
  import Textarea from '../ui/Textarea.svelte';
  import EndpointModels from '../EndpointModels.svelte';
  import { PP_CLOUD_MODELS } from '../ppModels.js';
  import { modelsFor } from '../modelCatalog.svelte.js';
  import { PP_PROVIDERS } from '../providers.js';
  import { PP_PRESETS, PRESET_PICKS } from './cleanupPresets.js';
  import { newId, prettyAppLabel } from './profiles.js';
  import { lm, loadRecentApps } from './languageModels.svelte.js';
  import { refreshLocalLlm } from './localLlm.svelte.js';

  let { cfg } = $props();

  // "New from a preset…": back to its placeholder once picked.
  let presetPick = $state('');

  // ---- Profiles library ----
  // The person's own profiles; the styles by app (a profile with a
  // `category`) are Styles by app's (settings/CleanupStyles.svelte).
  const ownProfiles = $derived((cfg.cleanupProfiles || []).filter((p) => !p.category));
  const styleProfiles = $derived((cfg.cleanupProfiles || []).filter((p) => p.category));
  function addProfile(seedBody = '', name = '') {
    const id = newId();
    const n = name || `Profile ${ownProfiles.length + 1}`;
    // provider '' = inherit the global AI-cleanup model (the default).
    cfg.cleanupProfiles = [
      ...cfg.cleanupProfiles,
      { id, name: n, prompt: seedBody, provider: '', baseUrl: '', model: '', apiKey: '' },
    ];
    return id;
  }

  const PROFILE_PROVIDERS = [
    { value: '', label: 'Model: the cleanup model' },
    ...PP_PROVIDERS.map((p) => ({ value: p.value, label: `Model: ${p.label}` })),
  ];
  // Picking a per-profile provider fills its base-URL default (mirrors the
  // global onProviderChange); clearing back to "global" wipes the override so
  // stale URLs/keys can't linger invisibly.
  function onProfileProviderChange(prof) {
    if (!prof.provider) {
      prof.baseUrl = '';
      prof.model = '';
      prof.apiKey = '';
      return;
    }
    const preset = PP_PROVIDERS.find((p) => p.value === prof.provider);
    if (preset && preset.baseUrl) prof.baseUrl = preset.baseUrl;
    if (prof.provider === 'ondevice') refreshLocalLlm();
  }
  function addProfileFromPreset(value) {
    const preset = PP_PRESETS.find((p) => p.value === value);
    if (!preset || preset.value === 'custom') return addProfile('', '');
    addProfile(preset.body ?? '', preset.label);
  }
  function removeProfile(id) {
    cfg.cleanupProfiles = cfg.cleanupProfiles.filter((p) => p.id !== id);
    // Unbind any rules that pointed at it (they fall back to the global default).
    for (const r of cfg.appRoutes) if (r.profileId === id) r.profileId = '';
  }

  // ---- App rules ----
  function addRoute(app) {
    const name = (app || lm.newRouteApp || '').trim();
    if (!name) return;
    if ((cfg.appRoutes || []).some((r) => (r.app || '').toLowerCase() === name.toLowerCase())) return;
    // Ensure at least one profile exists to bind to (seed from the global body).
    let profileId = ownProfiles[0]?.id;
    if (!profileId) profileId = addProfile(cfg.ppPrompt || '', 'Default');
    cfg.appRoutes = [
      ...(cfg.appRoutes || []),
      { app: name, label: prettyAppLabel(name), profileId, prompt: '' },
    ];
    lm.newRouteApp = '';
    lm.recentApps = lm.recentApps.filter((a) => a.toLowerCase() !== name.toLowerCase());
  }
  function removeRoute(i) {
    cfg.appRoutes = cfg.appRoutes.filter((_, j) => j !== i);
    loadRecentApps();
  }
</script>

<Group title="Smart routing" id="settings-smart-routing">
  <Row
    label="Only clean up in apps with a rule"
    desc="Dictations stay as they are everywhere else"
    hint="Off: the cleanup above applies everywhere, and rules change it per app."
    setting="smart-routing"
  >
    <Toggle
      checked={cfg.routingScope === 'selected_apps_only'}
      ariaLabel="Only clean up in apps with a rule"
      onchange={(v) => (cfg.routingScope = v ? 'selected_apps_only' : 'all_apps')}
    />
  </Row>
  <Row>
    {#snippet children()}
      <div class="routes">
        <div class="routes-sub">Profiles</div>
        <p class="note">
          Cleanup styles to reuse: terse for chat, formal for email, careful with code.
          Change one and every app using it follows.
        </p>
        {#if ownProfiles.length > 0}
          {#each ownProfiles as prof (prof.id)}
            <div class="route">
              <div class="route-head">
                <input class="route-label" placeholder="Profile name" bind:value={prof.name} />
                <span class="route-proc"></span>
                <button class="rm" title="Delete profile" aria-label="Delete profile" onclick={() => removeProfile(prof.id)}>×</button>
              </div>
              <Textarea bind:value={prof.prompt} rows={3} />
              <div class="prof-model">
                <Select
                  bind:value={prof.provider}
                  options={PROFILE_PROVIDERS}
                  ariaLabel="Which AI model runs this profile"
                  wide
                  onchange={() => onProfileProviderChange(prof)}
                />
                {#if prof.provider && prof.provider !== 'ondevice'}
                  <input class="route-input prof-inp" placeholder="Base URL (https://…/v1)" bind:value={prof.baseUrl} />
                  <input
                    class="route-input prof-inp"
                    placeholder="Model (e.g. openai/gpt-oss-20b)"
                    list="prof-models-{prof.id}"
                    bind:value={prof.model}
                  />
                  <input class="route-input prof-inp" type="password" placeholder="API key" bind:value={prof.apiKey} />
                  {#if PP_CLOUD_MODELS[prof.provider]}
                    <!-- The provider's models as Yap knows them now (its live list merged in). -->
                    <datalist id="prof-models-{prof.id}">
                      {#each [...modelsFor(prof.provider, prof.model).items, ...modelsFor(prof.provider, prof.model).more] as m (m.value)}
                        <option value={m.value}>{m.label}</option>
                      {/each}
                    </datalist>
                  {:else if prof.provider === 'local' || prof.provider === 'custom'}
                    <EndpointModels
                      id="prof-models-{prof.id}"
                      provider={prof.provider}
                      scope="profile:{prof.id}"
                      baseUrl={prof.baseUrl}
                      model={prof.model}
                    />
                  {/if}
                {:else if prof.provider === 'ondevice'}
                  <span class="prof-note">Runs on the on-device model: nothing leaves this PC, no key needed.</span>
                {/if}
              </div>
            </div>
          {/each}
        {:else}
          <div class="empty">No profiles yet. Add one, then give it to an app below.</div>
        {/if}
        <div class="route-add">
          <Select
            bind:value={presetPick}
            options={PRESET_PICKS}
            ariaLabel="New profile from a preset"
            onchange={(v) => {
              if (v) addProfileFromPreset(v);
              presetPick = '';
            }}
          />
          <Button variant="secondary" size="sm" onclick={() => addProfile()}>Blank profile</Button>
        </div>
      </div>
    {/snippet}
  </Row>
  <Row>
    {#snippet children()}
      <div class="routes">
        <div class="routes-sub">App rules</div>
        <p class="note">
          A profile for each app: Yap goes by the app you were in when you started dictating. A rule wins
          over Styles by app.
        </p>
        {#if cfg.appRoutes.length > 0}
          {#each cfg.appRoutes as route, i (i)}
            <div class="route-rule">
              <input class="route-label" placeholder="App name" bind:value={route.label} />
              <span class="route-proc">{route.app}</span>
              <span class="grow">
                <Select
                  bind:value={route.profileId}
                  options={[
                    { value: '', label: 'Default cleanup' },
                    ...ownProfiles.map((p) => ({ value: p.id, label: p.name })),
                    ...styleProfiles.map((p) => ({ value: p.id, label: `Style: ${p.name}` })),
                  ]}
                  ariaLabel={`Profile for ${route.label || route.app}`}
                  fill
                />
              </span>
              <button class="rm" title="Remove rule" aria-label="Remove rule" onclick={() => removeRoute(i)}>×</button>
            </div>
          {/each}
        {:else}
          <div class="empty">No per-app rules yet.</div>
        {/if}
        <div class="route-add">
          {#if lm.recentApps.length > 0}
            <Select
              bind:value={lm.newRouteApp}
              options={[{ value: '', label: 'Recent apps…' }, ...lm.recentApps.map((a) => ({ value: a, label: a }))]}
              ariaLabel="An app you've dictated into"
            />
          {/if}
          <input
            class="route-input"
            placeholder="or type an app, e.g. slack.exe"
            aria-label="App"
            bind:value={lm.newRouteApp}
          />
          <Button variant="secondary" size="sm" disabled={!lm.newRouteApp.trim()} onclick={() => addRoute()}>Add rule</Button>
        </div>
      </div>
    {/snippet}
  </Row>
</Group>

<style>
  .note {
    color: var(--yap-muted);
    font-size: 12.5px;
    margin: 0 0 10px;
    line-height: 1.5;
  }
  .rm {
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--yap-fg-62);
    cursor: pointer;
    font-size: 17px;
    line-height: 1;
  }
  .rm:hover {
    color: var(--yap-danger);
  }
  .rm:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .empty {
    color: var(--yap-muted);
    font-size: 12.5px;
    padding: 4px 0 8px;
  }
  /* Smart routing (profiles + per-app rules) */
  .routes {
    width: 100%;
  }
  .routes-sub {
    color: var(--yap-fg);
    font-size: 13.5px;
    font-weight: 560;
    margin-bottom: 2px;
  }
  .route-rule {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .route-rule .route-proc {
    flex: 0 1 auto;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .route-rule .grow {
    flex: 1 1 auto;
    min-width: 0;
  }
  .route {
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    padding: 10px 12px;
    margin-bottom: 10px;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
  }
  .route-head {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }
  .route-label {
    flex: 0 0 auto;
    width: 150px;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    color: var(--yap-fg);
    padding: 6px 9px;
    font: inherit;
    font-size: 12.5px;
  }
  .route-label:focus {
    outline: none;
  }
  .route-label:focus-visible {
    box-shadow: var(--yap-focus);
  }
  .route-proc {
    flex: 1 1 auto;
    color: var(--yap-muted);
    font-size: 12px;
    font-family: ui-monospace, monospace;
  }
  /* Per-profile model override (provider select + endpoint fields) */
  .prof-model {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 8px;
  }
  .prof-inp {
    flex: 1 1 130px;
    min-width: 0;
    font-size: 12px;
  }
  .prof-note {
    color: var(--yap-muted);
    font-size: 12.5px;
  }
  .route-add {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .route-input {
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    color: var(--yap-fg);
    height: 32px;
    box-sizing: border-box;
    padding: 0 10px;
    font: inherit;
    font-size: 12.5px;
  }
  .route-input {
    flex: 1 1 160px;
    min-width: 140px;
  }
  .route-input:focus {
    outline: none;
  }
  .route-input:focus-visible {
    box-shadow: var(--yap-focus);
  }
</style>
