<script module>
  /** The three settings, in the Claude app's words. */
  export const SETTINGS = [
    { value: 'allow', label: 'Always allow' },
    { value: 'ask', label: 'Ask first' },
    { value: 'off', label: 'Off' },
  ];

  /** The settings a tool may have: never Always allow for one that can delete; only Off for one Yap can't offer. */
  export function settingsFor(kind, unavailable = false) {
    if (unavailable) return SETTINGS.filter((s) => s.value === 'off');
    return kind === 'deletes' ? SETTINGS.filter((s) => s.value !== 'allow') : SETTINGS;
  }
</script>

<script>
  // One of a connector's tools (Settings → Connectors → a connector's page):
  // its title, two lines of its description, why it starts Off when it does
  // (its description reads like orders to the model, or its inputs couldn't
  // be made safe), and the three-way choice — Always allow · Ask first · Off.
  // A tool that can delete never offers Always allow (cache.rs holds it to
  // that too). A choice saves at once (`connector_set_tool`).
  //
  //   tool: Rust's ToolView; onset(setting): the choice made; busy: saving.
  let { tool, onset, busy = false } = $props();

  const unavailable = $derived(!!tool.skipped);
  const options = $derived(settingsFor(tool.kind, unavailable));
  const label = $derived(tool.title || tool.name);
</script>

<div class="tool" data-tool={tool.name}>
  <div class="text">
    <div class="name">
      {label}
      {#if tool.title && tool.title !== tool.name}<code>{tool.name}</code>{/if}
    </div>
    {#if tool.description}<div class="desc" title={tool.description}>{tool.description}</div>{/if}
    {#if tool.skipped}
      <div class="why">Yap can't offer it: {tool.skipped}</div>
    {:else if tool.flag}
      <div class="why">{tool.flag}</div>
    {/if}
  </div>
  <div class="choice" role="radiogroup" aria-label={`${label}: when Chat may use it`}>
    {#each options as o (o.value)}
      <button
        type="button"
        role="radio"
        class="opt"
        class:on={tool.setting === o.value}
        aria-checked={tool.setting === o.value}
        disabled={busy}
        onclick={() => tool.setting !== o.value && onset?.(o.value)}
      >
        {o.label}
      </button>
    {/each}
  </div>
</div>

<style>
  .tool {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 11px 0;
    border-top: 1px solid var(--yap-rule);
  }
  .text {
    flex: 1 1 auto;
    min-width: 0;
  }
  .name {
    font-size: 13px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  code {
    margin-left: 6px;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--yap-muted);
  }
  .desc {
    margin-top: 2px;
    font-size: 12.5px;
    line-height: 1.45;
    color: var(--yap-muted);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .why {
    margin-top: 3px;
    font-size: 12px;
    color: var(--yap-warning);
  }
  .choice {
    flex: 0 0 auto;
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: var(--yap-r);
    background: var(--yap-raised-soft);
  }
  .opt {
    height: 26px;
    padding: 0 10px;
    border: none;
    border-radius: 6px;
    background: none;
    color: var(--yap-fg-62);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }
  .opt:hover:not(:disabled):not(.on) {
    color: var(--yap-fg);
  }
  .opt.on {
    background: var(--yap-s2);
    color: var(--yap-fg);
    box-shadow: var(--yap-shadow-sm), 0 0 0 1px var(--yap-border-subtle);
  }
  .opt:focus-visible {
    outline: none;
    box-shadow: var(--yap-focus);
  }
  .opt:disabled {
    cursor: default;
  }
  @media (max-width: 960px) {
    .tool {
      flex-wrap: wrap;
    }
  }
</style>
