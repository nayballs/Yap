<script>
  // "Switch the Chat model": the full picker, a provider rail (the
  // maintainer's pick of the three concepts, 2026-10-19), opened from the
  // chip's short menu (ChatModelMenu.svelte: "Add a provider or download a
  // model…"), or straight from the chip with no AI set up. Two panes: on the
  // left Recent, every cloud provider Yap supports (a green dot when it has
  // a key, hollow when not), Custom when a server elsewhere is set up, and
  // On this PC; on the right that one's models, exactly as Settings lists
  // them (chatModels.svelte.js over modelCatalog.js). Search across the top
  // looks through all of them (Ctrl+K); a provider without a key connects
  // in place (the key saved where Settings keeps it, then its live list
  // checked at once); On this PC downloads Yap's on-device models right in
  // their row, and an installed one uninstalls from its right-click menu
  // (core/localModels.js: off while it's in use; the picker stays open and
  // the row goes back to Download, or away for a model the person added).
  // Picking changes the Chat scope only (chat_set_model) — except that the
  // on-device model is one for every feature, which the pane says when
  // cleanup uses it too.
  // Effort and Fast mode are the short menu's (ChatModelMenu.svelte).
  //
  // Keyboard: ↑/↓ move through the rail (it switches as it goes) or the
  // models, ←/→ (or Tab) between the two, Enter picks, typing searches,
  // Esc closes and gives the focus back to the chip. The window's in-page
  // hotkey fallbacks listen in the capture phase, before any of this.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';
  import ProviderMark from './ProviderMark.svelte';
  import LocalContext from '../../components/LocalContext.svelte';
  import { providerIcon } from '../../core/aiModel.js';
  import { PROVIDER_LABELS } from '../../core/providers.js';
  import { loadCatalog } from '../../stores/modelCatalog.svelte.js';
  import { forgetVisionTags } from '../../stores/visionTags.svelte.js';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { openContextMenu, menuKey, menuOpenFor } from '../../ui/contextMenu.svelte.js';
  import { uninstallItems } from '../../core/localModels.js';
  import {
    CLOUD_PROVIDERS,
    cloudInfo,
    isCloud,
    keyUrl,
    fmtSize,
    local,
    refreshLocal,
    followLocalChanges,
    downloadLocal,
    cancelLocal,
    progressLine,
    whereLocal,
    slowWarning,
    localRows,
    loadServer,
    serverLine,
    serverRows,
    cloudRows,
    providerLine,
    recentRows,
    recentEntry,
    pushRecent,
    searchGroups,
  } from './chatModels.svelte.js';

  let { status, anchor, onclose, onstatus, onopensettings } = $props();

  // "Reads images" (Snip to Chat): asked afresh each time the picker opens
  // (a provider's list, or a server's refusal, may have changed it).
  forgetVisionTags();

  // Opens on the current model's provider (or the one the scope is set to).
  function startTab() {
    const m = status?.ready ? status.model : null;
    const p = m?.provider || status?.provider || '';
    if (isCloud(p)) return p;
    if (p === 'ondevice') return 'pc';
    if (p === 'local' || p === 'custom') {
      const s = (status?.servers || []).find((x) => x.provider === p && (!m || x.host === status.host));
      return s && !s.onThisPc ? 'custom' : 'pc';
    }
    return 'groq';
  }

  let sel = $state(startTab());
  let q = $state('');
  let showMore = $state({});
  let busy = $state('');
  let navKey = $state('');
  let keyDraft = $state('');
  let keySaving = $state(false);
  let keyError = $state('');
  let changingKey = $state('');
  let pos = $state({ left: 12, bottom: 60, width: 560, maxH: 520 });

  let popEl = $state(null);
  let searchEl = $state(null);
  let railEl = $state(null);
  let listEl = $state(null);
  let keyEl = $state(null);
  // The rows by key: an installed model's keeps its hover look while its menu is open.
  let rowEls = $state({});

  const narrow = $derived(pos.width < 520);
  const servers = $derived(status?.servers || []);
  const remote = $derived(servers.filter((s) => !s.onThisPc));
  const pcReady = $derived(
    !!local.status?.installed || servers.some((s) => s.onThisPc) || (local.status?.own || []).length > 0
  );
  // `ready` draws the dot (green, or hollow); `state` says it in words.
  const tabs = $derived([
    { id: 'recent', label: 'Recent', glyph: 'recent' },
    ...CLOUD_PROVIDERS.map((p) => {
      const ready = !!status?.keyed?.includes(p.id);
      return { id: p.id, label: p.short, icon: providerIcon(p.id), ready, state: ready ? 'connected' : 'not set up', sepBefore: p.id === 'groq' };
    }),
    ...(remote.length ? [{ id: 'custom', label: 'Custom', glyph: 'server', ready: true, state: 'set up' }] : []),
    { id: 'pc', label: 'On this PC', glyph: 'pc', ready: pcReady, state: pcReady ? 'ready' : 'nothing installed yet', sepBefore: true },
  ]);

  const view = $derived.by(() => {
    if (q.trim()) return { kind: 'search', groups: searchGroups(q, status) };
    if (sel === 'recent') return { kind: 'recent', rows: recentRows(status) };
    if (sel === 'pc') return { kind: 'pc', ...localRows(status), servers: servers.filter((s) => s.onThisPc) };
    if (sel === 'custom') return { kind: 'custom', servers: remote };
    return { kind: 'cloud', provider: sel, ...cloudRows(sel, status) };
  });
  const firstPick = $derived(
    view.kind === 'search' ? view.groups.flatMap((g) => g.rows).find((r) => r.action === 'pick') : null
  );
  const warn = $derived(slowWarning(local.status));

  // ---------------------------------------------------------- placing

  // Above the chip's box (the Chat composer: `data-model-picker-above`), so
  // the message being typed stays in view; left-aligned with the chip, kept
  // inside the window.
  function place() {
    if (!anchor) return;
    const r = anchor.getBoundingClientRect();
    const top = (anchor.closest('[data-model-picker-above]') ?? anchor).getBoundingClientRect().top;
    const vw = window.innerWidth;
    const width = Math.min(560, vw - 24);
    pos = {
      width,
      left: Math.max(12, Math.min(r.left, vw - width - 12)),
      bottom: window.innerHeight - top + 8,
      maxH: Math.max(260, top - 8 - 52),
    };
  }

  // ------------------------------------------------------- focus + keys

  function navItems() {
    return listEl ? [...listEl.querySelectorAll('[data-nav]')] : [];
  }
  // One tab stop in the list (roving): the last one focused, else the
  // current model, else the first.
  function fixTabStops() {
    const items = navItems();
    if (!items.length) return;
    const active =
      items.find((el) => el.dataset.nav === navKey) ||
      items.find((el) => el.getAttribute('aria-pressed') === 'true') ||
      items[0];
    for (const el of items) el.tabIndex = el === active ? 0 : -1;
  }
  $effect(() => {
    void view;
    void navKey;
    void showMore;
    tick().then(fixTabStops);
  });

  function focusList() {
    const items = navItems();
    (items.find((el) => el.tabIndex === 0) || items[0] || keyEl || searchEl)?.focus();
  }
  function focusTab(id) {
    railEl?.querySelector(`[data-tab="${id}"]`)?.focus();
  }

  function selectTab(id, focus = false) {
    q = '';
    sel = id;
    changingKey = '';
    keyError = '';
    keyDraft = '';
    if (focus) tick().then(() => focusTab(id));
  }

  function onRailKey(e) {
    const ids = tabs.map((t) => t.id);
    const i = ids.indexOf(sel);
    const [next, prev, into] = narrow ? ['ArrowRight', 'ArrowLeft', 'ArrowDown'] : ['ArrowDown', 'ArrowUp', 'ArrowRight'];
    let j = -1;
    if (e.key === next) j = Math.min(ids.length - 1, i + 1);
    else if (e.key === prev) j = Math.max(0, i - 1);
    else if (e.key === 'Home') j = 0;
    else if (e.key === 'End') j = ids.length - 1;
    else if (e.key === into) {
      e.preventDefault();
      focusList();
      return;
    }
    if (j >= 0) {
      e.preventDefault();
      selectTab(ids[j], true);
    }
  }

  function onListKey(e) {
    if (e.target.matches?.('input, textarea, select')) return;
    // ← back to the rail (in the narrow strip, ↑ from the first model).
    if (e.key === 'ArrowLeft' && !narrow) {
      e.preventDefault();
      focusTab(sel);
      return;
    }
    const items = navItems();
    if (!items.length) return;
    const i = items.indexOf(e.target.closest?.('[data-nav]'));
    let to = null;
    if (e.key === 'ArrowDown') to = items[i < 0 ? 0 : Math.min(items.length - 1, i + 1)];
    else if (e.key === 'ArrowUp') to = i > 0 ? items[i - 1] : narrow ? railEl?.querySelector('[aria-selected="true"]') || searchEl : searchEl;
    else if (e.key === 'Home') to = items[0];
    else if (e.key === 'End') to = items[items.length - 1];
    if (to) {
      e.preventDefault();
      to.focus();
    }
  }

  function onSearchKey(e) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      focusList();
    } else if (e.key === 'Enter' && firstPick) {
      e.preventDefault();
      activate(firstPick);
    }
  }

  function onKey(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose(true);
      return;
    }
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'k') {
      e.preventDefault();
      e.stopPropagation();
      searchEl?.focus();
      searchEl?.select();
      return;
    }
    // Typing anywhere but a field searches.
    if (e.key.length === 1 && e.key !== ' ' && !e.ctrlKey && !e.metaKey && !e.altKey && !e.target.matches?.('input, textarea, select')) {
      e.preventDefault();
      q += e.key;
      searchEl?.focus();
      tick().then(() => searchEl?.setSelectionRange(q.length, q.length));
    }
  }

  // ------------------------------------------------------------ actions

  async function activate(row) {
    if (busy || !row) return;
    if (row.action === 'key') {
      selectTab(row.provider);
      await tick();
      keyEl?.focus();
      return;
    }
    if (row.action === 'download') return startDownload(row);
    if (row.action !== 'pick') return;
    if (row.current) {
      onclose(true);
      return;
    }
    busy = row.key;
    try {
      const s = await invoke('chat_set_model', {
        provider: row.provider,
        model: row.model || '',
        baseUrl: row.baseUrl || null,
        file: row.file || null,
      });
      pushRecent(recentEntry(row));
      onstatus(s);
      if (row.kind === 'ondevice') refreshLocal();
      onclose(true);
    } catch (e) {
      toast({ title: "Couldn't switch the model", description: String(e), variant: 'destructive' });
    } finally {
      busy = '';
    }
  }

  async function startDownload(row) {
    if (local.downloading || !row.curatedId) return;
    navKey = row.key;
    const done = downloadLocal(row.curatedId);
    await tick();
    listEl?.querySelector(`[data-nav="${CSS.escape(row.key)}"]`)?.focus();
    if (await done) {
      await tick();
      listEl?.querySelector(`[data-nav="${CSS.escape(row.key)}"]`)?.focus();
    }
  }

  // An installed on-device model's menu: Use, then Uninstall… (off while
  // it's in use, with why). Other rows have none.
  function rowMenu(e, r) {
    if (r.kind !== 'ondevice' || r.action !== 'pick') return;
    const el = e.currentTarget;
    openContextMenu(
      e,
      [
        !r.current ? { label: 'Use', icon: 'use', disabled: !!busy, onselect: () => activate(r) } : null,
        { separator: true },
        ...uninstallItems(
          { name: r.name, file: r.file, sizeMb: r.fileMb, own: !!r.own, inUse: r.inUse, inUseWhy: r.inUseWhy },
          { returnFocus: el, done: refreshLocal, after: () => afterUninstall(r) }
        ),
      ],
      { label: r.name, row: el }
    );
  }

  // Gone: the focus goes to its Download button (a curated model), else
  // to the list's first item.
  function afterUninstall(r) {
    const items = navItems();
    return items.find((el) => el.dataset.nav === r.key) || items[0] || searchEl;
  }

  async function saveKey(p) {
    const key = keyDraft.trim();
    if (!key || keySaving) return;
    keySaving = true;
    keyError = '';
    try {
      const s = await invoke('chat_provider_key', { provider: p, key });
      // Settings' copy of the config (it saves the whole thing): the key
      // where it keeps it, or its next save would drop it.
      window.dispatchEvent(new CustomEvent('yap-provider-key-saved', { detail: { provider: p, key } }));
      keyDraft = '';
      changingKey = '';
      onstatus(s);
      loadCatalog();
      await tick();
      focusList();
    } catch (e) {
      keyError = String(e);
    } finally {
      keySaving = false;
    }
  }

  function openSettings() {
    onclose(false);
    onopensettings?.('cleanup#chat');
  }

  onMount(() => {
    place();
    for (const s of servers) loadServer(s);
    followLocalChanges();
    window.addEventListener('resize', place);
    tick().then(() => {
      fixTabStops();
      const cur = navItems().find((el) => el.getAttribute('aria-pressed') === 'true');
      (cur || keyEl || navItems()[0] || searchEl)?.focus();
    });
    return () => window.removeEventListener('resize', place);
  });
