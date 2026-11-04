<script>
  // A box's `/` menu (stores/skills.svelte.js `SkillPicker`): the Skills
  // shown that run here — just "/", yours then Yap's under their headings;
  // a command begun, one list with the best match first. A click runs one
  // (one that needs words, /project-update, goes in the box for them), and
  // the keys stay in the box (↑ ↓, Enter runs it, Tab puts it in the box to
  // add words, Esc closes). More Skills opens Settings → Skills, where the
  // rest are switched on. Over the box: in the notepad's paper look, or the
  // main window's.
  import { needsWords, menuLine, runsOnLabel } from '../core/skills.js';
  import { openMoreSkills } from '../stores/skills.svelte.js';

  /** `picker`: the box's SkillPicker; `onpick(skill, run)`; `look`: 'paper' | 'app'. */
  let { picker, onpick, look = 'app' } = $props();

  let listEl = $state(null);
  $effect(() => {
    const i = picker.active;
    listEl?.querySelector(`#skill-opt-${i}`)?.scrollIntoView({ block: 'nearest' });
  });

  // The pointer picks a row only when it moves: a row scrolled under a
  // still pointer (↑ ↓ at the list's edge) never takes the highlight back.
  let pointer = { x: -1, y: -1 };
  function hover(e, i) {
    if (e.clientX === pointer.x && e.clientY === pointer.y) return;
    pointer = { x: e.clientX, y: e.clientY };
    picker.active = i;
  }

  // Just "/": grouped, so a row needn't say whose it is.
  const grouped = $derived(picker.groups.some((g) => g.label));

  // Settings → Skills over this: the menu stays, so a Skill switched on
  // there is in it on the way back.
  function more() {
    openMoreSkills();
  }
</script>

{#snippet option(s, i)}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    role="option"
    tabindex="-1"
    id={`skill-opt-${i}`}
    class="opt"
    aria-selected={i === picker.active}
    class:active={i === picker.active}
    title={`${s.name} · ${runsOnLabel(s.runsOn)}`}
    onmousedown={(e) => e.preventDefault()}
    onmousemove={(e) => hover(e, i)}
    onclick={() => onpick(s, !needsWords(s))}
  >
    <span class="cmd">/{s.command}</span>
    <span class="line">{menuLine(s)}</span>
    {#if !grouped && !s.builtin}<span class="tag">Yours</span>{/if}
  </div>
{/snippet}

{#if picker.open}
  <div class="skillmenu {look}" data-testid="skill-menu">
    <!-- The focus stays in the box: its keys move the highlight (aria-activedescendant there). -->
    <div class="list" role="listbox" aria-label="Skills" id="skill-list" bind:this={listEl}>
      {#each picker.groups as g, gi (g.label || gi)}
        {#if g.label}
          <div class="group" role="group" aria-labelledby={`skill-group-${gi}`}>
            <div class="ghead" id={`skill-group-${gi}`} aria-hidden="true">{g.label}</div>
            {#each g.items as s, k (s.id)}{@render option(s, g.start + k)}{/each}
          </div>
        {:else}
          <div class="group">
            {#each g.items as s, k (s.id)}{@render option(s, g.start + k)}{/each}
          </div>
        {/if}
      {/each}
    </div>
    <div class="foot">
      <span class="keys" aria-hidden="true">
        <span><kbd>↑</kbd><kbd>↓</kbd> move</span>
        <span><kbd>Enter</kbd> run</span>
        <span><kbd>Tab</kbd> add words</span>
      </span>
      <button type="button" class="more" tabindex="-1" onmousedown={(e) => e.preventDefault()} onclick={more}>More Skills</button>
    </div>
  </div>
{/if}

<style>
  .skillmenu {
    position: absolute;
    left: 0;
    right: 0;
    bottom: calc(100% + 6px);
    z-index: 30;
    max-height: 300px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: var(--yap-r-lg);
    pointer-events: auto;
  }
  .skillmenu.paper {
    border: 1px solid var(--yap-paper-s2);
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    box-shadow: var(--yap-shadow-popover);
  }
  .skillmenu.app {
    border: 1px solid var(--yap-border);
    background: var(--yap-s2);
    color: var(--yap-fg);
    box-shadow: var(--yap-shadow-menu);
  }
  .list {
    flex: 1 1 auto;
    min-height: 0;
    padding: 4px;
    overflow-y: auto;
  }
  .group + .group {
    margin-top: 4px;
  }
  .ghead {
    padding: 6px 8px 3px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.01em;
  }
  .paper .ghead {
    color: var(--yap-paper-muted);
  }
  .app .ghead {
    color: var(--yap-muted);
  }
  .opt {
    display: flex;
    align-items: baseline;
    gap: 12px;
    min-height: 32px;
    box-sizing: border-box;
    padding: 7px 8px;
    border-radius: var(--yap-r-sm);
    font-size: 13px;
    line-height: 1.35;
    cursor: pointer;
  }
  .paper .opt.active {
    background: var(--yap-paper-s1);
  }
  .app .opt.active {
    background: var(--yap-s3);
  }
  .cmd {
    flex: 0 0 auto;
    min-width: 8.5em;
    font-weight: 650;
    white-space: nowrap;
  }
  .line {
    flex: 1 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tag {
    flex: 0 0 auto;
    padding: 1px 6px;
    border-radius: var(--yap-r-full);
    font-size: 10.5px;
    font-weight: 600;
  }
  .paper .line {
    color: var(--yap-paper-muted);
  }
  .app .line {
    color: var(--yap-muted);
  }
  .paper .tag {
    background: var(--yap-paper-s2);
    color: var(--yap-paper-text-2);
  }
  .app .tag {
    background: var(--yap-raised);
    color: var(--yap-fg-80);
  }
  .foot {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 6px 8px 6px 12px;
    font-size: 11px;
  }
  .paper .foot {
    border-top: 1px solid var(--yap-paper-s2);
    color: var(--yap-paper-muted);
  }
  .app .foot {
    border-top: 1px solid var(--yap-border-subtle);
    color: var(--yap-muted);
  }
  .keys {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 12px;
    min-width: 0;
  }
  .keys > span {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    white-space: nowrap;
  }
  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 16px;
    box-sizing: border-box;
    padding: 0 4px;
    border-radius: 4px;
    font: inherit;
    font-size: 10px;
    font-weight: 600;
  }
  .paper kbd {
    border: 1px solid var(--yap-paper-s2-strong);
    background: var(--yap-paper-s1);
    color: var(--yap-paper-text-2);
  }
  .app kbd {
    border: 1px solid var(--yap-border);
    background: var(--yap-s1);
    color: var(--yap-fg-80);
  }
  .more {
    flex: 0 0 auto;
    padding: 3px 6px;
    border: none;
    border-radius: var(--yap-r-sm);
    background: none;
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .paper .more {
    color: var(--yap-paper-ink);
  }
  .app .more {
    color: var(--yap-link);
  }
  .paper .more:hover {
    background: var(--yap-paper-s1);
  }
  .app .more:hover {
    background: var(--yap-s3);
  }
</style>
