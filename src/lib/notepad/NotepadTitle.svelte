<script>
  // The meeting's title (EB Garamond; a made-up one waits, muted, for the AI
  // title) and the date in the locale's format. The title grows with what it
  // says (a textarea, like Wispr's), one line per line; Enter finishes it.
  import { onMount, tick } from 'svelte';
  import { dateLine } from '../core/notepadText.js';

  /** `onsave()`: the title was typed (saved once typing stops). */
  let { note = $bindable(), onsave } = $props();

  let titleEl = $state(null);
  function fitTitle() {
    if (!titleEl) return;
    titleEl.style.height = 'auto';
    titleEl.style.height = `${titleEl.scrollHeight}px`;
  }
  $effect(() => {
    void note?.title;
    void titleEl;
    tick().then(fitTitle);
  });
  function onTitleKeydown(e) {
    if (e.key === 'Enter') {
      e.preventDefault();
      e.currentTarget.blur();
    }
  }

  onMount(() => {
    window.addEventListener('resize', fitTitle);
    return () => window.removeEventListener('resize', fitTitle);
  });
</script>

<div class="titleblock">
  <textarea
    class="title"
    class:auto={note.titleAuto}
    rows="1"
    placeholder="New note"
    aria-label="Meeting title"
    bind:this={titleEl}
    bind:value={note.title}
    oninput={onsave}
    onkeydown={onTitleKeydown}
  ></textarea>
  <p class="date">{dateLine(note.createdTs)}</p>
</div>

<style>
  .titleblock {
    flex: 0 0 auto;
    padding: 12px 20px 0;
  }
  .title {
    display: block;
    width: 100%;
    box-sizing: border-box;
    margin: 0;
    padding: 8px 12px;
    border: none;
    background: transparent;
    color: var(--yap-paper-ink);
    font-family: var(--yap-font-display);
    font-size: 36px;
    font-weight: 400;
    line-height: 45px;
    letter-spacing: -0.04em;
    resize: none;
    overflow: hidden;
    outline: none;
  }
  .title::placeholder,
  .title.auto {
    color: var(--yap-paper-faint);
  }
  .title.auto:focus {
    color: var(--yap-paper-ink);
  }
  .date {
    margin: -8px 0 0;
    padding: 0 12px 8px;
    font-size: 12px;
    line-height: 16px;
    color: var(--yap-paper-date);
  }
</style>
