<script>
  // Chat's model menu (2026-10-31, after the Claude Code app's: "Opus 5.5 ✓,
  // Fable 5.1 2, Sonnet 5.5 3, Haiku 5.5 4, Effort ▸ Extra, More models ▸,
  // Fast mode"): what the model chip opens first. Compact, keyboard first:
  // - **the main models** (core/mainModels.js: the newest of each family from
  //   the providers with a key, the on-device model when one is installed,
  //   the model in use always), each with its provider quietly and its
  //   number key (1–7 while the menu is open), the ✓ on the one in use;
  // - **Effort ▸ <level>**: the levels the model in use takes (ai/effort.rs,
  //   core/effort.js), the one in effect ticked, a line on what the one in
  //   focus does; none for a model without a reasoning control or the one
  //   on this PC (its Think button);
  // - **More models ▸**: every other model of the connected providers, this
  //   PC's other models and self-hosted servers, grouped, newest first, a
  //   search at its top when it's long;
  // - **Fast mode** (Claude Opus 5.5): a switch, twice the price per token;
  // - **Add a provider or download a model…**: the full picker, the
  //   provider rail (ChatModelPicker.svelte: connecting a key, the on-device
  //   downloads and uninstall, search across everything).
  // A submenu is a flyout beside the menu (on the right, else the left), or
  // a section opening under its item when the window has room for neither.
  // Picking changes the Chat scope only (chat_set_model), as the rail does.
  //
  // Keyboard: ↑/↓, Home/End, Enter; → into a submenu, ← or Esc out of it;
  // 1–7 pick a main model; typing in More models searches; Esc (or Tab)
  // closes and gives the focus back to the chip.
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, tick, untrack } from 'svelte';
  import { modelName, providerName } from '../../core/aiModel.js';
  import { effortLabel, effortNow, effortOptions, effortHint, FAST_NOTE } from '../../core/effort.js';
  import { PROVIDER_LABELS } from '../../core/providers.js';
  import { mainModels, moreModels, MENU_ORDER } from '../../core/mainModels.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { local, refreshLocal, localRows, serverRows, loadServer, cloudRows, pushRecent, recentEntry } from './chatModels.svelte.js';

  let { status, anchor, onclose, onstatus, onrail } = $props();

  // Its own short line (the rail's FAST_NOTE names the model).
  const FAST_LINE = 'Faster answers, at twice the price per token';
  // More models gets a search past this many.
  const SEARCH_FROM = 10;
  // A long group in More: the first ones, then a line.
  const GROUP_MAX = 40;
  // The flyouts' widths before they're drawn (whether one fits beside the menu).
  const SUB_W = { effort: 232, more: 240 };

  let menuEl = $state(null);
  let subEl = $state(null);
  let searchEl = $state(null);
  let pos = $state({ left: 12, bottom: 60, maxH: 480, ready: false });
  // The open submenu ('effort' | 'more' | ''), where it goes, what's typed.
  let sub = $state('');
  // `minH`: More's height as it opened, kept while a search narrows it, so
  // nothing moves under the pointer or the caret.
  let subPos = $state({ left: 0, top: 0, inline: false, ready: false, minH: 0 });
  let q = $state('');
  // Effort: the level whose line shows (the focused or hovered one; '' its default; null the one in effect).
  let hintLevel = $state(null);
  let busy = $state('');

  const model = $derived(status?.ready ? status.model : null);
  const connected = $derived((status?.keyed || []).filter((p) => MENU_ORDER.includes(p)));
  // Every connected provider's models as the rail lists them (curated + live).
  const cloud = $derived(
    Object.fromEntries(
      connected.map((p) => {
        const c = cloudRows(p, status);
        return [p, [...c.rows, ...c.more]];
      })
    )
  );
  const servers = $derived(status?.servers || []);
  const pcRows = $derived(localRows(status).installed);
  const byServer = $derived(servers.map((s) => ({ s, rows: serverRows(s, status) })));
  // The on-device model Yap would run, when it's installed.
  const onDevice = $derived.by(() => {
    const st = local.status;
    if (!st?.installed) return null;
    const file = st.activeFile || st.modelFile;
    return pcRows.find((r) => r.file === file) ?? null;
  });
  // The model in use, as a row (its own when no list has it).
  const current = $derived.by(() => {
    if (!model) return null;
    const all = [...Object.values(cloud).flat(), ...pcRows, ...byServer.flatMap((x) => x.rows)];
    return (
      all.find((r) => r.current) ?? {
        key: `${model.provider}:${model.id}`,
        provider: model.provider,
        model: model.id,
        name: modelName(model),
        where: providerName(model),
        current: true,
      }
    );
  });
  const main = $derived(mainModels({ rows: cloud, connected, onDevice, current }));
  const more = $derived(
    moreModels({
      rows: cloud,
      connected,
      main,
      pc: [...pcRows, ...byServer.filter((x) => x.s.onThisPc).flatMap((x) => x.rows)],
      servers: byServer.filter((x) => !x.s.onThisPc).map((x) => ({ id: `server:${x.s.baseUrl}`, label: x.s.host, rows: x.rows })),
    })
  );
  const moreCount = $derived(more.reduce((n, g) => n + g.rows.length, 0));
  const terms = $derived(q.toLowerCase().split(/\s+/).filter(Boolean));
  const moreShown = $derived(
    terms.length
      ? more
          .map((g) => ({ ...g, rows: g.rows.filter((r) => terms.every((t) => `${r.name} ${r.model} ${groupLabel(g)}`.toLowerCase().includes(t))) }))
          .filter((g) => g.rows.length)
      : more
  );

  const eff = $derived(model ? status.effort : null);
  const effNow = $derived(effortNow(eff));
  const hasFast = $derived(typeof eff?.fast === 'boolean');
  const hint = $derived(eff ? effortHint(hintLevel === null ? eff : { ...eff, chosen: hintLevel || null }) : '');

  /** "Anthropic", "On this PC", a server's host. */
  function whereOf(r) {
    if (r.provider === 'ondevice') return 'On this PC';
    if (r.kind === 'cloud') return PROVIDER_LABELS[r.provider] || r.provider;
    return r.where || PROVIDER_LABELS[r.provider] || r.provider;
  }
  function groupLabel(g) {
    if (g.id === 'pc') return 'On this PC';
    return g.label || PROVIDER_LABELS[g.id] || g.id;
  }

  // ---------------------------------------------------------- placing

  // Above the chip, left-aligned with it, inside the window (as Claude
  // Code's menu sits over its chip).
  function place() {
    if (!anchor || !menuEl) return;
    const r = anchor.getBoundingClientRect();
    const w = menuEl.offsetWidth;
    pos = {
      left: Math.max(8, Math.min(r.left, window.innerWidth - w - 8)),
      bottom: window.innerHeight - r.top + 6,
      maxH: Math.max(160, r.top - 14),
      ready: true,
    };
    if (sub) placeSub();
  }

  /**
   * Beside the menu (right, else left), level with its item; else under its
   * item. `keep`: where it is, if it still fits (its lists arriving).
   */
  function placeSub(keep = false) {
    const trig = menuEl?.querySelector(`[data-sub="${sub}"]`);
    if (!trig) return;
    const m = 8;
    const box = menuEl.getBoundingClientRect();
    const w = subEl && !subPos.inline ? subEl.offsetWidth : SUB_W[sub];
    let left = box.right + 4;
    if (left + w > window.innerWidth - m) left = box.left - w - 4;
    if (left < m) {
      subPos = { left: 0, top: 0, inline: true, ready: true, minH: 0 };
      return;
    }
    const h = subEl && !subPos.inline ? subEl.getBoundingClientRect().height : 0;
    // Its bottom by its item's, as Claude Code opens it upwards from a menu at the window's foot.
    const want = keep && subPos.ready ? subPos.top : trig.getBoundingClientRect().bottom + 5 - h;
    const top = Math.max(m, Math.min(want, window.innerHeight - m - h));
    subPos = { left, top, inline: false, ready: !!h, minH: sub === 'more' && !keep ? h : subPos.minH };
  }

  // The flyout grows (More's lists arriving): kept in the window.
  $effect(() => {
    const el = subEl;
    if (!el || untrack(() => subPos.inline)) return;
    const ro = new ResizeObserver(() => placeSub(true));
    ro.observe(el);
    return () => ro.disconnect();
  });

  // ------------------------------------------------------- focus + keys

  const rootItems = () => (menuEl ? [...menuEl.querySelectorAll('[data-mi="root"]')] : []);
  const subItems = () => (subEl ? [...subEl.querySelectorAll('[data-mi="sub"]')] : []);
  function step(items, dir) {
    if (!items.length) return;
    const i = items.indexOf(document.activeElement);
    const next = i < 0 ? (dir > 0 ? 0 : items.length - 1) : (i + dir + items.length) % items.length;
    items[next].focus();
  }

  async function openSub(which, focus) {
    if (sub !== which) {
      sub = which;
      q = '';
      hintLevel = null;
      subPos = { left: 0, top: 0, inline: false, ready: false, minH: 0 };
      if (which === 'more') for (const s of servers) loadServer(s);
    }
    await tick();
    placeSub();
    await tick();
    placeSub();
    if (focus) {
      await tick();
      if (which === 'more' && searchEl) searchEl.focus();
      else (subItems().find((el) => el.getAttribute('aria-checked') === 'true') || subItems()[0])?.focus();
    }
  }

  function closeSub(refocus) {
    const which = sub;
    sub = '';
    q = '';
    hintLevel = null;
    if (refocus) tick().then(() => menuEl?.querySelector(`[data-sub="${which}"]`)?.focus());
  }

  function onRootKey(e) {
    // A submenu opened under its item has its own keys.
    if (e.target.closest?.('[data-submenu]')) return;
    const items = rootItems();
    switch (e.key) {
      case 'ArrowDown':
        step(items, 1);
        break;
      case 'ArrowUp':
        step(items, -1);
        break;
      case 'Home':
        items[0]?.focus();
        break;
      case 'End':
        items.at(-1)?.focus();
        break;
      case 'ArrowRight': {
        const which = document.activeElement?.dataset?.sub;
        if (!which) return;
        openSub(which, true);
        break;
      }
      case 'Escape':
      case 'Tab':
        onclose(true);
        break;
      default: {
        // 1–7: a main model.
        if (!/^[1-9]$/.test(e.key) || e.ctrlKey || e.altKey || e.metaKey) return;
        const r = main[Number(e.key) - 1];
        if (!r) return;
        pick(r);
      }
    }
    e.preventDefault();
    e.stopPropagation();
  }

  function onSubKey(e) {
    const items = subItems();
    const inInput = e.target === searchEl;
    switch (e.key) {
      case 'ArrowDown':
        if (inInput) items[0]?.focus();
        else step(items, 1);
        break;
      case 'ArrowUp':
        if (!inInput && items.indexOf(document.activeElement) === 0 && searchEl) searchEl.focus();
        else if (!inInput) step(items, -1);
        break;
      case 'Home':
      case 'End':
        if (inInput) return;
        (e.key === 'Home' ? items[0] : items.at(-1))?.focus();
        break;
      case 'ArrowLeft':
        if (inInput && (q || searchEl.selectionStart > 0)) return;
        closeSub(true);
        break;
      case 'Escape':
        closeSub(true);
        break;
      case 'Tab':
        onclose(true);
        break;
      default:
        // Typing on a model searches (More models).
        if (inInput || !searchEl || e.key.length !== 1 || e.key === ' ' || e.ctrlKey || e.metaKey || e.altKey) return;
        q += e.key;
        searchEl.focus();
        tick().then(() => searchEl?.setSelectionRange(q.length, q.length));
    }
    e.preventDefault();
    e.stopPropagation();
  }

  // A root item under the pointer: its submenu opens (no focus), another closes it.
  function hoverRoot(which) {
    if (which) {
      if (sub !== which) openSub(which, false);
    } else if (sub && !subEl?.contains(document.activeElement)) {
      closeSub(false);
    }
  }

  // ------------------------------------------------------------ actions

  async function pick(r) {
    if (busy || !r) return;
    if (r.current) {
      onclose(true);
      return;
    }
    busy = r.key;
    try {
      const s = await invoke('chat_set_model', {
        provider: r.provider,
        model: r.model || '',
        baseUrl: r.baseUrl || null,
        file: r.file || null,
      });
      pushRecent(recentEntry(r));
      onstatus(s);
      if (r.provider === 'ondevice') refreshLocal();
      onclose(true);
    } catch (e) {
      toast({ title: "Couldn't switch the model", description: String(e), variant: 'destructive' });
    } finally {
      busy = '';
    }
  }

  // The model in use's effort ('' = "Default": nothing sent), kept for it.
  async function setEffort(level) {
    if (!model) return;
    if (level !== effNow) {
      try {
        onstatus(await invoke('chat_set_effort', { provider: model.provider, model: model.id, level: level || null }));
      } catch (e) {
        toast({ title: "Couldn't change the effort", description: String(e), variant: 'destructive' });
        return;
      }
    }
    onclose(true);
  }

  async function toggleFast() {
    try {
      onstatus(await invoke('chat_set_fast', { on: !eff?.fast }));
    } catch (e) {
      toast({ title: "Couldn't change fast mode", description: String(e), variant: 'destructive' });
    }
  }

  onMount(() => {
    (async () => {
      await tick();
      place();
      // Visible first: a hidden element can't take the focus.
      await tick();
      const items = rootItems();
      (items.find((el) => el.getAttribute('aria-checked') === 'true' && el.dataset.model) || items[0] || menuEl)?.focus({ preventScroll: true });
    })();
    window.addEventListener('resize', place);
    return () => window.removeEventListener('resize', place);
  });
