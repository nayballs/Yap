<script>
  // "Import a transcript": meetings another app transcribed — Teams, Zoom,
  // Google Meet, Otter, Granola — as Yap meeting notes
  // (transcriptImport.svelte.js; Rust: meetings/import.rs). The main window
  // mounts it once, while it's open; Upload and the Meetings view's ⋯ open
  // it, and transcript files dropped on Upload arrive in it.
  //   empty    a drop zone: drop files here, or Choose files… (several at once);
  //   preview  a card per file, before anything is made: its format, the
  //            speakers found, lines and length; when it was (from the file,
  //            or the person's pick); which speaker is you; a title of their
  //            own (else the file's, else Yap's AI title); a file it can't
  //            read says why, and stays out of the import;
  //   import   "Import 2 meetings": a meeting note each, then their titles
  //            and summaries, one after another.
  // The look is Wispr's dialog, as "Connect your calendar" has it: a paper
  // card over a scrim. Esc, ✕ and the scrim close it (not mid-import); the
  // focus stays inside while it's open and goes back where it was.
  import { tick, untrack, onMount } from 'svelte';
  import { getCurrentWebview } from '@tauri-apps/api/webview';
  import {
    transcriptImport,
    closeTranscriptImport,
    chooseFiles,
    addFiles,
    removeItem,
    readyItems,
    runImport,
    importOpener,
    lengthText,
    whenFrom,
    retentionWarning,
    TRANSCRIPT_EXTENSIONS,
  } from '../stores/transcriptImport.svelte.js';

  let dialog = $state(null);
  let over = $state(false);
  let downOnScrim = false;

  const items = $derived(transcriptImport.items);
  const ready = $derived(readyItems());
  const busy = $derived(transcriptImport.importing);
  const importLabel = $derived(
    busy ? 'Importing…' : ready.length === 1 ? 'Import 1 meeting' : `Import ${ready.length} meetings`
  );

  function speakersLine(item) {
    const people = item.speakers.length;
    const parts = [
      people === 1 ? '1 speaker' : `${people} speakers`,
      item.lines === 1 ? '1 line' : `${item.lines} lines`,
      lengthText(item.lengthMs, item.estimated),
    ];
    return parts.join(' · ');
  }

  /** The speakers the person can say is them. */
  const people = (item) => item.speakers.filter((s) => s.person);
  const fileSaysYou = (item) => item.speakers.some((s) => s.you);

  // ---- open, close, focus ----

  onMount(() => {
    tick().then(focusFirst);
    // Files dropped on the dialog join the list.
    const drop = getCurrentWebview().onDragDropEvent((event) => {
      const t = event.payload.type;
      if (t === 'over' || t === 'enter') over = true;
      else if (t === 'leave') over = false;
      else if (t === 'drop') {
        over = false;
        const paths = event.payload.paths || [];
        if (paths.length) addFiles(paths);
      }
    });
    return () => {
      drop.then((u) => u && u()).catch(() => {});
      const back = importOpener() ?? document.querySelector('[data-dialog-return]');
      back?.focus?.({ preventScroll: true });
    };
  });

  // New cards: the list scrolls so the newest shows.
  let seen = 0;
  $effect(() => {
    const n = items.length;
    untrack(() => {
      if (n > seen) {
        tick().then(() => dialog?.querySelector('.cards .card:last-child')?.scrollIntoView?.({ block: 'nearest' }));
      }
      seen = n;
    });
  });

  function close() {
    closeTranscriptImport();
  }

  function focusables() {
    if (!dialog) return [];
    return [...dialog.querySelectorAll('button, input, select, a[href], [tabindex]:not([tabindex="-1"])')].filter(
      (el) => !el.disabled && el.getClientRects().length > 0
    );
  }

  function focusFirst() {
    const target = dialog?.querySelector('[data-first]:not(:disabled)') ?? focusables()[0] ?? dialog;
    target?.focus({ preventScroll: true });
  }

  // Esc closes; Tab goes round inside. On the window, capturing, so Esc is
  // the dialog's before the view's.
  function onKeydown(e) {
    if (!dialog) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if (e.key !== 'Tab') return;
    const list = focusables();
    if (list.length === 0) {
      e.preventDefault();
      dialog.focus();
      return;
    }
    const first = list[0];
    const last = list[list.length - 1];
    const active = document.activeElement;
    const inside = dialog.contains(active) && active !== dialog;
    if (e.shiftKey && (!inside || active === first)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && (!inside || active === last)) {
      e.preventDefault();
      first.focus();
    }
  }

  $effect(() => {
    window.addEventListener('keydown', onKeydown, true);
    return () => window.removeEventListener('keydown', onKeydown, true);
  });

  function onScrimDown(e) {
    downOnScrim = e.target === e.currentTarget;
  }
  function onScrimClick(e) {
    if (downOnScrim && e.target === e.currentTarget) close();
    downOnScrim = false;
  }
