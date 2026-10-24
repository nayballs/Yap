<script>
  // Settings → Voice Assistant → Opening apps (apps.rs): "open Spotify" in
  // Chat or to the Voice Assistant opens an app installed on this PC.
  // - "Let Yap open apps when you ask" (cfg.openApps, on by default);
  // - nicknames: say "my editor" → open Visual Studio Code
  //   (cfg.appNicknames: { say, app: the app's id, name });
  // - the apps Yap found (the Start menu's list, `apps_list`), searchable,
  //   each with its own icon (`apps_icons`, as Explorer draws it) and a
  //   switch: off = "Don't open this one" (cfg.appsBlocked: ids);
  // - Refresh (index again now).
  //
  //   cfg: Settings' config $state. Auto-save does the saving.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount } from 'svelte';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import Button from '../ui/Button.svelte';

  let { cfg } = $props();

  /** The apps Yap can open: [{ id, name }], sorted by name. */
  let apps = $state([]);
  let loaded = $state(false);
  let refreshing = $state(false);
  let loadError = $state('');
  let query = $state('');
  let showAll = $state(false);
  /** id → PNG data URL (an app with none: its letter). */
  let icons = $state({});
  const asked = new Set();
  const FIRST = 40;

  async function load(refresh = false) {
    refreshing = refresh;
    try {
      apps = await invoke('apps_list', { refresh });
      loadError = '';
    } catch (e) {
      loadError = String(e);
    } finally {
      loaded = true;
      refreshing = false;
    }
  }
  onMount(() => load(false));

  const blocked = $derived(new Set(cfg.appsBlocked ?? []));
  function setAllowed(id, allowed) {
    const rest = (cfg.appsBlocked ?? []).filter((x) => x !== id);
    cfg.appsBlocked = allowed ? rest : [...rest, id];
  }

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return q ? apps.filter((a) => a.name.toLowerCase().includes(q)) : apps;
  });
  const shown = $derived(showAll || query.trim() ? filtered.slice(0, 400) : filtered.slice(0, FIRST));

  // Icons for the rows on show, a batch at a time.
  $effect(() => {
    const ids = shown.map((a) => a.id).filter((id) => !asked.has(id));
    if (!ids.length) return;
    ids.forEach((id) => asked.add(id));
    invoke('apps_icons', { ids })
      .then((got) => {
        if (got && Object.keys(got).length) icons = { ...icons, ...got };
      })
      .catch(() => {});
  });

  // ---- nicknames ----
  const nicknames = $derived(cfg.appNicknames ?? []);
  let say = $state('');
  let appText = $state('');
  let appPick = $state(null); // { id, name }
  let pickerOpen = $state(false);
  let nickError = $state('');

  const suggestions = $derived.by(() => {
    const q = appText.trim().toLowerCase();
    if (!q || (appPick && appPick.name.toLowerCase() === q)) return [];
    return apps.filter((a) => a.name.toLowerCase().includes(q)).slice(0, 6);
  });

  function choose(app) {
    appPick = app;
    appText = app.name;
    pickerOpen = false;
  }

  function addNickname() {
    const words = say.trim().replace(/\s+/g, ' ');
    if (!words) {
      nickError = 'Type what you’ll say';
      return;
    }
    const pick = appPick && appPick.name === appText ? appPick : apps.find((a) => a.name.toLowerCase() === appText.trim().toLowerCase());
    if (!pick) {
      nickError = 'Pick an app from the list';
      return;
    }
    if (nicknames.some((n) => n.say.trim().toLowerCase() === words.toLowerCase())) {
      nickError = `“${words}” is already a nickname`;
      return;
    }
    cfg.appNicknames = [...nicknames, { say: words, app: pick.id, name: pick.name }];
    say = '';
    appText = '';
    appPick = null;
    nickError = '';
  }

  function removeNickname(i) {
    cfg.appNicknames = nicknames.filter((_, j) => j !== i);
  }

  /** A nickname's app, by its name now (or when it was made, if it's gone). */
  function nicknameApp(n) {
    const app = apps.find((a) => a.id === n.app);
    return app ? app.name : `${n.name || 'An app'} (not on this PC now)`;
  }
