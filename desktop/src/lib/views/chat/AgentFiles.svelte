<script>
  // **The files an agent changed**, in its tab (AgentTab.svelte): each with its
  // lines added and removed, opening to what changed (core/agents.js
  // `diffsOf`, from the agent's log: agents/diff.rs) — an edit's lines removed
  // in red and added in green with a few unchanged around them, a new file's
  // first lines ("New file · 12 lines"), "Rewritten" for a file it wrote
  // whole that was there before, "…N more lines" past the caps. A file with
  // no diff in the log (an older agent's, one swept) is its name alone.
  import { diffTotals, diffTitle, diffLine, moreLines } from '../../core/agents.js';

  /** `files`: the agent's files, as said; `diffs`: file → its diffs (`diffsOf`). */
  let { files = [], diffs = new Map() } = $props();

  let open = $state({});
</script>

<ul class="files">
  {#each files as f (f)}
    {@const ds = diffs.get(f) || []}
    {@const t = diffTotals(ds)}
    <li class="file" data-testid="chat-agent-file" data-file={f}>
      {#if ds.length}
        <button type="button" class="frow" aria-expanded={!!open[f]} onclick={() => (open = { ...open, [f]: !open[f] })} title={open[f] ? 'Hide the changes' : 'Show the changes'}>
          <svg class="chev" class:down={open[f]} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m9 6 6 6-6 6" /></svg>
          <code class="fname">{f}</code>
          {#if t.made === 'new'}<span class="tag">New</span>{/if}
          <span class="counts" aria-label={`${t.added} added, ${t.removed} removed`}>
            {#if t.added}<span class="plus">+{t.added}</span>{/if}
            {#if t.removed}<span class="minus">−{t.removed}</span>{/if}
          </span>
        </button>
        {#if open[f]}
          <div class="diffs">
            {#each ds as d, i (i)}
              <div class="diff" data-testid="chat-agent-diff" data-kind={d.kind}>
                {#if diffTitle(d)}<p class="dtitle">{diffTitle(d)}</p>{/if}
                {#if d.all}<p class="dtitle">Every match replaced</p>{/if}
                {#if d.lines?.length}
                  <div class="dlines" role="group" aria-label={`Changes to ${f}`}>
                    {#each d.lines as l, k (k)}
                      {@const x = diffLine(l)}
                      {#if x.mark === 'gap'}
                        <div class="dl gap" aria-hidden="true">⋯</div>
                      {:else}
                        <div class="dl {x.mark}"><span class="dm" aria-hidden="true">{x.mark === 'del' ? '−' : x.mark === 'add' ? '+' : ' '}</span><span class="dt">{x.text || ' '}</span>{#if x.mark !== 'ctx'}<span class="sr-only">{x.mark === 'del' ? ' (removed)' : ' (added)'}</span>{/if}</div>
                      {/if}
                    {/each}
                  </div>
                {/if}
                {#if d.more}<p class="dmore">{moreLines(d.more)}</p>{/if}
              </div>
            {/each}
          </div>
        {/if}
      {:else}
        <div class="frow still"><code class="fname">{f}</code></div>
      {/if}
    </li>
  {/each}
</ul>

<style>
  .files {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .file + .file {
    margin-top: 2px;
  }
  .frow {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    min-height: 26px;
    padding: 2px 6px 2px 2px;
    border: none;
    border-radius: 7px;
    background: none;
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 12.5px;
    text-align: left;
  }
  button.frow {
    cursor: pointer;
  }
  button.frow:hover {
    background: var(--yap-paper-s1);
  }
  button.frow:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .frow.still {
    padding-left: 22px;
  }
  .chev {
    flex: 0 0 auto;
    width: 14px;
    height: 14px;
    color: var(--yap-paper-muted);
    transition: transform 0.12s ease;
  }
  .chev.down {
    transform: rotate(90deg);
  }
  :global(:root[data-motion='reduced']) .chev {
    transition: none;
  }
  .fname {
    min-width: 0;
    overflow-wrap: anywhere;
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 12px;
    padding: 1px 5px;
    border-radius: 5px;
    background: var(--yap-paper-s1);
  }
  .tag {
    flex: 0 0 auto;
    padding: 0 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--yap-success) 14%, transparent);
    color: var(--yap-success-text);
    font-size: 11px;
    font-weight: 600;
  }
  .counts {
    margin-left: auto;
    flex: 0 0 auto;
    display: inline-flex;
    gap: 6px;
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 11.5px;
    font-weight: 600;
  }
  .plus {
    color: var(--yap-success-text);
  }
  .minus {
    color: var(--yap-danger);
  }
  .diffs {
    margin: 4px 0 8px 20px;
  }
  .diff + .diff {
    margin-top: 6px;
  }
  .dtitle {
    margin: 0 0 3px;
    color: var(--yap-paper-text-2);
    font-size: 11.5px;
    font-weight: 600;
  }
  .dlines {
    position: relative;
    overflow-x: auto;
    padding: 4px 0;
    border: 1px solid var(--yap-paper-rule);
    border-radius: 8px;
    background: var(--yap-paper);
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 11.5px;
    line-height: 1.55;
  }
  .dl {
    display: flex;
    width: max-content;
    min-width: 100%;
    white-space: pre;
    color: var(--yap-paper-text-2);
  }
  .dm {
    flex: 0 0 auto;
    width: 18px;
    text-align: center;
    user-select: none;
    color: var(--yap-paper-muted);
  }
  .dt {
    padding-right: 10px;
  }
  .dl.del {
    background: color-mix(in srgb, var(--yap-danger) 12%, transparent);
    color: var(--yap-paper-ink);
  }
  .dl.del .dm {
    color: var(--yap-danger);
  }
  .dl.add {
    background: color-mix(in srgb, var(--yap-success) 13%, transparent);
    color: var(--yap-paper-ink);
  }
  .dl.add .dm {
    color: var(--yap-success-text);
  }
  .dl.gap {
    justify-content: center;
    color: var(--yap-paper-muted);
    font-size: 10.5px;
    line-height: 1.2;
  }
  .dmore {
    margin: 3px 0 0;
    color: var(--yap-paper-muted);
    font-size: 11.5px;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
</style>
