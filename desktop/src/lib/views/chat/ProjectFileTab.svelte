<script>
  // **A big project's file beside the chat, at the part an answer cites** (16 Nov 2026, after the Claude app, whose
  // citations open a project's file; chat/projects/sources.rs): read only, the file as Yap read it — the text the
  // model was given — scrolled to the part and the part marked. Opened by a source's pill in the answer or its chip
  // under it (chatState's `openSource`, projectFile.js `partView`); a spreadsheet opens in the sheet view instead.
  //
  // - **As each kind reads** (projectFile.js `blocksOf`): a PDF page by page, each page's number its heading; a
  //   presentation slide by slide, its speaker notes quieter; a Word document's paragraphs, its heading styles as
  //   headings; Markdown as the document panel draws a document (core/markdown.js, its blocks' lines kept to mark the
  //   part); code as numbered lines.
  // - **Its place** in the header ("Page 12", "“Annual leave”"), and what the text is ("The PDF's text as Yap read
  //   it…": its pictures and layout aren't here).
  // - Rust reads it (chat/projects.rs `project_file_view`: only a file of that project, by its ids).
  import { invoke } from '@tauri-apps/api/core';
  import { tick, untrack } from 'svelte';
  import { renderMarkdown } from '../../core/markdown.js';
  import { blockLines, blocksOf, drawnAs, inPart, placeLabel, textNote } from './projectFile.js';

  /** `doc`: the panel's state (`doc.projectFile`: `{project, file, name, kind, from, to, at, place, section, seq}`). */
  let { doc, narrow = false } = $props();

  const ref = $derived(doc.projectFile);
  let view = $state.raw(null);
  let error = $state('');
  let loading = $state(false);
  let bodyEl = $state(null);
  let reading = 0;
  let loadedKey = '';

  async function load() {
    const r = ref;
    if (!r?.file) return;
    const key = `${r.project}:${r.file}`;
    const mine = ++reading;
    if (key !== loadedKey || !view) {
      loading = true;
      try {
        const v = await invoke('project_file_view', { project: r.project, file: r.file });
        if (mine !== reading) return;
        view = v;
        error = '';
        loadedKey = key;
      } catch (e) {
        if (mine !== reading) return;
        view = null;
        loadedKey = '';
        error = String(e?.message ?? e);
      } finally {
        if (mine === reading) loading = false;
      }
    }
    await tick();
    toPart();
  }

  // Opened (again: `seq`, another part), another file: read it, then go to the part.
  $effect(() => {
    void ref?.file;
    void ref?.seq;
    untrack(load);
  });

  const how = $derived(drawnAs(view?.kind || ref?.kind));
  const blocks = $derived(view && how !== 'markdown' ? blocksOf(view.text, view.kind) : []);
  const html = $derived(view && how === 'markdown' ? renderMarkdown(view.text, { lines: true }) : '');
  const name = $derived(view?.name || ref?.name || 'Project file');
  const label = $derived(placeLabel(ref));
  const pages = $derived(blocks.filter((b) => b.kind === 'page').length);
  const facts = $derived.by(() => {
    if (!view) return '';
    const lines = String(view.text || '').split('\n').length;
    const size = view.kind === 'pdf' && (view.pages || pages) ? `${(view.pages || pages).toLocaleString('en-GB')} pages` : `${lines.toLocaleString('en-GB')} lines`;
    return `${size} · read only${view.cut ? ' · only its start is shown' : ''}`;
  });

  /** The part: its blocks marked (a Markdown file's, drawn by the renderer: by their lines), the first at the top. */
  function toPart() {
    if (!bodyEl || !ref) return;
    let first = null;
    if (how === 'markdown') {
      for (const el of bodyEl.querySelectorAll('.pfmd > [data-line]')) {
        const l = blockLines(el.getAttribute('data-line'));
        const on = !!l && inPart(l, ref.from, ref.to);
        el.classList.toggle('pfmark', on);
        if (on && !first) first = el;
      }
    } else {
      first = bodyEl.querySelector('.pfmark');
    }
    if (first) bodyEl.scrollTop += first.getBoundingClientRect().top - bodyEl.getBoundingClientRect().top - 18;
    else bodyEl.scrollTop = 0;
  }
</script>

