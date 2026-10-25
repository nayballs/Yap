<script>
  // Settings → Dictation → My languages (dictation/languages.rs; Wispr Flow's
  // languages and its "Speaking a different language?" nudge, OpenWhispr's
  // dual-language hotkeys request):
  // - the languages you speak: a searchable list (`languages_catalog`), each
  //   one picked a chip with ✕, in the order picked;
  // - which installed speech model covers which (`my_languages_status`: your
  //   model first) and the one to recommend for them all. Yap never changes
  //   the model: Open Speech-to-Text does;
  // - the main shortcut's language (`selectedLanguage`): Auto — the likeliest
  //   of yours, as the model allows — or one of them;
  // - language shortcuts: a key that dictates in a language, or pastes it
  //   translated into another by the AI, checked against every other
  //   shortcut (`validateLanguageHotkey`), its live binding paused while a key
  //   is picked;
  // - "Tell me when I speak a language I didn't pick" (Whisper models only).
  //
  //   cfg:  Settings' config $state (auto-save does the saving).
  //   goto: opens another Settings page ("models").
  import { tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import Group from '../ui/Group.svelte';
  import Row from '../ui/Row.svelte';
  import Select from '../ui/Select.svelte';
  import Toggle from '../ui/Toggle.svelte';
  import HotkeyInput from '../ui/HotkeyInput.svelte';
  import { formatHotkeySpec } from '../core/hotkeys.js';
  import { onLanguageCapturing, validateLanguageHotkey } from './hotkeys.svelte.js';
  import {
    AUTO,
    MAX_LANGUAGE_SHORTCUTS,
    catalog,
    languageName,
    languageShortcuts,
    loadLanguageCatalog,
    myLanguages,
    newLanguageShortcut,
    shortcutName,
  } from './languages.svelte.js';

  let { cfg, goto = null } = $props();

  loadLanguageCatalog();

  const mine = $derived(myLanguages(cfg));
  const shortcuts = $derived(languageShortcuts(cfg));

  /** "English", "English and French", "English, French and German". */
  function names(codes, last = 'and') {
    const list = (codes || []).map(languageName);
    if (list.length <= 1) return list[0] || '';
    return `${list.slice(0, -1).join(', ')} ${last} ${list.at(-1)}`;
  }

  // ---- the languages you speak: a search box over the list ---------------
  let query = $state('');
  let open = $state(false);
  let active = $state(0);
  let inputEl = $state(null);
  const listId = $props.id();
  const matches = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return catalog.list
      .filter((l) => !mine.includes(l.code))
      .filter((l) => !q || l.name.toLowerCase().includes(q) || (l.native || '').toLowerCase().includes(q) || l.code === q)
      .slice(0, 60);
  });
  $effect(() => {
    query;
    active = 0;
  });

  function add(code) {
    if (!code || mine.includes(code)) return;
    cfg.myLanguages = [...mine, code];
    query = '';
    tick().then(() => inputEl?.focus());
  }

  function remove(code) {
    cfg.myLanguages = mine.filter((c) => c !== code);
    // The main shortcut fixed to it goes back to Auto.
    if (cfg.selectedLanguage === code) cfg.selectedLanguage = AUTO;
  }

  function onSearchKey(e) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      open = true;
      active = Math.min(active + 1, Math.max(matches.length - 1, 0));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      active = Math.max(active - 1, 0);
    } else if (e.key === 'Enter') {
      if (open && matches[active]) {
        e.preventDefault();
        add(matches[active].code);
      }
    } else if (e.key === 'Escape') {
      if (open || query) {
        e.preventDefault();
        e.stopPropagation();
        open = false;
        query = '';
      }
    }
  }

  // ---- your speech models: which covers which ------------------------------
  let status = $state(null);
  $effect(() => {
    const languages = [...mine];
    const model = cfg?.modelSize || '';
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
  const recommended = $derived(status?.recommended ?? null);

  /** What your model makes of your languages, in a sentence. */
  const coverage = $derived.by(() => {
    if (!model || !mine.length) return '';
    if (model.knows && model.tells === 'one') {
      const only = `${model.name}, your speech model, understands ${names(model.knows)} only.`;
      return missing.length ? `${only} ${names(missing)} ${missing.length === 1 ? 'needs' : 'need'} another model.` : only;
    }
    if (!missing.length) {
      return mine.length === 1
        ? `${model.name}, your speech model, understands ${names(mine)}.`
        : `${model.name}, your speech model, understands all of them.`;
    }
    return `${model.name}, your speech model, doesn't understand ${names(missing, 'or')}.`;
  });

  /** The model to switch to, when yours misses one. */
  const recommendation = $derived.by(() => {
    if (!mine.length || !missing.length) return '';
    if (!recommended) return 'No one speech model covers all of them.';
    const all = mine.length === 1 ? names(mine) : `all your languages`;
    return recommended.installed
      ? `${recommended.name} understands ${all}, and it's installed.`
      : `${recommended.name} understands ${all}.`;
  });

  /** How the main shortcut's Auto picks among them, with your model. */
  const autoDesc = $derived.by(() => {
    if (!model) return 'Auto: the language you speak, of yours';
    switch (model.tells) {
      case 'probabilities':
        return 'Auto: Whisper hears which of your languages you speak';
      case 'itself':
        return `Auto: ${model.name} recognises the language by itself`;
      case 'told':
        return `${model.name} can't tell languages apart: Auto dictates in ${languageName(mine.find((c) => model.covers.includes(c)) || 'en')}. Give your other languages a shortcut each`;
      default:
        return model.knows ? `${model.name} knows ${names(model.knows)} only` : 'Auto: the language you speak, of yours';
    }
  });

  const mainOptions = $derived([
    { value: AUTO, label: 'Auto' },
    ...mine.map((c) => ({ value: c, label: languageName(c) })),
  ]);
  const hotkeyName = $derived(formatHotkeySpec(cfg?.hotkey || '') || 'the dictation hotkey');

  // ---- language shortcuts ---------------------------------------------------
  let addEl = $state(null);

  function speakOptions(s) {
    const codes = [...mine];
    if (s.language && s.language !== AUTO && !codes.includes(s.language)) codes.push(s.language);
    return [{ value: AUTO, label: 'Any of my languages' }, ...codes.map((c) => ({ value: c, label: languageName(c) }))];
  }
  const pasteOptions = $derived([
    { value: '', label: 'As spoken' },
    ...catalog.list.map((l) => ({ value: l.code, label: `Translated into ${l.name}` })),
  ]);

  async function addShortcut() {
    // A second language of yours, else the first, else any.
    const language = mine[1] || mine[0] || AUTO;
    cfg.languageShortcuts = [...shortcuts, newLanguageShortcut(language)];
    await tick();
    document.querySelector(`[data-language-shortcut="${cfg.languageShortcuts.at(-1)?.id}"] select`)?.focus();
  }

  function removeShortcut(s) {
    cfg.languageShortcuts = shortcuts.filter((x) => x.id !== s.id);
    tick().then(() => addEl?.focus?.({ preventScroll: true }));
  }

  // The model a translation goes to: Commands', else cleanup's.
  const translating = $derived(shortcuts.some((s) => s.translateTo));
  const nudgeDesc = $derived(
    !model || model.tells === 'probabilities'
      ? 'After a dictation in another language, Yap offers to add it'
      : `Only Whisper models say which language they heard. ${model.name} doesn't, so Yap can't tell`
  );
</script>

<Group
  title="My languages"
  id="settings-languages"
  desc="The languages you dictate in. Yap tells them apart where your speech model can, and AI cleanup keeps each in the language you spoke."
>
  <Row label="Languages you speak" stack setting="my-languages">
    <div class="langs">
      {#if mine.length}
        <ul class="chips" aria-label="Your languages">
          {#each mine as code (code)}
            <li class="chip" data-language={code}>
              <span>{languageName(code)}</span>
              <button type="button" class="x" aria-label={`Remove ${languageName(code)}`} onclick={() => remove(code)}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      <div class="search">
        <input
          bind:this={inputEl}
          bind:value={query}
          type="search"
          role="combobox"
          aria-label="Add a language"
          aria-expanded={open}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={open && matches[active] ? `${listId}-${matches[active].code}` : undefined}
          placeholder={mine.length ? 'Add another language…' : 'Add a language…'}
          autocomplete="off"
          spellcheck="false"
          onfocus={() => (open = true)}
          onclick={() => (open = true)}
          oninput={() => (open = true)}
          onblur={() => (open = false)}
          onkeydown={onSearchKey}
        />
        {#if open}
          <ul class="list" id={listId} role="listbox" aria-label="Languages">
            {#each matches as l, i (l.code)}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <li
                id={`${listId}-${l.code}`}
                role="option"
                aria-selected={i === active}
                class:active={i === active}
                onmousedown={(e) => e.preventDefault()}
                onmouseenter={() => (active = i)}
                onclick={() => add(l.code)}
              >
                <span class="lname">{l.name}</span>
                {#if l.native && l.native !== l.name}<span class="native">{l.native}</span>{/if}
              </li>
            {:else}
              <li class="none" role="presentation">No language called “{query}”</li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>
  </Row>

  {#if mine.length}
    <Row label="Your speech model" stack setting="language-models">
      <div class="models">
        {#if coverage}
          <p class="line" class:warn={missing.length > 0} data-coverage>{coverage}</p>
        {/if}
        {#if recommendation}
          <p class="line rec" data-recommended={recommended?.id ?? ''}>
            {recommendation}
            {#if goto}
              <button type="button" class="set-link" onclick={() => goto('models')}>Open Speech-to-Text</button>
            {/if}
          </p>
        {/if}
        {#if status?.installed?.length && status.languages.length}
          <table class="cover" aria-label="Which speech model understands which of your languages">
            <thead>
              <tr>
                <th scope="col"><span class="vh">Model</span></th>
                {#each status.languages as l (l.code)}<th scope="col">{l.name}</th>{/each}
              </tr>
            </thead>
            <tbody>
              {#each status.installed as m (m.id)}
                <tr data-model={m.id}>
                  <th scope="row">{m.name}{#if m.active}<span class="yours">yours</span>{/if}</th>
                  {#each status.languages as l (l.code)}
                    {@const ok = m.covers.includes(l.code)}
                    <td class:ok aria-label={ok ? 'Yes' : 'No'}>{ok ? '✓' : '—'}</td>
                  {/each}
                </tr>
              {/each}
            </tbody>
          </table>
        {/if}
      </div>
    </Row>

    <Row label={`Main shortcut (${hotkeyName})`} desc={autoDesc} setting="main-language">
      <Select bind:value={cfg.selectedLanguage} options={mainOptions} ariaLabel="Main shortcut's language" />
    </Row>
  {/if}

  <div class="shortcuts" data-setting="language-shortcuts">
    <div class="sh-head">
      <span class="label">Language shortcuts</span>
      <span class="desc">A key of its own for a language, or to dictate in one language and paste another</span>
    </div>
    {#each shortcuts as s (s.id)}
      <div class="sc" data-language-shortcut={s.id}>
        <div class="pick">
          <Select bind:value={s.language} options={speakOptions(s)} ariaLabel="Language you speak" />
          <span class="arrow" aria-hidden="true">→</span>
          <Select bind:value={s.translateTo} options={pasteOptions} ariaLabel="Paste as" wide />
        </div>
        <div class="rhs">
          <HotkeyInput
            bind:value={s.hotkey}
            clearable
            validate={(spec) => validateLanguageHotkey(cfg, s.id, spec)}
            oncapturingchange={(on) => onLanguageCapturing(cfg, s.id, on)}
          />
          <button type="button" class="tool del" title="Delete" aria-label={`Delete the ${shortcutName(s)} shortcut`} onclick={() => removeShortcut(s)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 7h14M10 7V5h4v2M7 7l1 12h8l1-12" /></svg>
          </button>
        </div>
      </div>
    {/each}
    {#if shortcuts.length < MAX_LANGUAGE_SHORTCUTS}
      <button class="add" bind:this={addEl} type="button" data-setting="language-shortcut-add" onclick={addShortcut}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
        Add a language shortcut
      </button>
    {:else}
      <p class="full" data-setting="language-shortcut-add">That's {MAX_LANGUAGE_SHORTCUTS} language shortcuts, the most Yap keeps.</p>
    {/if}
    {#if translating}
      <p class="note">
        Translating uses your AI model: Commands' in Language Models, else Dictation Cleanup's. What you said stays one “Use what
        I said” away on the Yap bar.
      </p>
    {/if}
  </div>

  {#if mine.length}
    <Row setting="language-nudge">
      <Toggle bind:checked={cfg.languageNudge} label="Tell me when I speak a language I didn't pick" desc={nudgeDesc} />
    </Row>
  {/if}
</Group>

<style>
  .langs {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 28px;
    padding: 0 4px 0 11px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    color: var(--yap-fg);
    font-size: 12.5px;
    font-weight: 560;
  }
  .x {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r-full);
    background: none;
    color: var(--yap-fg-62);
    cursor: pointer;
  }
  .x svg {
    width: 12px;
    height: 12px;
  }
  .x:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .x:focus-visible,
  .add:focus-visible,
  .tool:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .search {
    position: relative;
    width: 340px;
    max-width: 100%;
  }
  .search input {
    width: 100%;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-bg);
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    box-sizing: border-box;
  }
  .search input:hover {
    border-color: var(--yap-border-hover);
  }
  .search input:focus {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .list {
    position: absolute;
    z-index: 5;
    top: calc(100% + 4px);
    left: 0;
    right: 0;
    max-height: 240px;
    overflow-y: auto;
    margin: 0;
    padding: 4px;
    list-style: none;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-raised);
    box-shadow: var(--yap-shadow-menu);
  }
  .list li {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
    padding: 6px 8px;
    border-radius: var(--yap-r-sm);
    font-size: 13px;
    color: var(--yap-fg);
    cursor: pointer;
  }
  .list li.active {
    background: var(--yap-s3);
  }
  .list .native {
    color: var(--yap-muted);
    font-size: 12px;
  }
  .list li.none {
    color: var(--yap-muted);
    cursor: default;
  }
  .models {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
  }
  .line {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--yap-fg-80);
  }
  .line.warn {
    color: var(--yap-fg);
    font-weight: 560;
  }
  .rec .set-link {
    margin-left: 4px;
  }
  .cover {
    border-collapse: collapse;
    font-size: 12px;
    color: var(--yap-fg-80);
  }
  .cover th,
  .cover td {
    padding: 4px 10px 4px 0;
    text-align: left;
    font-weight: 500;
    border-bottom: 1px solid var(--yap-rule);
  }
  .cover thead th {
    color: var(--yap-muted);
    font-weight: 560;
  }
  .cover td {
    color: var(--yap-muted);
    text-align: center;
  }
  .cover td.ok {
    color: var(--yap-success-text);
    font-weight: 700;
  }
  .yours {
    margin-left: 6px;
    padding: 1px 6px;
    border-radius: var(--yap-r-full);
    background: var(--yap-s3);
    color: var(--yap-fg-62);
    font-size: 11px;
  }
  .vh {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  .shortcuts {
    display: flex;
    flex-direction: column;
    padding: 12px 0;
  }
  .sh-head {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin-bottom: 4px;
  }
  .sh-head .label {
    font-size: 13.5px;
    font-weight: 560;
    color: var(--yap-fg);
  }
  .sh-head .desc,
  .note,
  .full {
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--yap-muted);
  }
  .note,
  .full {
    margin: 10px 0 0;
  }
  .sc {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto;
    align-items: center;
    column-gap: 16px;
    row-gap: 8px;
    padding: 10px 0;
    border-bottom: 1px solid var(--yap-rule);
  }
  .pick {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    min-width: 0;
  }
  .arrow {
    color: var(--yap-muted);
  }
  .rhs {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: flex-end;
  }
  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-fg-62);
    cursor: pointer;
  }
  .tool svg {
    width: 15px;
    height: 15px;
  }
  .tool.del:hover {
    background: var(--yap-s3);
    color: var(--yap-danger);
  }
  .add {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-top: 10px;
    height: 30px;
    padding: 0 12px 0 9px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-sm);
    color: var(--yap-fg-80);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .add:hover {
    border-color: var(--yap-border-hover);
    color: var(--yap-fg);
  }
  .add svg {
    width: 14px;
    height: 14px;
  }
  @media (max-width: 720px) {
    .sc {
      grid-template-columns: minmax(0, 1fr);
    }
    .rhs {
      justify-content: flex-start;
    }
  }
</style>
