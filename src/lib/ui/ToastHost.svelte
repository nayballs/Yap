<script>
  // Toast viewport — Wispr-Flow-style cards (see docs screenshots 2026-07-09):
  // dark rounded card, small category chip top-left (their lavender "Tip"
  // pill), always-visible circular ✕ top-right, bold white title, soft grey
  // body, optional light action button bottom-right ("Open Settings") with a
  // quiet secondary beside it ("Later"), and a small text link on its own
  // line under them for a rarer third answer ("Don't ask for Google Meet" —
  // a whole line, so long app names fit). Keeps OpenWhispr's timer behaviour:
  // hover-pause, hairline progress bar, destructive descriptions in a copyable
  // mono error box. Long-running toasts (the update toast) can also carry a
  // determinate progress bar, a busy spinner in the chip, and a "What's new"
  // toggle that unfolds release notes in the card. Mounted once in
  // ControlPanel.
  import { toastStore, dismiss, pauseToast, resumeToast } from './toast.svelte.js';
  import { renderMarkdown } from '../markdown.js';

  let copiedId = $state(null);
  // Toasts whose `expand` notes are unfolded.
  let expanded = $state([]);

  const CHIP_LABELS = { default: 'Tip', success: 'Done', destructive: 'Error' };

  function toggleExpand(id) {
    expanded = expanded.includes(id) ? expanded.filter((x) => x !== id) : [...expanded, id];
  }

  async function copyError(t) {
    if (!t.description) return;
    try {
      await navigator.clipboard.writeText(t.description);
      copiedId = t.id;
      setTimeout(() => (copiedId = null), 2000);
    } catch {
      /* clipboard unavailable */
    }
  }

  // The ✕: the card's own close answer, if it has one (a call prompt's
  // "Not now"), then away.
  function close(t) {
    try {
      t.onClose?.();
    } finally {
      dismiss(t.id);
    }
  }

  function runAction(t, which = 'action') {
    const a = t[which];
    try {
      a?.onClick?.();
    } finally {
      if (!a?.keepOpen) dismiss(t.id);
    }
  }
</script>

