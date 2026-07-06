<script>
  // Placeholder surface for sidebar items whose runtime lands with Phase 6/7
  // (Chat / Notes / Upload) — the shell gives them their home first, matching
  // OpenWhispr's control-panel structure.
  let { kind = 'chat', title = '', body = '', linkText = '', onlink = null } = $props();
</script>

<div class="wrap">
  <div class="card">
    <span class="icon">
      {#if kind === 'chat'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" /></svg>
      {:else if kind === 'notes'}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13.4 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-7.4" /><path d="M18.4 2.6a2 2 0 0 1 2.8 2.8L13 13.6 9 14.6l1-4z" /></svg>
      {:else}
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" /><path d="M17 8l-5-5-5 5" /><path d="M12 3v12" /></svg>
      {/if}
    </span>
    <span class="soon">Coming soon</span>
    <h2>{title}</h2>
    <p>{body}</p>
    {#if linkText && onlink}
      <button class="link" onclick={onlink}>{linkText} →</button>
    {/if}
  </div>
</div>

<style>
  .wrap {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    justify-content: center;
    overflow-y: auto;
    min-height: 0;
    padding: 30px;
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    max-width: 420px;
    text-align: center;
    color: var(--yap-muted);
  }
  .icon {
    display: inline-flex;
    width: 42px;
    height: 42px;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 12px;
    background: var(--yap-s2);
    color: var(--yap-muted);
    margin-bottom: 8px;
  }
  .icon svg {
    width: 20px;
    height: 20px;
  }
  .soon {
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.09em;
    color: var(--yap-primary);
    background: var(--yap-primary-wash);
    padding: 2px 8px;
    border-radius: var(--yap-r-sm);
  }
  h2 {
    margin: 4px 0 0;
    font-size: 16px;
    color: var(--yap-fg);
  }
  p {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.6;
  }
  .link {
    margin-top: 8px;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    padding: 0;
  }
  .link:hover {
    text-decoration: underline;
  }
</style>
