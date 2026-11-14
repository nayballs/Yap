<script>
  // A live web page beside the chat (Milestone 2 of the look-and-show plan,
  // 2026-11-01; chat/pages.rs `show_page`, chat/pages/view.rs): its tab of
  // the workspace, in DocPanel's own chunk.
  // - The page itself is its own webview, Rust's (its own WebView2 profile,
  //   no Yap IPC, public web pages only), laid over this tab's stage: the
  //   stage's place goes to Rust in physical pixels whenever it moves
  //   (`page_view_place`), and the page hides whenever anything of Yap's
  //   covers the stage (a menu, a dialog, Settings, a toast) or the tab goes
  //   — so it keeps its place (its scroll, a map's zoom) while another tab
  //   shows, and closes with its tab (chatDoc.svelte.js `closeTab`).
  // - Its bar, after Claude Code's browser pane (2026-11-01): Back, Forward,
  //   Reload, the address (where the page is now; type an address or words
  //   and Enter: `page_view_go`, a search on the person's engine — Ctrl+L
  //   in the panel, Esc puts it back; "Not secure" on a plain-http page,
  //   never a localhost one) across the whole row. Its buttons — a Code
  //   chat's Dev servers (ServersMenu.svelte), Open in <browser>
  //   (`open_external`, the browser Settings → General → Links chose),
  //   expand, ✕ — sit in the panel's tab row after its + (`actions`, drawn
  //   by DocPanel.svelte's `rowActions`), as Claude Code's app has them
  //   (14 Nov 2026, the maintainer's screenshot of it).
  // - A new tab (the row's +, Ctrl+T: chatDoc.svelte.js `newPage`) has no
  //   page yet: the address waits, over a Code chat's dev servers (▶ starts
  //   one and opens it here; a running one opens) and the pages this chat
  //   read (`recent`), as Claude Code's offers the project's servers; its
  //   webview is made once there's somewhere to go.
  // - While Chat acts on the page (chat/pages/act.rs: clicks, typing), a
  //   strip under the bar says "Yap is using this page", with Stop (the
  //   turn's ■: chatDoc.svelte.js `acting`), until the turn ends. When Yap
  //   asks before an action here ("Yap wants to press ‘Book now’…", the
  //   thread's card: chatDoc's `asking`), the strip asks it too, with Allow
  //   once / Don't — the panel may cover the thread (a narrow window, the
  //   page expanded) — and either place answers it.
  // - The page's own dialogs (alert, confirm, prompt, "Leave this page?")
  //   are never WebView2's: Rust dismisses them while Yap acts (view.rs),
  //   and here, while the person uses the page, a strip asks them
  //   (`yap-page-dialog` → `page_view_dialog`).
  // - Under it, where it's from, its section and "Open in Brave" in words
  //   (as a picture's "From … · Open in …"); a right-click on words
  //   selected in the page offers Ask Yap about this and Read aloud (its own
  //   menu, Rust's), and Snip to Chat.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { getContext, onMount, tick, untrack } from 'svelte';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { useBrowserName, openInLabel } from '../../stores/browserName.svelte.js';
  import { tabKeyOf } from './chatDoc.svelte.js';
  import { questionParts, whyOf } from './pageConfirm.js';
  import ServersMenu from './ServersMenu.svelte';

  /**
   * `doc`: the panel's state (its `page`); `narrow`: no room beside the chat
   * (no expand); `conversationId`: the chat a shot is kept in; `recent`: the
   * pages this conversation read ({url, title}), for a new tab; `workspace`: a
   * Code chat's folder (its Dev servers, ServersMenu.svelte), else null.
   */
  let { doc, narrow = false, conversationId = null, recent = [], workspace = null } = $props();

  // (DocPanel makes one of these per page tab: its key never changes.)
  const page = untrack(() => doc.page);
  const key = tabKeyOf(page);
  const browser = useBrowserName();

  // Its buttons — the Dev servers, Save a shot, Open in your browser, expand, close — in the panel's tab row,
  // after its + (14 Nov 2026; the maintainer, from the Claude Code app's browser pane: "this is where they put
  // their buttons"), so the address bar has the row below to itself (DocPanel.svelte `rowActions`).
  const rowActions = getContext('yap-doc-row-actions');
  onMount(() => {
    rowActions?.set(actions);
    return () => rowActions?.clear(actions);
  });

  // A Code chat's dev servers on a new tab (as Claude Code's app lists them there): what launch.json has.
  let servers = $state([]);
  let starting = $state('');
  let serverError = $state('');
  async function loadServers() {
    if (!workspace || !blank) return;
    try {
      servers = (await invoke('dev_servers', { workspace }))?.configs ?? [];
    } catch {
      servers = [];
    }
  }
  async function startServer(c) {
    starting = c.name;
    serverError = '';
    try {
      const info = await invoke('dev_server_start', { workspace, name: c.name });
      await go(info.url);
    } catch (e) {
      serverError = String(e);
    } finally {
      starting = '';
    }
  }
  onMount(() => {
    loadServers();
    let un = null;
    let on = true;
    listen('yap-dev-servers', () => loadServers()).then((u) => (on ? (un = u) : u()));
    return () => {
      on = false;
      un?.();
    };
  });

  // What Rust says the page is doing (`yap-page-view`).
  // `width`: the desktop width Chat laid the page out at (chat/pages.rs
  // `browser_tabs` width), shown scaled to fit — its chip's × puts it back.
  let st = $state({ title: '', url: '', canGoBack: false, canGoForward: false, loading: true, error: null, width: null });
  let stageEl = $state(null);
  let addrEl = $state(null);
  let failed = $state('');
  // Where the address bar last took this tab (a tab shown again opens there).
  const went = untrack(() => doc.pageNow[key]?.went || '');
  // A new tab, nowhere yet.
  let blank = $state(!page?.url && !went);
  // The address bar: what's typed while it has the focus, else where the page is.
  let typing = $state(false);
  let typed = $state('');
  let addrError = $state('');
  // The page's dialog waiting for the person ({kind, message, default}), and a prompt's answer.
  let dialog = $state(null);
  let dialogText = $state('');
  // Yap's question about this page, while it waits (the thread's card too).
  const ask = $derived(doc.asking?.key === key ? doc.asking : null);
  const askQ = $derived(ask ? questionParts(ask) : null);

  const hostOf = (u) => {
    try {
      return new URL(u).hostname.replace(/^www\./, '');
    } catch {
      return '';
    }
  };
  const here = $derived(st.url || went || page?.url || '');
  const title = $derived(st.title || page?.title || page?.site || hostOf(here) || 'Page');
  const site = $derived(hostOf(here) || page?.site || '');
  // (A dev server on this PC — localhost — is a secure context, as browsers count it: no warning.)
  const insecure = $derived(/^http:\/\//i.test(here) && !/^http:\/\/(localhost|[^/]*\.localhost|127\.\d+\.\d+\.\d+|\[::1\])(:\d+)?(\/|$)/i.test(here));

  // ---- where the page sits: over the stage, unless something covers it -------------------

  /** The stage's place in physical pixels, or null: too small, out of sight, or covered. */
  function measure() {
    const el = stageEl;
    if (!el || !el.isConnected) return null;
    const r = el.getBoundingClientRect();
    if (r.width < 24 || r.height < 24) return null;
    if (el.checkVisibility && !el.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true })) return null;
    // Anything of Yap's over it (a menu, a dialog, Settings, a toast): out of
    // sight until it goes (the page is a window of its own: it would cover them).
    for (const fx of [0.04, 0.5, 0.96]) {
      for (const fy of [0.04, 0.5, 0.96]) {
        const hit = document.elementFromPoint(r.left + r.width * fx, r.top + r.height * fy);
        if (!hit || !el.contains(hit)) return null;
      }
    }
    const dpr = window.devicePixelRatio || 1;
    return { x: Math.round(r.left * dpr), y: Math.round(r.top * dpr), w: Math.round(r.width * dpr), h: Math.round(r.height * dpr) };
  }

  let sent = '';
  let timer = 0;
  let ready = false;

  function follow() {
    if (!ready) return;
    const b = measure();
    const now = JSON.stringify(b);
    if (now === sent) return;
    sent = now;
    invoke('page_view_place', { key, bounds: b }).catch(() => {});
  }

  let alive = true;

  /** The page there: placed again at once (the stage may have moved while it was made), then followed. */
  function placed() {
    if (!alive) return;
    ready = true;
    sent = '';
    follow();
  }

  onMount(() => {
    alive = true;
    let un = null;
    listen('yap-page-view', (e) => {
      const p = e.payload;
      if (p?.key !== key) return;
      st = { title: p.title || '', url: p.url || '', canGoBack: !!p.canGoBack, canGoForward: !!p.canGoForward, loading: !!p.loading, error: p.error || null, width: p.width || null };
      if (p.url || p.title) doc.pageMoved(key, { title: p.title || '', url: p.url || '' });
    }).then((u) => (alive ? (un = u) : u()));
    let unDialog = null;
    listen('yap-page-dialog', (e) => {
      const p = e.payload;
      if (p?.key !== key) return;
      if (p.gone) {
        dialog = null;
        return;
      }
      dialog = { kind: p.kind || 'alert', message: p.message || '', default: p.default || '' };
      dialogText = dialog.default;
    }).then((u) => (alive ? (unDialog = u) : u()));
    if (blank) {
      // A new tab: the address bar waits.
      tick().then(() => addrEl?.focus());
    } else {
      // Made (or found) with its place now, then followed.
      const first = measure();
      sent = JSON.stringify(first);
      invoke('page_view_show', { key, url: went || page.url, section: went ? null : page.section || null, bounds: first })
        .then(placed)
        .catch((e) => {
          if (alive) failed = String(e);
        });
    }
    timer = setInterval(follow, 150);
    const onResize = () => follow();
    window.addEventListener('resize', onResize);
    return () => {
      alive = false;
      clearInterval(timer);
      window.removeEventListener('resize', onResize);
      un?.();
      unDialog?.();
      // Out of sight, its place kept (its tab closed closes it: chatDoc.svelte.js).
      invoke('page_view_place', { key, bounds: null }).catch(() => {});
    };
  });

  /** The page's dialog answered: OK (with a prompt's words) or Cancel. */
  function answerDialog(accept) {
    const d = dialog;
    if (!d) return;
    dialog = null;
    invoke('page_view_dialog', { key, accept: !!accept, text: d.kind === 'prompt' && accept ? dialogText : null }).catch(() => {});
  }

  // ---- the bar ----------------------------------------------------------------------

  function back() {
    invoke('page_view_back', { key }).catch(() => {});
  }

  function forward() {
    invoke('page_view_forward', { key }).catch(() => {});
  }

  function reload() {
    failed = '';
    invoke('page_view_reload', { key }).catch(() => {});
  }

  /** The chip's ×: the page back at the panel's own width. */
  function panelWidth() {
    st.width = null;
    invoke('page_view_width', { key, width: null }).catch(() => {});
  }

  /** `text` (typed, or a page offered on a new tab) opened here: an address, or a search on the person's engine. */
  async function go(text) {
    const words = String(text ?? '').trim();
    if (!words) return;
    addrError = '';
    failed = '';
    try {
      const url = await invoke('page_view_go', { key, typed: words, bounds: measure() });
      if (!alive) return;
      doc.pageMoved(key, { went: url });
      typing = false;
      addrEl?.blur();
      if (blank) {
        blank = false;
        placed();
      }
    } catch (e) {
      if (alive) addrError = String(e);
    }
  }

  function addrFocus() {
    typing = true;
    typed = blank ? '' : here;
    addrError = '';
    tick().then(() => addrEl?.select());
  }

  function addrKey(e) {
    if (e.key === 'Enter') {
      e.preventDefault();
      go(typed);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      typed = blank ? '' : here;
      addrError = '';
      if (!blank) addrEl?.blur();
    }
  }

  /** Ctrl+L in the panel: the address bar. */
  function tabKey(e) {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === 'l') {
      e.preventDefault();
      e.stopPropagation();
      addrEl?.focus();
    }
  }

  const openOutside = () => openExternalLink(here);