</script>

{#snippet tick_()}
  <svg class="tick" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m5 12 5 5 9-10" /></svg>
{/snippet}

{#snippet chev()}
  <svg class="chev" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
{/snippet}

{#snippet modelRow(r, n, level)}
  <button
    type="button"
    class="item model"
    role="menuitemradio"
    aria-checked={!!r.current}
    aria-keyshortcuts={n ? String(n) : undefined}
    aria-busy={busy === r.key}
    data-mi={level}
    data-model={r.key}
    tabindex="-1"
    onclick={() => pick(r)}
    onpointerenter={level === 'root' ? () => hoverRoot('') : undefined}
  >
    <span class="lbl">
      <span class="nm" title={r.name}>{r.name}</span>
      {#if level === 'root'}<span class="where">{whereOf(r)}</span>{/if}
      {#if r.missing}<span class="warn">no longer offered</span>{:else if level === 'sub' && r.isNew}<span class="new">New</span>{/if}
    </span>
    <span class="end">
      {#if busy === r.key}
        <span class="spin" aria-hidden="true"></span>
      {:else if r.current}
        {@render tick_()}
      {:else if n}
        <kbd>{n}</kbd>
      {/if}
    </span>
  </button>
{/snippet}

{#snippet effortMenu()}
  {@const opts = effortOptions(eff)}
  <div class="group" role="group" aria-label="Effort levels" onpointerleave={() => (hintLevel = subEl?.contains(document.activeElement) ? (document.activeElement.dataset.level ?? null) : null)}>
    {#each opts as o (o.value)}
      <button
        type="button"
        class="item"
        role="menuitemradio"
        aria-checked={o.value === effNow}
        data-mi="sub"
        data-level={o.value}
        tabindex="-1"
        onclick={() => setEffort(o.value)}
        onfocus={() => (hintLevel = o.value)}
        onpointerenter={() => (hintLevel = o.value)}
      >
        <span class="lbl"><span class="nm">{o.label}</span></span>
        <span class="end">{#if o.value === effNow}{@render tick_()}{/if}</span>
      </button>
    {/each}
  </div>
  <p class="hint" data-testid="chat-effort-hint">{hint}</p>
{/snippet}

{#snippet moreMenu()}
  {#if moreCount > SEARCH_FROM}
    <div class="search">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="6.5" /><path d="m20 20-4.2-4.2" /></svg>
      <input bind:this={searchEl} bind:value={q} type="search" placeholder="Search models" aria-label="Search models" spellcheck="false" />
    </div>
  {/if}
  <div class="scroll">
    {#each moreShown as g (g.id)}
      <div class="group" role="group" aria-label={groupLabel(g)}>
        <div class="ghead" aria-hidden="true">{groupLabel(g)}</div>
        {#each g.rows.slice(0, GROUP_MAX) as r (r.key)}{@render modelRow(r, 0, 'sub')}{/each}
        {#if g.rows.length > GROUP_MAX}
          <p class="note">And {g.rows.length - GROUP_MAX} more: search to narrow it down.</p>
        {/if}
      </div>
    {:else}
      <p class="note">
        {q.trim() ? `No model matches “${q.trim()}”.` : 'No other models yet: add a provider or download one.'}
      </p>
    {/each}
  </div>
{/snippet}

{#snippet subMenu()}
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div
    bind:this={subEl}
    class="mm sub"
    class:inline={subPos.inline}
    class:ready={subPos.ready || subPos.inline}
    class:wide={sub === 'more'}
    role="menu"
    aria-label={sub === 'effort' ? 'Effort' : 'More models'}
    data-submenu={sub}
    data-testid={sub === 'effort' ? 'chat-effort' : 'chat-more-models'}
    style={subPos.inline ? '' : `left: ${subPos.left}px; top: ${subPos.top}px; min-height: min(${subPos.minH}px, calc(100vh - 16px))`}
    onkeydown={onSubKey}
  >
    {#if sub === 'effort'}{@render effortMenu()}{:else}{@render moreMenu()}{/if}
  </div>
{/snippet}

<!-- svelte-ignore a11y_interactive_supports_focus -->
<div
  bind:this={menuEl}
  class="mm"
  class:ready={pos.ready}
  role="menu"
  aria-label="Chat model"
  tabindex="-1"
  data-testid="chat-model-menu"
  style="left: {pos.left}px; bottom: {pos.bottom}px; max-height: {pos.maxH}px"
  onkeydown={onRootKey}
>
  {#each main as r, i (r.key)}{@render modelRow(r, i < 9 ? i + 1 : 0, 'root')}{/each}
  {#if main.length}<div class="sep" role="separator"></div>{/if}

  {#if eff}
    <button
      type="button"
      class="item"
      class:open={sub === 'effort'}
      role="menuitem"
      aria-haspopup="menu"
      aria-expanded={sub === 'effort'}
      aria-label="Effort: {effortLabel(effNow) || 'Default'}"
      data-mi="root"
      data-sub="effort"
      tabindex="-1"
      onclick={() => openSub('effort', true)}
      onpointerenter={() => hoverRoot('effort')}
    >
      <span class="lbl"><span class="nm">Effort</span></span>
      <span class="val" data-testid="menu-effort">{effortLabel(effNow) || 'Default'}</span>
      {@render chev()}
    </button>
    {#if sub === 'effort' && subPos.inline}{@render subMenu()}{/if}
  {/if}

  <button
    type="button"
    class="item"
    class:open={sub === 'more'}
    role="menuitem"
    aria-haspopup="menu"
    aria-expanded={sub === 'more'}
    data-mi="root"
    data-sub="more"
    tabindex="-1"
    onclick={() => openSub('more', true)}
    onpointerenter={() => hoverRoot('more')}
  >
    <span class="lbl"><span class="nm">More models</span></span>
    {@render chev()}
  </button>
  {#if sub === 'more' && subPos.inline}{@render subMenu()}{/if}

  {#if hasFast}
    <div class="sep" role="separator"></div>
    <div class="ghead" aria-hidden="true">Fast mode</div>
    <button
      type="button"
      class="item fast"
      role="menuitemcheckbox"
      aria-checked={!!eff.fast}
      aria-label="Enable fast mode"
      aria-describedby="chat-fast-line"
      title={FAST_NOTE}
      data-mi="root"
      tabindex="-1"
      onclick={toggleFast}
      onpointerenter={() => hoverRoot('')}
    >
      <span class="lbl stack">
        <span class="nm">Enable fast mode</span>
        <span class="line" id="chat-fast-line">{FAST_LINE}</span>
      </span>
      <span class="switch" class:on={eff.fast} aria-hidden="true"><span class="knob"></span></span>
    </button>
  {/if}

  <div class="sep" role="separator"></div>
  <button type="button" class="item add" role="menuitem" data-mi="root" tabindex="-1" onclick={() => onrail?.()} onpointerenter={() => hoverRoot('')}>
    <svg class="plus" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
    <span class="lbl"><span class="nm">Add a provider or download a model…</span></span>
  </button>
</div>

{#if sub && !subPos.inline}{@render subMenu()}{/if}

<style>
  .mm,
  .mm * {
    box-sizing: border-box;
  }
  .mm {
    position: fixed;
    z-index: 40;
    display: flex;
    flex-direction: column;
    width: max-content;
    min-width: 248px;
    max-width: min(340px, calc(100vw - 16px));
    padding: 4px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-lg);
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-menu);
    color: var(--yap-fg);
    font-size: 12.5px;
    line-height: 1.35;
    overflow-y: auto;
    outline: none;
    visibility: hidden;
  }
  .mm.ready {
    visibility: visible;
  }
  @media (prefers-reduced-motion: no-preference) {
    .mm.ready:not(.inline) {
      animation: mm-in 0.12s var(--yap-ease-spring, ease-out) both;
    }
  }
  @keyframes mm-in {
    from {
      opacity: 0;
      transform: translateY(3px);
    }
  }
  /* a flyout */
  .mm.sub {
    min-width: 200px;
    max-height: calc(100vh - 16px);
    overflow: hidden;
  }
  /* Effort's line wraps at a menu's width. */
  .mm.sub:not(.wide):not(.inline) {
    width: 232px;
  }
  /* More: compact, its list scrolling past this height. */
  .mm.sub.wide {
    max-width: 320px;
    max-height: min(440px, calc(100vh - 16px));
    padding-top: 0;
  }
  /* …or a section under its item, when the window is narrow */
  .mm.sub.inline {
    position: static;
    width: auto;
    min-width: 0;
    max-width: none;
    margin: 2px 0 4px 10px;
    padding: 2px 0 2px 6px;
    border: 0;
    border-left: 2px solid var(--yap-border-subtle);
    border-radius: 0;
    background: none;
    box-shadow: none;
    visibility: visible;
  }
  .scroll {
    display: flex;
    flex-direction: column;
    min-height: 0;
    overflow-y: auto;
    padding-top: 4px;
  }
  .inline .scroll {
    max-height: 260px;
  }

  .item {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 30px;
    padding: 4px 9px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: default;
    outline: none;
    flex: 0 0 auto;
  }
  .item:hover,
  .item:focus-visible,
  .item.open {
    background: var(--yap-s3);
  }
  .item:focus-visible {
    box-shadow: inset 0 0 0 1.5px var(--yap-primary-line);
  }
  .lbl {
    display: flex;
    align-items: baseline;
    gap: 7px;
    flex: 1 1 auto;
    min-width: 0;
  }
  .lbl.stack {
    flex-direction: column;
    gap: 1px;
  }
  .nm {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
  }
  .model[aria-checked='true'] .nm {
    font-weight: 600;
  }
  .where {
    flex: 0 0 auto;
    color: var(--yap-muted);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .warn {
    flex: 0 0 auto;
    color: var(--yap-warning);
    font-size: 11.5px;
    white-space: nowrap;
  }
  .new {
    flex: 0 0 auto;
    padding: 0 6px;
    border-radius: var(--yap-r-full);
    background: var(--yap-primary-wash);
    color: var(--yap-primary-hover);
    font-size: 10.5px;
    font-weight: 600;
    line-height: 16px;
  }
  .line {
    color: var(--yap-muted);
    font-size: 11.5px;
  }
  .end {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex: 0 0 auto;
    min-width: 16px;
  }
  kbd {
    color: var(--yap-muted);
    font: inherit;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }
  .tick {
    width: 14px;
    height: 14px;
    color: var(--yap-primary);
  }
  .val {
    flex: 0 0 auto;
    color: var(--yap-muted);
    font-size: 12px;
  }
  .chev {
    width: 13px;
    height: 13px;
    flex: 0 0 auto;
    color: var(--yap-muted-70);
  }
  .plus {
    width: 13px;
    height: 13px;
    flex: 0 0 auto;
    color: var(--yap-muted);
  }
  .add .nm {
    font-weight: 400;
    color: var(--yap-fg-80);
  }
  .sep {
    flex: 0 0 auto;
    height: 1px;
    margin: 4px 6px;
    background: var(--yap-border-subtle);
  }
  .ghead {
    flex: 0 0 auto;
    padding: 6px 9px 2px;
    color: var(--yap-muted);
    font-size: 11px;
    font-weight: 500;
  }
  .group {
    display: flex;
    flex-direction: column;
  }
  .hint,
  .note {
    margin: 2px 9px 4px;
    color: var(--yap-muted);
    font-size: 11.5px;
    line-height: 1.45;
  }
  .hint {
    min-height: calc(2 * 1.45em);
    padding-top: 6px;
    border-top: 1px solid var(--yap-border-subtle);
  }

  /* More models' search */
  .search {
    display: flex;
    align-items: center;
    gap: 7px;
    flex: 0 0 auto;
    margin: 0 -4px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--yap-border-subtle);
    color: var(--yap-muted);
  }
  .inline .search {
    margin: 0;
    padding: 4px 6px 6px;
  }
  .search svg {
    width: 14px;
    height: 14px;
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
  .search:focus-within {
    box-shadow: inset 0 -2px 0 var(--yap-primary-tint);
  }

  /* Fast mode's switch (Toggle's look, drawn in the item: one control) */
  .switch {
    position: relative;
    flex: 0 0 auto;
    width: 32px;
    height: 18px;
    border: 1.5px solid var(--yap-switch-edge);
    border-radius: var(--yap-r-full);
    background: var(--yap-s2);
    transition:
      background var(--yap-dur) ease,
      border-color var(--yap-dur) ease;
  }
  .switch.on {
    border-color: var(--yap-ink);
    background: var(--yap-ink);
  }
  .knob {
    position: absolute;
    top: 50%;
    left: 3px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--yap-switch-edge);
    transform: translateY(-50%);
    transition:
      transform var(--yap-dur) ease,
      background var(--yap-dur) ease;
  }
  .switch.on .knob {
    width: 12px;
    height: 12px;
    background: var(--yap-ink-fg);
    transform: translate(12px, -50%);
  }

  .spin {
    width: 10px;
    height: 10px;
    border: 2px solid currentColor;
    border-top-color: transparent;
    border-radius: 50%;
    color: var(--yap-muted);
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
