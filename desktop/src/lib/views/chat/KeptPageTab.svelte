<script>
  // **A page Yap keeps, beside the chat** (chat/made/pages.rs, 15 Nov 2026; after the Claude app's artifact
  // panel — kept on this PC, never published): its title, "Saved", its versions (‹ Version 2 of 3 ›, the
  // newest when a turn saves another) and ↻ to start it again, over the page in the same sandboxed frame as a
  // visual (the `yapmade` scheme's `/p/…`, its own policy, `sandbox="allow-scripts"`: no Yap IPC), Yap's
  // colours handed to it. It scrolls in its frame; a button in it may send a message into this chat on the
  // person's click (`sendPrompt`, visualBridge.js), as a visual's.
  //
  // **Its ⋯** (as the Claude app's artifact menu has Download and Open): **Save as…** writes the version shown
  // to a file the person picks; **Open in browser** opens a copy of it in their browser (a file of Yap's own in
  // the temp folder, swept after a day). Either way it keeps working there: Yap's colours, the chart libraries
  // from cdnjs with their pinned hashes, the same policy; its `sendPrompt` buttons do nothing outside Yap.
  import { untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { registerFrame, pageSrc } from './visualBridge.js';
  import { made } from './made.svelte.js';
  import { openContextMenu } from '../../ui/contextMenu.svelte.js';
  import { toast } from '../../ui/toast.svelte.js';
  import { exportName } from './chatExport.js';

  /** `doc`: the panel (`doc.kept`: `{id, conversation, title, n, seq}`); `onprompt(text, page)`. */
  let { doc, conversationId = null, onprompt = null } = $props();

  const k = $derived(doc.kept);
  const conv = $derived(k?.conversation ?? conversationId);
  const page = $derived(made.of(conv).find((m) => m.id === k?.id) ?? null);
  const versions = $derived((page?.versions ?? []).map((v) => v.n).sort((a, b) => a - b));
  const newest = $derived(versions.at(-1) ?? 1);
  const title = $derived(page?.title || k?.title || 'Page');

  // The version shown: the one asked for (a card's), else the newest — and the newest again when a turn
  // saves another while this one's the newest.
  let shown = $state(0);
  let lastSeq = -1;
  let lastNewest = 0;
  $effect(() => {
    const seq = k?.seq ?? 0;
    const top = newest;
    const was = untrack(() => shown);
    if (seq !== lastSeq) {
      lastSeq = seq;
      shown = untrack(() => k?.n) || top;
    } else if (top !== lastNewest && was === lastNewest) {
      shown = top;
    }
    lastNewest = top;
  });
  const at = $derived(Math.max(0, versions.indexOf(shown)));
  const src = $derived(k?.id && conv != null ? pageSrc(conv, k.id, shown || newest) : '');
  let restart = $state(0);

  let frameEl = $state(null);
  $effect(() => {
    if (!frameEl) return;
    let loads = 0;
    const el = frameEl;
    const want = el.getAttribute('src');
    const unregister = registerFrame(el, { onsize: () => {}, onprompt: (text) => onprompt?.(text, { ...(page ?? {}), id: k?.id, outside: !!page?.outside }) });
    // A page that navigated itself away (a script's own `location`): put back.
    const loaded = () => {
      loads += 1;
      if (loads > 1) {
        loads = 0;
        el.src = want;
      }
    };
    el.addEventListener('load', loaded);
    return () => {
      el.removeEventListener('load', loaded);
      unregister();
    };
  });

  const go = (d) => {
    const next = versions[at + d];
    if (next != null) shown = next;
  };

  // ---- Outside Yap (chat/made/pages.rs)
  /** The version on screen: the one asked for, else its newest. */
  const onScreen = () => shown || newest;
  /** The file Save as… suggests: its title, and the version when it has several. */
  const suggested = () => `${exportName(title)}${versions.length > 1 ? ` v${onScreen()}` : ''}.html`;

  async function saveAs() {
    if (!k?.id || conv == null) return;
    const n = onScreen();
    try {
      const { save } = await import('@tauri-apps/plugin-dialog');
      const path = await save({ defaultPath: suggested(), filters: [{ name: 'Web page', extensions: ['html', 'htm'] }] });
      if (!path) return;
      const wrote = await invoke('chat_page_save_as', { conversationId: conv, id: k.id, n, path });
      toast({ title: 'Page saved', description: wrote, variant: 'success' });
    } catch (e) {
      toast({ title: "Couldn't save the page", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }

  async function openInBrowser() {
    if (!k?.id || conv == null) return;
    try {
      await invoke('chat_page_open_in_browser', { conversationId: conv, id: k.id, n: onScreen() });
    } catch (e) {
      toast({ title: "Couldn't open the page in your browser", description: String(e?.message ?? e), variant: 'destructive' });
    }
  }

  function moreMenu(e) {
    openContextMenu(
      e,
      [
        { label: 'Save as…', icon: 'download', onselect: saveAs },
        { label: 'Open in browser', icon: 'open', onselect: openInBrowser },
      ],
      { label: 'Page' }
    );
  }
</script>

<section class="kept" data-testid="chat-kept-page" aria-label={title}>
  <header class="khead">
    <svg class="kicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="8" y="3" width="13" height="13" rx="2" /><path d="M16 16v3a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-9a2 2 0 0 1 2-2h3" /></svg>
    <h2 class="ktitle" title={title}>{title}</h2>
    <span class="kstate">Saved</span>
    {#if versions.length > 1}
      <div class="kvers" role="group" aria-label="Versions">
        <button type="button" class="kbtn" onclick={() => go(-1)} disabled={at <= 0} aria-label="Earlier version" title="Earlier version" data-testid="chat-kept-earlier">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 6l-6 6 6 6" /></svg>
        </button>
        <span class="kver" data-testid="chat-kept-version">Version {shown || newest} of {versions.length}</span>
        <button type="button" class="kbtn" onclick={() => go(1)} disabled={at >= versions.length - 1} aria-label="Later version" title="Later version" data-testid="chat-kept-later">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
        </button>
      </div>
    {/if}
    <button type="button" class="kbtn" onclick={() => (restart += 1)} aria-label="Start it again" title="Start it again" data-testid="chat-kept-restart">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 11a8 8 0 1 0-2.3 5.7" /><path d="M20 4v7h-7" /></svg>
    </button>
    {#if src}
      <button type="button" class="kbtn" data-more aria-haspopup="menu" onclick={moreMenu} aria-label="More for the page" title="Save as…, Open in browser" data-testid="chat-kept-more">
        <svg viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><circle cx="5.5" cy="12" r="1.6" /><circle cx="12" cy="12" r="1.6" /><circle cx="18.5" cy="12" r="1.6" /></svg>
      </button>
    {/if}
  </header>
  <div class="kbody">
    {#if src}
      {#key `${src}#${restart}`}
        <iframe bind:this={frameEl} {src} title={title} sandbox="allow-scripts" referrerpolicy="no-referrer" data-testid="chat-kept-frame"></iframe>
      {/key}
    {:else}
      <p class="kgone">This page isn't here any more.</p>
    {/if}
  </div>
</section>

<style>
  .kept {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .khead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px 10px 16px;
    border-bottom: 1px solid var(--yap-border-subtle);
  }
  .kicon {
    flex: none;
    width: 16px;
    height: 16px;
    color: var(--yap-muted);
  }
  .ktitle {
    flex: 0 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 14px;
    font-weight: 600;
    color: var(--yap-fg);
  }
  .kstate {
    flex: 1 1 auto;
    color: var(--yap-muted);
    font-size: 12.5px;
  }
  .kvers {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }
  .kver {
    color: var(--yap-muted);
    font-size: 12.5px;
    white-space: nowrap;
  }
  .kbtn {
    display: inline-grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: 0;
    border-radius: var(--yap-r-sm, 6px);
    background: transparent;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .kbtn:hover:not(:disabled) {
    background: var(--yap-s3);
    color: var(--yap-fg);
  }
  .kbtn:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .kbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring, var(--yap-primary));
    outline-offset: 1px;
  }
  .kbtn svg {
    width: 15px;
    height: 15px;
  }
  .kbody {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
  }
  iframe {
    flex: 1 1 auto;
    width: 100%;
    height: 100%;
    border: 0;
    background: transparent;
    color-scheme: light;
  }
  :global(:root[data-yap-theme='dark']) iframe {
    color-scheme: dark;
  }
  .kgone {
    margin: 20px;
    color: var(--yap-muted);
  }
</style>