</script>

{#snippet actions()}
    {#if workspace}
      <!-- A Code chat's dev servers (.claude/launch.json): its page opens here when this tab has none, else in a new tab. -->
      <ServersMenu {workspace} onopen={(url, name) => (blank ? go(url) : doc.showPage({ url, title: name }))} />
    {/if}
    <button type="button" class="pagebtn" onclick={openOutside} disabled={blank} aria-label={openInLabel(browser.label)} title={`${openInLabel(browser.label)}\n${here}`}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6M20 4l-9 9" /><path d="M18 14v5H5V6h5" /></svg>
    </button>
    {#if !narrow}
      <button
        type="button"
        class="pagebtn"
        onclick={() => (doc.expanded = !doc.expanded)}
        aria-label={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
        title={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
      >
        {#if doc.expanded}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" /></svg>
        {:else}
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" /></svg>
        {/if}
      </button>
    {/if}
    <button type="button" class="pagebtn" onclick={() => doc.close()} aria-label="Close the panel" title="Close">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
    </button>
{/snippet}

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="pagetab" aria-label={blank ? 'New tab' : `Page: ${title}`} data-testid="chat-page" onkeydown={tabKey}>
  <h2 class="sr-title" data-testid="chat-page-title">{blank ? 'New tab' : title}</h2>
  <header class="pagebar">
    <div class="pagenav">
      <button type="button" class="pagebtn" onclick={back} disabled={blank || !st.canGoBack} aria-label="Back" title="Back">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 6l-6 6 6 6" /></svg>
      </button>
      <button type="button" class="pagebtn" onclick={forward} disabled={blank || !st.canGoForward} aria-label="Forward" title="Forward">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
      </button>
      <button type="button" class="pagebtn" onclick={reload} disabled={blank} aria-label="Reload" title="Reload">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 5v6h-6" /></svg>
      </button>
    </div>
    <div class="addr" class:typing class:bad={!!addrError}>
      {#if insecure && !typing}
        <span class="notsecure" title="This page isn't sent over a secure connection: don't type anything private into it.">Not secure</span>
      {/if}
      <input
        bind:this={addrEl}
        class="addrinput"
        type="text"
        spellcheck="false"
        autocomplete="off"
        aria-label="Address"
        placeholder="Search the web or type an address"
        value={typing ? typed : blank ? '' : here}
        oninput={(e) => (typed = e.currentTarget.value)}
        onfocus={addrFocus}
        onblur={() => (typing = false)}
        onkeydown={addrKey}
        title={blank ? '' : here}
        data-testid="chat-page-address"
      />
    </div>
    {#if st.width && !blank}
      <!-- Chat laid the page out at a desktop width (chat/pages.rs `browser_tabs`
           width): shown scaled to fit, the layout Chat reads and clicks in. -->
      <span class="widthchip" title={`Laid out ${st.width} px wide, as on a desktop, and shown scaled to fit the panel`} data-testid="chat-page-width">
        Desktop width
        <button type="button" class="widthoff" onclick={panelWidth} aria-label="Back to the panel's width" title="Back to the panel's width" data-testid="chat-page-width-off">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
        </button>
      </span>
    {/if}
    {#if !rowActions}
      <!-- (Outside the panel's tab row: here, as before.) -->
      <div class="pageactions">{@render actions()}</div>
    {/if}
  </header>
  {#if addrError}
    <p class="addrerror" role="alert" data-testid="chat-page-address-error">{addrError}</p>
  {/if}
  {#if ask}
    <!-- Yap asks before an action here (chat/pages/act/confirm.rs): the
         thread's card, said here too — the thread may be covered. -->
    <div class="asking" role="group" aria-label="Yap asks before acting on the page" data-testid="chat-page-ask">
      <div class="asktext">
        <p class="askq">{askQ.lead} <strong>‘{askQ.quoted}’</strong>{askQ.then}{#if ask.site}{' '}on {ask.site}{/if}</p>
        <p class="askwhy">{whyOf(ask)} Only you can say yes.</p>
      </div>
      <div class="askbtns">
        <button type="button" class="askallow" onclick={() => doc.answerAsking(true)} data-testid="chat-page-ask-allow">Allow once</button>
        <button type="button" class="askdeny" onclick={() => doc.answerAsking(false)} data-testid="chat-page-ask-deny">Don't</button>
      </div>
    </div>
  {:else if doc.acting === key}
    <!-- Yap is clicking or typing here (chat/pages/act.rs): said over the
         page (the page itself is a window of its own, so not on it), with
         Stop — the turn's ■ — until the turn ends. -->
    <div class="acting" aria-live="polite" data-testid="chat-page-acting">
      <span class="actdot" aria-hidden="true"></span>
      <span class="acttext">Yap is using this page</span>
      <button type="button" class="actstop" onclick={() => doc.stopUsing()} data-testid="chat-page-acting-stop">Stop</button>
    </div>
  {/if}
  {#if dialog}
    <!-- The page's own dialog (alert, confirm, prompt, "Leave this page?"),
         asked here instead of WebView2's: the page waits for the answer. -->
    <div class="pdialog" role="alertdialog" aria-label="The page asks" data-testid="chat-page-dialog">
      <p class="pdmsg"><span class="pdsite">{site || 'The page'} says</span>{#if dialog.kind === 'leave'} Leave this page? What you changed may not be saved.{:else if dialog.message}: {dialog.message}{/if}</p>
      {#if dialog.kind === 'prompt'}
        <input class="pdinput" bind:value={dialogText} aria-label="Your answer to the page" onkeydown={(e) => e.key === 'Enter' && answerDialog(true)} />
      {/if}
      <div class="askbtns">
        <button type="button" class="askallow" onclick={() => answerDialog(true)} data-testid="chat-page-dialog-ok">{dialog.kind === 'leave' ? 'Leave' : 'OK'}</button>
        {#if dialog.kind !== 'alert'}
          <button type="button" class="askdeny" onclick={() => answerDialog(false)} data-testid="chat-page-dialog-cancel">{dialog.kind === 'leave' ? 'Stay' : 'Cancel'}</button>
        {/if}
      </div>
    </div>
  {/if}

  <!-- The page's own webview sits over this (Rust's); under it, what shows
       until it's there, or when it couldn't open — or a new tab's start. -->
  <div class="stage" class:blank bind:this={stageEl} role="region" aria-label={blank ? 'New tab' : `The page: ${title}`} data-testid="chat-page-stage">
    {#if blank}
      <div class="newtab" data-testid="chat-page-new">
        {#if workspace && servers.length}
          <!-- A Code chat's dev servers (.claude/launch.json), as Claude Code's app offers them on a new tab: ▶ starts
               one and opens it here; a running one opens. -->
          <ul class="ntlist" aria-label="Dev servers" data-testid="chat-page-new-servers">
            {#each servers as c (c.name)}
              {@const on = c.server && (c.server.state === 'ready' || c.server.state === 'starting')}
              <li class="ntserver">
                <button type="button" class="ntrow" onclick={() => (on ? go(c.server.url) : startServer(c))} disabled={starting === c.name} title={on ? c.server.url : `Start: ${c.line}`}>
                  <svg class="nticon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="4" y="4" width="16" height="7" rx="1.5" /><rect x="4" y="13" width="16" height="7" rx="1.5" /><path d="M8 7.5h.01M8 16.5h.01" /></svg>
                  <span class="nttitle">{c.name}</span>
                  <span class="ntsite">:{on ? c.server.port : c.port}</span>
                  <span class="ntplay" class:on aria-hidden="true">
                    {#if starting === c.name}…{:else if on}<svg viewBox="0 0 24 24"><path d="M5 12h12M13 6l6 6-6 6" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /></svg>{:else}<svg viewBox="0 0 24 24"><path d="M8 6l10 6-10 6z" fill="currentColor" /></svg>{/if}
                  </span>
                </button>
              </li>
            {/each}
          </ul>
          {#if serverError}<p class="addrerror" role="alert">{serverError}</p>{/if}
        {/if}
        {#if recent.length}
          <ul class="ntlist" aria-label="Pages from this chat">
            {#each recent as r (r.url)}
              <li>
                <button type="button" class="ntrow" onclick={() => go(r.url)} title={r.url}>
                  <svg class="nticon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></svg>
                  <span class="nttitle">{r.title || hostOf(r.url)}</span>
                  <span class="ntsite">{hostOf(r.url)}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
        <p class="nthint">
          {#if workspace}
            Run a server to preview your app, or search the web and type an address.<br />Edit the servers in .claude/launch.json (or Dev servers, above).
          {:else}
            Search the web or type an address.<br />Pages open here, apart from your own browser and its sign-ins.
          {/if}
        </p>
      </div>
    {:else if failed}
      <div class="pagefail" data-testid="chat-page-failed">
        <p>Yap couldn't bring the page up here.</p>
        <p class="why">{failed}</p>
        <button type="button" class="pageopen" onclick={openOutside}>{openInLabel(browser.label)}</button>
      </div>
    {:else}
      <p class="pagewait" aria-live="polite">{st.loading ? 'Loading the page…' : ''}</p>
    {/if}
  </div>

  {#if !blank}
    <footer class="pageinfo">
      <p class="pagesource" data-testid="chat-page-source">
        <span class="from">From <span class="pagesite">{site || here}</span>{#if page?.section && !went}<span class="sep" aria-hidden="true">·</span><span class="section">{page.section}</span>{/if}</span>
        <span class="sep" aria-hidden="true">·</span>
        <button type="button" class="openlink" onclick={openOutside} title={here} data-testid="chat-page-open">{openInLabel(browser.label)}</button>
      </p>
      {#if st.error}
        <p class="pageerror" role="status">{st.error}</p>
      {:else}
        <p class="pagehint">Select words on the page and right-click to ask Yap about them or hear them read aloud.</p>
      {/if}
    </footer>
  {/if}
</section>

<style>
  .pagetab {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  /* The page's title for screen readers and tests: the tab's name shows it. */
  .sr-title {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
    border: 0;
  }
  /* The bar: ← → ⟳, the address, then the page's buttons (Claude Code's browser pane's). */
  .pagebar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px 6px 8px;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .pagenav {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 0;
  }
  .addr {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    height: 28px;
    padding: 0 10px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 7px;
    background: var(--yap-paper-s1);
  }
  .addr.typing {
    border-color: var(--yap-primary-line);
    background: var(--yap-paper);
    box-shadow: 0 0 0 2px var(--yap-focus-ring);
  }
  .addr.bad {
    border-color: var(--yap-paper-danger);
  }
  .addrinput {
    flex: 1 1 auto;
    min-width: 0;
    height: 100%;
    padding: 0;
    border: none;
    outline: none;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    text-overflow: ellipsis;
  }
  .addrinput::placeholder {
    color: var(--yap-paper-muted);
  }
  .addr:not(.typing) .addrinput {
    color: var(--yap-paper-text-2);
  }
  .notsecure {
    flex: 0 0 auto;
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-paper-danger);
  }
  /* "Desktop width ×": the page laid out wide, shown scaled to fit. */
  .widthchip {
    flex: 0 0 auto;
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: 22px;
    padding: 0 2px 0 8px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper-s1);
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-paper-text-2);
    white-space: nowrap;
  }
  .widthoff {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 0;
    border: none;
    border-radius: 999px;
    background: none;
    color: inherit;
    cursor: pointer;
  }
  .widthoff:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .widthoff svg {
    width: 12px;
    height: 12px;
  }
  .addrerror {
    margin: 0;
    padding: 6px 14px;
    border-bottom: 1px solid var(--yap-paper-s2);
    font-size: 11.5px;
    color: var(--yap-paper-danger);
  }
  /* Yap's question, and the page's own dialog: a strip between the bar and the page. */
  .asking,
  .pdialog {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 12px;
    padding: 8px 10px 8px 14px;
    border-bottom: 1px solid var(--yap-primary-line);
    background: var(--yap-primary-wash);
    font-size: 12.5px;
    color: var(--yap-paper-ink);
  }
  .pdialog {
    border-bottom-color: var(--yap-paper-rule);
    background: var(--yap-paper-s1);
  }
  .asktext {
    flex: 1 1 220px;
    min-width: 0;
  }
  .askq,
  .askwhy,
  .pdmsg {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .pdmsg {
    flex: 1 1 220px;
    min-width: 0;
  }
  .askq strong {
    font-weight: 650;
  }
  .askwhy {
    margin-top: 2px;
    font-size: 11.5px;
    color: var(--yap-paper-muted);
  }
  .pdsite {
    font-weight: 600;
  }
  .pdinput {
    flex: 1 1 160px;
    min-width: 0;
    height: 26px;
    padding: 0 8px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 6px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
  }
  .askbtns {
    display: flex;
    flex: 0 0 auto;
    gap: 6px;
  }
  .askbtns button {
    height: 26px;
    padding: 0 11px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .askbtns .askallow {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .askbtns button:hover {
    background: var(--yap-paper-s2);
  }
  .askbtns .askallow:hover {
    background: var(--yap-primary-hover);
  }
  .askbtns button:focus-visible,
  .pdinput:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  /* "Yap is using this page": a strip between the bar and the page. */
  .acting {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 10px 5px 14px;
    border-bottom: 1px solid var(--yap-primary-line);
    background: var(--yap-primary-wash);
    font-size: 12px;
    color: var(--yap-paper-ink);
  }
  .actdot {
    flex: 0 0 auto;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--yap-primary);
    animation: actpulse 1.2s ease-in-out infinite;
  }
  @keyframes actpulse {
    50% {
      opacity: 0.35;
    }
  }
  :global(:root[data-motion='reduced']) .actdot {
    animation: none;
  }
  .acttext {
    flex: 1 1 auto;
    min-width: 0;
    font-weight: 600;
  }
  .actstop {
    flex: 0 0 auto;
    height: 24px;
    padding: 0 10px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }
  .actstop:hover {
    background: var(--yap-paper-s1);
  }
  .actstop:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .ntplay {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: var(--yap-r);
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .ntplay.on {
    color: var(--yap-success-text);
  }
  .ntplay svg {
    width: 14px;
    height: 14px;
  }
  .pageactions {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 2px;
  }
  .pagebtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
  }
  .pagebtn:hover:not(:disabled) {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .pagebtn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .pagebtn svg {
    width: 15px;
    height: 15px;
  }
  .pagebtn:focus-visible,
  .pageopen:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  /* "Open in Brave" where the page couldn't come up: the one with words. */
  .pageopen {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 180px;
    height: 26px;
    margin: 0 4px;
    padding: 0 9px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }
  .pageopen:hover {
    background: var(--yap-paper-s1);
  }
  /* The stage: the page's own webview lies over it. */
  .stage {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
    background: var(--yap-paper-s1);
  }
  /* A new tab: the pages this chat read, centred; the hint at the foot. */
  .stage.blank {
    background: var(--yap-paper);
  }
  .newtab {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 24px 18px 64px;
  }
  .ntlist {
    width: min(100%, 420px);
    margin: 0;
    padding: 4px;
    list-style: none;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 10px;
    background: var(--yap-paper-s1);
  }
  .ntrow {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-width: 0;
    padding: 9px 10px;
    border: none;
    border-radius: 7px;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
  }
  .ntrow:hover {
    background: var(--yap-paper-s2);
  }
  .ntrow:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .nticon {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
    color: var(--yap-paper-muted);
  }
  .nttitle {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ntsite {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
  .nthint {
    position: absolute;
    left: 18px;
    right: 18px;
    bottom: 18px;
    margin: 0;
    text-align: center;
    font-size: 11.5px;
    line-height: 1.5;
    color: var(--yap-paper-muted);
  }
  .pagewait {
    position: absolute;
    inset: 0;
    display: grid;
    place-items: center;
    margin: 0;
    font-size: 12.5px;
    color: var(--yap-paper-muted);
  }
  .pagefail {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 18px;
    text-align: center;
    font-size: 12.5px;
    color: var(--yap-paper-text-2);
  }
  .pagefail p {
    margin: 0;
  }
  .pagefail .why {
    color: var(--yap-paper-muted);
    overflow-wrap: anywhere;
  }
  .pageinfo {
    flex: 0 0 auto;
    padding: 9px 18px 11px;
    border-top: 1px solid var(--yap-paper-s2);
  }
  .pagesource,
  .pagehint,
  .pageerror {
    margin: 0;
    font-size: 11.5px;
    color: var(--yap-paper-muted);
  }
  .pagesource {
    display: flex;
    align-items: baseline;
    min-width: 0;
    white-space: nowrap;
  }
  .from {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .openlink {
    flex: 0 0 auto;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .openlink:hover {
    text-decoration: underline;
  }
  .openlink:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .pagesite {
    color: var(--yap-paper-text-2);
    font-weight: 600;
  }
  .sep {
    margin: 0 6px;
  }
  .pagehint {
    margin-top: 2px;
  }
  .pageerror {
    margin-top: 2px;
    color: var(--yap-paper-danger);
  }
</style>
