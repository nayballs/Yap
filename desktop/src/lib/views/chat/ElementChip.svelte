<script>
  // **An element picked on a page beside the chat**, as a chip (14 Nov 2026;
  // the Claude Code app's, as the maintainer showed it): `<div class="…">`,
  // and on hover or focus its card — "Included with your message", its
  // picture, "Element <div class=… />", its words, and "Show N more details"
  // (where it is, its page, its styles, its HTML). In the message box with a
  // ×; in a sent question without (chat/pages/pick.rs, core/selectedElements.js).
  // On a React page (15 Nov 2026) its component and where its JSX was written
  // come first, its owners and props with the details.
  import { chatImageUrl } from './chatImages.js';
  import { elementLabel, elementLine, elementDetails, elementReact } from '../../core/selectedElements.js';

  /** `el`: the element; `picture`: its picture's id, or none; `onremove`: the box's ×. */
  let { el, picture = null, onremove = null } = $props();

  let more = $state(false);
  const label = $derived(elementLabel(el));
  const details = $derived(elementDetails(el));
  const words = $derived(String(el?.text || '').trim());
  const react = $derived(elementReact(el));
</script>

<span class="elchip" data-testid="chat-element-chip">
  <span class="elname" tabindex="0" role="button" aria-label={`Element ${label}`} title={label}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 3l12 6.5-5.2 1.6L9.6 16.5z" /><path d="M12.4 11.6 17 16.5" /></svg>
    <span class="eltext">{label}</span>
  </span>
  {#if onremove}
    <button type="button" class="elx" aria-label={`Remove ${label}`} title="Remove" onclick={onremove} data-testid="chat-element-remove">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
    </button>
  {/if}
  <span class="elcard" role="tooltip" data-testid="chat-element-card">
    <span class="elhead">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 3l12 6.5-5.2 1.6L9.6 16.5z" /></svg>
      Included with your message
    </span>
    {#if picture}
      <img class="elpic" src={chatImageUrl(picture)} alt="" draggable="false" decoding="async" />
    {/if}
    <span class="elgrid">
      <span class="elkey">Element</span><code class="elval" data-testid="chat-element-line">{elementLine(el)}</code>
      {#if react?.component}<span class="elkey">Component</span><code class="elval" data-testid="chat-element-component" title={react.build ? `React, ${react.build} build` : 'React'}>{react.component}</code>{/if}
      {#if react?.source}<span class="elkey">Source</span><code class="elval elsrc" data-testid="chat-element-source">{react.source}</code>{/if}
      {#if words}<span class="elkey">Text</span><span class="elval elwords" data-testid="chat-element-text">{words}</span>{/if}
      {#if more}
        {#each details as d (d.name)}
          <span class="elkey">{d.name}</span>
          {#if d.code}<pre class="elval elcode">{d.value}</pre>{:else}<span class="elval elwords">{d.value}</span>{/if}
        {/each}
      {/if}
    </span>
    {#if details.length}
      <button type="button" class="elmore" onclick={() => (more = !more)} data-testid="chat-element-more">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" class:open={more}><path d="M9 6l6 6-6 6" /></svg>
        {more ? 'Show fewer details' : `Show ${details.length} more details`}
      </button>
    {/if}
  </span>
</span>

<style>
  .elchip {
    position: relative;
    display: inline-flex;
    align-items: center;
    max-width: 280px;
    height: 30px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: var(--yap-r);
    background: var(--yap-paper-s1);
    color: var(--yap-paper-ink);
    font-size: 12.5px;
  }
  .elname {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 0 8px;
    height: 100%;
    border-radius: inherit;
    cursor: default;
    outline: none;
  }
  .elname:focus-visible {
    outline: 2px solid var(--yap-focus-ring);
    outline-offset: 1px;
  }
  .elname svg {
    flex: none;
    width: 14px;
    height: 14px;
    color: var(--yap-paper-muted);
  }
  .eltext {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: var(--yap-font-mono, ui-monospace, monospace);
  }
  .elx {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    margin-right: 3px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    color: var(--yap-paper-muted);
    cursor: pointer;
  }
  .elx:hover {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-ink);
  }
  .elx svg {
    width: 12px;
    height: 12px;
  }
  /* Its card: on hover or focus, over the chip. */
  .elcard {
    position: absolute;
    left: 0;
    bottom: calc(100% + 6px);
    z-index: 40;
    display: none;
    flex-direction: column;
    gap: 8px;
    width: 300px;
    max-height: 420px;
    overflow-x: hidden;
    overflow-y: auto;
    padding: 10px 12px;
    border: 1px solid var(--yap-paper-rule);
    border-radius: var(--yap-r-lg);
    background: var(--yap-paper-card);
    box-shadow: var(--yap-shadow-popover);
    color: var(--yap-paper-ink);
    font-size: 12.5px;
    line-height: 1.45;
    cursor: default;
  }
  .elchip:hover .elcard,
  .elchip:focus-within .elcard {
    display: flex;
  }
  .elhead {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--yap-paper-muted);
    font-size: 12px;
  }
  .elhead svg {
    width: 13px;
    height: 13px;
  }
  .elpic {
    display: block;
    max-width: 100%;
    max-height: 160px;
    object-fit: contain;
    align-self: flex-start;
    border-radius: var(--yap-r);
    border: 1px solid var(--yap-paper-rule);
  }
  .elgrid {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    min-width: 0;
    gap: 4px 12px;
    align-items: baseline;
  }
  .elkey {
    color: var(--yap-paper-muted);
  }
  .elval {
    min-width: 0;
    overflow-wrap: anywhere;
  }
  code.elval {
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 12px;
  }
  code.elsrc {
    color: var(--yap-paper-muted);
  }
  .elwords {
    white-space: pre-line;
    max-height: 140px;
    overflow: auto;
  }
  .elcode {
    margin: 0;
    max-height: 140px;
    overflow: auto;
    white-space: pre-wrap;
    font-family: var(--yap-font-mono, ui-monospace, monospace);
    font-size: 11.5px;
  }
  .elmore {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    align-self: flex-start;
    padding: 0;
    border: none;
    background: none;
    color: var(--yap-paper-muted);
    font: inherit;
    cursor: pointer;
  }
  .elmore:hover {
    color: var(--yap-paper-ink);
  }
  .elmore svg {
    width: 12px;
    height: 12px;
    transition: transform var(--yap-dur, 0.15s);
  }
  .elmore svg.open {
    transform: rotate(90deg);
  }
</style>
