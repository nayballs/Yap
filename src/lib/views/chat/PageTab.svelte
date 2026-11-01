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
  // - Header: Back, Reload, Save a shot (a picture of it, kept with the
  //   answer: `page_view_shot`), Open in <browser> (`open_external`, the
  //   browser Settings → General → Links chose), expand, ✕.
  // - Under it, where it's from, its section and "Open in Brave" in words
  //   (as a picture's "From … · Open in …"); a right-click on words
  //   selected in the page offers Ask Yap about this and Read aloud (its own
  //   menu, Rust's), and Snip to Chat.
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { onMount, untrack } from 'svelte';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { useBrowserName, openInLabel } from '../../stores/browserName.svelte.js';
  import { pageKey } from './chatDoc.svelte.js';

  /**
   * `doc`: the panel's state (its `page`); `narrow`: no room beside the chat
   * (no expand); `conversationId`: the chat a shot is kept in.
   */
  let { doc, narrow = false, conversationId = null } = $props();

  // (DocPanel makes one of these per page: its key never changes.)
  const page = untrack(() => doc.page);
  const key = pageKey(page?.url);
  const browser = useBrowserName();

  // What Rust says the page is doing (`yap-page-view`).
  let st = $state({ title: '', url: '', canGoBack: false, loading: true, error: null });
  let stageEl = $state(null);
  let shooting = $state(false);
  let failed = $state('');

  const title = $derived(st.title || page?.title || page?.site || 'Page');
  const site = $derived(page?.site || '');
  const here = $derived(st.url || page?.url || '');

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

  onMount(() => {
    let alive = true;
    let un = null;
    listen('yap-page-view', (e) => {
      const p = e.payload;
      if (p?.key !== key) return;
      st = { title: p.title || '', url: p.url || '', canGoBack: !!p.canGoBack, loading: !!p.loading, error: p.error || null };
    }).then((u) => (alive ? (un = u) : u()));
    // Made (or found) with its place now, then followed.
    const first = measure();
    sent = JSON.stringify(first);
    invoke('page_view_show', { key, url: page.url, section: page.section || null, bounds: first })
      .then(() => {
        if (!alive) return;
        ready = true;
        // Placed again at once: the stage may have moved while it was made.
        sent = '';
        follow();
      })
      .catch((e) => {
        if (alive) failed = String(e);
      });
    timer = setInterval(follow, 150);
    const onResize = () => follow();
    window.addEventListener('resize', onResize);
    return () => {
      alive = false;
      clearInterval(timer);
      window.removeEventListener('resize', onResize);
      un?.();
      // Out of sight, its place kept (its tab closed closes it: chatDoc.svelte.js).
      invoke('page_view_place', { key, bounds: null }).catch(() => {});
    };
  });

  // ---- the header's buttons ------------------------------------------------------------

  function back() {
    invoke('page_view_back', { key }).catch(() => {});
  }

  function reload() {
    failed = '';
    invoke('page_view_reload', { key }).catch(() => {});
  }

  /** Save a shot: a picture of the page as it is, kept with the answer, shown in its tab. */
  async function saveShot() {
    if (shooting) return;
    shooting = true;
    try {
      const picture = await invoke('page_view_shot', { key, url: here, title, conversationId });
      doc.showPicture(picture);
    } catch (e) {
      toast({ title: "Couldn't take a shot of the page", description: String(e), variant: 'destructive' });
    } finally {
      shooting = false;
    }
  }

  const openOutside = () => openExternalLink(here);
</script>

<section class="pagetab" aria-label="Page" data-testid="chat-page">
  <header class="pagehead">
    <svg class="pageicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" /></svg>
    <h2 class="pagetitle" title={here} data-testid="chat-page-title">{title}</h2>
    <div class="pageactions">
      <button type="button" class="pagebtn" onclick={back} disabled={!st.canGoBack} aria-label="Back" title="Back">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 6l-6 6 6 6" /></svg>
      </button>
      <button type="button" class="pagebtn" onclick={reload} aria-label="Reload" title="Reload">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 5v6h-6" /></svg>
      </button>
      <button type="button" class="pagebtn" onclick={saveShot} disabled={shooting || !!failed} aria-label="Save a shot of the page" title="Save a shot (a picture of the page as it is)" data-testid="chat-page-shot">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 8h3l2-3h6l2 3h3v11H4z" /><circle cx="12" cy="13" r="3.5" /></svg>
      </button>
      <button type="button" class="pagebtn" onclick={openOutside} aria-label={openInLabel(browser.label)} title={`${openInLabel(browser.label)}\n${here}`}>
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
    </div>
  </header>

  <!-- The page's own webview sits over this (Rust's); under it, what shows
       until it's there, or when it couldn't open. -->
  <div class="stage" bind:this={stageEl} role="region" aria-label={`The page: ${title}`} data-testid="chat-page-stage">
    {#if failed}
      <div class="pagefail" data-testid="chat-page-failed">
        <p>Yap couldn't bring the page up here.</p>
        <p class="why">{failed}</p>
        <button type="button" class="pageopen" onclick={openOutside}>{openInLabel(browser.label)}</button>
      </div>
    {:else}
      <p class="pagewait" aria-live="polite">{st.loading ? 'Loading the page…' : ''}</p>
    {/if}
  </div>

  <footer class="pageinfo">
    <p class="pagesource" data-testid="chat-page-source">
      <span class="from">From <span class="pagesite">{site || here}</span>{#if page?.section}<span class="sep" aria-hidden="true">·</span><span class="section">{page.section}</span>{/if}</span>
      <span class="sep" aria-hidden="true">·</span>
      <button type="button" class="openlink" onclick={openOutside} title={here} data-testid="chat-page-open">{openInLabel(browser.label)}</button>
    </p>
    {#if st.error}
      <p class="pageerror" role="status">{st.error}</p>
    {:else}
      <p class="pagehint">Select words on the page and right-click to ask Yap about them or hear them read aloud.</p>
    {/if}
  </footer>
</section>

<style>
  .pagetab {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .pagehead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 8px 18px;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .pageicon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-primary);
  }
  .pagetitle {
    flex: 1 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-paper-text-2);
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