</script>

{#snippet tags(r)}
  {#if r.tags?.length}
    <span class="tags">
      {#each r.tags as t (t.t)}<span class="tag {t.t}">{t.text}</span>{/each}
    </span>
  {/if}
{/snippet}

{#snippet check(r)}
  <span class="chk" aria-hidden="true">
    {#if busy === r.key}
      <span class="spin"></span>
    {:else if r.current}
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="m5 12 5 5 9-10" /></svg>
    {/if}
  </span>
{/snippet}

{#snippet row(r, where = false)}
  {#if r.action === 'download' || r.action === 'downloading'}
    {@const pct = local.progress?.percent || 0}
    <div class="mrow dlrow" data-row={r.key}>
      {@render check(r)}
      <ProviderMark icon={r.icon} glyph={r.glyph} size={18} />
      <span class="txt">
        <span class="nm" title={r.name}>{r.name}</span>
        <span class="ds">{where ? `${r.where} · ` : ''}{r.desc}</span>
      </span>
      {@render tags(r)}
      <div class="act">
        {#if r.action === 'downloading'}
          <div
            class="prog"
            data-nav={r.key}
            tabindex="-1"
            role="progressbar"
            aria-label="Downloading {r.name}"
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow={pct}
            aria-valuetext={progressLine(local.progress)}
            onfocus={() => (navKey = r.key)}
          >
            <span class="bar"><span style="width: {pct}%"></span></span>
            <span class="pline">{progressLine(local.progress)}</span>
          </div>
          <!-- Stops it; its half-downloaded file goes (shell/download.rs). -->
          <button class="btn sm" aria-label="Cancel the download of {r.name}" onclick={() => cancelLocal(r.curatedId)}>Cancel</button>
        {:else}
          <button
            class="btn sm"
            data-nav={r.key}
            aria-label="Download {r.name} ({fmtSize(r.sizeMb)})"
            disabled={!!local.downloading}
            onfocus={() => (navKey = r.key)}
            onclick={() => activate(r)}
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4v11M7 10.5l5 5 5-5M5 20h14" /></svg>
            {local.errorFor === r.curatedId ? 'Try again' : `Download ${fmtSize(r.sizeMb)}`}
          </button>
        {/if}
        {#if local.errorFor === r.curatedId}
          <p class="rowerr" role="alert">Couldn't download it: {local.error}</p>
        {/if}
      </div>
    </div>
  {:else}
    <button
      class="mrow"
      class:on={r.current}
      class:locked={r.action === 'key'}
      class:menuopen={menuOpenFor(rowEls[r.key])}
      bind:this={rowEls[r.key]}
      data-nav={r.key}
      aria-pressed={r.current}
      aria-busy={busy === r.key}
      onfocus={() => (navKey = r.key)}
      onclick={() => activate(r)}
      oncontextmenu={(e) => rowMenu(e, r)}
      onkeydown={(e) => menuKey(e, (k) => rowMenu(k, r))}
    >
      {@render check(r)}
      <ProviderMark icon={r.icon} glyph={r.glyph} size={18} />
      <span class="txt">
        <span class="nm" title={r.name}>{r.name}</span>
        {#if r.desc || where}<span class="ds">{where ? `${r.where}${r.desc ? ' · ' : ''}` : ''}{r.desc}</span>{/if}
      </span>
      {@render tags(r)}
    </button>
  {/if}
{/snippet}

{#snippet head(icon, glyph, title, lines, extra)}
  <div class="phead">
    <ProviderMark {icon} {glyph} size={30} tile />
    <div class="ptext">
      <div class="ptitle">{title}</div>
      {#each lines.filter((l) => l?.text) as l (l.text)}
        <div class="psub" class:warn={l.warn}>{l.text}</div>
      {/each}
    </div>
    {#if extra}{@render extra()}{/if}
  </div>
{/snippet}

{#snippet connect(p, replacing)}
  {@const info = cloudInfo(p)}
  <form class="connect" onsubmit={(e) => (e.preventDefault(), saveKey(p))}>
    <div>
      <div class="ctitle">{replacing ? `Replace the ${info.short} key` : `Connect ${info.short}`}</div>
      <div class="cdesc">
        {replacing
          ? `Paste a new ${info.name} API key. Yap checks its models with it straight away.`
          : `Paste an ${info.name} API key. Yap fetches the current models straight away.`}
      </div>
    </div>
    <div class="crow">
      <input
        bind:this={keyEl}
        bind:value={keyDraft}
        type="password"
        autocomplete="off"
        spellcheck="false"
        placeholder="{info.name} API key"
        aria-label="{info.name} API key"
        aria-invalid={!!keyError}
      />
      <button class="btn ink" type="submit" disabled={!keyDraft.trim() || keySaving}>{keySaving ? 'Saving…' : 'Save key'}</button>
      <button class="btn" type="button" onclick={() => openExternalLink(keyUrl(p))}>Get a key ↗</button>
      {#if replacing}<button class="btn" type="button" onclick={() => (changingKey = '')}>Cancel</button>{/if}
    </div>
    {#if keyError}<p class="cerr" role="alert">{keyError}</p>{/if}
    <p class="cnote">Stored only on this PC, like the keys in Settings.</p>
  </form>
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="pop"
  class:narrow
  role="dialog"
  aria-label="Switch the Chat model"
  tabindex="-1"
  bind:this={popEl}
  style="left: {pos.left}px; bottom: {pos.bottom}px; width: {pos.width}px; --maxh: {pos.maxH}px"
  onkeydown={onKey}
>
  <div class="search">
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6.5" /><path d="m20 20-4.2-4.2" /></svg>
    <input
      bind:this={searchEl}
      bind:value={q}
      type="search"
      placeholder="Search models and providers"
      aria-label="Search models and providers"
      aria-controls="chat-model-pane"
      onkeydown={onSearchKey}
    />
    {#if q}
      <button class="clear" type="button" aria-label="Clear the search" onclick={() => ((q = ''), searchEl?.focus())}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    {/if}
    <kbd>Ctrl K</kbd>
  </div>

  <div class="body">
    <div
      class="rail"
      role="tablist"
      aria-label="Providers"
      aria-orientation={narrow ? 'horizontal' : 'vertical'}
      tabindex="-1"
      bind:this={railEl}
      onkeydown={onRailKey}
    >
      {#each tabs as t (t.id)}
        {#if t.sepBefore}<div class="sep" role="presentation"></div>{/if}
        <button
          class="tab"
          class:on={!q.trim() && sel === t.id}
          role="tab"
          id="chat-model-tab-{t.id}"
          data-tab={t.id}
          aria-selected={!q.trim() && sel === t.id}
          aria-controls="chat-model-pane"
          tabindex={sel === t.id ? 0 : -1}
          onclick={() => selectTab(t.id)}
        >
          <!-- One line: the tab's text reads "Groq, connected". -->
          <ProviderMark icon={t.icon ?? null} glyph={t.glyph ?? 'spark'} size={18} /><span class="tlabel">{t.label}</span>{#if t.state}<span class="st" class:off={!t.ready} title={t.state[0].toUpperCase() + t.state.slice(1)}></span><span class="vh">, {t.state}</span>{/if}
        </button>
      {/each}
    </div>

    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="pane"
      id="chat-model-pane"
      role="tabpanel"
      aria-labelledby={view.kind === 'search' ? undefined : `chat-model-tab-${sel}`}
      aria-label={view.kind === 'search' ? 'Search results' : undefined}
      tabindex="-1"
      bind:this={listEl}
      onkeydown={onListKey}
    >
      {#if view.kind === 'search'}
        <div class="rhead">Results for “{q.trim()}”</div>
        {#each view.groups as g (g.id)}
          <div class="group" role="group" aria-label={g.label}>
            <div class="gh"><ProviderMark icon={g.icon ?? null} glyph={g.glyph ?? 'spark'} size={14} />{g.label}</div>
            {#each g.rows.slice(0, 40) as r (r.key)}{@render row(r)}{/each}
            {#if g.rows.length > 40}<p class="empty small">And {g.rows.length - 40} more: add a word to narrow it down.</p>{/if}
          </div>
        {:else}
          <p class="empty">No model matches “{q.trim()}”. Try a provider's name.</p>
        {/each}
      {:else if view.kind === 'recent'}
        {@render head(null, 'recent', 'Recent', [{ text: 'The models you used last in Chat' }])}
        {#each view.rows as r (r.key)}{@render row(r, true)}{:else}
          <p class="empty">The models you pick show up here.</p>
        {/each}
      {:else if view.kind === 'cloud'}
        {@const info = cloudInfo(view.provider)}
        {@const line = providerLine(view.provider, status)}
        {#snippet keyExtra()}
          {#if view.connected && status?.maskedKeys?.[view.provider]}
            <div class="keyside">
              <span class="keymask" title="The API key Yap uses">{status.maskedKeys[view.provider]}</span>
              {#if changingKey !== view.provider}
                <button class="linkbtn" onclick={() => ((changingKey = view.provider), tick().then(() => keyEl?.focus()))}>Change key</button>
              {/if}
            </div>
          {/if}
        {/snippet}
        {@render head(providerIcon(view.provider), 'spark', info.name, [line], keyExtra)}
        {#if !view.connected}
          {@render connect(view.provider, false)}
        {:else if changingKey === view.provider}
          {@render connect(view.provider, true)}
        {/if}
        {#if view.missing && view.rows.some((r) => r.current && r.model === view.missing.value)}
          <p class="note warn" role="alert">{PROVIDER_LABELS[view.provider]} no longer offers {view.missing.label}. Pick another model.</p>
        {/if}
        {#each view.rows as r (r.key)}{@render row(r)}{/each}
        {#if view.connected && view.more.length > 0}
          <button
            class="morebtn"
            data-nav="more:{view.provider}"
            aria-expanded={!!showMore[view.provider]}
            onfocus={() => (navKey = `more:${view.provider}`)}
            onclick={() => (showMore = { ...showMore, [view.provider]: !showMore[view.provider] })}
          >
            {showMore[view.provider]
              ? `Show fewer ${info.short} models`
              : `Show ${view.more.length} more ${info.short} ${view.more.length === 1 ? 'model' : 'models'}`}
          </button>
          {#if showMore[view.provider]}
            {#each view.more as r (r.key)}{@render row(r)}{/each}
          {/if}
        {/if}
      {:else if view.kind === 'custom'}
        {@render head(null, 'server', 'Custom server', [{ text: 'Your own OpenAI-compatible servers, as set up in Settings' }])}
        {#each view.servers as s (s.baseUrl)}
          {@const line = serverLine(s)}
          <div class="group" role="group" aria-label={s.host}>
            <div class="gh"><ProviderMark glyph="server" size={14} />{s.host}<span class="meta" class:warn={line.warn}>{line.text}</span></div>
            {#each serverRows(s, status) as r (r.key)}{@render row(r)}{/each}
          </div>
        {/each}
      {:else}
        {@render head(null, 'pc', 'On this PC', [{ text: 'Private: nothing leaves this PC' }, { text: whereLocal(local.status) }])}
        {#if warn}
          <div class="note warn" role="status" data-testid="picker-slow">
            <span>{warn.text}</span>
            {#if warn.small}
              {@const small = view.installed.find((r) => r.curatedId === warn.small.id)}
              {#if small}
                <button class="btn sm" data-nav="slow-switch" onfocus={() => (navKey = 'slow-switch')} onclick={() => activate(small)}>Use {warn.small.display}</button>
              {:else}
                {@const dl = view.available.find((r) => r.curatedId === warn.small.id)}
                {#if dl}
                  <button class="btn sm" data-nav="slow-switch" disabled={!!local.downloading} onfocus={() => (navKey = 'slow-switch')} onclick={() => activate(dl)}>Download {warn.small.display}</button>
                {/if}
              {/if}
            {/if}
          </div>
        {/if}
        {#if local.status?.installed}
          <!-- The on-device model's context (one sidecar for every feature). -->
          <LocalContext status={local.status} compact onchange={(v) => (local.status = { ...local.status, context: v })} />
        {/if}
        {#if local.status?.sharedOnDevice}
          <p class="note">Dictation cleanup uses the on-device model too: picking another one here switches it there as well.</p>
        {/if}
        {#if !local.status}
          <p class="empty">Reading this PC's models…</p>
        {/if}
        {#if view.installed.length}
          <div class="group" role="group" aria-label="Installed">
            <div class="gh">Installed</div>
            {#each view.installed as r (r.key)}{@render row(r)}{/each}
          </div>
        {/if}
        {#each view.servers as s (s.baseUrl)}
          {@const line = serverLine(s)}
          <div class="group" role="group" aria-label="Self-hosted · {s.host}">
            <div class="gh"><ProviderMark glyph="server" size={14} />Self-hosted · {s.host}<span class="meta" class:warn={line.warn}>{line.text}</span></div>
            {#each serverRows(s, status) as r (r.key)}{@render row(r)}{/each}
          </div>
        {/each}
        {#if view.available.length}
          <div class="group" role="group" aria-label="Download">
            <div class="gh">Download<span class="meta">Then it runs offline</span></div>
            {#each view.available as r (r.key)}{@render row(r)}{/each}
          </div>
        {/if}
        <p class="folder">
          Your own <code>.gguf</code> models go in the
          <button class="linkbtn" onclick={() => invoke('open_llm_folder').catch(() => {})}>Open models folder</button>
        </p>
      {/if}
    </div>
  </div>

  <div class="foot">
    <!-- On this PC the pane says it when it applies (the on-device model is
         shared with dictation cleanup), so the footer doesn't repeat it. -->
    {#if sel !== 'pc' || q}
      <span>
        {status?.ready && !status.fromScope
          ? "Chat uses Dictation Cleanup's model until you pick one here. Cleanup stays as it is."
          : 'Changes the model for Chat only. Dictation cleanup stays as it is.'}
      </span>
    {/if}
    <button class="footlink" onclick={openSettings}>Language model settings…</button>
  </div>
</div>

<style>
  .pop,
  .pop * {
    box-sizing: border-box;
  }
  .pop {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    max-height: var(--maxh);
    border: 1px solid var(--yap-border);
    border-radius: 16px;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-menu);
    color: var(--yap-fg);
    font-size: 13px;
    line-height: 1.45;
    overflow: hidden;
    outline: none;
  }
  @media (prefers-reduced-motion: no-preference) {
    .pop {
      animation: pop-in 140ms ease-out;
    }
    .tab,
    .mrow,
    .btn {
      transition:
        background var(--yap-dur),
        border-color var(--yap-dur),
        box-shadow var(--yap-dur);
    }
  }
  @keyframes pop-in {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
  }

  /* search */
  .search {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--yap-border-subtle);
    color: var(--yap-muted);
  }
  .search svg {
    width: 16px;
    height: 16px;
    flex: 0 0 auto;
  }
  .search input {
    flex: 1 1 auto;
    min-width: 0;
    border: 0;
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    outline: none;
  }
  .search input::placeholder {
    color: var(--yap-muted-70);
  }
  .search input::-webkit-search-cancel-button {
    appearance: none;
  }
  .clear {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .clear:hover {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .clear svg {
    width: 12px;
    height: 12px;
  }
  .clear:focus-visible {
    outline: 2px solid var(--yap-primary);
    outline-offset: 1px;
  }
  kbd {
    flex: 0 0 auto;
    padding: 0 5px;
    border: 1px solid var(--yap-border);
    border-radius: 5px;
    color: var(--yap-muted);
    font-family: inherit;
    font-size: 11px;
    line-height: 18px;
  }

  /* the two panes */
  .body {
    flex: 1 1 auto;
    display: grid;
    grid-template-columns: 184px minmax(0, 1fr);
    height: clamp(220px, calc(var(--maxh) - 96px), 420px);
    min-height: 0;
  }
  .rail {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px;
    border-right: 1px solid var(--yap-border-subtle);
    background: var(--yap-s1);
    overflow-y: auto;
    outline: none;
  }
  .tab {
    display: grid;
    grid-template-columns: 20px minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    width: 100%;
    padding: 7px 8px;
    border: 0;
    border-radius: 9px;
    background: transparent;
    color: var(--yap-fg);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .tab:hover {
    background: var(--yap-s3);
  }
  .tab.on {
    background: var(--yap-s2);
    box-shadow: 0 0 0 1px var(--yap-border);
    font-weight: 600;
  }
  .tlabel {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .st {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--yap-success);
  }
  .st.off {
    background: transparent;
    box-shadow: inset 0 0 0 1.5px var(--yap-paper-rule);
  }
  .sep {
    flex: 0 0 auto;
    height: 1px;
    margin: 6px 4px;
    background: var(--yap-border-subtle);
  }
  .pane {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    padding: 4px 8px 10px;
    overflow-y: auto;
    outline: none;
  }

  /* narrow: the rail as a strip across the top */
  .narrow .body {
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr);
  }
  .narrow .rail {
    flex-direction: row;
    border-right: 0;
    border-bottom: 1px solid var(--yap-border-subtle);
    overflow-x: auto;
    overflow-y: hidden;
  }
  .narrow .tab {
    width: auto;
    flex: 0 0 auto;
  }
  .narrow .sep {
    display: none;
  }

  /* a pane's header */
  .phead {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 10px 8px;
  }
  .ptext {
    flex: 1 1 auto;
    min-width: 0;
  }
  .ptitle {
    font-weight: 600;
  }
  .psub {
    color: var(--yap-muted);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .psub.warn {
    color: var(--yap-warning);
  }
  .keyside {
    flex: 0 0 auto;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 2px;
  }
  .keymask {
    padding: 1px 7px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 6px;
    background: var(--yap-s1);
    color: var(--yap-muted);
    font: 11px ui-monospace, Consolas, monospace;
  }
  .rhead {
    padding: 10px 10px 4px;
    font-weight: 600;
  }

  /* model rows */
  .mrow {
    display: grid;
    grid-template-columns: 16px 20px minmax(0, 1fr) auto;
    gap: 8px;
    align-items: center;
    width: 100%;
    padding: 7px 10px;
    border: 0;
    border-radius: 9px;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  button.mrow:hover,
  .mrow.menuopen {
    background: var(--yap-s3);
  }
  .mrow.on {
    background: var(--yap-primary-wash);
  }
  .mrow.locked {
    opacity: 0.62;
  }
  .mrow.locked:hover,
  .mrow.locked:focus-visible {
    opacity: 0.9;
  }
  .dlrow {
    cursor: default;
    grid-template-areas:
      'chk ic txt tags'
      '. . act act';
  }
  .dlrow > .chk {
    grid-area: chk;
  }
  .dlrow > :global(.pmark) {
    grid-area: ic;
  }
  .dlrow > .txt {
    grid-area: txt;
  }
  .dlrow > .tags {
    grid-area: tags;
  }
  .act {
    grid-area: act;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    min-width: 0;
  }
  .chk {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    color: var(--yap-primary);
  }
  .chk svg {
    width: 13px;
    height: 13px;
  }
  .txt {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .nm {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mrow.on .nm {
    font-weight: 600;
  }
  /* All of it, wrapped: never cut off with no way to read the rest
     (2026-10-27). Every description is a short line or two. */
  .ds {
    color: var(--yap-muted);
    font-size: 12px;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }
  /* Stacked: two tags side by side squeezed the description to a word a line. */
  .tags {
    display: flex;
    flex-direction: column;
    gap: 3px;
    align-items: flex-end;
    justify-content: center;
  }
  .tag {
    padding: 0 7px;
    border-radius: var(--yap-r-full);
    font-size: 10.5px;
    font-weight: 600;
    line-height: 18px;
    white-space: nowrap;
  }
  .tag.new,
  .tag.use {
    background: var(--yap-primary-wash);
    color: var(--yap-primary-hover);
  }
  .tag.rec {
    background: color-mix(in srgb, var(--yap-success) 13%, transparent);
    color: var(--yap-success);
  }
  /* Reads images (Snip to Chat): quiet, it's a fact about the model. */
  .tag.img {
    background: var(--yap-s3, var(--yap-raised));
    color: var(--yap-fg-80);
  }
  .tag.warn,
  .tag.gone {
    padding: 0;
    background: transparent;
    color: var(--yap-warning);
  }
  .tag.key {
    background: var(--yap-raised);
    color: var(--yap-fg-80);
  }

  /* the download in its row */
  .prog {
    display: flex;
    flex-direction: column;
    gap: 3px;
    width: 100%;
    outline: none;
    border-radius: 6px;
  }
  .bar {
    display: block;
    height: 5px;
    border-radius: 3px;
    background: var(--yap-raised);
    overflow: hidden;
  }
  .bar > span {
    display: block;
    height: 100%;
    border-radius: 3px;
    background: var(--yap-primary);
    transition: width 0.3s ease;
  }
  .pline {
    color: var(--yap-muted);
    font-size: 11.5px;
  }
  .rowerr {
    margin: 0;
    color: var(--yap-danger);
    font-size: 11.5px;
  }

  /* groups, notes, the folder line */
  .group {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .gh {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 12px 10px 4px;
    color: var(--yap-muted);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .gh .meta {
    margin-left: auto;
    font-weight: 400;
    letter-spacing: 0;
    text-transform: none;
    text-align: right;
  }
  .meta.warn {
    color: var(--yap-warning);
  }
  .note {
    margin: 2px 10px 6px;
    padding: 9px 11px;
    border-radius: 10px;
    background: var(--yap-s1);
    color: var(--yap-muted);
    font-size: 12px;
    line-height: 1.5;
  }
  .note.warn {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    border: 1px solid color-mix(in srgb, var(--yap-warning) 35%, var(--yap-border-subtle));
    background: color-mix(in srgb, var(--yap-warning) 8%, transparent);
    color: var(--yap-warning);
  }
  .empty {
    margin: 0;
    padding: 18px 10px;
    color: var(--yap-muted);
  }
  .empty.small {
    padding: 4px 10px 8px;
    font-size: 12px;
  }
  .folder {
    margin: 10px 10px 2px;
    color: var(--yap-muted);
    font-size: 12px;
  }
  .folder code {
    font-size: 11.5px;
  }
  .morebtn {
    align-self: flex-start;
    margin: 4px 10px;
    padding: 2px 0;
    border: 0;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .morebtn:hover,
  .linkbtn:hover,
  .footlink:hover {
    text-decoration: underline;
  }
  .linkbtn {
    padding: 0;
    border: 0;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }

  /* connect a provider */
  .connect {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 4px 10px 10px;
    padding: 14px;
    border: 1px dashed var(--yap-border);
    border-radius: 12px;
    background: var(--yap-s1);
  }
  .ctitle {
    font-weight: 600;
  }
  .cdesc,
  .cnote {
    color: var(--yap-muted);
    font-size: 12px;
  }
  .cnote {
    margin: 0;
    font-size: 11.5px;
  }
  .crow {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .crow input {
    flex: 1 1 160px;
    min-width: 0;
    padding: 6px 10px;
    border: 1px solid var(--yap-border);
    border-radius: 8px;
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
  }
  .crow input:focus {
    border-color: var(--yap-primary-line);
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .cerr {
    margin: 0;
    color: var(--yap-danger);
    font-size: 12px;
  }

  /* buttons */
  .btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    border: 1px solid var(--yap-border);
    border-radius: 8px;
    background: var(--yap-s2);
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
    font-weight: 500;
    white-space: nowrap;
    cursor: pointer;
  }
  .btn:hover:not(:disabled) {
    border-color: var(--yap-border-hover);
    background: var(--yap-s3);
  }
  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .btn.ink {
    border-color: var(--yap-ink);
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .btn.ink:hover:not(:disabled) {
    border-color: var(--yap-ink-hover);
    background: var(--yap-ink-hover);
  }
  .btn.sm {
    padding: 4px 10px;
    font-size: 12px;
  }
  .btn svg {
    width: 13px;
    height: 13px;
  }

  /* the footer */
  .foot {
    flex: 0 0 auto;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 6px 12px;
    padding: 9px 14px;
    border-top: 1px solid var(--yap-border-subtle);
    color: var(--yap-muted);
    font-size: 12px;
  }
  .footlink {
    padding: 0;
    border: 0;
    background: none;
    color: var(--yap-fg-80);
    font: inherit;
    font-weight: 500;
    cursor: pointer;
  }

  /* focus: the amber ring */
  .tab:focus-visible,
  .mrow:focus-visible,
  .btn:focus-visible,
  .morebtn:focus-visible,
  .linkbtn:focus-visible,
  .footlink:focus-visible,
  .prog:focus-visible {
    outline: 2px solid var(--yap-primary);
    outline-offset: 1px;
  }
  .search:focus-within {
    box-shadow: inset 0 -2px 0 var(--yap-primary-tint);
  }

  .vh {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  .spin {
    width: 10px;
    height: 10px;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
