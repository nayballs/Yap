<script>
  // A note's page (2026-10-31): the rich text editor Chat's documents edit
  // on (richEditor.js, TipTap on ProseMirror), for notes' My thoughts in
  // the Notes view, the meeting notepad's My thoughts and the quick note.
  // Loaded on demand with the editor (richtext.svelte.js: the note's plain
  // text box stands in until it's in). The note stays markdown, exactly: a
  // change rewrites only its own block, so an older Yap reads it as before.
  //
  // - The toolbar over it (RichToolbar: text style, code, table,
  //   checklist, + insert); `canSource`: a **Preview / Source** switch at
  //   its right, Source the note's markdown in a plain text box (what the
  //   editor can't hold as such is kept whole on the page, edited there),
  //   the choice remembered (`sourceKey`).
  // - `onchange(markdown, {paste})` as the person types, ticks a box or
  //   pastes (`paste`: Yap's dictation pasted in is one); `text` followed
  //   from outside (another window, a transform, Version history), the
  //   caret kept.
  // - `el`: the element taking the typing — the page, which answers as a
  //   text box does (`value`, `selectionStart`/`End` in the markdown,
  //   `setSelectionRange`, `select`, `readOnly`), or Source's box — so a
  //   note's right-click menu, its transforms on a selection, a search's
  //   words and Send work on either; `oncontextmenu` / `onkeydown` on it.
  //   `ed`: the editor (`insertPlain`: a dictation at the caret).
  // - Ctrl+F finds in the page (SummaryFind).
  // - `readonly` while a transform runs on it; `placeholder` on an empty one.
  import { untrack, tick } from 'svelte';
  import RichToolbar from './RichToolbar.svelte';
  import SummaryFind from '../SummaryFind.svelte';
  import { createRichEditor } from './richEditor.js';
  import { takeHandoff } from './richtext.svelte.js';

  let {
    text = '',
    onchange = () => {},
    label = 'Note',
    placeholder = '',
    readonly = false,
    look = 'paper',
    canSource = false,
    sourceKey = null,
    rawTitle = undefined,
    toolbarLabel = 'Format the note',
    oncontextmenu = null,
    onkeydown = null,
    el = $bindable(null),
    ed = $bindable(null),
  } = $props();

  /** The view remembered (`sourceKey` in this window's storage). */
  function remembered() {
    if (!canSource || !sourceKey) return false;
    try {
      return localStorage.getItem(sourceKey) === 'source';
    } catch {
      return false;
    }
  }

  let source = $state(untrack(remembered));
  let mount = $state(null);
  let srcEl = $state(null);
  let tools = $state({});
  // (Raw: the editor is used as it is, never wrapped.)
  let ctl = $state.raw(null);
  let finder = $state(null);
  let finding = $state(false);
  // The selection to carry into the view switched to: `{start, end}` in the markdown.
  let carry = null;
  // What an empty page says (read by the editor's placeholder).
  let shownPlaceholder = untrack(() => placeholder);

  // ---- the page ------------------------------------------------------------------------

  $effect(() => {
    const m = mount;
    if (!m) return;
    // What's on at the caret, for the toolbar: a moment later, never while
    // the window is drawing (a page removed with the focus in it reports
    // losing it right then).
    let next = null;
    const showTools = (s) => {
      const queued = next != null;
      next = s;
      if (!queued)
        queueMicrotask(() => {
          if (ctl === c && next) tools = next;
          next = null;
        });
    };
    const c = untrack(() =>
      createRichEditor({
        mount: m,
        text,
        label,
        rawTitle,
        textbox: true,
        editable: !readonly,
        placeholder: () => shownPlaceholder,
        onchange: (md, how) => onchange(md, how),
        onstate: showTools,
      })
    );
    const menu = (e) => oncontextmenu?.(e);
    const key = (e) => {
      if (findKey(e)) return;
      onkeydown?.(e);
    };
    m.addEventListener('contextmenu', menu);
    m.addEventListener('keydown', key);
    ctl = c;
    ed = c;
    el = m;
    // The caret where it was: in the plain box this took the place of, or in Source.
    const at = takeHandoff() ?? carry;
    carry = null;
    if (at) {
      c.focus();
      m.setSelectionRange(at.start, at.end);
    }
    return () => {
      m.removeEventListener('contextmenu', menu);
      m.removeEventListener('keydown', key);
      c.destroy();
      if (ctl === c) ctl = null;
      if (untrack(() => ed) === c) ed = null;
      if (untrack(() => el) === m) el = null;
      tools = {};
    };
  });

  // The note from outside (another window, a transform, a restore): only
  // the blocks that differ, the caret kept.
  $effect(() => {
    const t = text;
    const c = ctl;
    if (c) untrack(() => c.load(t));
  });

  $effect(() => {
    const on = !readonly;
    ctl?.setEditable(on);
    if (on) mount?.removeAttribute('aria-busy');
    else mount?.setAttribute('aria-busy', 'true');
  });

  $effect(() => {
    shownPlaceholder = placeholder;
    untrack(() => ctl?.refresh());
  });

  /** A click on the page under its last block: the caret at the end of the note. */
  function pageClick(e) {
    if (!ctl || !mount || mount.contains(e.target)) return;
    if (e.clientY < mount.getBoundingClientRect().bottom) return;
    ctl.focus('end');
  }

  // ---- Source --------------------------------------------------------------------------

  $effect(() => {
    const box = srcEl;
    if (!box) return;
    el = box;
    const at = carry;
    carry = null;
    if (at) {
      box.focus({ preventScroll: true });
      box.setSelectionRange(at.start, at.end);
    }
    return () => {
      if (untrack(() => el) === box) el = null;
    };
  });

  async function setView(v) {
    const next = v === 'source';
    if (next === source) return;
    // The selection goes with it.
    const box = el;
    carry = box && box.selectionStart != null ? { start: box.selectionStart, end: box.selectionEnd } : null;
    finder?.close();
    source = next;
    if (sourceKey) {
      try {
        localStorage.setItem(sourceKey, next ? 'source' : 'preview');
      } catch {
        /* this time only */
      }
    }
    await tick();
  }

  function viewKey(e) {
    if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
      e.preventDefault();
      const next = source ? 'preview' : 'source';
      setView(next);
      e.currentTarget.parentElement?.querySelector(`[data-view="${next}"]`)?.focus();
    }
  }

  // ---- find ------------------------------------------------------------------------------

  /** Ctrl+F on the page: find in it. */
  function findKey(e) {
    if (!(e.ctrlKey || e.metaKey) || e.altKey || e.shiftKey || e.key.toLowerCase() !== 'f') return false;
    e.preventDefault();
    e.stopPropagation();
    finder?.show();
    return true;
  }
