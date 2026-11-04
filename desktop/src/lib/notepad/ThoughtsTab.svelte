<script>
  // My thoughts: the note's own text, synced with the Notes view both ways
  // (`yap-note-changed`, `origin` = the window's label). While the meeting
  // records, its pre-meeting brief (calendar/brief.rs) sits over it in one
  // line that opens it on the Summary tab. A page you can format
  // (components/richtext/RichNote: the toolbar over it, Ctrl+F), the note
  // kept markdown; its plain box until the page has loaded (Notepad.svelte
  // loads it with the meeting).
  import { briefTeaser } from '../core/brief.js';
  import { richtext } from '../components/richtext/richtext.svelte.js';
  import Icon from './NotepadIcon.svelte';

  /** `onsave()`: the text was typed; `onopenbrief()`: the brief, unfolded. */
  let { note = $bindable(), recordingThis, brief, briefLineGone = $bindable(), onsave, onopenbrief } = $props();

  const placeholder = $derived(
    recordingThis ? 'Jot down your own notes while Yap listens. They go into the summary.' : 'Your own notes on this meeting.'
  );

  function onRichChange(md) {
    note.content = md;
    onsave();
  }
</script>

{#if brief && recordingThis && !briefLineGone}
  <!-- The meeting's brief, one line: opens it on the Summary tab. -->
  <div class="briefline">
    <button class="brieflink" onclick={onopenbrief}>
      <Icon name="note" />
      <span class="bw">Brief</span>
      <span class="bt">{briefTeaser(brief)}</span>
    </button>
    <button class="briefx" aria-label="Hide the brief's line" onclick={() => (briefLineGone = true)}><Icon name="x" /></button>
  </div>
{/if}
{#if richtext.Note}
  {@const Rich = richtext.Note}
  <div class="thoughtpage">
    {#key note.id}
      <Rich
        text={note.content}
        onchange={onRichChange}
        label="My thoughts"
        {placeholder}
        rawTitle="Edit this part in Notes, in Source"
        --rich-size="calc(15px * var(--yap-reading-scale))"
        --rich-leading="calc(1.6 * var(--yap-reading-leading))"
        --rich-pad="18px 12px 180px"
      />
    {/key}
  </div>
{:else}
  <textarea class="thoughts" aria-label="My thoughts" data-rich-fallback {placeholder} bind:value={note.content} oninput={onsave}></textarea>
{/if}

<style>
  button {
    font: inherit;
    cursor: pointer;
  }

  /* While a meeting records: its brief in one line over My thoughts. */
  .briefline {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 2px;
    margin: 14px 32px 0;
    border-radius: 10px;
    background: var(--yap-paper-s1);
  }
  .brieflink {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 10px;
    border: none;
    border-radius: 10px;
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    font-size: 13px;
    text-align: left;
    cursor: pointer;
  }
  .brieflink:hover {
    background: var(--yap-paper-s2);
  }
  .brieflink :global(svg) {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
  }
  .brieflink .bw {
    font-weight: 650;
    color: var(--yap-paper-ink);
  }
  .brieflink .bt {
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .briefx {
    flex: 0 0 auto;
    display: inline-flex;
    width: 28px;
    height: 28px;
    margin-right: 4px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--yap-paper-faint);
    cursor: pointer;
  }
  .briefx:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-muted);
  }
  .briefx :global(svg) {
    width: 13px;
    height: 13px;
  }
  /* The page: its toolbar over it, the words below (RichNote). */
  .thoughtpage {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-height: 0;
    margin: 6px 20px 0;
  }
  .thoughts {
    flex: 1 1 auto;
    min-height: 0;
    margin: 0 20px;
    padding: 20px 12px 180px;
    border: none;
    background: transparent;
    color: var(--yap-paper-ink);
    font: inherit;
    /* Reading text: Settings → General → Appearance (app.css). */
    font-family: var(--yap-font-reading);
    font-size: calc(15px * var(--yap-reading-scale));
    line-height: calc(1.6 * var(--yap-reading-leading));
    resize: none;
    outline: none;
    user-select: text;
  }
  .thoughts::placeholder {
    color: var(--yap-paper-faint);
  }
  /* Thin scrollbars, as on the page they sit on (8px, a soft thumb). */
  .thoughts::-webkit-scrollbar {
    width: 8px;
  }
  .thoughts::-webkit-scrollbar-thumb {
    border-radius: 4px;
    background: color-mix(in srgb, var(--yap-paper-ink) 16%, transparent);
  }
</style>
