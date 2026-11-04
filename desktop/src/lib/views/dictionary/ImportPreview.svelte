<script>
  // Import…'s preview: what the file adds, what's already in the
  // dictionary, what it spells another way (the person picks, entry by
  // entry: theirs by default) and what's left out and why — before
  // anything changes. Built from dictionaryFile.js's `planImport`; Add hands
  // the choices back (`onapply({leaveOut, useFile})`), which
  // `importChanges` turns into the change. A dialog like the confirm one:
  // Esc, the scrim and Cancel close it; Tab stays inside; the focus goes
  // back to Import….
  import { onMount, tick } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';
  import { importChanges } from './dictionaryFile.js';
  import { entriesLabel } from './dictionaryList.js';

  let { name = '', format = 'csv', plan, skipped = [], returnFocus = null, onapply, onclose } = $props();

  const uid = Math.random().toString(36).slice(2, 8);
  let card = $state(null);
  let primary = $state(null);
  const leaveOut = new SvelteSet();
  const useFile = new SvelteSet();

  const change = $derived(importChanges(plan, { leaveOut, useFile }));
  const respellCount = $derived(new Set(change.respell.map((r) => r.entry)).size);
  const allTicked = $derived(leaveOut.size === 0);

  const summary = $derived.by(() => {
    const parts = [];
    const n = plan.add.length;
    if (n) {
      const s = plan.add.filter((r) => r.snippet).length;
      parts.push(
        s && s < n
          ? `${(n - s).toLocaleString()} ${n - s === 1 ? 'correction' : 'corrections'} and ${s.toLocaleString()} ${s === 1 ? 'snippet' : 'snippets'} to add`
          : `${entriesLabel(n)} to add`
      );
    }
    if (plan.conflicts.length) parts.push(`${plan.conflicts.length.toLocaleString()} spelled differently`);
    if (plan.same.length) parts.push(`${plan.same.length.toLocaleString()} already in your dictionary`);
    const out = plan.repeats.length + skipped.length;
    if (out) parts.push(`${out.toLocaleString()} left out`);
    return parts.length ? `${parts.join(' · ')}.` : 'Nothing in this file to add.';
  });

  const applyLabel = $derived.by(() => {
    const a = change.rows;
    const c = respellCount;
    if (!a && !c) return 'Nothing to add';
    if (!c) return `Add ${entriesLabel(a)}`;
    if (!a) return `Change ${c.toLocaleString()}`;
    return `Add ${a.toLocaleString()}, change ${c.toLocaleString()}`;
  });

  // The focus starts on Add (Enter adds), or on the dialog when there's nothing to add.
  onMount(() => {
    tick().then(() => (primary?.disabled ? card : primary)?.focus({ preventScroll: true }));
  });

  function close() {
    onclose?.();
    tick().then(() => returnFocus?.isConnected && returnFocus.focus?.({ preventScroll: true }));
  }
  function apply() {
    onapply?.({ leaveOut: new Set(leaveOut), useFile: new Set(useFile) });
    tick().then(() => returnFocus?.isConnected && returnFocus.focus?.({ preventScroll: true }));
  }
  function tickAll() {
    if (allTicked) for (const r of plan.add) leaveOut.add(r);
    else leaveOut.clear();
  }
  function choose(c, file) {
    if (file) useFile.add(c);
    else useFile.delete(c);
  }
  function chooseAll(file) {
    for (const c of plan.conflicts) choose(c, file);
  }

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if (e.key !== 'Tab') return;
    const focusable = [...card.querySelectorAll('button:not(:disabled), input:not(:disabled), summary')].filter(
      (el) => el.offsetParent !== null
    );
    if (!focusable.length) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && (document.activeElement === first || document.activeElement === card)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  const firstLine = (t) => {
    const line = String(t ?? '').split('\n')[0];
    return line.length > 80 ? `${line.slice(0, 79)}…` : line;
  };
  /** A row as it reads: "heard → written", a word alone, a snippet's phrase → its first line. */
  const shows = (r) => (r.snippet ? `${r.from} → ${firstLine(r.to)}` : r.from === r.to ? r.to : `${r.from} → ${r.to}`);
