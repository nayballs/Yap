<script>
  // **A stretch of steps, opened** (11 Nov 2026, after Claude Code's own
  // transcript; the maintainer: "this is what happens when you click the drop
  // down in claude code app… can you build something that matches that?"):
  // a quiet box with a row for each step, as done — "Edited agents.js +24 −3",
  // "Read app.ts (424–503)", "Created notes.md +12", "Checked the edits
  // parse" — and a row opens to what it did:
  //
  // - **an edit**: the file's path and its diff, each line with its number
  //   in the file (core/agents.js `diffRows`: Claude Code's own patch says
  //   where each part is), removed lines red, added ones green, its code in
  //   colour by the file's language, a long line wrapped under itself;
  //   where a line was changed rather than
  //   taken out, the part that was replaced is marked in a stronger red and
  //   what replaced it in a stronger green (core/diffWords.js);
  // - **a command**: the command itself (`$ …`, with Copy), in colour, and
  //   what it printed, plain, its start and its end (agents/output.rs);
  // - **a picture** its call came back with, larger (small at the row's end).
  //
  // Before, an opened stretch was every step's line with each command dumped
  // whole under it. A row with nothing more to show doesn't open. The step
  // the agent is on now is the last row, as it is, with a spinner. Plumbing
  // ("Starting Claude Code", "Finding a tool") is no row once it's past
  // (`rowSteps`).
  //
  // **Code colours** (core/codeColours.js): the highlighter loads the first
  // time a row is opened; until it has, and for a file with no language Yap
  // knows, the text is plain. Each token is a text node in a span
  // (`.syn-k`…, app.css `--yap-syn-*`): nothing of a file is set as markup.
  import { rowSteps, stepRow, stepOpens, stepKind, diffRows, moreLines } from '../../core/agents.js';
  import { languageOf, shellOf, colourDiff, colourLines, loadColours } from '../../core/codeColours.js';
  import { pairChanges, linePieces } from '../../core/diffWords.js';
  import { copyText } from '../../core/itemMenus.js';
  import { placed, shown } from './chatImages.js';
  import { screenPixels } from './chatShots.svelte.js';
  import StepPictures from './StepPictures.svelte';

  /** A row's picture, opened: at most this big (CSS px). */
  const PIC_W = 280;
  const PIC_H = 180;

  /** `steps`: a stretch's (`workOf`); `busy`: the agent is on its last one now; `onview(picture)`: open a step's picture. */
  let { steps = [], busy = false, onview = null } = $props();

  /** The rows the person opened (by their place: a row keeps it as more come). */
  let open = $state({});
  /** The highlighter, once it has loaded (never made reactive inside: it's a library's object). */
  let colours = $state.raw(null);
  function toggle(k) {
    open = { ...open, [k]: !open[k] };
    if (!colours) {
      loadColours()
        .then((h) => (colours = h))
        .catch(() => {});
    }
  }
  let gone = $state({});
  /** A piece's classes: its colour's, and "chg" on the part that was replaced. */
  const pieceClass = (p) => [p.kind ? `syn-${p.kind}` : '', p.changed ? 'chg' : ''].filter(Boolean).join(' ') || undefined;
  const lineWord = (n) => `${Number(n).toLocaleString('en-GB')} more ${Number(n) === 1 ? 'line' : 'lines'}`;
</script>

