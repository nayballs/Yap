<script>
  // 👍 / 👎 under a meeting's summary (the notepad's Summary tab and the Notes
  // view's), Wispr Flow's "Was this summary useful?": kept in the note on
  // this PC only, never sent (notes/note_edits.rs `note_summary_rate`) — for
  // the model evaluations later. A 👎 offers "What was wrong?", a short note
  // the person may skip. Pressing the same thumb again takes it back. A new
  // summary from Yap starts unrated.
  import { tick } from 'svelte';
  import { rateSummary, applySummaryState } from '../core/summaryTools.js';
  import { toast } from '../ui/toast.svelte.js';

  /**
   * `note`: the note shown (its `summaryRating` changes in place); `origin`:
   * this window's label; `look`: 'paper' (the notepad) | 'app' (Notes);
   * `changing()`: marks the summary changed for the window's reloads.
   */
  let { note, origin, look = 'paper', changing = () => {} } = $props();

  const rating = $derived(note?.summaryRating ?? null);
  let asking = $state(false);
  let why = $state('');
  let whyEl = $state(null);
  let busy = $state(false);

  async function rate(next, reason = '') {
    if (!note || busy) return;
    const id = note.id;
    busy = true;
    try {
      const s = await rateSummary(id, next, reason, origin);
      if (note?.id === id) {
        changing();
        applySummaryState(note, s);
      }
      return true;
    } catch (e) {
      toast({ title: "Couldn't keep your rating", description: String(e), variant: 'destructive' });
      return false;
    } finally {
      busy = false;
    }
  }

  async function up() {
    asking = false;
    await rate(rating?.up ? null : 'up');
  }
  async function down() {
    if (rating && !rating.up) {
      asking = false;
      await rate(null);
      return;
    }
    if (await rate('down')) {
      why = '';
      asking = true;
      await tick();
      whyEl?.focus();
    }
  }
  async function saveWhy(e) {
    e?.preventDefault();
    if (why.trim()) await rate('down', why.trim());
    asking = false;
  }
  function onWhyKeydown(e) {
    if (e.key === 'Enter' && !e.shiftKey) saveWhy(e);
    else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      asking = false;
    }
  }
</script>

<div class="rate {look}">
  <div class="row" role="group" aria-label="Rate this summary" title="Kept on this PC only: Yap never sends your rating">
    <span class="q">
      {#if rating?.up}Thanks: glad it helped.{:else if rating}Thanks for telling Yap.{:else}Was this summary useful?{/if}
    </span>
    <button class="thumb" class:on={rating?.up} aria-label="Good summary" aria-pressed={!!rating?.up} title="Good summary" disabled={busy} onclick={up}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7.5 10.5v9H5a1 1 0 0 1-1-1v-7a1 1 0 0 1 1-1z" /><path d="M7.5 10.5l3.6-6.3a1.6 1.6 0 0 1 2.9 1.2l-.8 3.6h5.1a2 2 0 0 1 2 2.3l-1.1 6.4a2 2 0 0 1-2 1.7H7.5" /></svg>
    </button>
    <button class="thumb" class:on={rating && !rating.up} aria-label="Bad summary" aria-pressed={!!rating && !rating.up} title="Bad summary" disabled={busy} onclick={down}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M7.5 13.5v-9H5a1 1 0 0 0-1 1v7a1 1 0 0 0 1 1z" /><path d="M7.5 13.5l3.6 6.3a1.6 1.6 0 0 0 2.9-1.2l-.8-3.6h5.1a2 2 0 0 0 2-2.3l-1.1-6.4a2 2 0 0 0-2-1.7H7.5" /></svg>
    </button>
    {#if rating && !rating.up && rating.why && !asking}
      <span class="why-said" title={rating.why}>“{rating.why}”</span>
    {/if}
  </div>
  {#if asking}
    <form class="why" onsubmit={saveWhy}>
      <label for="summary-why">What was wrong? <span class="opt">(optional)</span></label>
      <textarea
        id="summary-why"
        bind:this={whyEl}
        bind:value={why}
        rows="2"
        maxlength="500"
        placeholder="A task missing, the wrong owner, too long…"
        onkeydown={onWhyKeydown}
      ></textarea>
      <div class="whybtns">
        <button type="submit" class="save" disabled={busy || !why.trim()}>Save</button>
        <button type="button" class="skip" onclick={() => (asking = false)}>Skip</button>
      </div>
      <p class="private">It stays on this PC with your rating: Yap never sends either.</p>
    </form>
  {/if}
</div>

<style>
  .rate {
    margin: 22px 0 0;
    padding-top: 12px;
  }
  .rate.paper {
    margin-inline: 12px;
    border-top: 1px solid var(--yap-paper-s2);
    color: var(--yap-paper-muted);
  }
  .rate.app {
    border-top: 1px solid var(--yap-border-subtle);
    color: var(--yap-muted);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }
  .q {
    margin-right: 6px;
    font-size: 12.5px;
  }
  .thumb {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: 8px;
    background: none;
    color: inherit;
    cursor: pointer;
  }
  .thumb svg {
    width: 16px;
    height: 16px;
  }
  .paper .thumb:hover:not(:disabled),
  .paper .thumb.on {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .app .thumb:hover:not(:disabled),
  .app .thumb.on {
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .thumb:focus-visible,
  .why button:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .why-said {
    min-width: 0;
    margin-left: 4px;
    overflow: hidden;
    font-size: 12px;
    font-style: italic;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .why {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: 8px;
  }
  .why label {
    font-size: 12.5px;
    font-weight: 600;
  }
  .paper .why label {
    color: var(--yap-paper-ink);
  }
  .app .why label {
    color: var(--yap-fg);
  }
  .opt {
    font-weight: 400;
    opacity: 0.75;
  }
  .why textarea {
    box-sizing: border-box;
    width: 100%;
    padding: 6px 8px;
    border-radius: 8px;
    font: inherit;
    font-size: 13px;
    resize: vertical;
  }
  .paper .why textarea {
    border: 1px solid var(--yap-paper-rule);
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
  }
  .app .why textarea {
    border: 1px solid var(--yap-border);
    background: var(--yap-s2);
    color: var(--yap-fg);
  }
  .why textarea:focus {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 0;
  }
  .whybtns {
    display: flex;
    gap: 6px;
  }
  .why button {
    height: 26px;
    padding: 0 12px;
    border-radius: 9999px;
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .paper .save {
    border: none;
    background: var(--yap-paper-ink);
    color: var(--yap-paper);
  }
  .app .save {
    border: none;
    background: var(--yap-ink, var(--yap-primary));
    color: var(--yap-ink-fg, var(--yap-primary-fg));
  }
  .save:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .skip {
    border: 1px solid currentColor;
    background: none;
    color: inherit;
  }
  .private {
    margin: 4px 0 0;
    font-size: 11px;
    opacity: 0.8;
  }
</style>
