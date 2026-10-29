<script>
  // Welcome back (after Wispr Flow's): one friendly card on Home for
  // someone who hasn't dictated for 14 days or more (dictation/stats.rs
  // `welcome`): their shortcut, and one thing that's new since they left
  // (core/whatsNew.js). Once per return: ✕ or its button answers it
  // (`welcome_back_dismiss`), and it goes by itself two days after they're
  // back.
  import { invoke } from '@tauri-apps/api/core';
  import { oneNewThing } from '../../core/whatsNew.js';

  // welcome: { since, days, back }; hotkeyLabel: the dictation shortcut, as
  // shown ("Ctrl+Shift+Space").
  let { welcome, hotkeyLabel = 'F9', onopensettings = null, onnavigate = null, ondismiss = null } = $props();

  const item = $derived(oneNewThing(welcome?.since));

  /** "2 weeks", "3 weeks", "a month", "2 months". */
  function gap(days) {
    if (days < 28) return `${Math.max(2, Math.round(days / 7))} weeks`;
    const months = Math.round(days / 30);
    return months <= 1 ? 'a month' : `${months} months`;
  }

  const keyParts = (label) =>
    String(label || '')
      .split('+')
      .map((p) => p.trim())
      .filter(Boolean);

  async function dismiss() {
    ondismiss?.();
    try {
      await invoke('welcome_back_dismiss');
    } catch {
      /* it may show again next time: harmless */
    }
  }

  function act() {
    const a = item?.action;
    if (a?.settings) onopensettings?.(a.settings);
    else if (a?.view) onnavigate?.(a.view);
    dismiss();
  }

  const keys = $derived(keyParts(hotkeyLabel));
</script>

<section class="welcome" aria-labelledby="welcome-back-title">
  <button class="close" aria-label="Close welcome back" title="Close" onclick={dismiss}>
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
  </button>
  <h2 id="welcome-back-title">Welcome back</h2>
  <p class="lead">
    {welcome.back ? `Good to hear from you again after ${gap(welcome.days)}.` : `It's been ${gap(welcome.days)} since your last dictation.`}
    Press
    <span class="keys">
      {#each keys as part, i}
        {#if i > 0}<span class="keyplus">+</span>{/if}
        <kbd class="key">{part}</kbd>
      {/each}
    </span>
    in any app to dictate.
  </p>
  {#if item}
    <div class="new">
      <p>
        <span class="tag">{item.isNew ? 'New' : 'Worth a try'}</span>
        <strong>{item.title}.</strong>
        {item.text}
      </p>
      {#if item.action}
        <button class="cta" onclick={act}>{item.action.label}</button>
      {/if}
    </div>
  {/if}
</section>

<style>
  .welcome {
    position: relative;
    margin-bottom: 18px;
    padding: 18px 44px 18px 22px;
    border: 1px solid var(--yap-primary-line);
    border-radius: var(--yap-r-xl);
    background: var(--yap-primary-wash);
  }
  h2 {
    margin: 0 0 6px;
    font-family: var(--yap-font-display);
    font-size: 21px;
    font-weight: 550;
    color: var(--yap-fg);
  }
  .lead {
    margin: 0;
    font-size: 13.5px;
    line-height: 1.7;
    color: var(--yap-fg-80);
  }
  .keys {
    display: inline-flex;
    align-items: baseline;
    gap: 4px;
    margin: 0 2px;
  }
  .key {
    display: inline-block;
    padding: 1px 7px 2px;
    font-family: inherit;
    font-size: 0.88em;
    font-weight: 650;
    color: var(--yap-key-fg);
    background: var(--yap-key-bg);
    border: 1px solid var(--yap-key-border);
    border-bottom-width: 2.5px;
    border-radius: 6px;
    white-space: nowrap;
  }
  .keyplus {
    color: var(--yap-muted-55);
  }
  .new {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px 14px;
    margin-top: 12px;
    padding-top: 12px;
    border-top: 1px solid var(--yap-primary-line);
  }
  .new p {
    flex: 1 1 260px;
    margin: 0;
    font-size: 13px;
    line-height: 1.55;
    color: var(--yap-fg-80);
  }
  .new strong {
    color: var(--yap-fg);
    font-weight: 650;
  }
  .tag {
    display: inline-block;
    margin-right: 6px;
    padding: 0 7px;
    border-radius: var(--yap-r-full);
    background: var(--yap-primary);
    color: var(--yap-primary-fg);
    font-size: 11px;
    font-weight: 650;
    line-height: 18px;
    vertical-align: 1px;
  }
  .cta {
    padding: 6px 12px;
    border: 1px solid var(--yap-border);
    border-radius: var(--yap-r);
    background: var(--yap-s1);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--yap-fg);
    cursor: pointer;
  }
  .cta:hover {
    border-color: var(--yap-border-hover);
  }
  .close {
    position: absolute;
    top: 10px;
    right: 10px;
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    padding: 0;
    border: none;
    border-radius: var(--yap-r);
    background: none;
    color: var(--yap-muted);
    cursor: pointer;
  }
  .close:hover {
    background: var(--yap-raised);
    color: var(--yap-fg);
  }
  .close svg {
    width: 15px;
    height: 15px;
  }
</style>