</script>

{#snippet views()}
  <!-- Preview (the page) or Source (its markdown); ←/→ switch. -->
  <div class="viewseg" class:app={look === 'app'} role="group" aria-label="View">
    {#each [['preview', 'Preview', 'The note as a page'], ['source', 'Source', 'Its Markdown, to read or edit']] as [v, name, hint] (v)}
      {@const on = (v === 'source') === source}
      <button type="button" class="segbtn" class:on data-view={v} aria-pressed={on} tabindex={on ? 0 : -1} title={hint} onclick={() => setView(v)} onkeydown={viewKey}
        >{name}</button
      >
    {/each}
  </div>
{/snippet}

<div class="richnote" class:app={look === 'app'} data-rich>
  <RichToolbar
    ed={source || readonly ? null : ctl}
    state={tools}
    disabled={source || readonly}
    why={source ? 'Formatting is in Preview' : ''}
    label={toolbarLabel}
    testid="note-toolbar"
    {look}
    end={canSource ? views : undefined}
  />
  {#if !source}
    <div class="richfind">
      <SummaryFind bind:this={finder} bind:open={finding} root={() => mount} {text} look={look === 'app' ? 'app' : 'paper'} label="Find in the note" />
    </div>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (a pointer's way to the end: the keyboard has Ctrl+End) -->
    <div class="richscroll" onclick={pageClick}>
      <div class="richpage" bind:this={mount}></div>
    </div>
  {:else}
    <textarea
      class="richsrc"
      bind:this={srcEl}
      aria-label={label}
      {placeholder}
      value={text}
      readonly={readonly}
      aria-busy={readonly}
      spellcheck="true"
      oninput={(e) => onchange(e.currentTarget.value, { paste: e.inputType === 'insertFromPaste' })}
      oncontextmenu={(e) => oncontextmenu?.(e)}
      onkeydown={(e) => onkeydown?.(e)}
    ></textarea>
  {/if}
</div>

<style>
  /* The note's page: the place's own reading text (`--rich-font`, `-size`,
     `-leading`, `-pad` from the host), the paper's colours (the notepad,
     the quick note) or the app's (Notes, `.app`). The blocks as Chat's
     document draws them, a size smaller: headings a level down, lists,
     checklists with their box where the bullet was, quotes, code, rules,
     filled table cells. ProseMirror's own needs here (TipTap's sheet is
     left out: its colours aren't Yap's). */
  .richnote {
    --rich-ink: var(--yap-paper-ink);
    --rich-text-2: var(--yap-paper-text-2);
    --rich-muted: var(--yap-paper-muted);
    --rich-faint: var(--yap-paper-faint);
    --rich-fill: var(--yap-paper-s1);
    --rich-fill-2: var(--yap-paper-s2);
    --rich-rule: var(--yap-paper-s2-strong);
    /* The toolbar's buttons from the page's left edge (`--rich-tools-x`). */
    --tool-pad-x: var(--rich-tools-x, 12px);
    flex: 1 1 auto;
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    color: var(--rich-ink);
  }
  .richnote.app {
    --rich-ink: var(--yap-fg);
    --rich-text-2: var(--yap-fg-80);
    --rich-muted: var(--yap-muted);
    --rich-faint: var(--yap-muted-55);
    --rich-fill: var(--yap-s1);
    --rich-fill-2: var(--yap-s2);
    --rich-rule: var(--yap-border);
    --tool-pad-x: var(--rich-tools-x, 0px);
  }
  .richfind :global(.find) {
    position: static;
    margin: 8px 0 0;
  }
  .richfind :global(.find.paper) {
    margin-inline: 12px;
  }
  .richscroll {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    cursor: text;
  }
  .richscroll::-webkit-scrollbar {
    width: 8px;
  }
  .richscroll::-webkit-scrollbar-thumb {
    border-radius: 4px;
    background: color-mix(in srgb, var(--rich-ink) 16%, transparent);
  }
  .richpage,
  .richsrc {
    box-sizing: border-box;
    padding: var(--rich-pad, 10px 2px 24px);
    font-family: var(--rich-font, var(--yap-font-reading));
    font-size: var(--rich-size, calc(14px * var(--yap-reading-scale)));
    line-height: var(--rich-leading, calc(1.6 * var(--yap-reading-leading)));
    color: var(--rich-ink);
  }
  .richpage {
    --gap: 0.7em;
    position: relative;
    min-height: 100%;
    outline: none;
    overflow-wrap: break-word;
    white-space: pre-wrap;
    white-space: break-spaces;
    word-wrap: break-word;
    font-variant-ligatures: none;
    caret-color: var(--rich-ink);
    user-select: text;
    -webkit-user-select: text;
  }
  .richpage::selection,
  .richpage :global(::selection) {
    background: color-mix(in srgb, var(--yap-primary) 26%, transparent);
  }
  .richsrc {
    flex: 1 1 auto;
    min-height: 0;
    width: 100%;
    margin: 0;
    border: none;
    background: transparent;
    resize: none;
    outline: none;
    user-select: text;
  }
  .richsrc::placeholder {
    color: var(--rich-faint);
  }
  /* An empty page: what to do, quiet (TipTap's placeholder). */
  .richpage :global(p.is-editor-empty:first-child::before) {
    content: attr(data-placeholder);
    float: left;
    height: 0;
    color: var(--rich-faint);
    pointer-events: none;
  }
  .richpage > :global(*) {
    margin: 0 0 var(--gap);
  }
  .richpage > :global(:last-child) {
    margin-bottom: 0;
  }
  .richpage :global([contenteditable='false']) {
    white-space: normal;
  }
  .richpage :global(img.ProseMirror-separator) {
    display: inline !important;
    width: 0 !important;
    height: 0 !important;
    margin: 0 !important;
    border: none !important;
  }
  .richpage:global(.ProseMirror-hideselection) :global(*::selection) {
    background: transparent;
  }
  .richpage:global(.ProseMirror-hideselection) {
    caret-color: transparent;
  }
  .richpage :global(.ProseMirror-selectednode) {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 3px;
    border-radius: 3px;
  }
  .richpage :global(td.selectedCell),
  .richpage :global(th.selectedCell) {
    background: color-mix(in srgb, var(--yap-primary) 22%, var(--rich-fill));
  }
  .richpage :global(.ProseMirror-gapcursor) {
    display: none;
    position: absolute;
    margin: 0;
    pointer-events: none;
  }
  .richpage :global(.ProseMirror-gapcursor)::after {
    content: '';
    display: block;
    position: absolute;
    top: -2px;
    width: 20px;
    border-top: 1px solid var(--rich-ink);
  }
  .richpage:global(.ProseMirror-focused) :global(.ProseMirror-gapcursor) {
    display: block;
  }
  :global(.docdrop) {
    background: var(--yap-primary);
  }
  .richpage :global(h2),
  .richpage :global(h3),
  .richpage :global(h4),
  .richpage :global(h5) {
    color: var(--rich-ink);
    text-wrap: balance;
  }
  .richpage > :global(h2) {
    margin: 1.2em 0 0.4em;
    font-family: var(--yap-font-display);
    font-size: 1.5em;
    font-weight: 600;
    line-height: 1.2;
  }
  .richpage > :global(h3) {
    margin: 1.1em 0 0.35em;
    font-family: var(--yap-font-display);
    font-size: 1.3em;
    font-weight: 600;
    line-height: 1.22;
  }
  .richpage > :global(h4) {
    margin: 1em 0 0.3em;
    font-size: 1.06em;
    font-weight: 650;
    line-height: 1.35;
  }
  .richpage > :global(h5) {
    margin: 0.9em 0 0.25em;
    font-size: 1em;
    font-weight: 650;
  }
  .richpage > :global(:is(h2, h3, h4, h5):first-child) {
    margin-top: 0;
  }
  .richpage :global(strong) {
    font-weight: 650;
    color: var(--rich-ink);
  }
  .richpage :global(ul),
  .richpage :global(ol) {
    padding-left: 1.35em;
  }
  .richpage :global(li + li) {
    margin-top: 0.2em;
  }
  .richpage :global(li > .itembody > p) {
    margin: 0;
  }
  .richpage :global(li > .itembody > * + *) {
    margin-top: 0.3em;
  }
  .richpage :global(li::marker) {
    color: var(--rich-muted);
  }
  /* Checklist items: the box where the bullet was; a ticked one quieter, struck through. */
  .richpage :global(li.task) {
    position: relative;
    list-style: none;
  }
  .richpage :global(li.task > input) {
    position: absolute;
    top: 0.34em;
    left: -1.3em;
    width: 0.92em;
    height: 0.92em;
    margin: 0;
    accent-color: var(--rich-ink);
    cursor: pointer;
  }
  .richpage :global(li.task[data-checked='true'] > .itembody) {
    color: var(--rich-muted);
    text-decoration: line-through;
    text-decoration-color: var(--rich-faint);
  }
  .richpage :global(a.md-link) {
    color: var(--yap-link);
    text-decoration-thickness: 1px;
    text-decoration-color: color-mix(in srgb, var(--yap-link) 45%, transparent);
  }
  .richpage :global(:not(pre) > code) {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 0.88em;
    background: var(--rich-fill);
    border-radius: 4px;
    padding: 0.1em 0.35em;
    box-decoration-break: clone;
  }
  .richpage :global(pre) {
    padding: 8px 12px;
    border-radius: 6px;
    background: var(--rich-fill);
    font-family: ui-monospace, Consolas, monospace;
    font-size: 0.86em;
    line-height: 1.55;
    white-space: pre-wrap;
    overflow-x: auto;
  }
  .richpage :global(blockquote) {
    margin-left: 0;
    padding-left: 1em;
    border-left: 2px solid var(--rich-rule);
    color: var(--rich-text-2);
  }
  .richpage :global(hr) {
    height: 0;
    margin: 1.2em 0;
    border: none;
    border-top: 1px solid var(--rich-fill-2);
  }
  /* Tables: cells filled, no rules — the header row a shade stronger; a
     wide one scrolls inside itself, never the page. */
  .richpage :global(.tableWrapper) {
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: thin;
  }
  .richpage :global(.tableWrapper table) {
    width: 100% !important;
    border-collapse: separate;
    border-spacing: 2px;
    font-size: 0.93em;
    line-height: 1.5;
    font-variant-numeric: tabular-nums lining-nums;
  }
  .richpage :global(.tableWrapper :is(th, td)) {
    min-width: 3em;
    padding: 5px 10px;
    border: none;
    border-radius: 3px;
    text-align: left;
    vertical-align: top;
    overflow-wrap: break-word;
  }
  .richpage :global(.tableWrapper th) {
    background: var(--rich-fill-2);
    font-weight: 600;
    color: var(--rich-ink);
    vertical-align: bottom;
  }
  .richpage :global(.tableWrapper td) {
    background: var(--rich-fill);
  }
  .richpage :global(.tableWrapper :is(th, td) > p) {
    margin: 0;
  }
  /* Markdown the editor keeps whole (edited in Source): drawn as the page is. */
  .richpage :global(.docraw > *) {
    margin: 0 0 var(--gap);
  }
  .richpage :global(.docraw > :last-child) {
    margin-bottom: 0;
  }
  .richpage :global(p.md-pending) {
    cursor: default;
  }
  /* A change that landed from outside: washed a moment. */
  .richpage > :global([data-changed]) {
    border-radius: 4px;
    animation: landed 1.6s ease-out;
  }
  @keyframes landed {
    0%,
    25% {
      background-color: var(--yap-primary-wash);
      box-shadow: 0 0 0 4px var(--yap-primary-wash);
    }
    100% {
      background-color: transparent;
      box-shadow: 0 0 0 4px transparent;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .richpage > :global([data-changed]) {
      animation: none;
    }
  }
  /* Preview | Source: a small segmented switch, the one shown raised. */
  .viewseg {
    flex: 0 0 auto;
    display: inline-flex;
    padding: 2px;
    border-radius: 8px;
    background: var(--yap-paper-s1);
  }
  .viewseg.app {
    background: var(--yap-s2);
  }
  .segbtn {
    height: 24px;
    padding: 0 9px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .viewseg.app .segbtn {
    color: var(--yap-muted);
  }
  .segbtn:hover {
    color: var(--yap-paper-ink);
  }
  .viewseg.app .segbtn:hover {
    color: var(--yap-fg);
  }
  .segbtn.on {
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    box-shadow: 0 1px 2px color-mix(in srgb, var(--yap-paper-ink) 12%, transparent);
  }
  .viewseg.app .segbtn.on {
    background: var(--yap-bg);
    color: var(--yap-fg);
  }
  .segbtn:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
</style>
