<script module>
  import { formatBytes } from '../core/downloads.js';

  /** How much there is of a kind now: "3 chats, 4 screenshots · 12 MB"; "Nothing kept". */
  export function amount(line) {
    if (line.empty) return 'Nothing kept';
    return [line.count, line.bytes > 0 ? formatBytes(line.bytes) : ''].filter(Boolean).join(' · ');
  }
</script>

<script>
  // Delete some of your data… (Settings → Privacy & data → Your data;
  // src-tauri/src/privacy/reset.rs): a tick box per kind of data, grouped
  // as the plan groups them, each saying what it removes and how much there
  // is now ("3 chats, 4 screenshots · 12 MB"; "Nothing kept" greys it).
  // Delete… hands the ticked kinds back (`onnext(kinds)`); YourData.svelte
  // asks with Yap's confirm dialog before anything goes. A dialog like
  // Import…'s preview: Esc, the scrim and Cancel close it; Tab stays inside;
  // the focus goes back to the button that opened it.
  import { onMount, tick } from 'svelte';
  import { SvelteSet } from 'svelte/reactivity';

  let { plan, busy = false, returnFocus = null, onnext, onclose } = $props();

  const uid = Math.random().toString(36).slice(2, 8);
  let card = $state(null);
  // Kept while the confirm dialog asks: its Cancel comes back to them.
  const ticked = new SvelteSet();

  const lines = $derived(plan?.goes ?? []);
  const groups = $derived.by(() => {
    const out = [];
    for (const line of lines) {
      let g = out.find((x) => x.title === line.group);
      if (!g) out.push((g = { title: line.group, lines: [] }));
      g.lines.push(line);
    }
    return out;
  });
  const can = (line) => !line.empty && !line.unread;
  const picked = $derived(lines.filter((l) => ticked.has(l.kind) && can(l)));
  const pickedBytes = $derived(picked.reduce((n, l) => n + (l.bytes || 0), 0));

  // The focus starts on the first tick box there's something to delete for.
  onMount(() => {
    tick().then(() => (card?.querySelector('input:not(:disabled)') ?? card)?.focus({ preventScroll: true }));
  });

  function close() {
    if (busy) return;
    onclose?.();
    tick().then(() => returnFocus?.isConnected && returnFocus.focus?.({ preventScroll: true }));
  }

  function next() {
    if (!picked.length || busy) return;
    onnext?.(picked.map((l) => l.kind));
  }

  function toggle(kind, on) {
    if (on) ticked.add(kind);
    else ticked.delete(kind);
  }

  function onKeydown(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    if (e.key !== 'Tab') return;
    const focusable = [...card.querySelectorAll('button:not(:disabled), input:not(:disabled)')].filter((el) => el.offsetParent !== null);
    if (!focusable.length) return;
    const head = focusable[0];
    const last = focusable[focusable.length - 1];
    if (e.shiftKey && (document.activeElement === head || document.activeElement === card)) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      head.focus();
    }
  }
</script>

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && close()}>
  <div
    bind:this={card}
    class="sheet"
    role="dialog"
    aria-modal="true"
    aria-labelledby="del-title-{uid}"
    aria-describedby="del-lead-{uid}"
    tabindex="-1"
    onkeydown={onKeydown}
  >
    <div class="head">
      <h2 id="del-title-{uid}">Delete some of your data</h2>
      <p id="del-lead-{uid}">
        Tick what goes from this PC. Your settings, dictionary, snippets, Skills, transforms, writing samples, models,
        paired phones and account stay.
      </p>
    </div>

    <div class="body">
      {#each groups as g, gi (g.title)}
        <section aria-labelledby="del-g{gi}-{uid}">
          <h3 id="del-g{gi}-{uid}">{g.title}</h3>
          <ul class="list">
            {#each g.lines as line (line.kind)}
              <li>
                <label class:off={!can(line)}>
                  <input
                    type="checkbox"
                    checked={ticked.has(line.kind) && can(line)}
                    disabled={!can(line) || busy}
                    onchange={(e) => toggle(line.kind, e.currentTarget.checked)}
                  />
                  <span class="text">
                    <span class="name">{line.label}</span>
                    <span class="says">{line.unread || line.says}</span>
                  </span>
                  <!-- The small print: its files in Yap's data folder, "(notes.json)" a part of one. -->
                  <span class="amount" data-kind={line.kind} title={`In Yap's data folder: ${(line.files ?? []).join(', ')}`}>{amount(line)}</span>
                </label>
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>

    <div class="foot">
      <span class="picked" aria-live="polite">
        {#if picked.length}
          {picked.length === 1 ? '1 kind' : `${picked.length} kinds`} ticked{#if pickedBytes > 0}{` · ${formatBytes(pickedBytes)}`}{/if}
        {:else}
          Nothing ticked yet
        {/if}
      </span>
      <div class="btns">
        <button type="button" class="btn" onclick={close} disabled={busy}>Cancel</button>
        <button type="button" class="btn danger" disabled={!picked.length || busy} onclick={next}>Delete…</button>
      </div>
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
    max-height: min(720px, calc(100vh - 48px));
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
  }
  .head p {
    margin: 0;
    font-size: 13.5px;
    line-height: 20px;
    color: var(--yap-paper-text-2);
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
  h3 {
    margin: 0 0 6px;
    font-size: 13px;
    font-weight: 600;
  }
  .list {
    margin: 0;
    padding: 2px 0;
    list-style: none;
    border: 1px solid var(--yap-paper-s2);
    border-radius: 8px;
    background: var(--yap-paper-card);
  }
  .list li + li {
    border-top: 1px solid var(--yap-paper-rule);
  }
  label {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 9px 12px;
    cursor: pointer;
  }
  label.off {
    cursor: default;
  }
  label.off .name,
  label.off .amount {
    color: var(--yap-paper-muted);
  }
  input {
    flex: 0 0 auto;
    margin: 2px 0 0;
    accent-color: var(--yap-danger-fill);
  }
  input:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 2px;
  }
  .text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    flex: 1 1 auto;
    min-width: 0;
  }
  .name {
    font-size: 13.5px;
    font-weight: 600;
    line-height: 19px;
  }
  .says {
    font-size: 12.5px;
    line-height: 17px;
    color: var(--yap-paper-text-2);
  }
  .amount {
    flex: 0 0 auto;
    max-width: 40%;
    font-size: 12.5px;
    line-height: 19px;
    text-align: right;
    color: var(--yap-paper-text-2);
    font-variant-numeric: tabular-nums;
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
  }
  .picked {
    font-size: 12.5px;
    color: var(--yap-paper-text-2);
    font-variant-numeric: tabular-nums;
  }
  .btns {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-left: auto;
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
  .btn.danger {
    border-color: transparent;
    background: var(--yap-danger-fill);
    color: #fff;
  }
  .btn.danger:hover:not(:disabled) {
    background: var(--yap-danger-fill-hover);
  }
  .btn:disabled {
    opacity: 0.55;
    cursor: default;
  }
  .btn:focus-visible {
    outline: none;
    box-shadow: 0 0 0 3px var(--yap-paper-ring);
  }
</style>