</script>

{#snippet appIcon(app)}
  <span class="appicon" aria-hidden="true">
    {#if icons[app.id]}
      <img src={icons[app.id]} alt="" width="20" height="20" />
    {:else}
      <span class="letter">{(app.name.match(/[\p{L}\p{N}]/u)?.[0] ?? '?').toUpperCase()}</span>
    {/if}
  </span>
{/snippet}

<div id="settings-opening-apps">
  <Group title="Opening apps">
    <Row setting="open-apps">
      <Toggle
        bind:checked={cfg.openApps}
        label="Let Yap open apps when you ask"
        desc="Say “open Spotify” to the Voice Assistant, or type it in Chat. Yap only opens apps installed on this PC"
        hint="From the list below: never a file, a link or a command."
      />
    </Row>

    <Row setting="app-nicknames">
      <div class="block" class:off={!cfg.openApps} role="group" aria-label="Nicknames">
        <span class="blabel">Nicknames</span>
        <span class="bdesc">Say a nickname, and Yap opens that app.</span>
        {#if nicknames.length}
          <ul class="nicks">
            {#each nicknames as n, i (n.say + n.app)}
              <li class="nick">
                <span class="said">Say <q>{n.say}</q></span>
                <span class="arrow" aria-hidden="true">→</span>
                <span class="opens">opens {nicknameApp(n)}</span>
                <button class="rm" aria-label={`Remove nickname ${n.say}`} title="Remove" onclick={() => removeNickname(i)}>×</button>
              </li>
            {/each}
          </ul>
        {/if}
        <div class="addnick">
          <input
            class="field"
            placeholder="Say… e.g. my editor"
            aria-label="Nickname to say"
            bind:value={say}
            disabled={!cfg.openApps}
            onkeydown={(e) => e.key === 'Enter' && addNickname()}
          />
          <span class="arrow" aria-hidden="true">→</span>
          <div class="picker">
            <input
              class="field"
              placeholder="App to open"
              aria-label="App to open"
              role="combobox"
              aria-expanded={pickerOpen && suggestions.length > 0}
              aria-controls="open-apps-suggestions"
              aria-autocomplete="list"
              bind:value={appText}
              disabled={!cfg.openApps}
              oninput={() => {
                appPick = null;
                pickerOpen = true;
              }}
              onfocus={() => (pickerOpen = true)}
              onblur={() => setTimeout(() => (pickerOpen = false), 150)}
              onkeydown={(e) => {
                if (e.key === 'Enter') {
                  if (suggestions.length && !appPick) choose(suggestions[0]);
                  else addNickname();
                }
              }}
            />
            {#if pickerOpen && suggestions.length}
              <ul class="suggest" id="open-apps-suggestions" role="listbox" aria-label="Apps">
                {#each suggestions as app (app.id)}
                  <li role="option" aria-selected="false">
                    <button type="button" onmousedown={(e) => e.preventDefault()} onclick={() => choose(app)}>
                      {@render appIcon(app)}
                      {app.name}
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
          <Button variant="secondary" size="sm" disabled={!cfg.openApps} onclick={addNickname}>Add</Button>
        </div>
        {#if nickError}<p class="err" role="alert">{nickError}</p>{/if}
      </div>
    </Row>

    <Row>
      <div class="block" class:off={!cfg.openApps} role="group" aria-label="Apps Yap found">
        <div class="bhead">
          <span class="lhs">
            <span class="blabel">Apps Yap found</span>
            <span class="bdesc">
              {#if !loaded}
                Looking at the Start menu…
              {:else if loadError}
                Couldn’t read the Start menu: {loadError}
              {:else}
                {apps.length} {apps.length === 1 ? 'app' : 'apps'} in the Start menu. Switch one off and Yap won’t open it.
              {/if}
            </span>
          </span>
          <Button variant="secondary" size="sm" disabled={refreshing} onclick={() => load(true)}>
            {refreshing ? 'Refreshing…' : 'Refresh'}
          </Button>
        </div>
        {#if apps.length}
          <input class="field search" type="search" placeholder="Search apps" aria-label="Search apps" bind:value={query} />
          <div class="applist">
            {#each shown as app (app.id)}
              <div class="approw" class:blocked={blocked.has(app.id)}>
                {@render appIcon(app)}
                <Toggle
                  bind:checked={() => !blocked.has(app.id), (on) => setAllowed(app.id, on)}
                  label={app.name}
                  desc={blocked.has(app.id) ? 'Yap won’t open this one' : ''}
                  disabled={!cfg.openApps}
                />
              </div>
            {:else}
              <p class="none">No app called “{query.trim()}”.</p>
            {/each}
          </div>
          {#if !showAll && !query.trim() && filtered.length > FIRST}
            <button class="more" onclick={() => (showAll = true)}>Show all {filtered.length} apps</button>
          {/if}
        {/if}
      </div>
    </Row>
  </Group>
</div>

<style>
  .block {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .block.off {
    opacity: 0.6;
  }
  .bhead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
  }
  .lhs {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .blabel {
    color: var(--yap-fg);
    font-size: 13.5px;
    font-weight: 560;
    line-height: 1.4;
  }
  .bdesc {
    color: var(--yap-muted);
    font-size: 12.5px;
    line-height: 1.45;
  }
  .field {
    box-sizing: border-box;
    background: var(--yap-s1);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    color: var(--yap-fg);
    padding: 7px 10px;
    font: inherit;
    font-size: 12.5px;
    min-width: 0;
  }
  .field:focus {
    outline: none;
  }
  .field:focus-visible {
    box-shadow: var(--yap-focus);
  }
  .field:disabled {
    opacity: 0.6;
  }
  .nicks {
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .nick {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12.5px;
    color: var(--yap-fg);
    background: var(--yap-s1);
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    padding: 6px 6px 6px 10px;
  }
  .nick q {
    font-weight: 600;
  }
  .nick .opens {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-fg-80);
  }
  .arrow {
    color: var(--yap-muted);
  }
  .rm {
    background: none;
    border: none;
    color: var(--yap-muted);
    cursor: pointer;
    font-size: 17px;
    line-height: 1;
    padding: 0 6px;
  }
  .rm:hover {
    color: var(--yap-danger);
  }
  .addnick {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto minmax(0, 1.3fr) auto;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
  }
  .picker {
    position: relative;
    min-width: 0;
  }
  .picker .field {
    width: 100%;
  }
  .addnick > .field {
    width: 100%;
  }
  .suggest {
    position: absolute;
    z-index: 5;
    left: 0;
    right: 0;
    top: calc(100% + 4px);
    margin: 0;
    padding: 4px;
    list-style: none;
    background: var(--yap-s2);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    box-shadow: var(--yap-shadow-menu);
  }
  .suggest button {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    background: none;
    border: none;
    border-radius: var(--yap-r-sm);
    padding: 6px 8px;
    font: inherit;
    font-size: 12.5px;
    color: var(--yap-fg);
    cursor: pointer;
    text-align: left;
  }
  .suggest button:hover {
    background: var(--yap-s3);
  }
  .err {
    margin: 6px 0 0;
    font-size: 11.5px;
    color: var(--yap-danger);
  }
  .search {
    width: 100%;
    margin-top: 10px;
  }
  .applist {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(max(200px, calc((100% - 32px) / 2)), 1fr));
    gap: 9px 32px;
    margin-top: 12px;
  }
  .approw {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .approw :global(.toggle-row) {
    flex: 1;
    min-width: 0;
  }
  .approw :global(.toggle-row .label) {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .approw.blocked .appicon {
    opacity: 0.45;
  }
  .appicon {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
  }
  .appicon img {
    width: 20px;
    height: 20px;
    object-fit: contain;
  }
  .letter {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border-radius: 5px;
    background: var(--yap-raised);
    color: var(--yap-fg-62);
    font-size: 11px;
    font-weight: 700;
  }
  .none {
    margin: 0;
    font-size: 12px;
    color: var(--yap-muted);
  }
  .more {
    align-self: flex-start;
    margin-top: 10px;
    background: none;
    border: none;
    padding: 0;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    color: var(--yap-link);
    cursor: pointer;
  }
  .more:hover {
    text-decoration: underline;
  }
</style>
