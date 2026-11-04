<script>
  // The main window's Help (2026-10-29; Wispr Flow's Help menu, by
  // behaviour): a "?" in the title bar, beside the bell, opening
  //   Keyboard shortcuts   the sheet (also Ctrl+/ anywhere in the window)
  //   Microphone help      Settings → Dictation → Microphone, at the mic quiz
  //   Languages            Settings → Dictation → My languages
  //   Report a problem     Settings → About → Report a problem
  //   What's new           the release notes, when Yap has them here; else
  //                        the release page on GitHub
  //   GitHub               Yap's source and issues
  // ↑/↓ move through it, Esc or a click outside closes it (ControlPanel).
  //
  //   open         bindable: the menu is open
  //   onshortcuts  open the shortcut sheet
  //   onsettings   open Settings at a section
  //   onwhatsnew   show the release notes
  import { tick } from 'svelte';
  import { openExternalLink } from '../../core/externalLinks.js';
  import { updates, releaseUrlFor } from '../../stores/updates.svelte.js';

  let { open = $bindable(false), onshortcuts, onsettings, onwhatsnew } = $props();

  const GITHUB = 'https://github.com/nayballs/Yap';

  let menu = $state(null);

  const items = $derived([
    { id: 'shortcuts', label: 'Keyboard shortcuts', hint: 'Ctrl+/', run: () => onshortcuts?.() },
    { id: 'mic', label: 'Microphone help', run: () => onsettings?.('dictation#mic-quiz') },
    { id: 'languages', label: 'Languages', run: () => onsettings?.('dictation#languages') },
    { id: 'report', label: 'Report a problem', run: () => onsettings?.('about#report') },
    {
      id: 'whatsnew',
      label: "What's new",
      run: () => (updates.notes ? onwhatsnew?.() : openExternalLink(releaseUrlFor(updates.currentVersion))),
    },
    { id: 'github', label: 'GitHub', external: true, run: () => openExternalLink(GITHUB) },
  ]);

  async function toggle() {
    open = !open;
    if (open) {
      await tick();
      menu?.querySelector('button')?.focus({ preventScroll: true });
    }
  }

  function pick(item) {
    open = false;
    item.run();
  }

  function onMenuKey(e) {
    const buttons = [...(menu?.querySelectorAll('button') ?? [])];
    const i = buttons.indexOf(document.activeElement);
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      buttons[(i + 1) % buttons.length]?.focus();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      buttons[(i - 1 + buttons.length) % buttons.length]?.focus();
    } else if (e.key === 'Home') {
      e.preventDefault();
      buttons[0]?.focus();
    } else if (e.key === 'End') {
      e.preventDefault();
      buttons.at(-1)?.focus();
    }
  }
</script>

<div class="helpwrap">
  <button
    class="winbtn help"
    class:open
    aria-label="Help"
    aria-haspopup="menu"
    aria-expanded={open}
    data-tip="Help"
    onclick={toggle}
  >
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.1" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M9.6 9.3a2.5 2.5 0 0 1 4.8.9c0 1.7-2.4 2.2-2.4 3.6" /><path d="M12 17.2h.01" /></svg>
  </button>
  {#if open}
    <!-- svelte-ignore a11y_interactive_supports_focus -->
    <div class="helpmenu" role="menu" aria-label="Help" bind:this={menu} onkeydown={onMenuKey}>
      {#each items as item (item.id)}
        <button class="helpitem" role="menuitem" data-help={item.id} onclick={() => pick(item)}>
          <span>{item.label}</span>
          {#if item.hint}
            <kbd>{item.hint}</kbd>
          {:else if item.external}
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M19 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V6a1 1 0 0 1 1-1h5" /></svg>
          {/if}
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .helpwrap {
    position: relative;
    display: flex;
    align-self: stretch;
  }
  /* The title bar's caption-button look (ControlPanel's .winbtn). */
  .winbtn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 100%;
    border: none;
    background: none;
    color: var(--yap-fg);
    cursor: pointer;
    position: relative;
    transition:
      background var(--yap-dur) ease,
      color var(--yap-dur) ease;
  }
  .winbtn svg {
    width: 18px;
    height: 18px;
  }
  .winbtn:hover,
  .winbtn.open {
    background: var(--yap-raised);
  }
  .winbtn::after {
    content: attr(data-tip);
    position: absolute;
    top: calc(100% + 2px);
    right: 4px;
    padding: 4px 9px;
    border-radius: 7px;
    background: var(--yap-ink);
    color: var(--yap-ink-fg);
    font-size: 11.5px;
    font-weight: 600;
    white-space: nowrap;
    opacity: 0;
    pointer-events: none;
    transition: opacity 120ms ease;
    z-index: 60;
  }
  .winbtn:hover:not(.open)::after {
    opacity: 1;
  }
  .helpmenu {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    width: 236px;
    padding: 6px;
    border: 1px solid var(--yap-border-subtle);
    border-radius: 12px;
    background: var(--yap-s2);
    box-shadow: var(--yap-shadow-menu);
    z-index: 90;
  }
  .helpitem {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    padding: 8px 10px;
    border: none;
    border-radius: 8px;
    background: none;
    color: var(--yap-fg);
    font: inherit;
    font-size: 13px;
    font-weight: 550;
    text-align: left;
    cursor: pointer;
  }
  .helpitem:hover,
  .helpitem:focus-visible {
    background: var(--yap-raised-soft);
  }
  .helpitem svg {
    width: 13px;
    height: 13px;
    color: var(--yap-muted);
  }
  kbd {
    padding: 1px 6px;
    border-radius: 5px;
    background: var(--yap-primary-tint);
    font: inherit;
    font-size: 11px;
    font-weight: 650;
  }
</style>