{#if toastStore.list.length > 0}
  <div class="viewport">
    {#each toastStore.list as t (t.id)}
      <div
        class="toast {t.variant}"
        class:exiting={t.isExiting}
        role="status"
        onmouseenter={() => pauseToast(t.id)}
        onmouseleave={() => resumeToast(t.id)}
      >
        <div class="toprow">
          <span class="chip">
            {#if t.busy}
              <span class="spinner" aria-hidden="true"></span>
            {:else if t.icon === 'update'}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 4v11" /><path d="M7 10.5l5 5 5-5" /><path d="M5 20h14" /></svg>
            {:else if t.icon === 'alert'}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 9v4" /><path d="M12 17h.01" /><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /></svg>
            {:else if t.icon === 'call'}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 16.9v3a2 2 0 0 1-2.2 2 19.8 19.8 0 0 1-8.6-3.1 19.5 19.5 0 0 1-6-6A19.8 19.8 0 0 1 2.1 4.2 2 2 0 0 1 4.1 2h3a2 2 0 0 1 2 1.7c.1.9.4 1.8.7 2.7a2 2 0 0 1-.5 2.1L8 9.8a16 16 0 0 0 6 6l1.3-1.3a2 2 0 0 1 2.1-.4c.9.3 1.8.6 2.7.7a2 2 0 0 1 1.7 2z" /></svg>
            {:else if t.icon === 'screen'}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="2.5" y="4" width="19" height="12.5" rx="2" /><path d="M8 20.5h8M12 16.5v4" /></svg>
            {:else if t.icon === 'timer'}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><circle cx="12" cy="13.5" r="8" /><path d="M12 9.5v4l2.5 2" /><path d="M9.5 2.5h5" /></svg>
            {:else if t.variant === 'success'}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12l5 5L20 6" /></svg>
            {:else if t.variant === 'destructive'}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 9v4" /><path d="M12 17h.01" /><path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" /></svg>
            {:else}
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M18.4 5.6 17 7M7 17l-1.4 1.4" /><circle cx="12" cy="12" r="4" /></svg>
            {/if}
            {t.chip || CHIP_LABELS[t.variant] || CHIP_LABELS.default}
          </span>
          <button class="close" aria-label="Close" onclick={() => close(t)}>
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
          </button>
        </div>
        {#if t.title}<div class="title">{t.title}</div>{/if}
        {#if t.description && t.variant === 'destructive'}
          <div class="errbox">
            <span class="errtext">{t.description}</span>
            <button class="errcopy" aria-label="Copy error" onclick={() => copyError(t)}>
              {#if copiedId === t.id}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12l5 5L20 6" /></svg>
              {:else}
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="12" height="12" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>
              {/if}
            </button>
          </div>
        {:else if t.description}
          <div class="desc">{t.description}</div>
        {/if}
        {#if typeof t.progress === 'number'}
          <div
            class="pbar"
            role="progressbar"
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow={Math.round(t.progress)}
          >
            <div class="pfill" style={`width:${Math.max(0, Math.min(100, t.progress))}%`}></div>
          </div>
        {/if}
        {#if t.expand?.markdown && expanded.includes(t.id)}
          <!-- renderMarkdown escapes all input first — no raw HTML gets through. -->
          <div class="notes">{@html renderMarkdown(t.expand.markdown)}</div>
        {/if}
        {#if t.action?.label || t.secondary?.label || t.expand?.markdown}
          <div class="actions">
            {#if t.expand?.markdown}
              <button
                class="expandbtn"
                aria-expanded={expanded.includes(t.id)}
                onclick={() => toggleExpand(t.id)}
              >
                {t.expand.label || 'Details'}
                <svg class:up={expanded.includes(t.id)} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
              </button>
            {/if}
            <span class="grow"></span>
            {#if t.secondary?.label}
              <button class="secondarybtn" onclick={() => runAction(t, 'secondary')}>{t.secondary.label}</button>
            {/if}
            {#if t.action?.label}
              <button class="actionbtn" onclick={() => runAction(t)}>{t.action.label}</button>
            {/if}
          </div>
        {/if}
        {#if t.tertiary?.label}
          <div class="tertiary">
            <button class="tertiarybtn" onclick={() => runAction(t, 'tertiary')}>{t.tertiary.label}</button>
          </div>
        {/if}
        {#if t.duration > 0 && !t.isExiting}
          <div class="progresswrap">
            <div class="progress" style={`animation-duration:${t.duration}ms`}></div>
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}

<style>
  /* A window can move the stack (the meeting notepad lifts it over its
     footer) with --yap-toast-bottom / --yap-toast-right. */
  .viewport {
    position: fixed;
    bottom: var(--yap-toast-bottom, 20px);
    right: var(--yap-toast-right, 20px);
    z-index: 100;
    display: flex;
    flex-direction: column;
    gap: 8px;
    pointer-events: none;
  }
  /* Wispr card: warm near-black, big radius, generous padding. */
  .toast {
    pointer-events: auto;
    position: relative;
    display: flex;
    flex-direction: column;
    width: 320px;
    padding: 14px 16px 15px;
    border-radius: 16px;
    overflow: hidden;
    background: #1c1a16;
    box-shadow: 0 12px 40px rgba(0, 0, 0, 0.4);
    animation: toast-in 300ms ease-out;
    transition:
      opacity 200ms ease-out,
      transform 200ms ease-out;
  }
  .toast.exiting {
    opacity: 0;
    transform: translateY(6px) scale(0.98);
  }
  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(12px) scale(0.98);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }
  .toprow {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 9px;
  }
  /* Category pill — Wispr's "Tip" chip, in Yap's warm palette per variant. */
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 10px;
    border-radius: 8px;
    background: #ecd9b8;
    color: #453413;
    font-size: 12px;
    font-weight: 650;
  }
  .chip svg {
    width: 12px;
    height: 12px;
  }
  .toast.success .chip {
    background: #c6e3c4;
    color: #1e4620;
  }
  .toast.destructive .chip {
    background: #f0c4bf;
    color: #5c1a14;
  }
  .title {
    font-size: 14.5px;
    font-weight: 700;
    line-height: 1.35;
    color: rgba(255, 255, 255, 0.95);
  }
  .desc {
    margin-top: 3px;
    font-size: 13px;
    line-height: 1.5;
    color: rgba(255, 255, 255, 0.55);
  }
  .errbox {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 6px;
    margin-top: 7px;
    padding: 6px 8px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.07);
  }
  .errtext {
    min-width: 0;
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11px;
    line-height: 1.45;
    color: rgba(252, 165, 165, 0.85);
    overflow-wrap: anywhere;
    user-select: all;
  }
  .errcopy {
    flex: 0 0 auto;
    display: inline-flex;
    width: 18px;
    height: 18px;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: 4px;
    background: none;
    color: rgba(255, 255, 255, 0.3);
    cursor: pointer;
  }
  .errcopy:hover {
    color: rgba(255, 255, 255, 0.7);
    background: rgba(255, 255, 255, 0.06);
  }
  .errcopy svg {
    width: 11px;
    height: 11px;
  }
  /* Always-visible circular ✕ in the card's corner (Wispr). */
  .close {
    flex: 0 0 auto;
    display: flex;
    width: 26px;
    height: 26px;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    border: none;
    background: rgba(255, 255, 255, 0.1);
    color: rgba(255, 255, 255, 0.85);
    cursor: pointer;
    transition: background 150ms ease;
  }
  .close:hover {
    background: rgba(255, 255, 255, 0.2);
    color: #fff;
  }
  .close svg {
    width: 11px;
    height: 11px;
  }
  /* Busy spinner in the chip (e.g. "Restarting Yap…"). */
  .spinner {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    border: 2px solid currentColor;
    border-right-color: transparent;
    animation: toast-spin 0.8s linear infinite;
  }
  @keyframes toast-spin {
    to {
      transform: rotate(360deg);
    }
  }
  /* Determinate progress (downloads) — Yap amber on a faint track. */
  .pbar {
    margin-top: 10px;
    height: 4px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.1);
    overflow: hidden;
  }
  .pfill {
    height: 100%;
    border-radius: 2px;
    background: #f0b04a;
    transition: width 0.2s ease;
  }
  /* Unfolded notes ("What's new"): compact, scrollable, rendered markdown. */
  .notes {
    margin-top: 10px;
    max-height: 150px;
    overflow-y: auto;
    padding: 8px 11px;
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.05);
    border: 1px solid rgba(255, 255, 255, 0.07);
    font-size: 12.5px;
    line-height: 1.5;
    color: rgba(255, 255, 255, 0.72);
    user-select: text;
  }
  .notes :global(:is(h2, h3, h4, h5)) {
    margin: 6px 0 2px;
    font-size: 12.5px;
    font-weight: 700;
    color: rgba(255, 255, 255, 0.9);
  }
  .notes :global(p) {
    margin: 0 0 6px;
  }
  .notes :global(:is(ul, ol)) {
    margin: 0 0 6px;
    padding-left: 18px;
  }
  .notes :global(code) {
    font-family: ui-monospace, Consolas, monospace;
    font-size: 11.5px;
  }
  .notes :global(:last-child) {
    margin-bottom: 0;
  }
  /* Action row: optional "What's new" toggle left, buttons right. */
  .actions {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 12px;
  }
  .grow {
    flex: 1;
  }
  .expandbtn {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 0;
    border: none;
    background: none;
    color: rgba(255, 255, 255, 0.6);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
    transition: color 150ms ease;
  }
  .expandbtn:hover {
    color: rgba(255, 255, 255, 0.9);
  }
  .expandbtn svg {
    width: 12px;
    height: 12px;
    transition: transform 150ms ease;
  }
  .expandbtn svg.up {
    transform: rotate(180deg);
  }
  /* Quiet secondary ("Later") beside the light primary. */
  .secondarybtn {
    height: 32px;
    padding: 0 12px;
    border: none;
    border-radius: 10px;
    background: none;
    color: rgba(255, 255, 255, 0.72);
    font: inherit;
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition:
      background 150ms ease,
      color 150ms ease;
  }
  .secondarybtn:hover {
    background: rgba(255, 255, 255, 0.08);
    color: #fff;
  }
  /* Light action button bottom-right — Wispr's "Open Settings". */
  .actionbtn {
    height: 32px;
    padding: 0 14px;
    border: none;
    border-radius: 10px;
    background: #f5f3ee;
    color: #26231c;
    font: inherit;
    font-size: 13px;
    font-weight: 650;
    cursor: pointer;
    transition: background 150ms ease;
  }
  .actionbtn:hover {
    background: #ffffff;
  }
  /* The rarer third answer: a small muted link on its own line, under the
     buttons, so the card stays two buttons wide whatever the app's name. */
  .tertiary {
    display: flex;
    justify-content: flex-end;
    margin-top: 8px;
  }
  .tertiarybtn {
    max-width: 100%;
    padding: 2px 4px;
    margin-right: -4px;
    border: none;
    border-radius: 6px;
    background: none;
    color: rgba(255, 255, 255, 0.5);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    text-decoration: underline;
    text-decoration-color: rgba(255, 255, 255, 0.25);
    text-underline-offset: 3px;
    overflow-wrap: anywhere;
    cursor: pointer;
    transition: color 150ms ease;
  }
  .tertiarybtn:hover {
    color: rgba(255, 255, 255, 0.85);
    text-decoration-color: currentColor;
  }
  .progresswrap {
    position: absolute;
    bottom: 0;
    left: 16px;
    right: 16px;
    height: 2px;
    overflow: hidden;
    border-radius: 2px 2px 0 0;
  }
  .progress {
    height: 100%;
    background: rgba(255, 255, 255, 0.14);
    animation-name: toast-progress;
    animation-timing-function: linear;
    animation-fill-mode: forwards;
  }
  .toast.success .progress {
    background: rgba(140, 205, 140, 0.35);
  }
  .toast.destructive .progress {
    background: rgba(248, 113, 113, 0.35);
  }
  @keyframes toast-progress {
    from {
      width: 100%;
    }
    to {
      width: 0%;
    }
  }
</style>