</script>

<div class="scrim" role="presentation" onpointerdown={onScrimDown} onclick={onScrimClick}>
  <div
    class="dialog"
    bind:this={dialog}
    role="dialog"
    aria-modal="true"
    aria-labelledby="ti-title"
    aria-describedby="ti-lead"
    tabindex="-1"
  >
    <header class="head">
      <h2 id="ti-title">Import a transcript</h2>
      <button class="x" type="button" aria-label="Close" onclick={close} disabled={busy}>
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M6.5 6.5l11 11M17.5 6.5l-11 11" /></svg>
      </button>
    </header>
    <p class="lead" id="ti-lead">
      From Teams, Zoom, Google Meet, Otter or Granola. Each file becomes a meeting note with its speakers, then Yap writes its summary.
    </p>

    <div class="body">
      {#if items.length === 0}
        <button class="drop" class:over type="button" data-first onclick={chooseFiles} disabled={transcriptImport.adding}>
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6M9 13h6M9 17h4" /></svg>
          <span class="d1">{transcriptImport.adding ? 'Reading…' : 'Drop transcript files here'}</span>
          <span class="d2">or click to choose them · {TRANSCRIPT_EXTENSIONS.filter((e) => e !== 'markdown').map((e) => `.${e}`).join(', ')}</span>
        </button>
        <ul class="kinds">
          <li><b>Teams</b>: the transcript's Download, as .vtt or .docx</li>
          <li><b>Zoom</b>: the recording's transcript (.vtt) or a saved transcript (.txt)</li>
          <li><b>Google Meet</b>: the transcript document, downloaded as .docx or .txt</li>
          <li><b>Otter</b>: Export as .txt, .docx or .srt</li>
          <li><b>Granola</b>: the notes copied or exported as Markdown, with the transcript</li>
        </ul>
      {:else}
        <ul class="cards" class:over aria-label="Transcripts to import">
          {#each items as item (item.id)}
            <li class="card" class:bad={!!item.error}>
              <div class="chead">
                <span class="ficon" aria-hidden="true">
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" /><path d="M14 2v6h6M9 13h6M9 17h4" /></svg>
                </span>
                <span class="fmeta">
                  <span class="fname">{item.name}</span>
                  {#if item.formatLabel}<span class="fkind">{item.formatLabel}</span>{/if}
                </span>
                <button class="rm" type="button" aria-label={`Remove ${item.name}`} title="Remove" onclick={() => removeItem(item.id)} disabled={busy}>
                  <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
                </button>
              </div>
              {#if item.error}
                <p class="err" role="alert">Couldn't read it. {item.error}</p>
              {:else}
                <p class="facts">{speakersLine(item)}</p>
                {#if item.speakers.length}
                  <ul class="spk" aria-label={`Speakers in ${item.name}`}>
                    {#each item.speakers.slice(0, 8) as s (s.name)}
                      <li class:you={s.you || (s.person && s.name === item.you)}>
                        {s.you || (s.person && s.name === item.you) ? `${s.name} (you)` : s.name}<span class="n">{s.lines}</span>
                      </li>
                    {/each}
                    {#if item.speakers.length > 8}<li class="more">+{item.speakers.length - 8} more</li>{/if}
                  </ul>
                {/if}
                <div class="fields">
                  <label class="field">
                    <span class="flabel">Title</span>
                    <input
                      type="text"
                      bind:value={item.ownTitle}
                      placeholder={item.title || 'Yap names it from what was said'}
                      aria-label={`Title for ${item.name}`}
                      disabled={busy}
                    />
                  </label>
                  <label class="field">
                    <span class="flabel">When</span>
                    <input type="datetime-local" bind:value={item.when} aria-label={`When ${item.name} was`} disabled={busy} />
                    <span class="hint">{whenFrom(item)}</span>
                  </label>
                  <label class="field">
                    <span class="flabel">You</span>
                    {#if fileSaysYou(item)}
                      <span class="fixed">The file's “Me”</span>
                    {:else}
                      <select bind:value={item.you} aria-label={`Which speaker in ${item.name} is you`} disabled={busy || !people(item).length}>
                        <option value="">Not one of the speakers</option>
                        {#each people(item) as s (s.name)}
                          <option value={s.name}>{s.name}</option>
                        {/each}
                      </select>
                    {/if}
                  </label>
                </div>
                {#if item.notes}<p class="note">Its notes go in My thoughts.</p>{/if}
                {#if retentionWarning(item)}<p class="warn">{retentionWarning(item)}</p>{/if}
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
      {#if transcriptImport.error}<p class="err" role="alert">{transcriptImport.error}</p>{/if}
    </div>

    <footer class="foot">
      {#if items.length}
        <button class="ghost" type="button" onclick={chooseFiles} disabled={busy || transcriptImport.adding}>
          {transcriptImport.adding ? 'Reading…' : 'Add files…'}
        </button>
      {/if}
      <span class="grow"></span>
      <button class="ghost" type="button" onclick={close} disabled={busy}>Cancel</button>
      <button class="go" type="button" data-first onclick={runImport} disabled={busy || ready.length === 0}>{importLabel}</button>
    </footer>
    <p class="sr" aria-live="polite">{busy ? 'Importing…' : ''}</p>
  </div>
</div>

<style>
  /* Wispr's dialog, as ConnectCalendarDialog.svelte has it: a scrim, a
     paper card with a hairline border and the dialog shadow, scaling in. */
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 70;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: clamp(16px, calc(50vh - 320px), 140px) 16px 16px;
    box-sizing: border-box;
    background: var(--yap-paper-scrim);
    animation: scrim-in 0.15s ease-out;
  }
  @keyframes scrim-in {
    from {
      opacity: 0;
    }
  }
  .dialog {
    position: relative;
    display: flex;
    flex-direction: column;
    width: 580px;
    max-width: 100%;
    max-height: 100%;
    box-sizing: border-box;
    overflow: hidden;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 20px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    box-shadow: var(--yap-shadow-dialog);
    outline: none;
    animation: dialog-in 0.15s var(--yap-ease-spring) both;
  }
  @keyframes dialog-in {
    from {
      opacity: 0;
      transform: scale(0.9);
    }
  }
  .head {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 22px 16px 0 24px;
  }
  h2 {
    flex: 1 1 auto;
    margin: 0;
    font-size: 22px;
    font-weight: 600;
    line-height: 28px;
    letter-spacing: -0.01em;
  }
  .x {
    display: flex;
    flex: 0 0 auto;
    width: 28px;
    height: 28px;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: color-mix(in srgb, var(--yap-paper-ink) 6%, transparent);
    color: var(--yap-paper-ink);
    cursor: pointer;
  }
  .x:hover:not(:disabled) {
    background: color-mix(in srgb, var(--yap-paper-ink) 12%, transparent);
  }
  .x svg {
    width: 13px;
    height: 13px;
  }
  .lead {
    margin: 6px 24px 14px;
    font-size: 14px;
    line-height: 20px;
    color: var(--yap-paper-text-2);
    text-wrap: pretty;
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 24px 8px;
  }

  .drop {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 34px 20px;
    box-sizing: border-box;
    border: 1.5px dashed var(--yap-paper-s2-strong);
    border-radius: 14px;
    background: var(--yap-paper-card);
    color: var(--yap-paper-text-2);
    font: inherit;
    cursor: pointer;
    transition:
      border-color 0.15s ease,
      background-color 0.15s ease;
  }
  .drop:hover:not(:disabled),
  .drop.over {
    border-color: var(--yap-paper-text-2);
    background: var(--yap-paper-s1);
  }
  .drop:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .drop svg {
    width: 28px;
    height: 28px;
    color: var(--yap-paper-muted);
  }
  .d1 {
    font-size: 15px;
    font-weight: 600;
    color: var(--yap-paper-ink);
  }
  .d2 {
    font-size: 12.5px;
    color: var(--yap-paper-muted);
  }
  .kinds {
    margin: 14px 0 4px;
    padding-left: 18px;
    font-size: 13px;
    line-height: 20px;
    color: var(--yap-paper-text-2);
  }
  .kinds b {
    font-weight: 600;
    color: var(--yap-paper-ink);
  }

  .cards {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin: 0;
    padding: 0;
    list-style: none;
    border-radius: 14px;
  }
  .cards.over {
    box-shadow: 0 0 0 2px var(--yap-paper-ring);
  }
  .card {
    padding: 12px 14px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 12px;
    background: var(--yap-paper-card);
  }
  .card.bad {
    background: var(--yap-paper);
  }
  .chead {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .ficon {
    display: flex;
    flex: 0 0 auto;
    width: 30px;
    height: 30px;
    align-items: center;
    justify-content: center;
    border-radius: 8px;
    background: var(--yap-paper-s1);
    color: var(--yap-paper-text-2);
  }
  .ficon svg {
    width: 16px;
    height: 16px;
  }
  .fmeta {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-width: 0;
  }
  .fname {
    overflow: hidden;
    font-size: 14px;
    font-weight: 600;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .fkind {
    font-size: 12px;
    color: var(--yap-paper-muted);
  }
  .rm {
    display: flex;
    flex: 0 0 auto;
    width: 26px;
    height: 26px;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
  }
  .rm:hover:not(:disabled) {
    background: var(--yap-paper-s1);
    color: var(--yap-paper-ink);
  }
  .rm svg {
    width: 12px;
    height: 12px;
  }
  .facts {
    margin: 10px 0 6px;
    font-size: 13px;
    color: var(--yap-paper-text-2);
  }
  .spk {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin: 0 0 10px;
    padding: 0;
    list-style: none;
  }
  .spk li {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 9px;
    border-radius: 12px;
    background: var(--yap-paper-s1);
    font-size: 12.5px;
  }
  .spk li.you {
    background: color-mix(in srgb, var(--yap-live) 16%, transparent);
  }
  .spk .n {
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
  .spk .more {
    color: var(--yap-paper-muted);
  }
  .fields {
    display: grid;
    grid-template-columns: 1fr;
    gap: 8px;
  }
  .field {
    display: grid;
    grid-template-columns: 52px 1fr;
    align-items: center;
    column-gap: 10px;
    row-gap: 2px;
  }
  .flabel {
    font-size: 12.5px;
    color: var(--yap-paper-muted);
  }
  .field input,
  .field select {
    min-width: 0;
    height: 32px;
    box-sizing: border-box;
    padding: 0 10px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 8px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 13px;
    user-select: text;
    -webkit-user-select: text;
  }
  .field input::placeholder {
    color: var(--yap-paper-date);
  }
  .field input:focus,
  .field select:focus {
    outline: none;
    border-color: var(--yap-paper-text-2);
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .fixed {
    font-size: 13px;
  }
  .hint {
    grid-column: 2;
    font-size: 11.5px;
    color: var(--yap-paper-muted);
  }
  .note,
  .warn {
    margin: 8px 0 0;
    font-size: 12.5px;
    line-height: 18px;
    color: var(--yap-paper-text-2);
  }
  .warn {
    color: var(--yap-danger);
  }
  .err {
    margin: 10px 0 0;
    font-size: 13px;
    line-height: 19px;
    color: var(--yap-danger);
  }

  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 24px 20px;
    border-top: 1px solid var(--yap-paper-s1);
  }
  .grow {
    flex: 1 1 auto;
  }
  .ghost {
    height: 36px;
    padding: 0 14px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 8px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .ghost:hover:not(:disabled) {
    border-color: var(--yap-paper-s2-strong);
    background: var(--yap-paper-card);
  }
  .go {
    height: 36px;
    padding: 0 16px;
    border: none;
    border-radius: 8px;
    background: var(--yap-paper-ink);
    color: var(--yap-paper);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .go:disabled,
  .ghost:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .ghost:focus-visible,
  .go:focus-visible,
  .rm:focus-visible,
  .x:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
    border: 0;
  }
</style>