{#snippet words(r, now)}
  {#if now}<span class="spin" aria-hidden="true"></span>{/if}
  <span class="rtext"
    >{r.text}{#if r.name}{' '}<span class="rname">{r.name}</span>{/if}{#if r.tail}{' '}<span class="rtail">{r.tail}</span>{/if}</span
  >
  {#if r.added}<span class="plus">+{r.added}</span>{/if}
  {#if r.removed}<span class="minus">−{r.removed}</span>{/if}
  {#if r.failed}<span class="rfailed">failed</span>{/if}
{/snippet}

<ol class="rows" data-testid="chat-agent-work-rows" data-colours={colours ? 'true' : 'false'}>
  {#each rowSteps(steps, busy) as { step: s, i: k, now } (k)}
    {@const r = stepRow(s, now)}
    {@const can = stepOpens(s)}
    {@const isOpen = can && !!open[k]}
    {@const pics = (s.pictures || []).filter((p) => !gone[p.id])}
    <li class="row" data-testid="chat-agent-work-row" data-kind={stepKind(s)} data-open={isOpen ? 'true' : 'false'}>
      <div class="rline">
        {#if can}
          <button type="button" class="rhead" aria-expanded={isOpen} title={isOpen ? 'Hide what it did' : 'Show what it did'} onclick={() => toggle(k)}>
            {@render words(r, now)}
            <svg class="chev" class:down={isOpen} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
          </button>
        {:else}
          <div class="rhead still">{@render words(r, now)}</div>
        {/if}
        {#if pics.length && !isOpen}<StepPictures pictures={pics} max={2} {onview} />{/if}
      </div>
      {#if isOpen}
        <div class="rbody">
          {#if s.diff?.lines?.length}
            {@const rows = colourDiff(colours, pairChanges(diffRows(s.diff)), languageOf(s.file))}
            {@const digits = Math.max(1, ...rows.map((x) => String(x.no ?? '').length))}
            <p class="rpath">{s.file}{#if s.diff.all}<span class="rnote"> · every match replaced</span>{/if}</p>
            <!-- (A box that scrolls takes the keyboard's focus, so its arrows can scroll it.) -->
            <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
            <div class="dbox" role="group" aria-label={`Changes to ${s.file}`} tabindex="0" data-testid="chat-agent-work-diff" style:--digits={digits}>
              {#each rows as x, n (n)}
                {#if x.mark === 'gap'}
                  <div class="dl gap" aria-hidden="true">⋯</div>
                {:else}
                  <div class="dl {x.mark}" data-no={x.no ?? ''}>
                    <span class="dno" aria-hidden="true">{x.no ?? (x.mark === 'del' ? '−' : x.mark === 'add' ? '+' : '')}</span><span class="dt"
                      >{#each linePieces(x) as p, m (m)}<span class={pieceClass(p)}>{p.text}</span>{:else}{' '}{/each}</span
                    >{#if x.mark !== 'ctx'}<span class="sr-only">{x.mark === 'del' ? ' (removed)' : ' (added)'}</span>{/if}
                  </div>
                {/if}
              {/each}
            </div>
            {#if s.diff.more}<p class="rmore">{moreLines(s.diff.more)}</p>{/if}
          {/if}
          {#if s.command}
            {@const typed = colourLines(colours, s.command, shellOf(s.command))}
            <div class="cbox" data-testid="chat-agent-work-command" data-coloured={typed ? 'true' : 'false'}>
              <!-- (A box that scrolls takes the keyboard's focus, so its arrows can scroll it.) -->
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <pre class="ccmd" tabindex="0" aria-label="The command"><span class="cprompt" aria-hidden="true">$ </span
                >{#if typed}{#each typed as words, n (n)}{n ? '\n' : ''}{#each words as t, m (m)}<span class={t.kind ? `syn-${t.kind}` : undefined}>{t.text}</span>{/each}{/each}{:else}{s.command}{/if}</pre>
              <button type="button" class="ccopy" title="Copy the command" aria-label="Copy the command" onclick={() => copyText(s.command, 'Command copied')}>
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="11" height="11" rx="2" /><path d="M5 15V6a2 2 0 0 1 2-2h9" /></svg>
              </button>
            </div>
            {#if s.output}
              <!-- (A box that scrolls takes the keyboard's focus, so its arrows can scroll it.) -->
              <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
              <div class="cout" class:failed={s.output.failed} tabindex="0" role="group" aria-label="What it printed" data-testid="chat-agent-work-output">
                <pre>{s.output.text}</pre>
                {#if s.output.more}
                  <p class="cgap">⋯ {lineWord(s.output.more)} ⋯</p>
                  <pre>{s.output.tail}</pre>
                {/if}
              </div>
            {/if}
          {/if}
          {#each pics as p (p.id)}
            {@const q = placed(p, { maxW: PIC_W, maxH: PIC_H, ratio: screenPixels.ratio })}
            <span class="shotframe spic" style:width="{q.frame.w}px" style:aspect-ratio="{q.frame.w} / {q.frame.h}">
              <button type="button" class="spicbtn" data-testid="chat-agent-work-step-picture" title="Open the picture" aria-label="Open the picture: {p.caption || 'Picture'}" onclick={() => onview?.(p)}>
                <img
                  src={q.src}
                  alt=""
                  draggable="false"
                  decoding="async"
                  use:shown
                  style:left={q.pic.x}
                  style:top={q.pic.y}
                  style:width={q.pic.w}
                  style:height={q.pic.h}
                  onerror={() => (gone = { ...gone, [p.id]: true })}
                />
              </button>
            </span>
          {/each}
        </div>
      {/if}
    </li>
  {/each}
</ol>

<style>
  /* The box: a row a step, a hairline between. */
  .rows {
    margin: 6px 0 4px;
    padding: 0;
    list-style: none;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 10px;
    overflow: hidden;
    color: var(--yap-muted);
    font-size: 12px;
  }
  .row {
    min-width: 0;
  }
  .row + .row {
    border-top: 1px solid var(--yap-border-subtle);
  }
  .rline {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    padding-right: 8px;
  }
  .rhead {
    display: flex;
    flex: 1 1 auto;
    align-items: center;
    gap: 6px;
    min-width: 0;
    margin: 0;
    padding: 7px 4px 7px 10px;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
  }
  button.rhead {
    cursor: pointer;
  }
  button.rhead:hover {
    color: var(--yap-fg-80);
  }
  button.rhead:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: -2px;
    border-radius: 8px;
  }
  .rtext {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* The file's name, set apart from the words round it. */
  .rname {
    color: var(--yap-fg);
  }
  .plus,
  .minus,
  .rfailed {
    flex: 0 0 auto;
    font-variant-numeric: tabular-nums;
  }
  .plus {
    color: var(--yap-success-text);
  }
  .minus {
    color: var(--yap-danger);
  }
  .rfailed {
    color: var(--yap-danger);
    font-size: 11px;
  }
  .chev {
    flex: 0 0 auto;
    width: 12px;
    height: 12px;
    opacity: 0.8;
    transition: transform 120ms ease;
  }
  .chev.down {
    transform: rotate(90deg);
  }
  .spin {
    flex: 0 0 auto;
    width: 10px;
    height: 10px;
    border: 1.5px solid color-mix(in srgb, var(--yap-code) 30%, transparent);
    border-top-color: var(--yap-code);
    border-radius: 50%;
    animation: row-spin 0.9s linear infinite;
  }
  @keyframes row-spin {
    to {
      transform: rotate(360deg);
    }
  }
  :global(:root[data-motion='reduced']) .spin {
    animation: none;
  }
  :global(:root[data-motion='reduced']) .chev {
    transition: none;
  }

  /* A row opened. */
  .rbody {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    padding: 0 10px 10px;
  }
  .rpath {
    margin: 0;
    color: var(--yap-fg-80);
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 11px;
    overflow-wrap: anywhere;
  }
  .rnote {
    color: var(--yap-muted);
    font-family: var(--yap-font, inherit);
  }
  .rmore {
    margin: 0;
    font-size: 11px;
  }
  /* Its diff: numbered lines, as long as they are (the box scrolls). */
  .dbox,
  .cout,
  .cbox {
    border: 1px solid var(--yap-border-subtle);
    border-radius: 8px;
    background: var(--yap-s2);
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 11.5px;
    line-height: 1.55;
  }
  .dbox {
    max-height: 320px;
    overflow: auto;
    padding: 4px 0;
  }
  .dbox:focus-visible,
  .cout:focus-visible,
  .ccmd:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  /* A long line wraps under itself, beside its number (12 Nov 2026, as Claude Code's does): never a
     box to scroll sideways. */
  .dl {
    display: flex;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--yap-fg-80);
    /* A bar down its left edge says which it is, as its colour does. */
    box-shadow: inset 2px 0 0 transparent;
  }
  .dno {
    flex: 0 0 auto;
    box-sizing: content-box;
    width: calc(var(--digits, 1) * 1ch);
    padding: 0 8px 0 10px;
    color: var(--yap-muted);
    text-align: right;
    user-select: none;
    font-variant-numeric: tabular-nums;
  }
  .dt {
    flex: 1 1 auto;
    min-width: 0;
    padding-right: 10px;
  }
  .dl.del {
    background: color-mix(in srgb, var(--yap-danger) 13%, transparent);
    box-shadow: inset 2px 0 0 var(--yap-danger);
    color: var(--yap-fg);
  }
  .dl.add {
    background: color-mix(in srgb, var(--yap-success) 14%, transparent);
    box-shadow: inset 2px 0 0 var(--yap-success);
    color: var(--yap-fg);
  }
  /* What was replaced inside a changed line, and what replaced it: stronger than the line's own tint. */
  .dl.del :global(.chg) {
    background: color-mix(in srgb, var(--yap-danger) 34%, transparent);
  }
  .dl.add :global(.chg) {
    background: color-mix(in srgb, var(--yap-success) 36%, transparent);
  }
  .dl.gap {
    justify-content: center;
    color: var(--yap-muted);
    font-size: 10.5px;
    line-height: 1.2;
  }
  /* (The code's colours are app.css's `.syn-*`, shared with an answer's code.) */
  /* Its command, and what it printed. */
  .cbox {
    position: relative;
    display: flex;
    align-items: flex-start;
  }
  .ccmd {
    flex: 1 1 auto;
    min-width: 0;
    max-height: 200px;
    margin: 0;
    padding: 8px 34px 8px 10px;
    overflow: auto;
    color: var(--yap-fg);
    font: inherit;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .cprompt {
    color: var(--yap-muted);
    user-select: none;
  }
  .ccopy {
    position: absolute;
    top: 5px;
    right: 5px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 6px;
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .ccopy:hover {
    background: var(--yap-s3, var(--yap-border-subtle));
    color: var(--yap-fg);
  }
  .ccopy:focus-visible {
    outline: 2px solid var(--yap-primary-line);
    outline-offset: 1px;
  }
  .ccopy svg {
    width: 14px;
    height: 14px;
  }
  .cout {
    max-height: 260px;
    overflow: auto;
    padding: 8px 10px;
    background: none;
    color: var(--yap-fg-80);
  }
  .cout pre {
    margin: 0;
    font: inherit;
    white-space: pre;
  }
  .cgap {
    margin: 4px 0;
    color: var(--yap-muted);
    font-family: var(--yap-font, inherit);
    font-size: 11px;
  }
  /* A row's picture, opened: in a screenshot's frame (app.css .shotframe), smaller. */
  .spic {
    display: block;
    border-radius: 6px;
  }
  .spicbtn {
    position: absolute;
    inset: 0;
    display: block;
    padding: 0;
    border: none;
    border-radius: inherit;
    overflow: hidden;
    background: none;
    cursor: zoom-in;
  }
  .spicbtn:focus-visible {
    outline: none;
  }
  .spic:has(.spicbtn:focus-visible) {
    box-shadow: var(--yap-focus);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
</style>
