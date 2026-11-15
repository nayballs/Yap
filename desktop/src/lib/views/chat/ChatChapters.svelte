<script>
  // **A long chat's contents** (`mark_chapter`, chat/cards/chapters.rs, 15
  // Nov 2026; the Claude app's floating table of contents): a small button at
  // the thread's top right while the chat has chapters, saying the one you're
  // reading; it opens the list — the chat's start, then each chapter's title —
  // and a click scrolls the thread to its divider (ChatThread draws them).
  // Esc or a click elsewhere closes it.
  import { onMount, untrack } from 'svelte';
  import { motionMs } from '../../core/appearance.js';
  import { currentChapter } from './chapters.js';

  /** `chapters`: `[{at, title, summary}]` (chapters.js); `thread`: the scrolling thread element. */
  let { chapters = [], thread = null } = $props();

  let open = $state(false);
  let now = $state(-1);
  let box = $state(null);

  /** The dividers in the thread, in order. */
  const dividers = () => (thread ? [...thread.querySelectorAll('[data-testid="chat-chapter"]')] : []);
  function measure() {
    if (!thread) return;
    const top = thread.getBoundingClientRect().top - thread.scrollTop;
    const tops = dividers().map((d) => d.getBoundingClientRect().top - top);
    now = currentChapter(tops, thread.scrollTop);
  }
  $effect(() => {
    const el = thread;
    void chapters.length;
    if (!el) return;
    untrack(measure);
    const on = () => measure();
    el.addEventListener('scroll', on, { passive: true });
    return () => el.removeEventListener('scroll', on);
  });

  /** Chapter `i` (-1: the chat's start) brought into view. */
  function go(i) {
    open = false;
    if (!thread) return;
    const behavior = motionMs(200) ? 'smooth' : 'auto';
    if (i < 0) {
      thread.scrollTo({ top: 0, behavior });
      return;
    }
    dividers()[i]?.scrollIntoView({ block: 'start', behavior });
  }

  onMount(() => {
    const away = (e) => {
      if (open && box && !box.contains(e.target)) open = false;
    };
    const key = (e) => {
      if (open && e.key === 'Escape') {
        e.stopPropagation();
        open = false;
      }
    };
    document.addEventListener('pointerdown', away, true);
    document.addEventListener('keydown', key, true);
    return () => {
      document.removeEventListener('pointerdown', away, true);
      document.removeEventListener('keydown', key, true);
    };
  });

  const label = $derived(now >= 0 ? chapters[now]?.title : 'Start');
</script>

{#if chapters.length}
  <div class="chapters" bind:this={box} data-testid="chat-chapters">
    <button type="button" class="ctoggle" aria-expanded={open} aria-haspopup="true" title="Chapters" onclick={() => (open = !open)}>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6h11M9 12h11M9 18h11" /><path d="M4.5 6h.01M4.5 12h.01M4.5 18h.01" /></svg>
      <span class="clabel">{label}</span>
    </button>
    {#if open}
      <div class="clist" role="menu" aria-label="Chapters">
        <button type="button" role="menuitem" class="citem" class:on={now < 0} onclick={() => go(-1)}>Start</button>
        {#each chapters as c, i (c.at)}
          <button type="button" role="menuitem" class="citem" class:on={now === i} title={c.summary || undefined} onclick={() => go(i)} data-testid="chat-chapters-item">{c.title}</button>
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .chapters {
    position: absolute;
    top: 8px;
    right: calc(14px + var(--thread-gutter, 0px));
    z-index: 3;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    max-width: min(280px, 60%);
  }
  .ctoggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    max-width: 100%;
    height: 26px;
    padding: 0 10px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 999px;
    background: var(--yap-s1);
    color: var(--yap-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    box-shadow: 0 1px 3px rgb(0 0 0 / 8%);
  }
  .ctoggle:hover,
  .ctoggle[aria-expanded='true'] {
    color: var(--yap-fg);
    background: var(--yap-s2);
  }
  .ctoggle:focus-visible,
  .citem:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .ctoggle svg {
    flex: none;
    width: 14px;
    height: 14px;
  }
  .clabel {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .clist {
    display: flex;
    flex-direction: column;
    gap: 1px;
    width: max-content;
    max-width: 280px;
    max-height: 320px;
    margin-top: 4px;
    padding: 4px;
    overflow: auto;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 10px;
    background: var(--yap-s1);
    box-shadow: 0 6px 18px rgb(0 0 0 / 14%);
  }
  .citem {
    padding: 5px 10px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
    cursor: pointer;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .citem:hover {
    background: var(--yap-s2);
  }
  .citem.on {
    color: var(--yap-primary);
    font-weight: 600;
  }
</style>
