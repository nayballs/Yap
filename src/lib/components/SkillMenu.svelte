<script>
  // A box's `/` menu (stores/skills.svelte.js `SkillPicker`): the Skills
  // that run here, the command typed so far first; a click runs one, and
  // the keys stay in the box (↑ ↓, Enter runs it, Tab puts it in the box to
  // add words, Esc closes). Over the box: in the notepad's paper look, or
  // the main window's.
  import { runsOnLabel } from '../core/skills.js';

  /** `picker`: the box's SkillPicker; `onpick(skill, run)`; `look`: 'paper' | 'app'. */
  let { picker, onpick, look = 'app' } = $props();

  let listEl = $state(null);
  $effect(() => {
    const i = picker.active;
    listEl?.querySelectorAll('[role="option"]')[i]?.scrollIntoView({ block: 'nearest' });
  });
</script>

{#if picker.open}
  <div class="skillmenu {look}" data-testid="skill-menu">
    <div class="skillhead">Skills</div>
    <!-- The focus stays in the box: its keys move the highlight (aria-activedescendant there). -->
    <ul role="listbox" aria-label="Skills" id="skill-list" bind:this={listEl}>
      {#each picker.items as s, i (s.id)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <li
          role="option"
          id={`skill-opt-${i}`}
          aria-selected={i === picker.active}
          class:active={i === picker.active}
          onmousedown={(e) => e.preventDefault()}
          onmouseenter={() => (picker.active = i)}
          onclick={() => onpick(s, true)}
        >
          <span class="cmd">/{s.command}</span>
          <span class="what">
            <span class="name">{s.name}</span>
            {#if s.description}<span class="desc">{s.description}</span>{/if}
          </span>
          <span class="tag" title={runsOnLabel(s.runsOn)}>{s.builtin ? 'Yap' : 'Yours'}</span>
        </li>
      {/each}
    </ul>
    <div class="skillfoot">Enter runs it · Tab adds your own words</div>
  </div>
{/if}

<style>
  .skillmenu {
    position: absolute;
    left: 0;
    right: 0;
    bottom: calc(100% + 6px);
    z-index: 30;
    max-height: 260px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: 12px;
    pointer-events: auto;
  }
  .skillmenu.paper {
    border: 1px solid var(--yap-paper-s2);
    background: var(--yap-paper-card);
    color: var(--yap-paper-ink);
    box-shadow: var(--yap-shadow-popover);
  }
  .skillmenu.app {
    border: 1px solid var(--yap-border-subtle);
    background: var(--yap-bg);
    color: var(--yap-fg);
    box-shadow: var(--yap-shadow-menu);
  }
  .skillhead,
  .skillfoot {
    flex: 0 0 auto;
    padding: 6px 12px;
    font-size: 11px;
    font-weight: 600;
  }
  .paper .skillhead,
  .paper .skillfoot {
    color: var(--yap-paper-muted);
  }
  .app .skillhead,
  .app .skillfoot {
    color: var(--yap-muted);
  }
  .skillfoot {
    font-weight: 500;
  }
  .paper .skillfoot {
    border-top: 1px solid var(--yap-paper-s2);
  }
  .app .skillfoot {
    border-top: 1px solid var(--yap-border-subtle);
  }
  ul {
    flex: 1 1 auto;
    min-height: 0;
    margin: 0;
    padding: 0 4px;
    overflow-y: auto;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px;
    border-radius: 8px;
    font-size: 13px;
    cursor: pointer;
  }
  .paper li.active {
    background: var(--yap-paper-s2);
  }
  .app li.active {
    background: var(--yap-s2);
  }
  .cmd {
    flex: 0 0 auto;
    font-weight: 650;
  }
  .what {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    gap: 6px;
    overflow: hidden;
    white-space: nowrap;
  }
  .name {
    flex: 0 0 auto;
  }
  .desc {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .paper .desc,
  .paper .tag {
    color: var(--yap-paper-muted);
  }
  .app .desc,
  .app .tag {
    color: var(--yap-muted);
  }
  .tag {
    flex: 0 0 auto;
    font-size: 11px;
  }
</style>
