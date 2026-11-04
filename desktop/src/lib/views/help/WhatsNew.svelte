<script>
  // The Help menu's What's new, when Yap has release notes on this PC: the
  // update it found (updates.rs keeps the notes of the version it's offering)
  // in a dialog, rendered as markdown, with the release page and, when it's
  // ready, Restart to update. (Without notes here, the menu opens the
  // release page instead.) Esc, ✕ or the scrim close it.
  import { onMount, tick } from 'svelte';
  import { renderMarkdown } from '../../core/markdown.js';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { updates, releaseUrlFor } from '../../stores/updates.svelte.js';

  let { onclose } = $props();
  let dialog = $state(null);
  let opener = null;

  const html = $derived(renderMarkdown(updates.notes || ''));

  onMount(() => {
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    tick().then(() => dialog?.focus({ preventScroll: true }));
    return () => opener?.focus?.({ preventScroll: true });
  });

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      onclose?.();
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose?.()}>
  <div class="box" bind:this={dialog} role="dialog" aria-modal="true" aria-labelledby="wn-title" tabindex="-1" onkeydown={onKeydown}>
    <header>
      <div>
        <h2 id="wn-title">What's new</h2>
        <p class="ver">Yap {updates.version || updates.currentVersion}{#if updates.version && updates.version !== updates.currentVersion} · you have {updates.currentVersion}{/if}</p>
      </div>
      <button type="button" class="x" aria-label="Close" onclick={() => onclose?.()}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6.5 6.5l11 11M17.5 6.5l-11 11" /></svg>
      </button>
    </header>
    <!-- Sanitised by renderMarkdown (DOMPurify). -->
    <div class="notes md">{@html html}</div>
    <footer>
      <button type="button" class="link" onclick={() => openExternalLink(updates.releaseUrl || releaseUrlFor(updates.version))}>Release page on GitHub</button>
    </footer>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 120;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--yap-scrim);
  }
  .box {
    display: flex;
    flex-direction: column;
    width: min(560px, calc(100vw - 48px));
    max-height: calc(100vh - 64px);
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r-xl);
    background: var(--yap-s1);
    box-shadow: var(--yap-shadow-modal);
    color: var(--yap-fg);
    outline: none;
  }
  header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    padding: 18px 20px 6px 24px;
  }
  h2 {
    margin: 0;
    font-family: var(--yap-font-display);
    font-size: 26px;
    font-weight: 600;
  }
  .ver {
    margin: 2px 0 0;
    font-size: 12.5px;
    color: var(--yap-muted);
  }
  .x {
    display: inline-flex;
    width: 32px;
    height: 32px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--yap-fg-62);
    cursor: pointer;
  }
  .x svg {
    width: 18px;
    height: 18px;
  }
  .notes {
    overflow-y: auto;
    padding: 4px 24px 12px;
    font-size: 13.5px;
    line-height: 1.6;
  }
  footer {
    padding: 10px 24px 16px;
    border-top: 1px solid var(--yap-border-subtle);
  }
  .link {
    border: none;
    background: none;
    padding: 0;
    color: var(--yap-link);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
    text-decoration: underline;
  }
</style>
