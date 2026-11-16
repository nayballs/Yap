<script>
  // **A new document's outline, before the writing starts** (chat/cards/
  // outline.rs, 15 Nov 2026; the Claude app's plan review): the title, then
  // each section's heading and the one line of what it will say, with **Go**
  // and **Edit**. The turn waits; nothing is written until the person
  // answers. Edit: each heading and its line in a box, ↑ ↓ to reorder, ✕ to
  // take one out, "Add a section", and a note for the writing — then **Go
  // with these changes** (`chat_outline_answer` with `{sections, note}`,
  // outlineEdit.js `answerOf`). Asked out loud (`card.outline.voice`), the
  // card says it can be answered by voice too ("go ahead", "drop the second
  // part"). Once answered it stays in the thread, quiet: the headings as
  // they were written, what changed and the note. Kept with the answer
  // (ChatMessage.confirms), so it comes back with the chat.
  import { tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { draftOf, blankRow, moved, sectionsOf, answerOf, endingOf, MAX_SECTIONS } from './outlineEdit.js';

  /** `card`: the turn's outline card (`confirm` event, act "outline": `outline: {title, summary, sections, changed, note, said, voice}`). */
  let { card } = $props();

  const o = $derived(card.outline || { title: card.label || '', sections: [] });
  // A research's plan (chat/research.rs), not a document's outline: its own words.
  const research = $derived(!!o.research);
  // Answered here, before Rust's next event says so.
  let sent = $state('');
  const phase = $derived(card.state === 'asking' && sent ? sent : card.state);
  const asking = $derived(phase === 'asking' || phase === 'sending');

  // Edit: the rows being changed, and the note.
  let editing = $state(false);
  let rows = $state([]);
  let note = $state('');
  const left = $derived(sectionsOf(rows));

  function startEdit() {
    rows = draftOf(o.sections);
    note = '';
    editing = true;
  }
  async function answer(outline) {
    if (sent) return;
    sent = 'sending';
    try {
      await invoke('chat_outline_answer', { id: card.id, outline });
    } catch {
      /* the outline had lapsed (the turn stopped): its card says so */
    }
  }
  function goEdited() {
    const a = answerOf(rows, o.sections, note);
    if (a === undefined) return;
    answer(a);
  }
  const setRow = (i, patch) => (rows = rows.map((r, k) => (k === i ? { ...r, ...patch } : r)));
  const removeRow = (i) => (rows = rows.filter((_, k) => k !== i));
  async function addRow() {
    if (rows.length >= MAX_SECTIONS) return;
    const row = blankRow();
    rows = [...rows, row];
    // The new heading's box, ready to type in.
    await tick();
    document.querySelector(`[data-outline-row="${row.id}"] input`)?.focus();
  }

  /** The headings the document was written with: the person's, else the model's. */
  const written = $derived(o.changed?.length ? o.changed : o.sections);
  const ending = $derived(endingOf(phase, o));
</script>

{#if asking}
  <div class="coutline asking" role="group" aria-label={research ? 'The research plan, before any search' : 'The outline, before the writing'} data-testid="chat-outline-card" data-research={research || null} data-state={phase}>
    {#if research}
      <svg class="oicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="10.5" cy="10.5" r="6" /><path d="m15 15 5 5" /><path d="M8 10.5h5M10.5 8v5" /></svg>
    {:else}
      <svg class="oicon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M8 6h12M8 12h12M8 18h8" /><path d="M4 6h.01M4 12h.01M4 18h.01" /></svg>
    {/if}
    <div class="obody">
      <p class="otitle"><strong>{research ? 'Research plan' : 'The outline'}</strong> · <span class="oname">{o.title}</span>{#if o.voice}<span class="osay"> · Say go ahead, or what to change</span>{/if}</p>
      {#if !editing}
        {#if o.summary}<p class="osum">{o.summary}</p>{/if}
        <ol class="olist" data-testid="chat-outline-sections">
          {#each o.sections as s, i (i)}
            <li>
              <span class="okey">{s.key}</span>
              {#if s.intent}<span class="ointent">{s.intent}</span>{/if}
            </li>
          {/each}
        </ol>
        <p class="owhy">{research ? 'Nothing is looked up until you say. Edit to change, reorder or take out a part, or add a note for the research.' : 'Nothing is written until you say. Edit to change, reorder or take out a heading, or add a note.'}</p>
        <div class="obtns">
          <button type="button" class="ogo" onclick={() => answer(null)} disabled={phase === 'sending'}>{research ? 'Start research' : 'Go'}</button>
          <button type="button" onclick={startEdit} disabled={phase === 'sending'}>Edit</button>
        </div>
      {:else}
        <ol class="oedit" data-testid="chat-outline-edit">
          {#each rows as r, i (r.id)}
            <li data-outline-row={r.id}>
              <span class="onum" aria-hidden="true">{i + 1}</span>
              <div class="ofields">
                <input class="okeyin" value={r.key} oninput={(e) => setRow(i, { key: e.currentTarget.value })} placeholder="Heading" aria-label={`Section ${i + 1}: its heading`} maxlength="60" />
                <input class="ointentin" value={r.intent} oninput={(e) => setRow(i, { intent: e.currentTarget.value })} placeholder="What it covers (optional)" aria-label={`Section ${i + 1}: what it covers`} maxlength="140" />
              </div>
              <div class="omove">
                <button type="button" class="oicon-btn" onclick={() => (rows = moved(rows, i, -1))} disabled={i === 0} aria-label={`Move section ${i + 1} up`} title="Move up">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 15 6-6 6 6" /></svg>
                </button>
                <button type="button" class="oicon-btn" onclick={() => (rows = moved(rows, i, 1))} disabled={i === rows.length - 1} aria-label={`Move section ${i + 1} down`} title="Move down">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6" /></svg>
                </button>
                <button type="button" class="oicon-btn" onclick={() => removeRow(i)} disabled={rows.length === 1} aria-label={`Take out section ${i + 1}`} title="Take it out">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12" /></svg>
                </button>
              </div>
            </li>
          {/each}
        </ol>
        {#if rows.length < MAX_SECTIONS}
          <button type="button" class="oadd" onclick={addRow}>+ Add a section</button>
        {/if}
        <label class="onote">
          <span>{research ? 'A note for the research' : 'A note for the writing'} <span class="oopt">(optional)</span></span>
          <textarea bind:value={note} rows="2" maxlength="2000" placeholder={research ? 'Focus on the 1870s, leave out modern keyboards…' : 'Keep it short, for a beginner…'} data-testid="chat-outline-note"></textarea>
        </label>
        <div class="obtns">
          <button type="button" class="ogo" onclick={goEdited} disabled={phase === 'sending' || !left.length}>{research ? 'Start with these changes' : 'Go with these changes'}</button>
          <button type="button" onclick={() => (editing = false)} disabled={phase === 'sending'}>Cancel</button>
        </div>
      {/if}
    </div>
  </div>
{:else}
  <!-- Answered: the headings it was written with, what changed and the note, quiet (as a question card keeps its answer). -->
  <div class="coutline done" role="group" aria-label={research ? 'The research plan' : 'The outline'} data-testid="chat-outline-card" data-research={research || null} data-state={phase}>
    <p class="dtitle">{research ? 'Research plan' : 'Outline'} · {o.title}</p>
    <p class="dkeys" data-testid="chat-outline-written">{written.map((s) => s.key).join(' · ')}</p>
    {#if o.note}<p class="dline" data-testid="chat-outline-note-kept">Your note: “{o.note}”</p>{/if}
    {#if o.said}<p class="dline">You said: “{o.said}”</p>{/if}
    {#if ending}<p class="dline">{ending}</p>{/if}
  </div>
{/if}

<style>
  .coutline {
    box-sizing: border-box;
    color: var(--yap-paper-ink);
    font-size: 13px;
  }
  .coutline.asking {
    display: flex;
    gap: 10px;
    width: min(600px, 100%);
    margin: 6px 0 2px;
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--yap-primary) 40%, var(--yap-paper-rule));
    border-radius: 10px;
    background: var(--yap-paper);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--yap-primary) 12%, transparent);
  }
  .oicon {
    flex: 0 0 auto;
    width: 18px;
    height: 18px;
    margin-top: 1px;
    color: var(--yap-primary);
  }
  .obody {
    flex: 1 1 auto;
    min-width: 0;
  }
  .otitle,
  .osum,
  .owhy {
    margin: 0;
  }
  .otitle strong {
    font-weight: 650;
  }
  .oname {
    overflow-wrap: anywhere;
  }
  .osay {
    color: var(--yap-paper-muted);
    font-size: 12px;
  }
  .osum {
    margin-top: 4px;
    color: var(--yap-paper-text-2, var(--yap-paper-muted));
    font-size: 12.5px;
    overflow-wrap: anywhere;
  }
  .olist {
    margin: 8px 0 0;
    padding: 8px 12px 8px 30px;
    border-radius: 8px;
    background: var(--yap-paper-s1);
  }
  .olist li {
    padding: 2px 0;
  }
  .olist li::marker {
    color: var(--yap-paper-muted);
    font-size: 12px;
  }
  .okey {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .ointent {
    display: block;
    color: var(--yap-paper-muted);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
  .owhy {
    margin-top: 6px;
    font-size: 12px;
    color: var(--yap-paper-muted);
  }
  .oedit {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 8px 0 0;
    padding: 0;
    list-style: none;
  }
  .oedit li {
    display: flex;
    gap: 8px;
    align-items: flex-start;
  }
  .onum {
    flex: none;
    width: 16px;
    padding-top: 6px;
    color: var(--yap-paper-muted);
    font-size: 12px;
    text-align: right;
  }
  .ofields {
    display: flex;
    flex: 1 1 auto;
    flex-direction: column;
    gap: 3px;
    min-width: 0;
  }
  .ofields input,
  .onote textarea {
    box-sizing: border-box;
    width: 100%;
    padding: 5px 8px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 7px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
  }
  .okeyin {
    font-weight: 600;
  }
  .ointentin {
    color: var(--yap-paper-text-2, var(--yap-paper-ink));
  }
  .ofields input:focus-visible,
  .onote textarea:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 0;
    border-color: transparent;
  }
  .omove {
    display: flex;
    flex: none;
    gap: 2px;
    padding-top: 2px;
  }
  .oicon-btn {
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
  }
  .oicon-btn svg {
    width: 14px;
    height: 14px;
  }
  .oicon-btn:hover:not(:disabled) {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .oicon-btn:disabled {
    cursor: default;
    opacity: 0.35;
  }
  .oicon-btn:focus-visible,
  .oadd:focus-visible,
  .obtns button:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .oadd {
    margin: 6px 0 0 24px;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-primary);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .oadd:hover {
    text-decoration: underline;
  }
  .onote {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-top: 10px;
    font-size: 12px;
    color: var(--yap-paper-muted);
  }
  .oopt {
    font-weight: 400;
  }
  .onote textarea {
    resize: vertical;
    min-height: 40px;
  }
  .obtns {
    display: flex;
    gap: 8px;
    margin-top: 9px;
  }
  .obtns button {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 999px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .obtns .ogo {
    border-color: var(--yap-primary-line);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
  }
  .obtns button:hover:not(:disabled) {
    background: var(--yap-paper-s2);
  }
  .obtns .ogo:hover:not(:disabled) {
    background: var(--yap-primary-hover);
  }
  .obtns button:disabled {
    cursor: default;
    opacity: 0.55;
  }
  /* Answered: quiet, as a question card keeps its answer. */
  .coutline.done {
    width: fit-content;
    max-width: min(520px, 100%);
    margin: 6px 0 2px;
    padding: 9px 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 10px;
    background: var(--yap-paper-s1);
  }
  .dtitle {
    margin: 0;
    font-weight: 650;
    overflow-wrap: anywhere;
  }
  .dkeys {
    margin: 2px 0 0;
    color: var(--yap-paper-text-2, var(--yap-paper-ink));
    overflow-wrap: anywhere;
  }
  .dline {
    margin: 6px 0 0;
    color: var(--yap-paper-muted);
    font-size: 12px;
    overflow-wrap: anywhere;
  }
</style>