<section class="pftab" aria-label="Project file" data-testid="chat-project-file" data-file={ref?.file ?? ''} data-from={ref?.from ?? ''} data-to={ref?.to ?? ''}>
  <header class="pfhead">
    <svg class="pficon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5" /><path d="M9 12h6M9 16h4" /></svg>
    <h2 class="pftitle" title={view?.project ? `${name} · in ${view.project}` : name}>
      <span class="pfname" data-testid="chat-project-file-name">{name}</span>{#if view?.project}<span class="pfproj">{view.project}</span>{/if}
    </h2>
    {#if label}<span class="pfplace" data-testid="chat-project-file-place" title="Where the answer's source is">{label}</span>{/if}
    <div class="pfactions">
      {#if !narrow}
        <button
          type="button"
          class="pfbtn"
          onclick={() => (doc.expanded = !doc.expanded)}
          aria-label={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
          title={doc.expanded ? 'Back beside the chat' : 'Expand to full width'}
        >
          {#if doc.expanded}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" /></svg>
          {:else}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" /></svg>
          {/if}
        </button>
      {/if}
      <button type="button" class="pfbtn" onclick={() => doc.close()} aria-label="Close the panel" title="Close">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
      </button>
    </div>
  </header>
  {#if error}
    <p class="pfnote" role="alert" data-testid="chat-project-file-error">{error}</p>
  {:else if view}
    <!-- (A box that scrolls takes the keyboard's focus, so its arrows can scroll it.) -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="pfbody" bind:this={bodyEl} tabindex="0" role="region" aria-label={`${name} (read only)`} data-testid="chat-project-file-body">
      <article class="pfpage" class:lines={how === 'lines'}>
        <p class="pfabout">{textNote(view.kind)}</p>
        {#if how === 'markdown'}
          <!-- eslint-disable-next-line svelte/no-at-html-tags — renderMarkdown sanitises its output -->
          <div class="pfmd">{@html html}</div>
        {:else}
          {#each blocks as b, i (i)}
            {@const on = inPart(b, ref.from, ref.to)}
            {#if b.kind === 'page'}
              <h3 class="pfpagehead" class:pfmark={on} data-page={b.n}>Page {b.n}{#if b.ocr}<span class="pfocr">read with OCR</span>{/if}</h3>
            {:else if b.kind === 'slide'}
              <h3 class="pfslide" class:pfmark={on} data-slide={b.n}>Slide {b.n}{#if b.text}: {b.text}{/if}{#if b.hidden}<span class="pfocr">hidden</span>{/if}</h3>
            {:else if b.kind === 'heading'}
              <svelte:element this={`h${Math.min(6, b.level + 2)}`} class="pfheading" class:pfmark={on}>{b.text}</svelte:element>
            {:else if b.kind === 'line'}
              <div class="pfline" class:pfmark={on} data-no={b.n}><span class="pfno" aria-hidden="true">{b.n}</span><span class="pft">{b.text || ' '}</span></div>
            {:else}
              <!-- (A PDF's paragraph flows to the panel's width: its line ends are the page's layout, not its words'.) -->
              <p class="pfpara" class:reflow={view.kind === 'pdf'} class:pfnotes={b.kind === 'note'} class:pfmark={on}>{b.text}</p>
            {/if}
          {/each}
        {/if}
      </article>
    </div>
  {:else if loading}
    <p class="pfnote">Reading…</p>
  {/if}
  {#if facts}<p class="pffacts" data-testid="chat-project-file-facts">{facts}</p>{/if}
</section>

<style>
  .pftab {
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    color: var(--yap-paper-ink);
  }
  .pfhead {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 12px 12px 8px 18px;
    border-bottom: 1px solid var(--yap-paper-rule);
  }
  .pficon {
    flex: 0 0 auto;
    width: 15px;
    height: 15px;
    color: var(--yap-paper-muted);
  }
  .pftitle {
    display: flex;
    align-items: baseline;
    gap: 8px;
    flex: 0 1 auto;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    white-space: nowrap;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-paper-text-2);
  }
  .pfname {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* (The project's name gives way first, then the file's: the place stays readable.) */
  .pfproj {
    flex: 0 100 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-weight: 400;
    font-size: 11.5px;
    color: var(--yap-paper-muted);
  }
  .pfplace {
    flex: 0 0 auto;
    min-width: 0;
    max-width: 48%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--yap-primary-tint);
    font-size: 11px;
    font-weight: 600;
    color: var(--yap-primary);
  }
  .pfactions {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: auto;
  }
  .pfbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    height: 28px;
    min-width: 28px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
  }
  .pfbtn:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .pfbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .pfbtn svg {
    width: 15px;
    height: 15px;
  }
  .pfbody {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    background: var(--yap-paper);
    font-family: var(--yap-font-reading);
    font-size: calc(14px * var(--yap-reading-scale, 1));
    line-height: calc(1.62 * var(--yap-reading-leading, 1));
    overflow-wrap: break-word;
  }
  .pfbody:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: -2px;
  }
  .pfpage {
    max-width: 68ch;
    margin: 0 auto;
    padding: 16px clamp(18px, 6%, 56px) 56px;
    user-select: text;
    -webkit-user-select: text;
  }
  .pfpage.lines {
    max-width: none;
    padding: 10px 0 24px;
  }
  .pfabout {
    margin: 0 0 14px;
    font-family: var(--yap-font-ui, inherit);
    font-size: 11.5px;
    color: var(--yap-paper-muted);
  }
  .pfpage.lines .pfabout {
    padding: 0 18px;
  }
  .pfpagehead,
  .pfslide {
    display: flex;
    align-items: baseline;
    gap: 8px;
    margin: 22px 0 8px;
    padding-top: 10px;
    border-top: 1px solid var(--yap-paper-rule);
    font-family: var(--yap-font-ui, inherit);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--yap-paper-muted);
  }
  .pfslide {
    text-transform: none;
    letter-spacing: 0;
    font-family: var(--yap-font-display);
    font-size: calc(17px * var(--yap-reading-step, 1));
    font-weight: 600;
    color: var(--yap-paper-ink);
  }
  .pfocr {
    font-weight: 500;
    text-transform: none;
    letter-spacing: 0;
    color: var(--yap-paper-faint);
  }
  .pfheading {
    margin: 22px 0 8px;
    font-family: var(--yap-font-display);
    font-weight: 600;
    line-height: 1.25;
    color: var(--yap-paper-ink);
  }
  h3.pfheading {
    font-size: calc(20px * var(--yap-reading-step, 1));
  }
  h4.pfheading {
    font-size: calc(17px * var(--yap-reading-step, 1));
  }
  h5.pfheading,
  h6.pfheading {
    font-size: calc(15px * var(--yap-reading-step, 1));
  }
  .pfpara {
    margin: 0 0 12px;
    white-space: pre-line;
  }
  .pfpara.reflow {
    white-space: normal;
  }
  .pfpara.pfnotes {
    font-size: 0.93em;
    color: var(--yap-paper-text-2);
  }
  /* Markdown, as the document panel draws a document. */
  .pfmd > :global(*) {
    margin: 0 0 13px;
  }
  .pfmd :global(h1),
  .pfmd :global(h2),
  .pfmd :global(h3),
  .pfmd :global(h4) {
    margin-top: 22px;
    font-family: var(--yap-font-display);
    font-weight: 600;
    line-height: 1.25;
    color: var(--yap-paper-ink);
  }
  .pfmd :global(h1) {
    font-size: calc(24px * var(--yap-reading-step, 1));
  }
  .pfmd :global(h2) {
    font-size: calc(20px * var(--yap-reading-step, 1));
  }
  .pfmd :global(h3) {
    font-size: calc(17px * var(--yap-reading-step, 1));
  }
  /* The part the answer cites: a bar in the accent beside it, on a light wash (a whole page stays easy to read). */
  .pfpage :global(.pfmark) {
    margin-left: -12px;
    padding-left: 10px;
    border-left: 2px solid var(--yap-primary);
    background: color-mix(in srgb, var(--yap-primary) 7%, transparent);
    border-radius: 0 3px 3px 0;
  }
  .pfpagehead.pfmark,
  .pfslide.pfmark {
    color: var(--yap-primary);
  }
  .pfline {
    display: flex;
    padding-right: 14px;
    font-family: ui-monospace, Consolas, monospace;
    font-size: calc(11.5px * var(--yap-reading-step, 1));
    line-height: 1.55;
  }
  .pfpage :global(.pfline.pfmark) {
    margin-left: 0;
    padding-left: 0;
    border-left: none;
    border-radius: 0;
    box-shadow: inset 2px 0 0 var(--yap-primary);
  }
  .pfno {
    flex: 0 0 auto;
    width: 56px;
    padding-right: 12px;
    text-align: right;
    color: var(--yap-paper-faint);
    user-select: none;
  }
  .pft {
    min-width: 0;
    white-space: pre-wrap;
    tab-size: 4;
  }
  .pfnote {
    margin: 18px;
    color: var(--yap-paper-muted);
    font-size: 13px;
  }
  .pffacts {
    margin: 0;
    padding: 6px 18px;
    border-top: 1px solid var(--yap-paper-rule);
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
</style>