</script>

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div
    bind:this={card}
    class="sheet"
    role="dialog"
    aria-modal="true"
    aria-labelledby="imp-title-{uid}"
    aria-describedby="imp-sum-{uid}"
    tabindex="-1"
    onkeydown={onKeydown}
  >
    <div class="head">
      <h2 id="imp-title-{uid}">Import from “{name}”</h2>
      <p id="imp-sum-{uid}">{summary}</p>
      <p class="fmt">Read as {format === 'csv' ? 'CSV' : 'text, one entry a line'}.</p>
    </div>

    <div class="body">
      {#if plan.add.length}
        <section aria-labelledby="imp-add-{uid}">
          <div class="sh">
            <h3 id="imp-add-{uid}">To add <span class="n">{plan.add.length.toLocaleString()}</span></h3>
            <button type="button" class="link" onclick={tickAll}>{allTicked ? 'Untick all' : 'Tick all'}</button>
          </div>
          <ul class="list">
            {#each plan.add as row (row)}
              <li>
                <label>
                  <input
                    type="checkbox"
                    checked={!leaveOut.has(row)}
                    onchange={(e) => (e.currentTarget.checked ? leaveOut.delete(row) : leaveOut.add(row))}
                  />
                  <span class="what">{shows(row)}</span>
                  {#if row.snippet}<span class="tag">Snippet</span>{/if}
                  {#if row.into}<span class="tag" title={`Another way “${row.to}” is heard, in the entry you have`}>Another way it's heard</span>{/if}
                  {#if row.starred}<span class="tag">Starred</span>{/if}
                </label>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if plan.conflicts.length}
        <section aria-labelledby="imp-conf-{uid}">
          <div class="sh">
            <h3 id="imp-conf-{uid}">Spelled differently <span class="n">{plan.conflicts.length.toLocaleString()}</span></h3>
            {#if plan.conflicts.length > 1}
              <span class="links">
                <button type="button" class="link" onclick={() => chooseAll(false)}>Keep all of yours</button>
                <button type="button" class="link" onclick={() => chooseAll(true)}>Use all of the file's</button>
              </span>
            {/if}
          </div>
          <ul class="list conflicts">
            {#each plan.conflicts as c, i (c)}
              <li>
                <fieldset>
                  <legend>
                    {#if c.why === 'heard'}
                      {c.row.snippet ? 'Saying' : 'Heard as'} “{c.row.from}”
                    {:else}
                      The word “{c.row.to}”{#if c.row.from.toLowerCase() !== c.row.to.toLowerCase()}, heard as “{c.row.from}”{/if}
                    {/if}
                  </legend>
                  <label>
                    <input type="radio" name="imp-{uid}-{i}" checked={!useFile.has(c)} onchange={() => choose(c, false)} />
                    Keep yours: <b>{firstLine(c.existing.to)}</b>
                  </label>
                  <label>
                    <input type="radio" name="imp-{uid}-{i}" checked={useFile.has(c)} onchange={() => choose(c, true)} />
                    Use the file's: <b>{firstLine(c.row.to)}</b>
                  </label>
                </fieldset>
              </li>
            {/each}
          </ul>
        </section>
      {/if}

      {#if plan.same.length}
        <details>
          <summary>Already in your dictionary <span class="n">{plan.same.length.toLocaleString()}</span></summary>
          <ul class="list quiet">
            {#each plan.same as s (s.row)}<li>{shows(s.row)}</li>{/each}
          </ul>
        </details>
      {/if}

      {#if plan.repeats.length || skipped.length}
        <details>
          <summary>Left out <span class="n">{(plan.repeats.length + skipped.length).toLocaleString()}</span></summary>
          <ul class="list quiet">
            {#each skipped as s (s)}
              <li>Line {s.line}: “{firstLine(s.source)}” — {s.reason}</li>
            {/each}
            {#each plan.repeats as r (r)}
              <li>Line {r.line}: “{shows(r)}” — said again earlier in the file.</li>
            {/each}
          </ul>
        </details>
      {/if}
    </div>

    <div class="btns">
      <button type="button" class="btn" data-cancel onclick={close}>Cancel</button>
      <button
        type="button"
        class="btn primary"
        bind:this={primary}
        disabled={!change.rows && !respellCount}
        onclick={apply}
      >{applyLabel}</button>
    </div>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: 90;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 16px;
    background: var(--yap-paper-scrim);
    animation: scrim-in 0.15s ease-out;
  }
  @keyframes scrim-in {
    from {
      opacity: 0;
    }
  }
  .sheet {
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: min(560px, 100%);
    max-height: min(680px, calc(100vh - 48px));
    box-sizing: border-box;
    padding: 22px 24px 20px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 14px;
    background: var(--yap-paper);
    color: var(--yap-paper-ink);
    box-shadow: var(--yap-shadow-dialog);
    outline: none;
    animation: sheet-in 0.15s var(--yap-ease-spring) both;
  }
  @keyframes sheet-in {
    from {
      opacity: 0;
      transform: scale(0.96);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .sheet {
      animation: none;
    }
  }
  h2 {
    margin: 0 0 6px;
    font-size: 17px;
    font-weight: 600;
    line-height: 24px;
    overflow-wrap: anywhere;
  }
  .head p {
    margin: 0;
    font-size: 13.5px;
    line-height: 20px;
    color: var(--yap-paper-text-2);
  }
  .head .fmt {
    margin-top: 2px;
    font-size: 12px;
    color: var(--yap-paper-muted);
  }
  .body {
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding-right: 4px;
  }
  .sh {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 6px;
  }
  h3 {
    margin: 0;
    font-size: 13px;
    font-weight: 600;
  }
  .n {
    margin-left: 4px;
    font-weight: 400;
    color: var(--yap-paper-muted);
    font-variant-numeric: tabular-nums;
  }
  .links {
    display: flex;
    gap: 12px;
  }
  .link {
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-link);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .link:hover {
    text-decoration: underline;
  }
  .list {
    margin: 0;
    padding: 4px 0;
    list-style: none;
    max-height: 220px;
    overflow-y: auto;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 8px;
    background: var(--yap-paper-card);
  }
  .list li {
    padding: 0 10px;
    font-size: 13px;
    line-height: 1.4;
    content-visibility: auto;
    contain-intrinsic-size: auto 28px;
  }
  .list label {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    cursor: pointer;
  }
  .list input {
    margin: 0;
    accent-color: var(--yap-primary);
  }
  .what {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag {
    flex: 0 0 auto;
    padding: 0 6px;
    border-radius: var(--yap-r-full);
    background: var(--yap-paper-band);
    color: var(--yap-paper-text-2);
    font-size: 11px;
    line-height: 18px;
  }
  .conflicts li + li {
    border-top: 1px solid var(--yap-paper-rule);
  }
  fieldset {
    margin: 0;
    padding: 6px 0 8px;
    border: none;
    min-width: 0;
  }
  legend {
    padding: 0;
    font-size: 12.5px;
    color: var(--yap-paper-text-2);
    overflow-wrap: anywhere;
  }
  fieldset label {
    min-height: 24px;
  }
  fieldset b {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  details summary {
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    border-radius: 4px;
  }
  details .list {
    margin-top: 6px;
  }
  .quiet li {
    padding: 3px 10px;
    color: var(--yap-paper-text-2);
    font-size: 12.5px;
    overflow-wrap: anywhere;
  }
  .btns {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
  .btn {
    height: 36px;
    padding: 0 16px;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 9999px;
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    font: inherit;
    font-size: 14px;
    font-weight: 600;
    cursor: pointer;
  }
  .btn:hover:not(:disabled) {
    border-color: var(--yap-paper-s2-strong);
  }
  .btn.primary {
    border-color: transparent;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
  }
  .btn.primary:hover:not(:disabled) {
    background: var(--yap-ink-hover);
  }
  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .btn:focus-visible,
  .link:focus-visible,
  summary:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
  .list input:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
  }
</style>
