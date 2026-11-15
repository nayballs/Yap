<script>
  // **A long chat's contents** (`mark_chapter`, chat/cards/chapters.rs, 15
  // Nov 2026; the Claude app's floating table of contents): while the chat has
  // chapters, a slim rail at the thread's right edge — a tick for its start and
  // one for each chapter, the one you're reading marked — that never covers a
  // message. Pointing at it (or a click, or Enter) opens the list beside it:
  // Start, then each chapter's title; a click scrolls the thread to its
  // divider (ChatThread draws them). Esc, or a click elsewhere, closes it.
  import { onMount, untrack } from 'svelte';
  import { motionMs } from '../../core/appearance.js';
  import { currentChapter } from './chapters.js';

  /** `chapters`: `[{at, title, summary}]` (chapters.js); `thread`: the scrolling thread element. */
  let { chapters = [], thread = null } = $props();

  // Opened by a click (stays), or pointed at (while the pointer's there).
  let open = $state(false);
  let hover = $state(false);
  const shown = $derived(open || hover);
  let now = $state(-1);
  let box = $state(null);

  /** The dividers in the thread, in order. */
  const dividers = () => (thread ? [...thread.querySelectorAll('[data-testid="chat-chapter"]')] : []);
  function measure() {
    if (!thread) return;
    const top = thread.getBoundingClientRect().top - thread.scrollTop;
    const tops = dividers().map((d) => d.getBoundingClientRect().top - top);
    const atEnd = thread.scrollTop + thread.clientHeight >= thread.scrollHeight - 4;
    now = currentChapter(tops, thread.scrollTop, 48, thread.clientHeight, atEnd);
  }
  $effect(() => {
    const el = thread;
    void chapters.length;
    if (!el) return;
    untrack(measure);
    const on = () => measure();
    el.addEventListener('scroll', on, { passive: true });
    // (The thread grows as an answer comes: measured again then too.)
    const grew = new ResizeObserver(on);
    if (el.firstElementChild) grew.observe(el.firstElementChild);
    return () => {
      el.removeEventListener('scroll', on);
      grew.disconnect();
    };
  });

  /** Chapter `i` (-1: the chat's start) brought into view. */
  function go(i) {
    open = false;
    hover = false;
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
      if (shown && e.key === 'Escape') {
        e.stopPropagation();
        open = false;
        hover = false;
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
  <!-- svelte-ignore a11y_no_static_element_interactions (pointing only shows what the button opens) -->
  <div class="chapters" class:shown bind:this={box} data-testid="chat-chapters" onmouseenter={() => (hover = true)} onmouseleave={() => (hover = false)}>
    {#if shown}
      <div class="clist" role="menu" aria-label="Chapters">
        <button type="button" role="menuitem" class="citem" class:on={now < 0} onclick={() => go(-1)}>Start</button>
        {#each chapters as c, i (c.at)}
          <button type="button" role="menuitem" class="citem" class:on={now === i} title={c.summary || undefined} onclick={() => go(i)} data-testid="chat-chapters-item">{c.title}</button>
        {/each}
      </div>
    {/if}
    <button type="button" class="crail" aria-expanded={shown} aria-haspopup="true" aria-label={`Chapters: ${label}`} title={`Chapters: ${label}`} onclick={() => (open = !open)}>
      <span class="tick" class:on={now < 0}></span>
      {#each chapters as c, i (c.at)}
        <span class="tick" class:on={now === i}></span>
      {/each}
    </button>
  </div>
{/if}

<style>
  /* At the thread's right edge, left of its scrollbar, in the column's margin. */
  .chapters {
    position: absolute;
    top: 50%;
    right: calc(3px + var(--thread-gutter, 0px));
    z-index: 3;
    display: flex;
    align-items: center;
    gap: 6px;
    transform: translateY(-50%);
  }
  .crail {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 7px;
    max-height: 60vh;
    padding: 8px 4px;
    overflow: hidden;
    border: none;
    border-radius: 8px;
    background: none;
    cursor: pointer;
  }
  .chapters.shown .crail,
  .crail:hover {
    background: var(--yap-s2);
  }
  .crail:focus-visible,
  .citem:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .tick {
    flex: none;
    display: block;
    width: 10px;
    height: 2px;
    border-radius: 1px;
    background: var(--yap-border-subtle);
  }
  .crail:hover .tick,
  .chapters.shown .tick {
    background: var(--yap-muted);
  }
  .tick.on,
  .crail:hover .tick.on,
  .chapters.shown .tick.on {
    width: 14px;
    background: var(--yap-primary);
  }
  .clist {
    display: flex;
    flex-direction: column;
    gap: 1px;
    width: max-content;
    max-width: 280px;
    max-height: 60vh;
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
